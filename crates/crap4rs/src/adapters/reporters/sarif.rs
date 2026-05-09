//! SARIF v2.1.0 reporter — for GitHub Code Scanning.
//!
//! Pure formatting function. SARIF is a *gate translation*, not a
//! display: results derive from `view.full.functions.iter().filter(|v|
//! v.exceeds)` so PR annotations reflect the unshapeable gate. Filters,
//! sorts, and truncations from the View are intentionally ignored.

use serde::Serialize;

use crate::domain::types::{FunctionVerdict, RiskLevel, SourceSpan};
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

    // SARIF `result.properties` is a free-form bag; we expose the
    // `Diagnostic` shape under `properties.diagnostic` so Code Scanning
    // consumers can read the same advice they'd get under `--format
    // advice`. The shape is byte-identical because both formats route
    // through the same `Serialize` impl.
    let properties = verdict.diagnostic.as_deref().map(|diag| SarifProperties {
        diagnostic: serde_json::to_value(diag)
            .expect("Diagnostic Serialize impl is total (only owned strings, ints, vecs)"),
    });

    SarifResult {
        rule_id: RULE_ID,
        level,
        message: SarifText { text: message },
        locations: vec![SarifLocation {
            physical_location: SarifPhysicalLocation {
                artifact_location: SarifArtifactLocation {
                    uri: s.identity.file_path.clone(),
                },
                region: region_for_span(&s.identity.span),
            },
        }],
        partial_fingerprints: SarifPartialFingerprints {
            function_identity: fingerprint,
        },
        properties,
    }
}

/// Build a SARIF region from a `SourceSpan`. Columns are emitted only
/// when *both* `start_column` and `end_column` are nonzero — `0` means
/// "unknown" per the `SourceSpan` contract, and a half-known span is
/// strictly worse than no column data at all (consumers can't tell
/// which side is bogus).
///
/// SARIF v2.1.0 §3.30.7 specifies `endColumn` as "one greater than the
/// column number of the last character in the region" — i.e., exclusive
/// end. `SourceSpan::end_column` is 1-based inclusive (parallel with
/// `end_line`), so we add 1 here at the wire boundary.
fn region_for_span(span: &SourceSpan) -> SarifRegion {
    let columns_known = span.start_column > 0 && span.end_column > 0;
    SarifRegion {
        start_line: span.start_line,
        end_line: span.end_line,
        start_column: columns_known.then_some(span.start_column),
        end_column: columns_known.then_some(span.end_column + 1),
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
    /// SARIF v2.1.0 §3.27.6 — `properties` is a free-form extension
    /// bag. Omitted when no diagnostic so SARIF stays byte-identical
    /// for non-advice runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<SarifProperties>,
}

#[derive(Serialize)]
struct SarifProperties {
    diagnostic: serde_json::Value,
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
    #[serde(rename = "startColumn", skip_serializing_if = "Option::is_none")]
    start_column: Option<usize>,
    #[serde(rename = "endColumn", skip_serializing_if = "Option::is_none")]
    end_column: Option<usize>,
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

    #[test]
    fn schema_uri_and_top_level_shape() {
        let result = make_multi_function_result();
        let view = make_view_default(&result);
        let v = parse(&format_sarif(&view, "0.2.2"));
        assert_eq!(
            v["$schema"],
            "https://json.schemastore.org/sarif-2.1.0.json"
        );
        assert_eq!(v["version"], "2.1.0");
        assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "crap4rs");
        assert_eq!(v["runs"][0]["tool"]["driver"]["version"], "0.2.2");
    }

    #[test]
    fn rule_definition_present_on_every_run() {
        // The rule must be defined on the run regardless of whether any
        // result references it, so consumers can introspect the schema.
        let empty = make_empty_result();
        let v = parse(&format_sarif(&make_view_default(&empty), "0.2.2"));
        let rules = v["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], "crap/threshold-exceeded");
        assert_eq!(rules[0]["name"], "ThresholdExceeded");
    }

    #[test]
    fn full_sarif_snapshot() {
        let result = make_multi_function_result();
        let view = make_view_default(&result);
        let out = format_sarif(&view, "0.2.2");
        insta::assert_snapshot!(out);
    }

    /// Build a verdict whose span carries known nonzero columns. Used to
    /// drive the conditional `region.startColumn` / `endColumn` emission.
    fn verdict_with_columns(start_column: usize, end_column: usize) -> FunctionVerdict {
        let mut v = make_verdict(
            "complex_fn",
            "src/lib.rs",
            10,
            0.0,
            30.0,
            RiskLevel::High,
            8.0,
        );
        v.scored.identity.span.start_column = start_column;
        v.scored.identity.span.end_column = end_column;
        v
    }

    fn result_with_single(verdict: FunctionVerdict) -> crate::domain::types::AnalysisResult {
        use crate::domain::types::{AnalysisResult, AnalysisSummary};
        AnalysisResult {
            functions: vec![verdict],
            summary: AnalysisSummary {
                total_functions: 1,
                ..Default::default()
            },
            passed: false,
        }
    }

    #[test]
    fn region_emits_columns_when_both_nonzero() {
        // `SourceSpan` columns are 1-based inclusive; SARIF endColumn is
        // 1-based exclusive (one greater than the last character). The
        // reporter adds 1 at the wire boundary, so a span ending at
        // column 32 inclusive becomes SARIF endColumn 33.
        let result = result_with_single(verdict_with_columns(5, 32));
        let view = make_view_default(&result);
        let v = parse(&format_sarif(&view, "test-version"));
        let region = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(
            region["startColumn"], 5,
            "startColumn passes through (1-based inclusive == SARIF 1-based inclusive)"
        );
        assert_eq!(
            region["endColumn"], 33,
            "endColumn = span.end_column + 1 per SARIF v2.1.0 §3.30.7"
        );
    }

    #[test]
    fn region_omits_columns_when_both_zero() {
        // Default fixtures emit columns of 0 — meaning "unknown". SARIF
        // must not surface meaningless columns to GitHub Code Scanning.
        let result = result_with_single(verdict_with_columns(0, 0));
        let view = make_view_default(&result);
        let v = parse(&format_sarif(&view, "test-version"));
        let region = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert!(
            region.get("startColumn").is_none(),
            "startColumn key must be absent when span column is 0"
        );
        assert!(
            region.get("endColumn").is_none(),
            "endColumn key must be absent when span column is 0"
        );
    }

    #[test]
    fn region_omits_columns_when_only_one_known() {
        // A half-known span (start known, end unknown or vice-versa) is
        // worse than no columns at all — emit neither so consumers see
        // "no column info" instead of a half-truth.
        for (sc, ec) in [(5usize, 0usize), (0, 32)] {
            let result = result_with_single(verdict_with_columns(sc, ec));
            let view = make_view_default(&result);
            let v = parse(&format_sarif(&view, "test-version"));
            let region = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
            assert!(
                region.get("startColumn").is_none() && region.get("endColumn").is_none(),
                "half-known span ({sc}, {ec}) must omit both column keys"
            );
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::make_view_default;
    use crate::test_strategies::arb_analysis_result;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_format_sarif_always_valid_json(result in arb_analysis_result()) {
            let view = make_view_default(&result);
            let out = format_sarif(&view, "0.2.2");
            let _: serde_json::Value = serde_json::from_str(&out)
                .expect("format_sarif must produce parseable JSON");
        }

        #[test]
        fn prop_sarif_results_count_matches_exceeders(result in arb_analysis_result()) {
            let view = make_view_default(&result);
            let out = format_sarif(&view, "0.2.2");
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            let results = v["runs"][0]["results"].as_array().unwrap();
            let expected = result.functions.iter().filter(|fv| fv.exceeds).count();
            prop_assert_eq!(results.len(), expected);
        }

        #[test]
        fn prop_every_result_has_mandatory_sarif_fields(result in arb_analysis_result()) {
            let view = make_view_default(&result);
            let out = format_sarif(&view, "0.2.2");
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            for r in v["runs"][0]["results"].as_array().unwrap() {
                prop_assert!(r["ruleId"].is_string());
                prop_assert!(r["level"].is_string());
                prop_assert!(r["message"]["text"].is_string());
                prop_assert!(r["locations"].is_array());
                prop_assert!(r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"].is_string());
                prop_assert!(r["locations"][0]["physicalLocation"]["region"]["startLine"].is_u64());
                prop_assert!(r["locations"][0]["physicalLocation"]["region"]["endLine"].is_u64());
                prop_assert!(r["partialFingerprints"]["functionIdentity"].is_string());
            }
        }

        #[test]
        fn prop_severity_is_one_of_three_values(result in arb_analysis_result()) {
            let view = make_view_default(&result);
            let out = format_sarif(&view, "0.2.2");
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            for r in v["runs"][0]["results"].as_array().unwrap() {
                let level = r["level"].as_str().unwrap();
                prop_assert!(
                    matches!(level, "error" | "warning" | "note"),
                    "unexpected level: {}", level
                );
            }
        }
    }
}
