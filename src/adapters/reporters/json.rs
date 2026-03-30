//! JSON reporter — formats `AnalysisResult` as structured JSON
//! with a versioned envelope for CI pipelines and tooling consumption.

use crate::domain::types::{AnalysisDiagnostics, AnalysisResult, ComplexityMetric};
use serde::Serialize;

/// Configuration for the JSON envelope metadata.
#[derive(Debug)]
pub struct JsonConfig<'a> {
    pub tool_version: String,
    pub metric: ComplexityMetric,
    pub threshold: f64,
    pub timestamp: String,
    /// When present, diagnostics are included in the JSON output (--verbose).
    pub diagnostics: Option<&'a AnalysisDiagnostics>,
    /// Git ref used for diff filtering (`--diff <ref>`). `None` when not in diff mode.
    pub diff_ref: Option<&'a str>,
}

#[derive(Serialize)]
struct JsonEnvelope<'a> {
    schema_version: u32,
    tool_version: &'a str,
    language: &'static str,
    timestamp: &'a str,
    metric: &'a ComplexityMetric,
    threshold: f64,
    diff_ref: Option<&'a str>,
    result: &'a AnalysisResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a AnalysisDiagnostics>,
}

// Note: per-function thresholds are already visible in each FunctionVerdict's
// `threshold` field. Consumers can compare individual function thresholds
// against the envelope's global `threshold` to detect overrides.

/// Format an analysis result as pretty-printed JSON with a versioned envelope.
pub fn format_json(
    result: &AnalysisResult,
    config: &JsonConfig<'_>,
) -> Result<String, serde_json::Error> {
    let envelope = JsonEnvelope {
        schema_version: 1,
        tool_version: &config.tool_version,
        language: "rust",
        timestamp: &config.timestamp,
        metric: &config.metric,
        threshold: config.threshold,
        diff_ref: config.diff_ref,
        result,
        diagnostics: config.diagnostics,
    };
    serde_json::to_string_pretty(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;
    use crate::domain::types::{ComplexityMetric, RiskLevel};

    fn default_config() -> JsonConfig<'static> {
        JsonConfig {
            tool_version: "0.1.0".to_string(),
            metric: ComplexityMetric::Cognitive,
            threshold: 8.0,
            timestamp: "2026-03-28T12:00:00Z".to_string(),
            diagnostics: None,
            diff_ref: None,
        }
    }

    fn parse_json(result: &AnalysisResult, config: &JsonConfig) -> serde_json::Value {
        let json_str = format_json(result, config).expect("format_json should succeed");
        serde_json::from_str(&json_str).expect("output should be valid JSON")
    }

    #[test]
    fn test_envelope_contains_all_fields() {
        let result = make_empty_result();
        let v = parse_json(&result, &default_config());

        assert!(v.get("schema_version").is_some());
        assert!(v.get("tool_version").is_some());
        assert!(v.get("language").is_some());
        assert!(v.get("timestamp").is_some());
        assert!(v.get("metric").is_some());
        assert!(v.get("threshold").is_some());
        assert!(v.get("result").is_some());
    }

    #[test]
    fn test_result_nested_correctly() {
        let result = make_empty_result();
        let v = parse_json(&result, &default_config());
        let r = v.get("result").expect("should have result key");

        assert!(r.get("functions").is_some());
        assert!(r.get("summary").is_some());
        assert!(r.get("passed").is_some());
    }

    #[test]
    fn test_schema_version_is_integer() {
        let result = make_empty_result();
        let v = parse_json(&result, &default_config());
        let sv = v.get("schema_version").unwrap();
        assert!(sv.is_number());
        assert_eq!(sv.as_u64(), Some(1));
    }

    #[test]
    fn test_function_all_scored_fields() {
        let result = make_single_function_result(
            "compute_crap",
            "src/domain/crap.rs",
            5,
            80.0,
            5.16,
            RiskLevel::Acceptable,
            8.0,
        );
        let v = parse_json(&result, &default_config());
        let func = &v["result"]["functions"][0];

        // identity
        assert_eq!(func["scored"]["identity"]["qualified_name"], "compute_crap");
        assert_eq!(
            func["scored"]["identity"]["file_path"],
            "src/domain/crap.rs"
        );
        // complexity
        assert_eq!(func["scored"]["complexity"], 5);
        // coverage
        assert_eq!(func["scored"]["coverage_percent"], 80.0);
        // crap
        assert_eq!(func["scored"]["crap"]["value"], 5.16);
        assert_eq!(func["scored"]["crap"]["risk_level"], "acceptable");
        // threshold fields
        assert_eq!(func["exceeds"], false);
        assert_eq!(func["threshold"], 8.0);
    }

    #[test]
    fn test_summary_aggregate_stats() {
        let result = make_multi_function_result();
        let v = parse_json(&result, &default_config());
        let s = &v["result"]["summary"];

        assert_eq!(s["total_functions"], 3);
        assert_eq!(s["exceeding_threshold"], 2);
        assert!(s["average_crap"].is_number());
        assert!(s["median_crap"].is_number());
    }

    #[test]
    fn test_summary_distribution() {
        let result = make_multi_function_result();
        let v = parse_json(&result, &default_config());
        let d = &v["result"]["summary"]["distribution"];

        assert_eq!(d["low"], 1);
        assert_eq!(d["acceptable"], 0);
        assert_eq!(d["moderate"], 1);
        assert_eq!(d["high"], 1);
    }

    #[test]
    fn test_passed_true() {
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let v = parse_json(&result, &default_config());
        assert_eq!(v["result"]["passed"], true);
    }

    #[test]
    fn test_passed_false() {
        let result = make_multi_function_result();
        let v = parse_json(&result, &default_config());
        assert_eq!(v["result"]["passed"], false);
    }

    #[test]
    fn test_empty_valid_json() {
        let result = make_empty_result();
        let v = parse_json(&result, &default_config());

        let funcs = v["result"]["functions"].as_array().unwrap();
        assert!(funcs.is_empty());
        assert_eq!(v["result"]["summary"]["total_functions"], 0);
        assert_eq!(v["result"]["passed"], true);
    }

    #[test]
    fn test_metric_from_config() {
        let result = make_empty_result();
        let config = JsonConfig {
            metric: ComplexityMetric::Cognitive,
            ..default_config()
        };
        let v = parse_json(&result, &config);
        assert_eq!(v["metric"], "cognitive");

        let config2 = JsonConfig {
            metric: ComplexityMetric::Cyclomatic,
            ..default_config()
        };
        let v2 = parse_json(&result, &config2);
        assert_eq!(v2["metric"], "cyclomatic");
    }

    #[test]
    fn test_threshold_from_config() {
        let result = make_empty_result();
        let config = JsonConfig {
            threshold: 12.5,
            ..default_config()
        };
        let v = parse_json(&result, &config);
        assert_eq!(v["threshold"], 12.5);
    }

    #[test]
    fn test_timestamp_passthrough() {
        let result = make_empty_result();
        let config = JsonConfig {
            timestamp: "2026-01-15T09:30:00Z".to_string(),
            ..default_config()
        };
        let v = parse_json(&result, &config);
        assert_eq!(v["timestamp"], "2026-01-15T09:30:00Z");
    }

    #[test]
    fn test_full_json_snapshot() {
        let result = make_single_function_result(
            "compute_crap",
            "src/domain/crap.rs",
            5,
            80.0,
            5.16,
            RiskLevel::Acceptable,
            8.0,
        );
        let json_str = format_json(&result, &default_config()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        insta::assert_json_snapshot!(v);
    }

    #[test]
    fn test_diff_ref_present_in_json() {
        let result = make_empty_result();
        let config = JsonConfig {
            diff_ref: Some("main"),
            ..default_config()
        };
        let v = parse_json(&result, &config);
        assert_eq!(v["diff_ref"], "main");
    }

    #[test]
    fn test_diff_ref_null_when_none() {
        let result = make_empty_result();
        let v = parse_json(&result, &default_config());
        assert!(
            v.get("diff_ref").is_some(),
            "diff_ref key should be present"
        );
        assert!(v["diff_ref"].is_null(), "diff_ref should be null");
    }

    #[test]
    fn test_diagnostics_omitted_when_none() {
        let result = make_empty_result();
        let v = parse_json(&result, &default_config());
        assert!(
            v.get("diagnostics").is_none(),
            "diagnostics should be absent without --verbose"
        );
    }

    #[test]
    fn test_diagnostics_included_when_present() {
        use crate::domain::types::{AnalysisDiagnostics, ParseDiagnostic};

        let diag = AnalysisDiagnostics {
            parse_diagnostics: vec![ParseDiagnostic::MalformedRecord {
                line_number: 5,
                content: "DA:bad".to_string(),
            }],
            files_found: 10,
            files_unparseable: 1,
            functions_extracted: 42,
            functions_matched: 40,
            functions_no_coverage: 2,
            files_analyzed: 8,
            files_zero_coverage: 2,
        };

        let result = make_empty_result();
        let config = JsonConfig {
            diagnostics: Some(&diag),
            ..default_config()
        };
        let v = parse_json(&result, &config);

        let d = v.get("diagnostics").expect("should have diagnostics key");
        assert_eq!(d["files_found"], 10);
        assert_eq!(d["files_unparseable"], 1);
        assert_eq!(d["functions_extracted"], 42);
        assert_eq!(d["functions_matched"], 40);
        assert_eq!(d["functions_no_coverage"], 2);

        let parse_diags = d["parse_diagnostics"].as_array().unwrap();
        assert_eq!(parse_diags.len(), 1);
        assert_eq!(parse_diags[0]["kind"], "malformed_record");
        assert_eq!(parse_diags[0]["line_number"], 5);
        assert_eq!(parse_diags[0]["content"], "DA:bad");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::domain::types::{
        AnalysisSummary, CrapScore, FunctionIdentity, FunctionVerdict, RiskDistribution, RiskLevel,
        ScoredFunction, SourceSpan,
    };
    use proptest::prelude::*;

    fn arb_risk_level() -> impl Strategy<Value = RiskLevel> {
        prop_oneof![
            Just(RiskLevel::Low),
            Just(RiskLevel::Acceptable),
            Just(RiskLevel::Moderate),
            Just(RiskLevel::High),
        ]
    }

    fn arb_verdict() -> impl Strategy<Value = FunctionVerdict> {
        (
            "[a-z_]{1,20}",
            "src/[a-z/]{1,30}\\.rs",
            1..100u32,
            0.0..=100.0f64,
            1.0..200.0f64,
            arb_risk_level(),
            1.0..100.0f64,
        )
            .prop_map(
                |(name, file, complexity, coverage, crap_value, risk, threshold)| FunctionVerdict {
                    scored: ScoredFunction {
                        identity: FunctionIdentity {
                            file_path: file,
                            qualified_name: name,
                            span: SourceSpan {
                                start_line: 1,
                                end_line: 10,
                            },
                        },
                        complexity,
                        complexity_metric: ComplexityMetric::Cognitive,
                        coverage_percent: coverage,
                        crap: CrapScore {
                            value: crap_value,
                            risk_level: risk,
                        },
                        contributors: vec![],
                    },
                    threshold,
                    exceeds: crap_value > threshold,
                },
            )
    }

    /// Build an AnalysisResult with a hand-constructed summary.
    /// Summary values are structurally valid but not semantically precise —
    /// reporters only format what they receive, so accuracy doesn't matter.
    fn arb_analysis_result() -> impl Strategy<Value = crate::domain::types::AnalysisResult> {
        prop::collection::vec(arb_verdict(), 0..10).prop_map(|verdicts| {
            let total = verdicts.len();
            let exceeding = verdicts.iter().filter(|v| v.exceeds).count();
            let passed = exceeding == 0;
            let max_crap = verdicts
                .iter()
                .max_by(|a, b| {
                    a.scored
                        .crap
                        .value
                        .partial_cmp(&b.scored.crap.value)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|v| v.scored.crap);
            let avg = if total > 0 {
                verdicts.iter().map(|v| v.scored.crap.value).sum::<f64>() / total as f64
            } else {
                0.0
            };
            crate::domain::types::AnalysisResult {
                functions: verdicts,
                summary: AnalysisSummary {
                    total_functions: total,
                    total_files: total,
                    exceeding_threshold: exceeding,
                    average_crap: avg,
                    median_crap: avg,
                    max_crap,
                    worst_function: None,
                    distribution: RiskDistribution {
                        low: 0,
                        acceptable: 0,
                        moderate: 0,
                        high: 0,
                    },
                },
                passed,
            }
        })
    }

    fn arb_config() -> impl Strategy<Value = JsonConfig<'static>> {
        (1.0..100.0f64,).prop_map(|(threshold,)| JsonConfig {
            tool_version: "0.1.0".to_string(),
            metric: ComplexityMetric::Cognitive,
            threshold,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            diagnostics: None,
            diff_ref: None,
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_format_json_always_valid(
            result in arb_analysis_result(),
            config in arb_config(),
        ) {
            let json_str = format_json(&result, &config)
                .expect("format_json should never fail on valid input");
            let _: serde_json::Value = serde_json::from_str(&json_str)
                .expect("output should be valid JSON");
        }

        #[test]
        fn prop_format_json_functions_count(
            result in arb_analysis_result(),
            config in arb_config(),
        ) {
            let json_str = format_json(&result, &config).unwrap();
            let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
            let funcs = v["result"]["functions"].as_array().unwrap();
            prop_assert_eq!(funcs.len(), result.functions.len());
        }
    }
}
