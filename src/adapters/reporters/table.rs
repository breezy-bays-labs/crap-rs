//! Terminal table reporter — formats `AnalysisResult` as a colored,
//! sorted terminal table.

use crate::domain::types::{AnalysisResult, RiskLevel};
use colored::Colorize;
use comfy_table::{ContentArrangement, Table};

/// Format an analysis result as a colored terminal table.
///
/// The table is sorted by CRAP score descending, with risk-level and
/// coverage coloring. When `threshold` differs across functions (per-path
/// overrides), the summary shows "varied (default: N)" instead of a single
/// number. Returns a ready-to-print `String`.
pub fn format_table(result: &AnalysisResult, threshold: f64, breakdown: bool) -> String {
    format_table_with_explain(result, threshold, breakdown, false)
}

/// Format an analysis result as a colored terminal table with optional
/// breakdown explanation text.
pub fn format_table_with_explain(
    result: &AnalysisResult,
    threshold: f64,
    breakdown: bool,
    explain: bool,
) -> String {
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

    if breakdown {
        output.push_str(&inject_breakdown_subrows(&table.to_string(), &sorted));
    } else {
        output.push_str(&table.to_string());
    }

    if breakdown
        && explain
        && sorted
            .iter()
            .filter(|verdict| verdict.exceeds)
            .flat_map(|verdict| verdict.scored.contributors.iter())
            .any(|contributor| contributor.increment > 1)
    {
        output.push('\n');
        output.push_str("\nLegend: +1 = base structural increment.\n");
        output.push_str("        +N (nested) = +1 base plus +(N-1) from active nesting depth.\n");
        output.push_str(
            "        Nesting depth increases inside if/else branches, match arms, \
while/for/loop bodies, let-else diverging branches, and closures.\n",
        );
    }

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

    let threshold_display = if has_varied_thresholds(&result.functions) {
        format!("varied (default: {})", threshold)
    } else {
        format!("{}", threshold)
    };

    output.push_str(&format!(
        "\nSummary: {} functions | {} above threshold ({}) | worst: {} | {}\n",
        result.summary.total_functions,
        result.summary.exceeding_threshold,
        threshold_display,
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

/// Inject contributor sub-rows into the table string after each exceeding function's row.
///
/// Splits the rendered table into lines, locates each exceeding function's row via
/// word-boundary name matching, and inserts indented contributor sub-rows after it.
///
/// The 15-space indent is a fixed approximation of the File column width produced by
/// comfy-table's Dynamic arrangement. It aligns sub-rows under the Function column
/// in typical output. If comfy-table column widths change (e.g., very long file paths),
/// the alignment may drift — switching to position-indexed matching (per the ADR) would
/// make this robust.
fn inject_breakdown_subrows(
    table_str: &str,
    sorted: &[&crate::domain::types::FunctionVerdict],
) -> String {
    let mut result_lines: Vec<String> = Vec::new();
    for line in table_str.lines() {
        result_lines.push(line.to_string());
        for verdict in sorted.iter() {
            if verdict.exceeds
                && !verdict.scored.contributors.is_empty()
                && row_contains_name(line, &verdict.scored.identity.qualified_name)
            {
                let contributors = &verdict.scored.contributors;
                let last_idx = contributors.len() - 1;
                for (i, c) in contributors.iter().enumerate() {
                    result_lines.push(format_contributor_subrow(c, i == last_idx));
                }
                break;
            }
        }
    }
    result_lines.join("\n")
}

/// Format a single contributor sub-row with tree-drawing prefix.
fn format_contributor_subrow(
    c: &crate::domain::types::ComplexityContributor,
    is_last: bool,
) -> String {
    let tree = if is_last { "└─" } else { "├─" };
    let increment_str = if c.increment > 1 {
        format!("+{} (nested)", c.increment)
    } else {
        format!("+{}", c.increment)
    };
    format!(
        "               {} line {}: {} ({})",
        tree, c.line, c.kind, increment_str
    )
}

/// Check if a table row line contains the given function name as a whole word.
/// The character after the name must be a space, `|`, or end-of-string,
/// preventing "parse" from matching a row containing "parse_extra".
///
/// Checks all occurrences (not just the first) so that a function name that
/// appears in the file-path column is not mistakenly matched before the
/// function-name column is reached.
fn row_contains_name(line: &str, name: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = line[start..].find(name) {
        let abs_pos = start + pos;
        if matches!(
            line.as_bytes().get(abs_pos + name.len()),
            None | Some(b' ') | Some(b'|')
        ) {
            return true;
        }
        start = abs_pos + 1;
    }
    false
}

/// Check if functions have different threshold values (per-path overrides active).
fn has_varied_thresholds(functions: &[crate::domain::types::FunctionVerdict]) -> bool {
    if functions.len() <= 1 {
        return false;
    }
    let first = functions[0].threshold;
    functions.iter().any(|v| v.threshold != first)
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
    use crate::domain::types::{AnalysisResult, RiskLevel};

    // ── Snapshot tests (color disabled) ────────────────────────────────

    #[test]
    fn test_empty_shows_no_functions() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_empty_result();
        let output = format_table(&result, 8.0, false);
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
        let output = format_table(&result, 8.0, false);
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
        let output = format_table(&result, 8.0, false);
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
        let output = format_table(&result, 8.0, false);
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
        let output = format_table(&result, 8.0, false);
        assert!(output.contains("5.00"));
    }

    #[test]
    fn test_coverage_one_decimal_place() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 85.0, 1.0, RiskLevel::Low, 8.0);
        let output = format_table(&result, 8.0, false);
        assert!(output.contains("85.0"));
    }

    #[test]
    fn test_version_header() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_empty_result();
        let output = format_table(&result, 8.0, false);
        assert!(output.starts_with(&format!("crap4rs v{}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn test_summary_line_contents() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(&result, 8.0, false);
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
        let output = format_table(&result, 8.0, false);
        assert!(output.contains("PASS"));
        assert!(!output.contains("FAIL"));
    }

    #[test]
    fn test_summary_distribution() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(&result, 8.0, false);
        assert!(output.contains("avg: 21.1"));
        assert!(output.contains("median: 15.0"));
        assert!(output.contains("low: 1"));
        assert!(output.contains("acceptable: 0"));
        assert!(output.contains("moderate: 1"));
        assert!(output.contains("high: 1"));
    }

    // ── Breakdown sub-row tests ────────────────────────────────────────

    fn make_contributors() -> Vec<crate::domain::types::ComplexityContributor> {
        use crate::domain::types::{ComplexityContributor, ContributorKind};
        vec![
            ComplexityContributor {
                kind: ContributorKind::IfBranch,
                line: 5,
                column: Some(4),
                increment: 1,
            },
            ComplexityContributor {
                kind: ContributorKind::ForLoop,
                line: 10,
                column: Some(4),
                increment: 2,
            },
        ]
    }

    fn make_nested_contributor() -> Vec<crate::domain::types::ComplexityContributor> {
        use crate::domain::types::{ComplexityContributor, ContributorKind};
        vec![ComplexityContributor {
            kind: ContributorKind::Match,
            line: 3,
            column: Some(4),
            increment: 3,
        }]
    }

    #[test]
    fn test_breakdown_off_no_sub_rows() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let verdict = make_verdict_with_contributors(
            make_verdict("my_fn", "src/lib.rs", 5, 30.0, 45.0, RiskLevel::High, 8.0),
            make_contributors(),
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(&result, 8.0, false);
        assert!(
            !output.contains("├─"),
            "breakdown=false should not show sub-rows: {output}"
        );
        assert!(
            !output.contains("└─"),
            "breakdown=false should not show sub-rows: {output}"
        );
    }

    #[test]
    fn test_breakdown_on_exceeding_shows_sub_rows() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let verdict = make_verdict_with_contributors(
            make_verdict(
                "risky_fn",
                "src/lib.rs",
                5,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            ),
            make_contributors(),
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(&result, 8.0, true);
        assert!(
            output.contains("line 5:"),
            "Should show line 5 contributor: {output}"
        );
        assert!(
            output.contains("line 10:"),
            "Should show line 10 contributor: {output}"
        );
        assert!(
            output.contains("if-branch"),
            "Should show kind 'if-branch': {output}"
        );
        assert!(
            output.contains("for-loop"),
            "Should show kind 'for-loop': {output}"
        );
    }

    #[test]
    fn test_breakdown_on_within_threshold_no_sub_rows() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let verdict = make_verdict_with_contributors(
            make_verdict("safe_fn", "src/lib.rs", 2, 90.0, 2.0, RiskLevel::Low, 8.0),
            make_contributors(),
        );
        // exceeds = crap_value > threshold → 2.0 > 8.0 = false
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_empty_result().summary,
            passed: true,
        };
        let output = format_table(&result, 8.0, true);
        assert!(
            !output.contains("├─"),
            "Non-exceeding fn should not show sub-rows even with breakdown=true: {output}"
        );
    }

    #[test]
    fn test_breakdown_tree_chars_last_uses_corner() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let verdict = make_verdict_with_contributors(
            make_verdict(
                "corner_fn",
                "src/lib.rs",
                5,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            ),
            make_contributors(), // 2 contributors: first ├─, last └─
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(&result, 8.0, true);
        assert!(output.contains("├─"), "Should have branch char: {output}");
        assert!(output.contains("└─"), "Should have corner char: {output}");
        // Make sure corner appears after branch
        let branch_pos = output.find("├─").unwrap();
        let corner_pos = output.find("└─").unwrap();
        assert!(branch_pos < corner_pos, "├─ should appear before └─");
    }

    #[test]
    fn test_breakdown_nesting_suffix() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let verdict = make_verdict_with_contributors(
            make_verdict(
                "nested_fn",
                "src/lib.rs",
                5,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            ),
            make_nested_contributor(), // increment=3
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(&result, 8.0, true);
        assert!(
            output.contains("(nested)"),
            "increment > 1 should show '(nested)': {output}"
        );
        assert!(output.contains("+3"), "Should show +3 increment: {output}");
    }

    #[test]
    fn test_explain_adds_legend_for_nested_breakdown() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let verdict = make_verdict_with_contributors(
            make_verdict(
                "nested_fn",
                "src/lib.rs",
                5,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            ),
            make_nested_contributor(),
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table_with_explain(&result, 8.0, true, true);
        assert!(output.contains("Legend: +1 = base structural increment."));
        assert!(output.contains("+N (nested) = +1 base plus +(N-1)"));
        assert!(output.contains("if/else branches, match arms"));
        assert!(output.contains("while/for/loop bodies"));
        assert!(output.contains("let-else diverging branches, and closures"));
    }

    #[test]
    fn test_explain_without_breakdown_is_inert() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let verdict = make_verdict_with_contributors(
            make_verdict(
                "nested_fn",
                "src/lib.rs",
                5,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            ),
            make_nested_contributor(),
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table_with_explain(&result, 8.0, false, true);
        assert!(!output.contains("Legend:"));
        assert!(!output.contains("line 3: match (+3 (nested))"));
    }

    #[test]
    fn test_explain_suppressed_without_nested_contributors() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        let verdict = make_verdict_with_contributors(
            make_verdict(
                "plain_fn",
                "src/lib.rs",
                5,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            ),
            vec![crate::domain::types::ComplexityContributor {
                kind: crate::domain::types::ContributorKind::Match,
                line: 3,
                column: Some(4),
                increment: 1,
            }],
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table_with_explain(&result, 8.0, true, true);
        assert!(!output.contains("Legend:"));
    }

    #[test]
    fn test_breakdown_sorted_by_line() {
        use crate::domain::types::{ComplexityContributor, ContributorKind};
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);
        // Contributors must be pre-sorted by (line, column) — as FunctionFinder guarantees.
        let verdict = make_verdict_with_contributors(
            make_verdict(
                "mixed_fn",
                "src/lib.rs",
                5,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            ),
            vec![
                ComplexityContributor {
                    kind: ContributorKind::IfBranch,
                    line: 5,
                    column: Some(4),
                    increment: 1,
                },
                ComplexityContributor {
                    kind: ContributorKind::ForLoop,
                    line: 20,
                    column: Some(4),
                    increment: 1,
                },
            ],
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(&result, 8.0, true);
        let line5_pos = output.find("line 5:").unwrap();
        let line20_pos = output.find("line 20:").unwrap();
        assert!(
            line5_pos < line20_pos,
            "line 5 should appear before line 20: {output}"
        );
    }

    #[test]
    fn test_breakdown_substring_name_no_collision() {
        // Regression: sub-row injection uses `line.contains(qualified_name)`.
        // If one name is a prefix of another, both match the same table row.
        // Assert each function's contributors appear exactly once.
        use crate::domain::types::{ComplexityContributor, ContributorKind};
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);

        let v1 = make_verdict_with_contributors(
            make_verdict("parse", "src/lib.rs", 5, 30.0, 45.0, RiskLevel::High, 8.0),
            vec![ComplexityContributor {
                kind: ContributorKind::IfBranch,
                line: 3,
                column: Some(4),
                increment: 1,
            }],
        );
        let v2 = make_verdict_with_contributors(
            make_verdict(
                "parse_extra",
                "src/lib.rs",
                5,
                30.0,
                40.0,
                RiskLevel::High,
                8.0,
            ),
            vec![ComplexityContributor {
                kind: ContributorKind::ForLoop,
                line: 7,
                column: Some(4),
                increment: 1,
            }],
        );
        let result = AnalysisResult {
            functions: vec![v1, v2],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(&result, 8.0, true);

        // Each contributor kind should appear exactly once
        let if_branch_count = output.matches("if-branch").count();
        let for_loop_count = output.matches("for-loop").count();
        assert_eq!(
            if_branch_count, 1,
            "if-branch should appear exactly once (not doubled by prefix match): {output}"
        );
        assert_eq!(
            for_loop_count, 1,
            "for-loop should appear exactly once: {output}"
        );
    }

    #[test]
    fn test_breakdown_name_in_file_path_still_matches() {
        // Regression: row_contains_name used line.find() (first occurrence only).
        // If the function name is a substring of its own file path (e.g. `fn parse()`
        // in `src/parse/mod.rs`), the first match lands in the path column where the
        // next char is `/`, failing the word-boundary check and silently skipping
        // sub-row injection. The fix loops all occurrences until one passes.
        use crate::domain::types::{ComplexityContributor, ContributorKind};
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);

        let v = make_verdict_with_contributors(
            make_verdict(
                "parse",
                "src/parse/mod.rs", // "parse" appears in path before function column
                5,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            ),
            vec![ComplexityContributor {
                kind: ContributorKind::IfBranch,
                line: 3,
                column: Some(4),
                increment: 1,
            }],
        );
        let result = AnalysisResult {
            functions: vec![v],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(&result, 8.0, true);
        assert!(
            output.contains("if-branch"),
            "Sub-row should appear even when function name is in file path: {output}"
        );
    }

    // ── Varied threshold tests ──────────────────────────────────────────

    #[test]
    fn test_varied_thresholds_detected() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);

        let mut result = make_multi_function_result();
        // Set different thresholds on different functions
        result.functions[0].threshold = 5.0;
        result.functions[1].threshold = 10.0;
        result.functions[2].threshold = 8.0;

        let output = format_table(&result, 8.0, false);
        assert!(
            output.contains("varied (default: 8)"),
            "Should show varied threshold: {output}"
        );
    }

    #[test]
    fn test_uniform_thresholds_not_varied() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);

        let result = make_multi_function_result();
        let output = format_table(&result, 8.0, false);
        assert!(
            !output.contains("varied"),
            "Uniform thresholds should not show 'varied': {output}"
        );
    }

    #[test]
    fn test_single_function_not_varied() {
        let _guard = COLOR_LOCK.lock().unwrap();
        colored::control::set_override(false);

        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let output = format_table(&result, 8.0, false);
        assert!(!output.contains("varied"));
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
        let output = format_table(&result, 8.0, false);
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
                .map(|v| v.scored.crap);
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

    fn arb_contributor() -> impl Strategy<Value = crate::domain::types::ComplexityContributor> {
        use crate::domain::types::{ComplexityContributor, ContributorKind};
        (
            prop_oneof![
                Just(ContributorKind::IfBranch),
                Just(ContributorKind::ForLoop),
                Just(ContributorKind::WhileLoop),
                Just(ContributorKind::Match),
                Just(ContributorKind::Try),
                Just(ContributorKind::LogicalOperator),
            ],
            1usize..500,
            1u32..5,
        )
            .prop_map(|(kind, line, increment)| ComplexityContributor {
                kind,
                line,
                column: None,
                increment,
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_format_table_never_panics(result in arb_analysis_result()) {
            let _guard = super::COLOR_LOCK.lock().unwrap();
            colored::control::set_override(false);
            let _ = format_table(&result, 8.0, false);
        }

        #[test]
        fn prop_format_table_with_breakdown_never_panics(
            mut result in arb_analysis_result(),
            contributors in prop::collection::vec(arb_contributor(), 0..5),
        ) {
            let _guard = super::COLOR_LOCK.lock().unwrap();
            colored::control::set_override(false);
            // Inject contributors into the first exceeding function (if any)
            if let Some(v) = result.functions.iter_mut().find(|v| v.exceeds) {
                v.scored.contributors = contributors;
            }
            // Must not panic regardless of contributor content
            let _ = format_table(&result, 8.0, true);
        }

        #[test]
        fn prop_format_table_row_count(result in arb_analysis_result()) {
            let _guard = super::COLOR_LOCK.lock().unwrap();
            colored::control::set_override(false);
            let output = format_table(&result, 8.0, false);
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
