//! Istanbul JSON coverage parser — minimal statement-only implementation.
//!
//! Consumes the per-file `coverage-final.json` map jest, vitest, and
//! nyc emit. Each entry maps a `path` to a `statementMap` (statement
//! id → `{start: {line, column}, end: {line, column}}`) and an `s`
//! hash (statement id → exec count). The parser joins these two maps
//! to produce one `LineCoverage { line, hits }` record per statement
//! in `statementMap`, keyed by the entry's path (after normalization
//! against `effective_src` so the downstream line-range join sees
//! workspace-relative paths).
//!
//! ## Scope (W1.1)
//!
//! - Statement coverage (`s` + `statementMap`) only. Branch coverage
//!   (`b` + `branchMap`) lands in W2.3 — the corresponding struct
//!   fields are declared with `#[serde(default)]` so the W2.3 PR is
//!   purely consumer-side (no type-surface churn).
//! - Single emitter shape: minimal jest-flavored Istanbul.
//!   vitest / nyc + extension dispatch land in W2.4.
//! - Single source-type dispatch happens upstream in the walker
//!   (W1.2); this parser is metric- and language-agnostic.
//!
//! ## Path normalization
//!
//! Mirrors `crap4rs::adapters::coverage::LcovParser::normalize_path` —
//! a pure `strip_prefix(effective_src)`. The orchestrator pre-
//! canonicalizes `effective_src` at the factory-closure boundary, so
//! the parser does no filesystem I/O during path joining. Entries
//! whose paths fall outside `effective_src` emit an
//! `IstanbulParseDiagnostic { kind: PathUnresolved, … }` and are
//! skipped — per Resolved Q10 in shaping, the scorecard NEVER aborts
//! on first failure and NEVER silent-drops a record.

use crap_core::domain::types::{CrapError, LineCoverage};
use crap_core::ports::{CoveragePort, ParseOutput};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::parse_diagnostic::{IstanbulDiagnosticKind, IstanbulParseDiagnostic};

/// Istanbul JSON coverage parser.
///
/// Constructed via [`IstanbulCoverage::new`] with the post-merge,
/// pre-canonicalized source root. Implements `CoveragePort` with
/// `Diagnostic = IstanbulParseDiagnostic` so the binary's
/// `crap_core::cli::run` dispatch picks it up via the factory closure
/// in `src/main.rs`.
pub struct IstanbulCoverage {
    /// Canonical source root used to normalize per-entry `path`
    /// fields. The orchestrator's `canonicalize_src` call in
    /// `crap_core::cli::run` hands us an already-canonical path, so
    /// the parser does no further `canonicalize()` walking inside
    /// `normalize_path`.
    effective_src: PathBuf,
}

impl IstanbulCoverage {
    /// Construct a new parser rooted at the post-merge `--src` value.
    ///
    /// Callers in `crap_core::cli::run` pass the already-canonicalized
    /// effective source root; smoke tests should canonicalize the
    /// temp-dir root before passing it through so fixture path
    /// stripping matches.
    pub fn new(root: PathBuf) -> Self {
        Self {
            effective_src: root,
        }
    }

    /// Strip `effective_src` from `raw` and return a workspace-relative
    /// path, or `None` if the entry resolves outside the source tree.
    ///
    /// Pure: no filesystem I/O. Mirrors `LcovParser::normalize_path`
    /// in `crap4rs`. The orchestrator canonicalizes `effective_src`
    /// before construction, so no further `canonicalize()` walk is
    /// needed here. Joining relative entries against `effective_src`
    /// and stripping the prefix is the same two-step the LCOV parser
    /// uses on its own platform.
    fn normalize_path(&self, raw: &str) -> Option<PathBuf> {
        let path = PathBuf::from(raw);
        let joined = if path.is_absolute() {
            path
        } else {
            self.effective_src.join(raw)
        };
        joined
            .strip_prefix(&self.effective_src)
            .ok()
            .map(|p| p.to_path_buf())
    }
}

// ── Internal Istanbul schema types ───────────────────────────────────
//
// All types are private (parser-internal). They model the minimal
// jest-flavored shape W1.1 consumes; `#[serde(default)]` on optional
// fields keeps deserialization permissive against jest/vitest/nyc
// metadata fields we don't care about (`hash`, `contentHash`, `all`,
// etc.) — serde drops unknown fields by default.

/// One per-file entry in `coverage-final.json`. Field types are widened
/// to `u64` (vs the breadboard's `u32`) to handle high-iteration code
/// — Istanbul's `s` counts can comfortably exceed `u32::MAX` on stress
/// tests, and `LineCoverage.hits` is `u64` downstream anyway.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IstanbulCoverageFile {
    path: String,
    /// `statement_id` → execution count.
    #[serde(default)]
    s: HashMap<String, u64>,
    /// `statement_id` → `{ start: {line, column}, end: {line, column} }`.
    #[serde(default)]
    statement_map: HashMap<String, StatementLoc>,
    /// W2.3: `branch_id` → `[hit count per branch arm]`. Declared with
    /// `#[serde(default)]` now so the W2.3 consumer-side PR introduces
    /// no type surface churn; ignored in W1.1.
    #[serde(default)]
    #[allow(dead_code)]
    b: HashMap<String, Vec<u64>>,
    /// W2.3: `branch_id` → branch location. See `b` above.
    #[serde(default)]
    #[allow(dead_code)]
    branch_map: HashMap<String, BranchLoc>,
    /// W2.3 fallback: `function_id` → execution count. Declared now so
    /// W2.3's `fnMap` backfill (if arrow-function undercount surfaces)
    /// is purely consumer-side.
    #[serde(default)]
    #[allow(dead_code)]
    f: HashMap<String, u64>,
    /// W2.3 fallback: `function_id` → function metadata.
    #[serde(default)]
    #[allow(dead_code)]
    fn_map: HashMap<String, FnLoc>,
}

/// Range location for a statement (or function decl/body). `start`
/// and `end` carry 1-based line numbers + 0-based columns.
#[derive(Debug, Deserialize)]
struct StatementLoc {
    start: Position,
    #[allow(dead_code)]
    end: Position,
}

/// 1-based line + 0-based column. Matches Istanbul's emitter
/// convention; downstream `LineCoverage.line` is 1-based `usize`.
#[derive(Debug, Deserialize)]
struct Position {
    line: u32,
    #[allow(dead_code)]
    column: u32,
}

/// W2.3: branch metadata. Declared minimal here so the type lands
/// with the rest of the schema; `r#type` carries the kebab-cased
/// Istanbul branch kind (`if`, `switch`, `cond-expr`, etc.).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct BranchLoc {
    loc: StatementLoc,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    locations: Vec<StatementLoc>,
    #[serde(default)]
    line: Option<u32>,
}

/// W2.3 fallback: function metadata. Declared minimal here for the
/// same reason as `BranchLoc`; `name` + `decl` + `loc` + `line` is the
/// minimum surface needed if `f`/`fnMap` later backfills arrow
/// coverage that `s`/`statementMap` undercounts.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FnLoc {
    name: String,
    decl: StatementLoc,
    loc: StatementLoc,
    #[serde(default)]
    line: Option<u32>,
}

impl CoveragePort for IstanbulCoverage {
    type Diagnostic = IstanbulParseDiagnostic;

    /// Parse a `coverage-final.json` payload into per-file
    /// `LineCoverage` records.
    ///
    /// Top-level shape errors return `Err(CrapError::SourceParse(
    /// "istanbul: …"))` — these are fatal because there is no
    /// per-entry recovery path for "the whole document didn't parse."
    /// Per-entry errors (path unresolved, missing field) push an
    /// `IstanbulParseDiagnostic` and skip the entry; the scorecard
    /// still produces results for the other entries. Branch
    /// coverage parsing (`branches: Some(...)`) lands in W2.3 — for
    /// W1.1 the field is always `None`.
    fn parse(&self, data: &str) -> Result<ParseOutput<IstanbulParseDiagnostic>, CrapError> {
        let raw: HashMap<String, IstanbulCoverageFile> = serde_json::from_str(data)
            .map_err(|e| CrapError::SourceParse(format!("istanbul: {e}")))?;

        let mut coverage: HashMap<String, Vec<LineCoverage>> = HashMap::new();
        let mut diagnostics: Vec<IstanbulParseDiagnostic> = Vec::new();

        for (_key, entry) in raw {
            let Some(normalized) = self.normalize_path(&entry.path) else {
                diagnostics.push(IstanbulParseDiagnostic {
                    file_path: entry.path.clone(),
                    kind: IstanbulDiagnosticKind::PathUnresolved,
                    message: format!(
                        "path '{}' does not resolve to a discovered source file under {}",
                        entry.path,
                        self.effective_src.display()
                    ),
                    line: None,
                });
                continue;
            };

            // Build per-line records by joining `s` (counts) to
            // `statementMap` (line spans). Statements whose IDs do not
            // appear in `statementMap` are skipped; statements that
            // span multiple lines emit one `LineCoverage` per line in
            // the span keyed by `start.line` (Istanbul records hits at
            // the start-of-statement granularity, per its emitter).
            let mut lines: Vec<LineCoverage> = Vec::with_capacity(entry.s.len());
            for (stmt_id, hits) in &entry.s {
                if let Some(loc) = entry.statement_map.get(stmt_id) {
                    lines.push(LineCoverage {
                        line: loc.start.line as usize,
                        hits: *hits,
                    });
                }
            }
            // Sort by line for deterministic downstream consumption
            // (mirrors LCOV parser ordering).
            lines.sort_by_key(|lc| lc.line);

            let key = normalized.to_string_lossy().into_owned();
            coverage.insert(key, lines);
        }

        Ok(ParseOutput {
            coverage,
            branches: None,
            diagnostics,
        })
    }

    /// Pre-flight structural check: parse the file as Istanbul JSON and
    /// require at least one entry with a non-empty `statementMap`.
    ///
    /// Returns `Err("not a recognizable Istanbul JSON shape: …")` when
    /// `serde_json::from_str` cannot deserialize the top-level
    /// `HashMap<String, IstanbulCoverageFile>` (drives the
    /// `schema-unrecognized` user-facing error). Returns
    /// `Err("no statement coverage records")` when every entry has an
    /// empty `statementMap` (drives the "regenerate coverage" hint).
    /// CLI layer surfaces these via `AdapterMeta::coverage_hint`.
    fn validate(&self, path: &Path) -> Result<(), String> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let raw: HashMap<String, IstanbulCoverageFile> = serde_json::from_str(&data)
            .map_err(|e| format!("not a recognizable Istanbul JSON shape: {e}"))?;
        if raw.values().all(|f| f.statement_map.is_empty()) {
            return Err("no statement coverage records".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn istanbul_coverage_constructible() {
        let _c = IstanbulCoverage::new(PathBuf::from("/tmp"));
    }

    #[test]
    fn parse_empty_object_returns_ok_with_no_coverage() {
        let c = IstanbulCoverage::new(PathBuf::from("/tmp"));
        let out = c.parse("{}").expect("empty JSON object parses cleanly");
        assert!(out.coverage.is_empty());
        assert!(out.diagnostics.is_empty());
        assert!(out.branches.is_none());
    }

    #[test]
    fn parse_malformed_json_returns_source_parse_with_istanbul_prefix() {
        let c = IstanbulCoverage::new(PathBuf::from("/tmp"));
        let err = c.parse("{not json").unwrap_err();
        match err {
            CrapError::SourceParse(msg) => {
                assert!(msg.starts_with("istanbul: "), "msg: {msg}");
            }
            other => panic!("expected SourceParse, got {other:?}"),
        }
    }
}
