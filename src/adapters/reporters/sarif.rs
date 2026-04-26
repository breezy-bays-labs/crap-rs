//! SARIF v2.1.0 reporter — for GitHub Code Scanning.
//!
//! Pure formatting function. SARIF is a *gate translation*, not a
//! display: results derive from `view.full.functions.iter().filter(|v|
//! v.exceeds)` so PR annotations reflect the unshapeable gate. Filters,
//! sorts, and truncations from the View are intentionally ignored.

use serde::Serialize;

use crate::domain::types::{FunctionVerdict, RiskLevel};
use crate::domain::view::AnalysisView;

const SCHEMA_URI: &str = "https://json.schemastore.org/sarif-2.1.0.json";
const RULE_ID: &str = "crap/threshold-exceeded";
const TOOL_NAME: &str = "crap4rs";
const TOOL_INFO_URI: &str = "https://github.com/breezy-bays-labs/crap4rs";
const RULE_HELP_URI: &str = "https://github.com/breezy-bays-labs/crap4rs#crap-formula";

/// Format an `AnalysisView` as SARIF v2.1.0 JSON.
///
/// One SARIF `result` per `FunctionVerdict` whose `exceeds == true`,
/// iterating the *full* analysis (not the shaped slice). `tool_version`
/// is threaded through to `runs[0].tool.driver.version`.
pub fn format_sarif(view: &AnalysisView<'_>, tool_version: &str) -> String {
    let results: Vec<SarifResult> = view
        .full
        .functions
        .iter()
        .filter(|v| v.exceeds)
        .map(result_for)
        .collect();

    let log = SarifLog {
        schema: SCHEMA_URI,
        version: "2.1.0",
        runs: vec![SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: TOOL_NAME,
                    version: tool_version.to_string(),
                    information_uri: TOOL_INFO_URI,
                    rules: vec![rule()],
                },
            },
            results,
        }],
    };

    serde_json::to_string_pretty(&log)
        .expect("SARIF serialization is infallible — all fields are owned strings or numbers")
}

fn rule() -> SarifRule {
    SarifRule {
        id: RULE_ID,
        name: "ThresholdExceeded",
        short_description: SarifText {
            text: "Function CRAP score exceeds the configured threshold.",
        },
        full_description: SarifText {
            text: "Functions whose CRAP score (complexity * complexity * (1 - coverage)^3 + complexity) \
                   exceeds the threshold are change-risk hot spots: cover them first, then extract \
                   sub-functions if complexity remains the driver.",
        },
        help_uri: RULE_HELP_URI,
    }
}

fn result_for(verdict: &FunctionVerdict) -> SarifResult {
    let s = &verdict.scored;
    let level = severity_for(s.crap.risk_level);
    let message = format!(
        "Function `{}` has CRAP {:.2} (complexity={}, coverage={:.1}%) which exceeds threshold {:.1}",
        s.identity.qualified_name,
        s.crap.value,
        s.complexity,
        s.coverage_percent,
        verdict.threshold,
    );
    let fingerprint = format!("{}:{}", s.identity.file_path, s.identity.qualified_name);

    SarifResult {
        rule_id: RULE_ID,
        level,
        message: SarifText { text: message },
        locations: vec![SarifLocation {
            physical_location: SarifPhysicalLocation {
                artifact_location: SarifArtifactLocation {
                    uri: s.identity.file_path.clone(),
                },
                region: SarifRegion {
                    start_line: s.identity.span.start_line,
                    end_line: s.identity.span.end_line,
                },
            },
        }],
        partial_fingerprints: SarifPartialFingerprints {
            function_identity: fingerprint,
        },
    }
}

fn severity_for(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::High => "error",
        RiskLevel::Moderate => "warning",
        RiskLevel::Acceptable | RiskLevel::Low => "note",
    }
}

// ── SARIF v2.1.0 envelope structs ───────────────────────────────────────
//
// Internal-only: serialization shape is not part of any public API. The
// public contract is the JSON schema (sarif-2.1.0.json), not these
// structs. `text` and message strings hold borrowed `&'static str` where
// possible; only per-result fields own their data.

#[derive(Serialize)]
struct SarifText<S: Serialize> {
    text: S,
}

#[derive(Serialize)]
struct SarifLog {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<SarifRun>,
}

#[derive(Serialize)]
struct SarifRun {
    tool: SarifTool,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Serialize)]
struct SarifDriver {
    name: &'static str,
    version: String,
    #[serde(rename = "informationUri")]
    information_uri: &'static str,
    rules: Vec<SarifRule>,
}

#[derive(Serialize)]
struct SarifRule {
    id: &'static str,
    name: &'static str,
    #[serde(rename = "shortDescription")]
    short_description: SarifText<&'static str>,
    #[serde(rename = "fullDescription")]
    full_description: SarifText<&'static str>,
    #[serde(rename = "helpUri")]
    help_uri: &'static str,
}

#[derive(Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: &'static str,
    level: &'static str,
    message: SarifText<String>,
    locations: Vec<SarifLocation>,
    #[serde(rename = "partialFingerprints")]
    partial_fingerprints: SarifPartialFingerprints,
}

#[derive(Serialize)]
struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
}

#[derive(Serialize)]
struct SarifPhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: SarifArtifactLocation,
    region: SarifRegion,
}

#[derive(Serialize)]
struct SarifArtifactLocation {
    uri: String,
}

#[derive(Serialize)]
struct SarifRegion {
    #[serde(rename = "startLine")]
    start_line: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
}

#[derive(Serialize)]
struct SarifPartialFingerprints {
    #[serde(rename = "functionIdentity")]
    function_identity: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;
    use crate::domain::types::RiskLevel;

    fn parse(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("format_sarif must produce valid JSON")
    }

    #[test]
    fn empty_result_produces_empty_results_array() {
        let result = make_empty_result();
        let view = make_view_default(&result);
        let v = parse(&format_sarif(&view, "test-version"));
        assert_eq!(v["version"], "2.1.0");
        assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn single_exceeder_produces_one_result() {
        let result = make_single_function_result(
            "complex_fn",
            "src/lib.rs",
            10,
            30.0,
            30.0,
            RiskLevel::High,
            8.0,
        );
        let view = make_view_default(&result);
        let v = parse(&format_sarif(&view, "test-version"));
        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["ruleId"], "crap/threshold-exceeded");
        assert_eq!(results[0]["level"], "error");
    }

    #[test]
    fn severity_mapping_covers_all_risk_levels() {
        // Build four verdicts, one per RiskLevel, each exceeding threshold.
        // The severity mapping is the contract:
        //   High → error, Moderate → warning, Acceptable → note, Low → note.
        // (Low normally doesn't exceed in production, but the mapping must
        // still be defined defensively.)
        use crate::domain::types::{AnalysisResult, AnalysisSummary};
        let v_high = make_verdict("h", "src/h.rs", 10, 0.0, 30.0, RiskLevel::High, 8.0);
        let v_mod = make_verdict("m", "src/m.rs", 6, 50.0, 15.0, RiskLevel::Moderate, 8.0);
        let v_acc = make_verdict("a", "src/a.rs", 3, 70.0, 9.0, RiskLevel::Acceptable, 8.0);
        let v_low = make_verdict("l", "src/l.rs", 2, 95.0, 8.5, RiskLevel::Low, 8.0);
        let result = AnalysisResult {
            functions: vec![v_high, v_mod, v_acc, v_low],
            summary: AnalysisSummary {
                total_functions: 4,
                ..Default::default()
            },
            passed: false,
        };
        let view = make_view_default(&result);
        let v = parse(&format_sarif(&view, "test-version"));
        let results = v["runs"][0]["results"].as_array().unwrap();
        let levels: Vec<&str> = results
            .iter()
            .map(|r| r["level"].as_str().unwrap())
            .collect();
        assert_eq!(levels, vec!["error", "warning", "note", "note"]);
    }

    #[test]
    fn partial_fingerprints_use_file_and_qualified_name() {
        let result = make_single_function_result(
            "MyType::method",
            "src/lib.rs",
            10,
            0.0,
            30.0,
            RiskLevel::High,
            8.0,
        );
        let view = make_view_default(&result);
        let v = parse(&format_sarif(&view, "test-version"));
        let r0 = &v["runs"][0]["results"][0];
        assert_eq!(
            r0["partialFingerprints"]["functionIdentity"],
            "src/lib.rs:MyType::method"
        );
    }
}
