//! JSON reporter — formats an `AnalysisView` as structured JSON
//! with a versioned envelope for CI pipelines and tooling consumption.
//!
//! The envelope carries both the unshapeable underlying analysis
//! (`result`) and the View metadata (`view`) describing how the
//! reported rows were filtered, sorted, and truncated. The wire schema
//! itself is the canonical [`wire::Envelope`]; this module's job is
//! presentation assembly — projecting the borrowed view / delta state
//! into the owned envelope at emit time. The clone this implies trades
//! the previous zero-copy serialization for a single schema type shared
//! with every envelope reader; for a fire-and-exit CLI about to
//! pretty-print the same data, the cost is negligible.

use crate::adapters::wire::{self, DeltaBlock, Envelope, ViewBlock};
use crate::domain::delta::DeltaView;
use crate::domain::types::{AnalysisDiagnostics, ComplexityMetric, MissingCoveragePolicy};
use crate::domain::view::AnalysisView;
use crate::ports::ParseDiagnostic;

/// Configuration for the JSON envelope metadata.
///
/// Generic over `P: ParseDiagnostic` since `AnalysisDiagnostics<P>`
/// is generic across adapters. crap4rs concretizes via the
/// `JsonConfig<'a>` type alias in `crap4rs::adapters::reporters::json`
/// so v0.4 callers' type paths stay byte-identical.
#[derive(Debug)]
pub struct JsonConfig<'a, P: ParseDiagnostic> {
    pub tool_version: String,
    /// Lowercase wire language token for the envelope's `language` field
    /// (`"rust"` / `"typescript"`). Sourced from the active adapter's
    /// `AdapterMeta::config_lang_key`, so each adapter stamps its own
    /// language rather than a shared literal.
    pub language: String,
    pub metric: ComplexityMetric,
    /// Resolved missing-coverage policy for this run. Recorded in the
    /// envelope (unless `Pessimistic`) so a baseline captures the policy
    /// it was generated under and a later delta run can warn on a
    /// mismatch.
    pub missing_coverage_policy: MissingCoveragePolicy,
    pub threshold: f64,
    /// Resolved threshold-border epsilon (`--threshold-epsilon` /
    /// `[delta] epsilon`; `0.0` when unset). Recorded on the envelope so a
    /// downstream consumer that recomputes the delta from result envelopes
    /// (`crap-render`) applies the same band the gate used (crap-rs#379).
    pub epsilon: f64,
    pub timestamp: String,
    /// When present, diagnostics are included in the JSON output (--verbose).
    pub diagnostics: Option<&'a AnalysisDiagnostics<P>>,
    /// Git ref used for diff filtering (`--diff <ref>`). `None` when not in diff mode.
    pub diff_ref: Option<&'a str>,
    /// When true, the per-row `view.shown` array is omitted (`--minimal-view`).
    /// All other view metadata (`spec`, `eligible_count`, `truncated`,
    /// `shown_summary`) is preserved so consumers retain scope context.
    pub minimal_view: bool,
    /// When present, the envelope grows a top-level `delta` block
    /// describing changes vs the baseline. None means no `--baseline`
    /// was passed; the `delta` key is omitted entirely.
    pub delta: Option<DeltaContext<'a, P>>,
}

/// Bundles everything the JSON reporter needs to render the `delta`
/// block: the shaped view (post-filter / sort / truncate) plus the
/// underlying delta and baseline metadata captured when the baseline
/// envelope was loaded.
///
/// Generic over `P: ParseDiagnostic` — see `JsonConfig` for the
/// rationale.
#[derive(Debug)]
pub struct DeltaContext<'a, P: ParseDiagnostic> {
    /// Shaped view — drives `shown`, `spec`, `eligible_count`, `truncated`.
    pub view: &'a DeltaView<'a>,
    pub baseline_tool_version: &'a str,
    pub baseline_timestamp: &'a str,
    pub baseline_diagnostics: Option<&'a AnalysisDiagnostics<P>>,
}

/// Project the borrowed row view into the owned wire block.
///
/// A minimal view elides the per-row `shown` list; every other view key
/// remains for scope context.
fn view_block(view: &AnalysisView<'_>, minimal: bool) -> ViewBlock {
    ViewBlock {
        spec: view.spec.clone(),
        eligible_count: view.eligible_count,
        truncated: view.truncated,
        shown: (!minimal).then(|| view.shown.iter().map(|v| (*v).clone()).collect()),
        shown_summary: view.shown_summary.clone(),
        grouped: view.grouped.clone(),
    }
}

/// Project the borrowed delta context into the owned wire block.
fn delta_block<P: ParseDiagnostic>(ctx: &DeltaContext<'_, P>) -> DeltaBlock<P> {
    DeltaBlock {
        summary: ctx.view.full.summary,
        spec: ctx.view.spec.clone(),
        eligible_count: ctx.view.eligible_count,
        truncated: ctx.view.truncated,
        baseline_ref: None,
        baseline_tool_version: ctx.baseline_tool_version.to_string(),
        baseline_timestamp: ctx.baseline_timestamp.to_string(),
        shown: ctx.view.shown.iter().map(|c| (*c).clone()).collect(),
        baseline_diagnostics: ctx.baseline_diagnostics.cloned(),
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
pub fn format_json<P: ParseDiagnostic>(
    view: &AnalysisView<'_>,
    config: &JsonConfig<'_, P>,
) -> Result<String, serde_json::Error> {
    let envelope = Envelope {
        schema_version: wire::CURRENT_SCHEMA_VERSION,
        tool_version: config.tool_version.clone(),
        language: config.language.clone(),
        timestamp: config.timestamp.clone(),
        metric: Some(config.metric),
        threshold: Some(config.threshold),
        diff_ref: config.diff_ref.map(str::to_string),
        result: view.full.clone(),
        view: view_block(view, config.minimal_view),
        delta: config.delta.as_ref().map(delta_block),
        diagnostics: config.diagnostics.cloned(),
        missing_coverage_policy: config.missing_coverage_policy,
        epsilon: config.epsilon,
    };
    serde_json::to_string_pretty(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;
    use crate::domain::types::{AnalysisResult, ComplexityMetric, RiskLevel};
    use crate::test_strategies::DummyParseDiagnostic;

    /// Concrete `P` for in-module tests — the JSON reporter's behavior
    /// is `P`-agnostic for the cases asserted here (no test reaches
    /// into per-variant fields of a parse diagnostic). Pinning to the
    /// dummy stub decouples these tests from any specific adapter's
    /// diagnostic shape.
    type TestJsonConfig = JsonConfig<'static, DummyParseDiagnostic>;

    fn default_config() -> TestJsonConfig {
        JsonConfig {
            tool_version: "0.1.0".to_string(),
            language: "rust".to_string(),
            metric: ComplexityMetric::Cognitive,
            missing_coverage_policy: MissingCoveragePolicy::Pessimistic,
            threshold: 8.0,
            epsilon: 0.0,
            timestamp: "2026-03-28T12:00:00Z".to_string(),
            diagnostics: None,
            diff_ref: None,
            minimal_view: false,
            delta: None,
        }
    }

    fn parse_json<'a>(
        result: &AnalysisResult,
        config: &JsonConfig<'a, DummyParseDiagnostic>,
    ) -> serde_json::Value {
        let view = make_view_default(result);
        let json_str = format_json(&view, config).expect("format_json should succeed");
        serde_json::from_str(&json_str).expect("output should be valid JSON")
    }

    #[test]
    fn envelope_omits_epsilon_when_zero_and_emits_when_set() {
        let result = make_empty_result();
        // Default (epsilon 0.0): key elided so existing envelopes stay
        // byte-identical (crap-rs#379, additive — no schema_version bump).
        let v0 = parse_json(&result, &default_config());
        assert!(
            v0.get("epsilon").is_none(),
            "epsilon key must be elided at 0.0"
        );
        // Set: key present with the resolved value, for crap-render to read.
        let mut cfg = default_config();
        cfg.epsilon = 0.5;
        let v = parse_json(&result, &cfg);
        assert_eq!(v["epsilon"], serde_json::json!(0.5));
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
    fn missing_coverage_policy_elided_when_pessimistic() {
        // The default policy keeps the envelope byte-identical to before
        // the field existed — the key is absent entirely.
        let result = make_empty_result();
        let v = parse_json(&result, &default_config());
        assert!(
            v.get("missing_coverage_policy").is_none(),
            "pessimistic policy must elide the envelope key"
        );
    }

    #[test]
    fn missing_coverage_policy_emitted_when_non_default() {
        let result = make_empty_result();
        for (policy, wire) in [
            (MissingCoveragePolicy::Optimistic, "optimistic"),
            (MissingCoveragePolicy::Skip, "skip"),
        ] {
            let config = JsonConfig {
                missing_coverage_policy: policy,
                ..default_config()
            };
            let v = parse_json(&result, &config);
            assert_eq!(
                v.get("missing_coverage_policy").and_then(|x| x.as_str()),
                Some(wire),
                "non-default policy must serialize its wire token"
            );
        }
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
    fn test_diagnostics_included_when_present_p_agnostic_top_level() {
        // P-agnostic slice: counts on AnalysisDiagnostics<P> serialize
        // through regardless of how `parse_diagnostics` flatten. The
        // LCOV-specific per-variant wire shape is asserted in the
        // crap4rs-side companion test
        // `tests/json_reporter_lcov_diagnostics.rs::diagnostics_included_when_present`,
        // which lives next to LcovParseDiagnostic rather than in
        // crap-core (which is `P`-generic).
        use crate::domain::types::AnalysisDiagnostics;

        let diag: AnalysisDiagnostics<DummyParseDiagnostic> = AnalysisDiagnostics {
            parse_diagnostics: vec![],
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
        // parse_diagnostics is empty under DummyParseDiagnostic; the
        // crap4rs-side companion exercises the populated case.
        let parse_diags = d["parse_diagnostics"].as_array().unwrap();
        assert!(parse_diags.is_empty());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::make_view_default;
    use crate::test_strategies::{DummyParseDiagnostic, arb_analysis_result};
    use proptest::prelude::*;

    fn arb_config() -> impl Strategy<Value = JsonConfig<'static, DummyParseDiagnostic>> {
        // Generate epsilon across both the elided (0.0) and present (>0)
        // branches so the new `skip_serializing_if` field is exercised.
        (1.0..100.0f64, prop_oneof![Just(0.0f64), 0.001..2.0f64]).prop_map(
            |(threshold, epsilon)| JsonConfig {
                tool_version: "0.1.0".to_string(),
                language: "rust".to_string(),
                metric: ComplexityMetric::Cognitive,
                missing_coverage_policy: MissingCoveragePolicy::Pessimistic,
                threshold,
                epsilon,
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                diagnostics: None,
                diff_ref: None,
                minimal_view: false,
                delta: None,
            },
        )
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
