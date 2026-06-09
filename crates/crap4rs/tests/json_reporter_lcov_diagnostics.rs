//! LCOV-specific JSON-reporter wire-shape assertion that originally
//! lived inside `crap_core::adapters::reporters::json::tests::
//! test_diagnostics_included_when_present`.
//!
//! When the JSON reporter relocated to crap-core in S3 (#135), the
//! reporter became generic over `P: ParseDiagnostic` and crap-core's
//! own unit suite kept only the `P`-agnostic slice (counts on
//! `AnalysisDiagnostics<P>` serialize through). The per-variant wire
//! shape — `kind`, `line_number`, `content` for
//! `LcovParseDiagnostic::MalformedRecord` — is intrinsically a crap4rs
//! concern (LCOV-flavored), so the populated-case assertion moved here
//! to live next to `LcovParseDiagnostic`.
//!
//! Mirrors the original test exactly: same fixture values, same
//! per-key assertions. Pure relocation — no behavior change.

use crap4rs::adapters::reporters::{JsonConfig, format_json};
use crap4rs::domain::types::{
    AnalysisDiagnostics, AnalysisResult, ComplexityMetric, MissingCoveragePolicy,
};
use crap4rs::domain::view::{self, ViewSpec};

fn make_empty_result() -> AnalysisResult {
    // Round-trip an empty envelope's `result` block. Keeps the test
    // independent of `AnalysisResult`'s `#[non_exhaustive]` status —
    // serde construction is the documented external escape hatch.
    let json = r#"{
        "functions": [],
        "summary": {
            "total_functions": 0,
            "total_files": 0,
            "exceeding_threshold": 0,
            "average_crap": 0.0,
            "median_crap": 0.0,
            "max_crap": null,
            "worst_function": null,
            "distribution": { "low": 0, "acceptable": 0, "moderate": 0, "high": 0 }
        },
        "passed": true
    }"#;
    serde_json::from_str(json).expect("empty result JSON should deserialize")
}

#[test]
fn diagnostics_included_when_present() {
    // Construct via serde round-trip — the documented external escape
    // hatch for `#[non_exhaustive]` result types (mirrors
    // `make_empty_result` above). Keeps the test independent of the
    // domain struct's exact field set; the wire-shape assertion below
    // is what matters.
    let diag_json = r#"{
        "parse_diagnostics": [
            {"kind": "malformed_record", "line_number": 5, "content": "DA:bad"}
        ],
        "files_found": 10,
        "files_unparseable": 1,
        "functions_extracted": 42,
        "functions_matched": 40,
        "functions_no_coverage": 2,
        "files_analyzed": 8,
        "files_zero_coverage": 2
    }"#;
    let diag: AnalysisDiagnostics =
        serde_json::from_str(diag_json).expect("diagnostics JSON should deserialize");

    let result = make_empty_result();
    let config = JsonConfig {
        tool_version: "0.1.0".to_string(),
        metric: ComplexityMetric::Cognitive,
        missing_coverage_policy: MissingCoveragePolicy::Pessimistic,
        threshold: 8.0,
        epsilon: 0.0,
        timestamp: "2026-03-28T12:00:00Z".to_string(),
        diagnostics: Some(&diag),
        diff_ref: None,
        minimal_view: false,
        delta: None,
    };

    let analysis_view = view::apply(&result, ViewSpec::default());
    let json_str = format_json(&analysis_view, &config).expect("format_json should succeed");
    let v: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

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
