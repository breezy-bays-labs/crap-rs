//! One-line analysis verdict reporter (`--summary`).
//!
//! Emits a single human-and-CI-friendly line summarising the run:
//!
//! ```text
//! PASS: 1082 functions | 0 above threshold (25) | worst: 13.0 | avg: 1.6
//! FAIL: 412 functions | 7 above threshold (25) | worst: 47.5 | avg: 3.2
//! ```
//!
//! Mirrors crap4ts's `formatSummaryLine` byte-for-byte for the shared
//! subset (status, function count, exceeding count, threshold, worst,
//! avg) so a CI line-template can match either tool. Threshold formats
//! integer-when-whole via Rust's default `f64` Display, which matches
//! TypeScript's `Number.toString()`. Worst and average are fixed at one
//! decimal place to match `Number.toFixed(1)`.
//!
//! Empty-analysis behaviour (`summary.max_crap = None`): renders
//! `worst: 0.0`. Documented choice — neither the issue's AC nor
//! crap4ts's reference specifies it (crap4ts would throw on the missing
//! deref).

use crate::domain::types::AnalysisResult;

/// Format the single-line analysis verdict.
///
/// `threshold` is the effective post-merge display threshold (after
/// `--strict` / `--lenient` / config-file precedence), not the raw
/// `cli.output.threshold`. Pass `inputs.threshold` from the dispatch
/// site.
pub fn format_summary_line(result: &AnalysisResult, threshold: f64) -> String {
    let status = if result.passed { "PASS" } else { "FAIL" };
    let worst = result
        .summary
        .max_crap
        .as_ref()
        .map(|c| c.value)
        .unwrap_or(0.0);
    format!(
        "{status}: {total} functions | {exceeding} above threshold ({threshold}) | worst: {worst:.1} | avg: {avg:.1}",
        total = result.summary.total_functions,
        exceeding = result.summary.exceeding_threshold,
        avg = result.summary.average_crap,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::{
        make_empty_result, make_multi_function_result, make_single_function_result,
    };
    use crate::domain::types::RiskLevel;

    #[test]
    fn pass_line_shape_matches_crap4ts() {
        let result = make_single_function_result(
            "tiny_fn",
            "src/lib.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            25.0,
        );
        let line = format_summary_line(&result, 25.0);
        assert_eq!(
            line,
            "PASS: 1 functions | 0 above threshold (25) | worst: 1.0 | avg: 1.0"
        );
    }

    #[test]
    fn fail_line_shape_matches_crap4ts() {
        let mut result = make_single_function_result(
            "scary_fn",
            "src/lib.rs",
            20,
            30.0,
            47.5,
            RiskLevel::High,
            25.0,
        );
        // single_function_result derived .passed from exceeds vs threshold 25.0,
        // and average_crap from crap_value — both match what we want.
        result.summary.average_crap = 3.2;
        let line = format_summary_line(&result, 25.0);
        assert_eq!(
            line,
            "FAIL: 1 functions | 1 above threshold (25) | worst: 47.5 | avg: 3.2"
        );
    }

    #[test]
    fn threshold_renders_as_integer_when_whole() {
        let result = make_empty_result();
        let line = format_summary_line(&result, 25.0);
        assert!(line.contains("(25)"), "expected '(25)' in {line:?}");
        assert!(!line.contains("(25.0)"), "should not include '.0'");
    }

    #[test]
    fn threshold_renders_with_decimals_when_fractional() {
        let result = make_empty_result();
        let line = format_summary_line(&result, 25.5);
        assert!(line.contains("(25.5)"), "expected '(25.5)' in {line:?}");
    }

    #[test]
    fn empty_analysis_renders_worst_zero() {
        let result = make_empty_result();
        assert!(result.summary.max_crap.is_none());
        let line = format_summary_line(&result, 25.0);
        assert_eq!(
            line,
            "PASS: 0 functions | 0 above threshold (25) | worst: 0.0 | avg: 0.0"
        );
    }

    #[test]
    fn worst_and_avg_fixed_one_decimal() {
        let mut result = make_multi_function_result();
        // multi-function fixture already gives max_crap = 45.2 (high),
        // average_crap computed from compute_summary — overwrite to a
        // known float to assert the {:.1} formatter.
        result.summary.max_crap = Some(crate::domain::types::CrapScore {
            value: 13.04,
            risk_level: RiskLevel::Moderate,
        });
        result.summary.average_crap = 1.642;
        let line = format_summary_line(&result, 25.0);
        assert!(
            line.contains("worst: 13.0"),
            "expected 'worst: 13.0' in {line:?}"
        );
        assert!(line.contains("avg: 1.6"), "expected 'avg: 1.6' in {line:?}");
    }

    #[test]
    fn pass_status_from_result_passed_field() {
        let mut result = make_empty_result();
        result.passed = false;
        let line = format_summary_line(&result, 25.0);
        assert!(
            line.starts_with("FAIL:"),
            "expected FAIL prefix in {line:?}"
        );
    }
}
