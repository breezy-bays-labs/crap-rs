//! Terminal table reporter — formats `AnalysisResult` as a colored,
//! sorted terminal table.

use crate::domain::types::{AnalysisResult, RiskLevel};
use colored::Colorize;
use comfy_table::{ContentArrangement, Table};

/// Format an analysis result as a colored terminal table.
///
/// The table is sorted by CRAP score descending, with risk-level and
/// coverage coloring. Returns a ready-to-print `String`.
pub fn format_table(result: &AnalysisResult, threshold: f64) -> String {
    let mut output = String::new();

    // Header
    output.push_str(&format!(
        "crap4rs v{} — CRAP Score Analysis\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Empty guard
    if result.functions.is_empty() {
        output.push_str("\nNo functions analyzed\n");
        return output;
    }

    // Sort by CRAP score descending
    let mut sorted: Vec<_> = result.functions.iter().collect();
    sorted.sort_by(|a, b| {
        b.scored
            .crap
            .value
            .partial_cmp(&a.scored.crap.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build table
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["File", "Function", "CC", "Cov%", "CRAP", "Risk"]);

    for verdict in &sorted {
        let s = &verdict.scored;
        let cov_str = format!("{:.1}", s.coverage_percent);
        let crap_str = format!("{:.2}", s.crap.value);

        table.add_row(vec![
            s.identity.file_path.clone(),
            s.identity.qualified_name.clone(),
            s.complexity.to_string(),
            coverage_color(s.coverage_percent, &cov_str),
            crap_color(verdict.exceeds, &crap_str),
            risk_color(&s.crap.risk_level, &s.crap.risk_level.to_string()),
        ]);
    }

    output.push('\n');
    output.push_str(&table.to_string());
    output.push('\n');

    // Summary line 1
    let pass_fail = if result.passed {
        "PASS".green().bold().to_string()
    } else {
        "FAIL".red().bold().to_string()
    };
    let worst = result
        .summary
        .max_crap
        .map(|c| format!("{:.1}", c.value))
        .unwrap_or_else(|| "N/A".to_string());

    output.push_str(&format!(
        "\nSummary: {} functions | {} above threshold ({}) | worst: {} | {}\n",
        result.summary.total_functions,
        result.summary.exceeding_threshold,
        threshold,
        worst,
        pass_fail,
    ));

    // Summary line 2
    let d = &result.summary.distribution;
    output.push_str(&format!(
        "         avg: {:.1} | median: {:.1} | low: {} | acceptable: {} | moderate: {} | high: {}\n",
        result.summary.average_crap,
        result.summary.median_crap,
        d.low,
        d.acceptable,
        d.moderate,
        d.high,
    ));

    output
}

fn risk_color(level: &RiskLevel, text: &str) -> String {
    match level {
        RiskLevel::Low => text.green().to_string(),
        RiskLevel::Acceptable => text.to_string(),
        RiskLevel::Moderate => text.yellow().to_string(),
        RiskLevel::High => text.red().bold().to_string(),
    }
}

fn coverage_color(percent: f64, text: &str) -> String {
    if percent < 50.0 {
        text.red().to_string()
    } else if percent < 80.0 {
        text.yellow().to_string()
    } else {
        text.green().to_string()
    }
}

fn crap_color(exceeds: bool, text: &str) -> String {
    if exceeds {
        text.red().bold().to_string()
    } else {
        text.to_string()
    }
}

/// Guards `colored::control::set_override()` — a process-global flag.
/// All tests that call `set_override` must hold this lock to prevent
/// races under `cargo test` (threaded). Nextest doesn't need this
/// (process isolation) but the lock is harmless there.
#[cfg(test)]
static COLOR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;
    use crate::domain::types::RiskLevel;

    // ── Snapshot tests (color disabled) ────────────────────────────────

    #[test]
    fn test_empty_shows_no_functions() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_empty_result();
        let output = format_table(&result, 8.0);
        assert!(output.contains("crap4rs v"));
        assert!(output.contains("No functions analyzed"));
        // No table header should be present
        assert!(!output.contains("File"));
    }

    #[test]
    fn test_sorted_by_crap_descending() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(&result, 8.0);
        let lines: Vec<&str> = output.lines().collect();

        // Find data rows (after header row + separator)
        let first_data = lines
            .iter()
            .position(|l| l.contains("complex_fn"))
            .expect("should contain complex_fn");
        let second_data = lines
            .iter()
            .position(|l| l.contains("parse_record"))
            .expect("should contain parse_record");
        let third_data = lines
            .iter()
            .position(|l| l.contains("simple_fn"))
            .expect("should contain simple_fn");

        assert!(first_data < second_data, "45.2 should appear before 15.0");
        assert!(second_data < third_data, "15.0 should appear before 3.0");
    }

    #[test]
    fn test_all_columns_present() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_single_function_result(
            "test_fn",
            "src/lib.rs",
            5,
            80.0,
            5.16,
            RiskLevel::Acceptable,
            8.0,
        );
        let output = format_table(&result, 8.0);
        assert!(output.contains("File"));
        assert!(output.contains("Function"));
        assert!(output.contains("CC"));
        assert!(output.contains("Cov%"));
        assert!(output.contains("CRAP"));
        assert!(output.contains("Risk"));
    }

    #[test]
    fn test_function_details_in_columns() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_single_function_result(
            "parse_record",
            "src/adapters/coverage/mod.rs",
            6,
            72.5,
            8.13,
            RiskLevel::Moderate,
            8.0,
        );
        let output = format_table(&result, 8.0);
        assert!(output.contains("src/adapters/coverage/mod.rs"));
        assert!(output.contains("parse_record"));
        assert!(output.contains("6"));
        assert!(output.contains("72.5"));
        assert!(output.contains("8.13"));
    }

    #[test]
    fn test_crap_two_decimal_places() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 5.0, RiskLevel::Low, 8.0);
        let output = format_table(&result, 8.0);
        assert!(output.contains("5.00"));
    }

    #[test]
    fn test_coverage_one_decimal_place() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 85.0, 1.0, RiskLevel::Low, 8.0);
        let output = format_table(&result, 8.0);
        assert!(output.contains("85.0"));
    }

    #[test]
    fn test_version_header() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_empty_result();
        let output = format_table(&result, 8.0);
        assert!(output.starts_with(&format!("crap4rs v{}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn test_summary_line_contents() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(&result, 8.0);
        assert!(output.contains("3 functions"));
        assert!(output.contains("2 above threshold (8)"));
        assert!(output.contains("worst: 45.2"));
        assert!(output.contains("FAIL"));
    }

    #[test]
    fn test_summary_pass_variant() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let output = format_table(&result, 8.0);
        assert!(output.contains("PASS"));
        assert!(!output.contains("FAIL"));
    }

    #[test]
    fn test_summary_distribution() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(&result, 8.0);
        assert!(output.contains("avg: 21.1"));
        assert!(output.contains("median: 15.0"));
        assert!(output.contains("low: 1"));
        assert!(output.contains("acceptable: 0"));
        assert!(output.contains("moderate: 1"));
        assert!(output.contains("high: 1"));
    }

    // ── Color helper tests (force color on for ANSI assertions) ───────

    #[test]
    fn test_risk_color_low_green() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let out = risk_color(&RiskLevel::Low, "low");
        assert!(out.contains("\x1b[32m"), "Expected green ANSI: {out:?}");
    }

    #[test]
    fn test_risk_color_acceptable_no_color() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let out = risk_color(&RiskLevel::Acceptable, "acceptable");
        assert!(!out.contains("\x1b["), "Expected no ANSI escapes: {out:?}");
        assert_eq!(out, "acceptable");
    }

    #[test]
    fn test_risk_color_moderate_yellow() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let out = risk_color(&RiskLevel::Moderate, "moderate");
        assert!(out.contains("\x1b[33m"), "Expected yellow ANSI: {out:?}");
    }

    #[test]
    fn test_risk_color_high_bold_red() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let out = risk_color(&RiskLevel::High, "high");
        // colored combines bold+red as \x1b[1;31m
        assert!(
            out.contains("\x1b[1;31m"),
            "Expected bold+red ANSI: {out:?}"
        );
    }

    #[test]
    fn test_coverage_color_thresholds() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let low = coverage_color(30.0, "30.0");
        assert!(low.contains("\x1b[31m"), "Expected red for <50%: {low:?}");

        let mid = coverage_color(65.0, "65.0");
        assert!(
            mid.contains("\x1b[33m"),
            "Expected yellow for <80%: {mid:?}"
        );

        let high = coverage_color(90.0, "90.0");
        assert!(
            high.contains("\x1b[32m"),
            "Expected green for >=80%: {high:?}"
        );
    }

    #[test]
    fn test_coverage_color_boundary_50() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let at_50 = coverage_color(50.0, "50.0");
        assert!(
            at_50.contains("\x1b[33m"),
            "Expected yellow at exactly 50%: {at_50:?}"
        );
    }

    #[test]
    fn test_coverage_color_boundary_80() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let at_80 = coverage_color(80.0, "80.0");
        assert!(
            at_80.contains("\x1b[32m"),
            "Expected green at exactly 80%: {at_80:?}"
        );
    }

    #[test]
    fn test_crap_exceeding_bold_red() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let out = crap_color(true, "15.00");
        assert!(
            out.contains("\x1b[1;31m"),
            "Expected bold+red ANSI: {out:?}"
        );
    }

    #[test]
    fn test_crap_within_no_emphasis() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(true);
        let out = crap_color(false, "5.00");
        assert!(!out.contains("\x1b["), "Expected no ANSI: {out:?}");
        assert_eq!(out, "5.00");
    }

    #[test]
    fn test_full_table_snapshot() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(&result, 8.0);
        insta::assert_snapshot!(output);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::domain::types::{
        AnalysisResult, AnalysisSummary, CrapScore, FunctionIdentity, FunctionVerdict,
        RiskDistribution, RiskLevel, ScoredFunction, SourceSpan,
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
                        complexity_metric: crate::domain::types::ComplexityMetric::Cognitive,
                        coverage_percent: coverage,
                        crap: CrapScore {
                            value: crap_value,
                            risk_level: risk,
                        },
                    },
                    threshold,
                    exceeds: crap_value > threshold,
                },
            )
    }

    /// Build an AnalysisResult with a hand-constructed summary.
    /// Summary values are structurally valid but not semantically precise —
    /// reporters only format what they receive, so accuracy doesn't matter.
    fn arb_analysis_result() -> impl Strategy<Value = AnalysisResult> {
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
                .map(|v| v.scored.crap.clone());
            let avg = if total > 0 {
                verdicts.iter().map(|v| v.scored.crap.value).sum::<f64>() / total as f64
            } else {
                0.0
            };
            AnalysisResult {
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_format_table_never_panics(result in arb_analysis_result()) {
            let _guard = super::COLOR_LOCK.lock().unwrap();
            colored::control::set_override(false);
            let _ = format_table(&result, 8.0);
        }

        #[test]
        fn prop_format_table_row_count(result in arb_analysis_result()) {
            let _guard = super::COLOR_LOCK.lock().unwrap();
            colored::control::set_override(false);
            let output = format_table(&result, 8.0);
            if result.functions.is_empty() {
                prop_assert!(output.contains("No functions analyzed"));
            } else {
                // Each function should have its qualified_name in the output
                for v in &result.functions {
                    prop_assert!(
                        output.contains(&v.scored.identity.qualified_name),
                        "Missing function {} in output",
                        v.scored.identity.qualified_name
                    );
                }
            }
        }
    }
}
