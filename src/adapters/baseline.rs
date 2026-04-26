//! Baseline-envelope loader — reads a previously-emitted crap4rs JSON
//! envelope from disk and extracts the `result` block, plus the
//! envelope metadata (tool_version, timestamp) needed for delta
//! reporting.
//!
//! The on-the-wire JSON envelope (see `adapters::reporters::json`)
//! includes far more than `result`: `view`, `diagnostics`, `delta` (in
//! the future), etc. The baseline loader ignores everything except
//! `schema_version`, `result`, `tool_version`, `timestamp`, and
//! optionally `diagnostics` so that consumers can produce baseline
//! envelopes that contain extra fields without breaking us.
//!
//! Schema version validation: only `schema_version == 1` is accepted
//! today. Future schema bumps will need an explicit migration path.

use crate::domain::types::{AnalysisDiagnostics, AnalysisResult};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufReader, ErrorKind};
use std::path::Path;

/// Currently-supported envelope schema version. Lockstep with
/// `adapters::reporters::json::JsonEnvelope::schema_version`.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// On-disk shape we read. Mirrors the relevant subset of
/// `JsonEnvelope`. `serde(default)` on optional fields keeps us
/// forward-compatible with envelopes that omit fields we don't need.
#[derive(Debug, Deserialize)]
struct BaselineEnvelope {
    schema_version: u32,
    #[serde(default)]
    tool_version: String,
    #[serde(default)]
    timestamp: String,
    result: AnalysisResult,
    #[serde(default)]
    diagnostics: Option<AnalysisDiagnostics>,
}

/// What the loader returns to callers. The fields are all the metadata
/// the delta envelope's `delta.baseline_*` keys ultimately surface.
#[derive(Debug, Clone)]
pub struct BaselineSnapshot {
    pub result: AnalysisResult,
    pub tool_version: String,
    pub timestamp: String,
    pub diagnostics: Option<AnalysisDiagnostics>,
}

/// Errors raised while loading a baseline envelope.
///
/// Tag-only — variants carry numeric / string context but no
/// pre-formatted prose. The CLI translates these into user-facing
/// stderr messages (keeps the adapter language-neutral for future
/// `crap-core` extraction).
#[derive(Debug, thiserror::Error)]
pub enum BaselineError {
    #[error("baseline file not found: {path}")]
    NotFound { path: String },
    #[error("baseline file is not readable: {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse baseline JSON ({path}): {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "unsupported baseline schema_version: {found} (this build of crap4rs accepts {supported})"
    )]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
}

/// Load a crap4rs JSON envelope from disk and return the baseline
/// snapshot. Streams the file through a `BufReader` rather than
/// reading the whole envelope into memory — large codebases produce
/// envelopes in the multi-MB range and there's no reason to allocate
/// that twice.
pub fn load(path: &Path) -> Result<BaselineSnapshot, BaselineError> {
    let path_str = path.display().to_string();

    let file = File::open(path).map_err(|source| match source.kind() {
        ErrorKind::NotFound => BaselineError::NotFound {
            path: path_str.clone(),
        },
        _ => BaselineError::Io {
            path: path_str.clone(),
            source,
        },
    })?;

    let envelope: BaselineEnvelope =
        serde_json::from_reader(BufReader::new(file)).map_err(|source| BaselineError::Parse {
            path: path_str.clone(),
            source,
        })?;

    if envelope.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(BaselineError::UnsupportedSchemaVersion {
            found: envelope.schema_version,
            supported: SUPPORTED_SCHEMA_VERSION,
        });
    }

    Ok(BaselineSnapshot {
        result: envelope.result,
        tool_version: envelope.tool_version,
        timestamp: envelope.timestamp,
        diagnostics: envelope.diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_envelope(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("create temp file");
        write!(file, "{content}").expect("write temp file");
        file.flush().expect("flush temp file");
        file
    }

    fn minimal_envelope_json() -> &'static str {
        r#"{
            "schema_version": 1,
            "tool_version": "0.2.0",
            "language": "rust",
            "timestamp": "2026-04-26T10:00:00Z",
            "metric": "cognitive",
            "threshold": 25.0,
            "diff_ref": null,
            "result": {
                "functions": [],
                "summary": {
                    "total_functions": 0,
                    "total_files": 0,
                    "exceeding_threshold": 0,
                    "average_crap": 0.0,
                    "median_crap": 0.0,
                    "max_crap": null,
                    "worst_function": null,
                    "distribution": {
                        "low": 0,
                        "acceptable": 0,
                        "moderate": 0,
                        "high": 0
                    }
                },
                "passed": true
            }
        }"#
    }

    #[test]
    fn load_minimal_envelope_extracts_result_and_metadata() {
        let file = write_envelope(minimal_envelope_json());
        let snapshot = load(file.path()).expect("load minimal envelope");
        assert_eq!(snapshot.tool_version, "0.2.0");
        assert_eq!(snapshot.timestamp, "2026-04-26T10:00:00Z");
        assert_eq!(snapshot.result.functions.len(), 0);
        assert!(snapshot.result.passed);
        assert!(snapshot.diagnostics.is_none());
    }

    #[test]
    fn load_envelope_with_function_round_trips_verdict_fields() {
        let json = r#"{
            "schema_version": 1,
            "tool_version": "0.2.0",
            "language": "rust",
            "timestamp": "2026-04-26T10:00:00Z",
            "metric": "cognitive",
            "threshold": 25.0,
            "diff_ref": null,
            "result": {
                "functions": [
                    {
                        "scored": {
                            "identity": {
                                "file_path": "src/foo.rs",
                                "qualified_name": "foo::bar",
                                "span": { "start_line": 10, "end_line": 20 }
                            },
                            "complexity": 5,
                            "complexity_metric": "cognitive",
                            "coverage_percent": 75.0,
                            "crap": { "value": 8.0, "risk_level": "acceptable" },
                            "contributors": []
                        },
                        "threshold": 25.0,
                        "exceeds": false
                    }
                ],
                "summary": {
                    "total_functions": 1,
                    "total_files": 1,
                    "exceeding_threshold": 0,
                    "average_crap": 8.0,
                    "median_crap": 8.0,
                    "max_crap": { "value": 8.0, "risk_level": "acceptable" },
                    "worst_function": {
                        "file_path": "src/foo.rs",
                        "qualified_name": "foo::bar",
                        "span": { "start_line": 10, "end_line": 20 }
                    },
                    "distribution": { "low": 0, "acceptable": 1, "moderate": 0, "high": 0 }
                },
                "passed": true
            }
        }"#;
        let file = write_envelope(json);
        let snapshot = load(file.path()).expect("load function envelope");
        assert_eq!(snapshot.result.functions.len(), 1);
        let v = &snapshot.result.functions[0];
        assert_eq!(v.scored.identity.qualified_name, "foo::bar");
        assert_eq!(v.scored.identity.file_path, "src/foo.rs");
        assert_eq!(v.scored.crap.value, 8.0);
        assert!(!v.exceeds);
    }

    #[test]
    fn load_nonexistent_path_returns_not_found() {
        let result = load(Path::new("/tmp/definitely-does-not-exist-xyzzy.json"));
        match result {
            Err(BaselineError::NotFound { .. }) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn load_malformed_json_returns_parse_error() {
        let file = write_envelope("{ not valid JSON");
        let err = load(file.path()).unwrap_err();
        match err {
            BaselineError::Parse { .. } => {}
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn load_unsupported_schema_version_rejects() {
        let json = r#"{
            "schema_version": 2,
            "result": {
                "functions": [],
                "summary": {
                    "total_functions": 0, "total_files": 0, "exceeding_threshold": 0,
                    "average_crap": 0.0, "median_crap": 0.0,
                    "max_crap": null, "worst_function": null,
                    "distribution": { "low": 0, "acceptable": 0, "moderate": 0, "high": 0 }
                },
                "passed": true
            }
        }"#;
        let file = write_envelope(json);
        let err = load(file.path()).unwrap_err();
        match err {
            BaselineError::UnsupportedSchemaVersion {
                found: 2,
                supported: 1,
            } => {}
            other => panic!("expected UnsupportedSchemaVersion(2,1), got {other:?}"),
        }
    }

    #[test]
    fn load_envelope_propagates_diagnostics_when_present() {
        let json = r#"{
            "schema_version": 1,
            "result": {
                "functions": [],
                "summary": {
                    "total_functions": 0, "total_files": 0, "exceeding_threshold": 0,
                    "average_crap": 0.0, "median_crap": 0.0,
                    "max_crap": null, "worst_function": null,
                    "distribution": { "low": 0, "acceptable": 0, "moderate": 0, "high": 0 }
                },
                "passed": true
            },
            "diagnostics": {
                "parse_diagnostics": [],
                "files_found": 5,
                "files_unparseable": 0,
                "functions_extracted": 12,
                "functions_matched": 10,
                "functions_no_coverage": 2,
                "files_analyzed": 5,
                "files_zero_coverage": 0
            }
        }"#;
        let file = write_envelope(json);
        let snapshot = load(file.path()).expect("load envelope with diagnostics");
        let diag = snapshot.diagnostics.expect("diagnostics should be present");
        assert_eq!(diag.files_found, 5);
        assert_eq!(diag.functions_matched, 10);
    }

    #[test]
    fn load_envelope_with_extra_unknown_fields_is_forward_compatible() {
        // We don't deny_unknown_fields — future envelopes may add keys we
        // don't care about (e.g. delta-on-delta, future view shapes).
        let json = r#"{
            "schema_version": 1,
            "tool_version": "0.99.0",
            "result": {
                "functions": [],
                "summary": {
                    "total_functions": 0, "total_files": 0, "exceeding_threshold": 0,
                    "average_crap": 0.0, "median_crap": 0.0,
                    "max_crap": null, "worst_function": null,
                    "distribution": { "low": 0, "acceptable": 0, "moderate": 0, "high": 0 }
                },
                "passed": true
            },
            "future_field": { "unknown": "shape" }
        }"#;
        let file = write_envelope(json);
        let snapshot = load(file.path()).expect("forward-compat load");
        assert_eq!(snapshot.tool_version, "0.99.0");
    }
}
