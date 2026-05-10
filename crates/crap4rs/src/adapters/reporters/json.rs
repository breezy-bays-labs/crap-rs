//! JSON reporter — formats an `AnalysisView` as structured JSON
//! with a versioned envelope for CI pipelines and tooling consumption.
//!
//! The envelope carries both the unshapeable underlying analysis
//! (`result`) and the View metadata (`view`) describing how the
//! reported rows were filtered, sorted, and truncated. `view.full` is
//! `#[serde(skip)]` so the analysis is emitted exactly once.

use crate::domain::delta::{DeltaSummary, DeltaView, DeltaViewSpec, FunctionChange};
use crate::domain::types::{
    AnalysisDiagnostics, AnalysisResult, AnalysisSummary, ComplexityMetric, FunctionVerdict,
};
use crate::domain::view::{AnalysisView, GroupedView, ViewSpec};
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
    /// When true, the per-row `view.shown` array is omitted (`--minimal-view`).
    /// All other view metadata (`spec`, `eligible_count`, `truncated`,
    /// `shown_summary`) is preserved so consumers retain scope context.
    pub minimal_view: bool,
    /// When present, the envelope grows a top-level `delta` block
    /// describing changes vs the baseline. None means no `--baseline`
    /// was passed; the `delta` key is omitted entirely.
    pub delta: Option<DeltaContext<'a>>,
}

/// Bundles everything the JSON reporter needs to render the `delta`
/// block: the shaped view (post-filter / sort / truncate) plus the
/// underlying delta and baseline metadata captured when the baseline
/// envelope was loaded.
#[derive(Debug)]
pub struct DeltaContext<'a> {
    /// Shaped view — drives `shown`, `spec`, `eligible_count`, `truncated`.
    pub view: &'a DeltaView<'a>,
    pub baseline_tool_version: &'a str,
    pub baseline_timestamp: &'a str,
    pub baseline_diagnostics: Option<&'a AnalysisDiagnostics>,
}

/// JSON envelope. Field order is **load-bearing** —
/// `tests/features/cli_ergonomics.feature:243-246` asserts the
/// declaration order is exactly:
///   schema_version, tool_version, language, timestamp, metric,
///   threshold, diff_ref, result, view
/// Per ADR D2 the schema is additive across minor versions; the
/// `schema_version` bump from 1 → 2 in 0.4.0 reflects the
/// `ComplexityContributor.column` 0-based → 1-based convention shift
/// (#107). Older v1 baselines remain loadable for delta reporting
/// (matching is identity-keyed, not column-keyed).
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
    view: ViewWire<'a>,
    /// Delta block. Present iff `--baseline` was passed; absent (key
    /// elided) otherwise so existing consumers see byte-identical
    /// output for the no-delta case. Additive — does not itself bump
    /// `schema_version` (ADR D2).
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<DeltaWire<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a AnalysisDiagnostics>,
}

/// On-the-wire delta representation. Mirrors the `delta` envelope key
/// shape documented in ADR D7 §DeltaView.
#[derive(Serialize)]
struct DeltaWire<'a> {
    /// Aggregate counts over the *unshaped* change set. The gate
    /// keystone — shaping never alters this.
    summary: &'a DeltaSummary,
    /// Echoes the resolved [`DeltaViewSpec`] so consumers can
    /// reconstruct what filters / sort / limit produced `shown`.
    spec: &'a DeltaViewSpec,
    /// Post-filter, pre-truncate count. With `truncated`, lets
    /// consumers render "Showing X of Y".
    eligible_count: usize,
    truncated: bool,
    /// Reserved for a future `--baseline-ref <label>` flag (F2 follow-up).
    /// Always `null` today.
    baseline_ref: Option<&'a str>,
    baseline_tool_version: &'a str,
    baseline_timestamp: &'a str,
    /// Per-change list, post-filter / sort / truncate. References
    /// (the borrows hold for the envelope's lifetime via the View).
    shown: Vec<&'a FunctionChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline_diagnostics: Option<&'a AnalysisDiagnostics>,
}

impl<'a> DeltaWire<'a> {
    fn from_context(ctx: &'a DeltaContext<'a>) -> Self {
        DeltaWire {
            summary: &ctx.view.full.summary,
            spec: &ctx.view.spec,
            eligible_count: ctx.view.eligible_count,
            truncated: ctx.view.truncated,
            baseline_ref: None,
            baseline_tool_version: ctx.baseline_tool_version,
            baseline_timestamp: ctx.baseline_timestamp,
            shown: ctx.view.shown.clone(),
            baseline_diagnostics: ctx.baseline_diagnostics,
        }
    }
}

/// On-the-wire view representation. Mirrors `AnalysisView`'s serialized
/// shape exactly when `shown` is `Some`, so the default JSON output is
/// byte-identical to the prior `view: &AnalysisView` serialization.
/// `--minimal-view` sets `shown = None`, which `skip_serializing_if`
/// elides — every other key remains for scope context.
#[derive(Serialize)]
struct ViewWire<'a> {
    spec: &'a ViewSpec,
    eligible_count: usize,
    truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    shown: Option<&'a [&'a FunctionVerdict]>,
    shown_summary: &'a AnalysisSummary,
    /// Per-key aggregation block. Always serialized (emits `null`
    /// when grouping is inactive) so consumers can distinguish
    /// "default invocation" from "schema doesn't carry grouping".
    grouped: Option<&'a GroupedView>,
}

impl<'a> ViewWire<'a> {
    fn from_view(view: &'a AnalysisView<'a>, minimal: bool) -> Self {
        ViewWire {
            spec: &view.spec,
            eligible_count: view.eligible_count,
            truncated: view.truncated,
            shown: if minimal {
                None
            } else {
                Some(view.shown.as_slice())
            },
            shown_summary: &view.shown_summary,
            grouped: view.grouped.as_ref(),
        }
    }
}

// Note: per-function thresholds are already visible in each FunctionVerdict's
// `threshold` field. Consumers can compare individual function thresholds
// against the envelope's global `threshold` to detect overrides.

/// Format a view as pretty-printed JSON with a versioned envelope.
///
/// `view.full` is the canonical analysis (gate); the envelope's
/// `result` field serializes it. The additive `view` field carries
/// the spec, eligible/truncated metadata, and the shaped row list.
pub fn format_json(
    view: &AnalysisView<'_>,
    config: &JsonConfig<'_>,
) -> Result<String, serde_json::Error> {
    let delta_wire: Option<DeltaWire> = config.delta.as_ref().map(DeltaWire::from_context);

    let envelope = JsonEnvelope {
        schema_version: 2,
        tool_version: &config.tool_version,
        language: "rust",
        timestamp: &config.timestamp,
        metric: &config.metric,
        threshold: config.threshold,
        diff_ref: config.diff_ref,
        result: view.full,
        view: ViewWire::from_view(view, config.minimal_view),
        delta: delta_wire,
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
            minimal_view: false,
            delta: None,
        }
    }

    fn parse_json(result: &AnalysisResult, config: &JsonConfig) -> serde_json::Value {
        let view = make_view_default(result);
        let json_str = format_json(&view, config).expect("format_json should succeed");
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
        assert_eq!(sv.as_u64(), Some(2));
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
        let view = make_view_default(&result);
        let json_str = format_json(&view, &default_config()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        insta::assert_json_snapshot!(v);
    }

    #[test]
    fn test_envelope_field_declaration_order() {
        // cli_ergonomics.feature:243-246: the envelope key declaration
        // order is exactly schema_version, tool_version, language,
        // timestamp, metric, threshold, diff_ref, result, view.
        // Asserted on the raw `format_json` string (NOT the parsed
        // serde_json::Value, which alphabetizes via BTreeMap).
        let result = make_empty_result();
        let view = make_view_default(&result);
        let json_str = format_json(&view, &default_config()).unwrap();
        let keys = [
            "schema_version",
            "tool_version",
            "language",
            "timestamp",
            "metric",
            "threshold",
            "diff_ref",
            "result",
            "view",
        ];
        // Top-level keys in serde_json's pretty printer sit at indent 2.
        // Anchor the substring search to `\n  "<key>"` so future nested
        // fields with the same name can't shadow the top-level
        // position (CodeRabbit CR-N5).
        let positions: Vec<usize> = keys
            .iter()
            .map(|k| {
                json_str
                    .find(&format!("\n  \"{k}\""))
                    .unwrap_or_else(|| panic!("missing top-level key {k} in:\n{json_str}"))
            })
            .collect();
        for (k_prev, w) in keys.windows(2).zip(positions.windows(2)) {
            assert!(
                w[0] < w[1],
                "envelope key order: expected {} before {}, got positions {} and {}",
                k_prev[0],
                k_prev[1],
                w[0],
                w[1],
            );
        }
    }

    #[test]
    fn test_view_block_present_in_envelope() {
        // The envelope unconditionally carries a `view` block; defaults
        // are echoed (filters empty, sort=crap, limit absent).
        let result = make_multi_function_result();
        let view = make_view_default(&result);
        let json_str = format_json(&view, &default_config()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let view_block = v.get("view").expect("envelope missing view block");
        assert!(view_block.get("spec").is_some());
        assert!(view_block.get("eligible_count").is_some());
        assert!(view_block.get("truncated").is_some());
        assert!(view_block.get("shown").is_some());
        assert!(view_block.get("shown_summary").is_some());
        // view.full is `#[serde(skip)]` — it must NOT appear.
        assert!(
            view_block.get("full").is_none(),
            "view.full should be elided from JSON (envelope's `result` already serializes it)"
        );
        // Default spec echoed
        let spec = view_block.get("spec").unwrap();
        assert_eq!(spec["sort"], "crap");
        assert_eq!(spec["limit"], serde_json::Value::Null);
        assert_eq!(spec["filters"]["only_failing"], false);
    }

    #[test]
    fn test_view_grouped_null_by_default() {
        // No --group-by ⇒ view.grouped is null, view.spec.group_by is null.
        let result = make_multi_function_result();
        let v = parse_json(&result, &default_config());
        let view_block = &v["view"];
        assert_eq!(view_block["grouped"], serde_json::Value::Null);
        assert_eq!(view_block["spec"]["group_by"], serde_json::Value::Null);
    }

    #[test]
    fn test_view_grouped_populated_under_group_by_file() {
        use crate::domain::view::{self, GroupKey, ViewSpec};
        let result = make_multi_function_result();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            ..Default::default()
        };
        let view = view::apply(&result, spec);
        let json_str = format_json(&view, &default_config()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let grouped = &v["view"]["grouped"];
        assert!(grouped.is_object(), "view.grouped must be populated");
        assert_eq!(grouped["key"], "file");
        assert!(grouped["files"].is_array());
        let files = grouped["files"].as_array().unwrap();
        assert_eq!(files.len(), 3); // 3 distinct files in fixture
        // Spot-check first file structure carries all the FileSummary keys.
        let f0 = &files[0];
        assert!(f0["file_path"].is_string());
        assert!(f0["function_count"].is_number());
        assert!(f0["exceeding_count"].is_number());
        assert!(f0["average_crap"].is_number());
        assert!(f0["average_coverage"].is_number());
        assert!(f0["max_complexity"].is_number());
        assert!(f0["distribution"].is_object());
        // spec.group_by echoed as "file"
        assert_eq!(v["view"]["spec"]["group_by"], "file");
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
        use crate::domain::types::AnalysisDiagnostics;
        use crate::parse_diagnostic::LcovParseDiagnostic;

        let diag = AnalysisDiagnostics {
            parse_diagnostics: vec![LcovParseDiagnostic::MalformedRecord {
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
    use crate::adapters::reporters::test_fixtures::make_view_default;
    use crap_core::test_strategies::arb_analysis_result;
    use proptest::prelude::*;

    fn arb_config() -> impl Strategy<Value = JsonConfig<'static>> {
        (1.0..100.0f64,).prop_map(|(threshold,)| JsonConfig {
            tool_version: "0.1.0".to_string(),
            metric: ComplexityMetric::Cognitive,
            threshold,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            diagnostics: None,
            diff_ref: None,
            minimal_view: false,
            delta: None,
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_format_json_always_valid(
            result in arb_analysis_result(),
            config in arb_config(),
        ) {
            let view = make_view_default(&result);
            let json_str = format_json(&view, &config)
                .expect("format_json should never fail on valid input");
            let _: serde_json::Value = serde_json::from_str(&json_str)
                .expect("output should be valid JSON");
        }

        #[test]
        fn prop_format_json_functions_count(
            result in arb_analysis_result(),
            config in arb_config(),
        ) {
            let view = make_view_default(&result);
            let json_str = format_json(&view, &config).unwrap();
            let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
            let funcs = v["result"]["functions"].as_array().unwrap();
            prop_assert_eq!(funcs.len(), result.functions.len());
        }
    }
}
