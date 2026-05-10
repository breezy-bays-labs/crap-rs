//! Markdown reporter — formats an `AnalysisView` as GitHub-flavored
//! Markdown with a pipe-syntax table and a readable summary block.
//!
//! No ANSI. Suitable for piping into PR comments, issue bodies, or
//! documentation.

use crate::domain::delta::{DeltaView, FunctionChange};
use crate::domain::types::FunctionVerdict;
use crate::domain::view::AnalysisView;

/// Format an `AnalysisView` as GitHub-flavored Markdown.
///
/// Default body shape: title + summary block (multi-metric stats +
/// risk distribution) + a top-N spotlight (failures if any exceed
/// threshold, otherwise the worst by CRAP). Designed to fit comfortably
/// in a PR comment — bounded output regardless of codebase size.
///
/// `breakdown` injects an indented bullet list of complexity
/// contributors under each exceeding function in the spotlight (or
/// the full table when `full_table` is set). `explain` adds a trailing
/// legend describing increment semantics (only meaningful when
/// `breakdown` is set).
///
/// `full_table` switches the body to the legacy row-per-function table
/// rendered after the summary — useful when piping into a longer
/// document instead of a PR comment. Off by default.
///
/// `top_n` bounds the spotlight table size. The summary block is
/// always full-fidelity (computed from `view.full.summary`).
///
/// When `delta` is `Some`, a `## CRAP Scorecard` section is appended
/// after the analysis body — designed for PR-comment rendering.
pub fn format_markdown(
    view: &AnalysisView<'_>,
    delta: Option<&DeltaView<'_>>,
    threshold: f64,
    breakdown: bool,
    explain: bool,
    full_table: bool,
    top_n: usize,
) -> String {
    let mut out = format_markdown_body(view, threshold, breakdown, explain, full_table, top_n);
    if let Some(delta_view) = delta {
        out.push('\n');
        out.push_str(&format_markdown_delta(delta_view));
    }
    out
}

/// Render the analysis-only body (no delta block). Branches on
/// empty / grouped / spotlight / full-table paths; the delta block is
/// appended once by the caller.
fn format_markdown_body(
    view: &AnalysisView<'_>,
    threshold: f64,
    breakdown: bool,
    explain: bool,
    full_table: bool,
    top_n: usize,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# crap4rs v{} — CRAP Score Analysis\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    if view.full.functions.is_empty() {
        out.push_str("No functions analyzed.\n");
        return out;
    }

    out.push_str(&summary_block(view, threshold));

    if view.grouped.is_some() {
        out.push_str("\n## Per-file aggregates\n\n");
        out.push_str(&format_grouped_table_md(view));
        return out;
    }

    out.push('\n');
    if full_table {
        out.push_str(&full_table_block(view, breakdown, explain));
    } else {
        out.push_str(&spotlight_block(view, threshold, top_n, breakdown, explain));
    }

    out
}

/// Default body section: top-N failures when violations exist,
/// otherwise the worst-by-CRAP slice. Bounded to `top_n` rows.
/// Honors `--breakdown` and `--explain` so the spotlight matches the
/// legacy full-table format's level of detail.
fn spotlight_block(
    view: &AnalysisView<'_>,
    threshold: f64,
    top_n: usize,
    breakdown: bool,
    explain: bool,
) -> String {
    let mut out = String::new();
    let summary = &view.full.summary;

    if summary.exceeding_threshold == 0 {
        // Clean run — show the hot spots so reviewers see what's
        // approaching threshold.
        let worst = top_n_by_crap(view.shown.iter().copied(), top_n);
        if worst.is_empty() {
            return out;
        }
        out.push_str(&format!("## Top {} worst by CRAP\n\n", worst.len()));
        out.push_str(&function_table(&worst, breakdown));
        if breakdown && explain && needs_legend(view) {
            out.push_str(LEGEND);
        }
        out.push_str("\n_All functions are within threshold._\n");
        return out;
    }

    // Failure path. Render up to `top_n` failures sorted CRAP-desc.
    let shown_failures = top_n_by_crap(view.shown.iter().copied().filter(|v| v.exceeds), top_n);

    let header = if summary.exceeding_threshold > shown_failures.len() {
        format!(
            "## Failures (top {} of {} above threshold {})\n\n",
            shown_failures.len(),
            summary.exceeding_threshold,
            format_threshold(view, threshold),
        )
    } else {
        format!(
            "## Failures ({} above threshold {})\n\n",
            summary.exceeding_threshold,
            format_threshold(view, threshold),
        )
    };
    out.push_str(&header);
    out.push_str(&function_table(&shown_failures, breakdown));
    if breakdown && explain && needs_legend(view) {
        out.push_str(LEGEND);
    }
    out
}

fn top_n_by_crap<'a, I>(iter: I, n: usize) -> Vec<&'a FunctionVerdict>
where
    I: IntoIterator<Item = &'a FunctionVerdict>,
{
    let mut v: Vec<&FunctionVerdict> = iter.into_iter().collect();
    v.sort_by(|a, b| {
        b.scored
            .crap
            .value
            .partial_cmp(&a.scored.crap.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(n);
    v
}

fn function_table(verdicts: &[&FunctionVerdict], breakdown: bool) -> String {
    let mut out = String::new();
    out.push_str("| File | Function | CC | Cov% | CRAP | Risk |\n");
    out.push_str("|------|----------|----|------|------|------|\n");
    for v in verdicts {
        out.push_str(&row_for(v));
        out.push('\n');
        append_breakdown_bullets(&mut out, v, breakdown);
    }
    out
}

const LEGEND: &str = "\n_Legend: +1 = base structural increment. +N (nested) = +1 base plus +(N-1) from active nesting depth (if/else, match arms, while/for/loop, let-else diverging branches, closures)._\n";

/// Legacy row-per-function table (preserved behind `--md-full-table`).
fn full_table_block(view: &AnalysisView<'_>, breakdown: bool, explain: bool) -> String {
    let mut out = String::new();
    out.push_str("## All functions\n\n");
    out.push_str(&function_table(view.shown.as_slice(), breakdown));
    if breakdown && explain && needs_legend(view) {
        out.push_str(LEGEND);
    }
    out
}

fn format_threshold(view: &AnalysisView<'_>, threshold: f64) -> String {
    if has_varied_thresholds(&view.full.functions) {
        format!("varied (default: {})", threshold)
    } else {
        format!("{}", threshold)
    }
}

fn append_breakdown_bullets(out: &mut String, verdict: &FunctionVerdict, breakdown: bool) {
    if !breakdown || !verdict.exceeds || verdict.scored.contributors.is_empty() {
        return;
    }
    for c in verdict.scored.contributors.iter() {
        out.push_str(&format!("  - L{} {} +{}\n", c.line, c.kind, c.increment));
    }
}

fn needs_legend(view: &AnalysisView<'_>) -> bool {
    view.shown
        .iter()
        .filter(|v| v.exceeds)
        .flat_map(|v| v.scored.contributors.iter())
        .any(|c| c.increment > 1)
}

/// Render the delta scorecard block. Format is stable enough to drop
/// into PR comments verbatim. Counts come from `view.full.summary`
/// (the unshapeable gate); regression / new-violation tables iterate
/// `view.shown` so `--delta-top` / `--delta-only` shape the rendered
/// rows but not the counts.
fn format_markdown_delta(view: &DeltaView<'_>) -> String {
    let summary = &view.full.summary;
    let status = if summary.passed { "PASS" } else { "FAIL" };

    let mut out = String::new();
    out.push_str("## CRAP Scorecard\n\n");
    out.push_str(&format!("- **Delta status:** {status}\n"));
    out.push_str(&format!(
        "- **Changes:** +{added} added, {removed} removed, {modified} modified\n",
        added = summary.added,
        removed = summary.removed,
        modified = summary.modified,
    ));
    out.push_str(&format!(
        "- **Regressions:** {regressions} · **Improvements:** {improvements} · **New violations:** {new_violations}\n",
        regressions = summary.regressions,
        improvements = summary.improvements,
        new_violations = summary.new_violations,
    ));

    // Filter threshold matches the `{:.2}` cell-rendering precision:
    // a delta below 0.005 rounds to "+0.00" in the table and looks
    // like a falsely-flagged regression. Anything that rounds up to
    // ≥ +0.01 is admitted. (CrapScore values are themselves
    // 2-decimal rounded, so this gate rarely fires in practice — but
    // float arithmetic can produce sub-0.005 noise on identity
    // comparisons.)
    let regressions: Vec<&FunctionChange> = view
        .shown
        .iter()
        .copied()
        .filter(|c| {
            matches!(c, FunctionChange::Modified { .. }) && c.score_delta().unwrap_or(0.0) >= 0.005
        })
        .collect();
    if !regressions.is_empty() {
        out.push_str("\n### Regressions\n\n");
        out.push_str("| File | Function | Baseline CRAP | Current CRAP | Δ |\n");
        out.push_str("|------|----------|--------------:|-------------:|--:|\n");
        for change in regressions {
            let baseline = change.baseline_score().unwrap_or(0.0);
            let current = change.current_score().unwrap_or(0.0);
            let delta = change.score_delta().unwrap_or(0.0);
            out.push_str(&format!(
                "| {} | {} | {:.2} | {:.2} | +{:.2} |\n",
                escape_cell(change.file_path()),
                escape_cell(change.qualified_name()),
                baseline,
                current,
                delta,
            ));
        }
    }

    let new_violations: Vec<&FunctionChange> = view
        .shown
        .iter()
        .copied()
        .filter(|c| match c {
            FunctionChange::Added { current } => current.exceeds,
            FunctionChange::Modified { baseline, current } => !baseline.exceeds && current.exceeds,
            FunctionChange::Removed { .. } => false,
            // `FunctionChange` is `#[non_exhaustive]` and lives in
            // crap-core. Future variants default to "not a new violation"
            // until the reporter is updated.
            _ => false,
        })
        .collect();
    if !new_violations.is_empty() {
        out.push_str("\n### New violations\n\n");
        out.push_str("| File | Function | Current CRAP |\n");
        out.push_str("|------|----------|-------------:|\n");
        for change in new_violations {
            let current = change.current_score().unwrap_or(0.0);
            out.push_str(&format!(
                "| {} | {} | {:.2} |\n",
                escape_cell(change.file_path()),
                escape_cell(change.qualified_name()),
                current,
            ));
        }
    }

    out
}

fn row_for(verdict: &FunctionVerdict) -> String {
    let s = &verdict.scored;
    format!(
        "| {} | {} | {} | {:.1} | {:.2} | {} |",
        escape_cell(&s.identity.file_path),
        escape_cell(&s.identity.qualified_name),
        s.complexity,
        s.coverage_percent,
        s.crap.value,
        s.crap.risk_level,
    )
}

/// Escape characters with special meaning inside a GFM table cell.
/// Pipes break the cell boundary; backslashes can interfere with
/// downstream rendering. Newlines are replaced with spaces — qualified
/// names and file paths shouldn't contain them, but defend anyway.
fn escape_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '|' => out.push_str("\\|"),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn summary_block(view: &AnalysisView<'_>, threshold: f64) -> String {
    let summary = &view.full.summary;
    let pass_fail = if view.full.passed { "PASS" } else { "FAIL" };
    let crap_max = summary
        .max_crap
        .as_ref()
        .map(|c| format!("{:.2}", c.value))
        .unwrap_or_else(|| "—".to_string());
    let threshold_display = format_threshold(view, threshold);
    let d = &summary.distribution;

    let mut out = String::new();
    out.push_str("## Summary\n\n");
    out.push_str(&format!(
        "**Result:** {pass_fail} · **Functions:** {} · **Above threshold ({threshold_display}):** {}\n\n",
        summary.total_functions, summary.exceeding_threshold,
    ));
    out.push_str("| Metric     | Worst | Average | Median |\n");
    out.push_str("|------------|------:|--------:|-------:|\n");
    out.push_str(&format!(
        "| CRAP       | {crap_max} | {:>7.2} | {:>6.2} |\n",
        summary.average_crap, summary.median_crap,
    ));
    out.push_str(&format!(
        "| Complexity | {:>5} | {:>7.1} | {:>6.1} |\n",
        summary.max_complexity, summary.average_complexity, summary.median_complexity,
    ));
    out.push_str(&format!(
        "| Coverage   | {min:>4.1}% | {avg:>6.1}% | {median:>5.1}% |\n",
        min = summary.min_coverage,
        avg = summary.average_coverage,
        median = summary.median_coverage,
    ));
    out.push_str(&format!(
        "\n**Risk distribution:** low {} · acceptable {} · moderate {} · high {}\n",
        d.low, d.acceptable, d.moderate, d.high,
    ));
    out
}

fn format_grouped_table_md(view: &AnalysisView<'_>) -> String {
    let grouped = view
        .grouped
        .as_ref()
        .expect("format_grouped_table_md called without grouped block");
    let mut out = String::new();
    out.push_str("| File | Functions | Failing | Avg CRAP | Worst CRAP | Worst Fn |\n");
    out.push_str("|------|-----------|---------|----------|------------|----------|\n");
    for f in &grouped.files {
        let worst_crap = f
            .max_crap
            .as_ref()
            .map(|c| format!("{:.2}", c.value))
            .unwrap_or_else(|| "N/A".to_string());
        let worst_fn = f
            .worst_function
            .as_ref()
            .map(|id| escape_cell(&id.qualified_name))
            .unwrap_or_else(|| "—".to_string());
        out.push_str(&format!(
            "| {} | {} | {} | {:.2} | {} | {} |\n",
            escape_cell(&f.file_path),
            f.function_count,
            f.exceeding_count,
            f.average_crap,
            worst_crap,
            worst_fn,
        ));
    }
    out
}

fn has_varied_thresholds(functions: &[FunctionVerdict]) -> bool {
    let mut iter = functions.iter().map(|v| v.threshold);
    let Some(first) = iter.next() else {
        return false;
    };
    iter.any(|t| (t - first).abs() > f64::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;

    #[test]
    fn header_row_pipes_and_columns() {
        let result = make_multi_function_result();
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            false,
            10,
        );
        assert!(out.contains("| File | Function | CC | Cov% | CRAP | Risk |"));
        assert!(out.contains("|------|"));
    }

    #[test]
    fn empty_analysis_says_no_functions() {
        let result = make_empty_result();
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            false,
            10,
        );
        assert!(out.contains("No functions analyzed"));
        assert!(!out.contains("| File |"));
    }

    #[test]
    fn pipe_in_function_name_is_escaped() {
        let result =
            make_single_function_result("a|b", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            false,
            10,
        );
        assert!(out.contains("a\\|b"), "expected escaped pipe in: {out}");
    }

    #[test]
    fn summary_reflects_full_analysis_not_view() {
        // Even if the view is filtered, summary derives from view.full
        // (the gate keystone).
        let result = make_multi_function_result();
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            false,
            10,
        );
        assert!(out.contains("**Result:** FAIL"));
        assert!(out.contains("**Functions:** 3"));
        assert!(out.contains("**Above threshold (8):** 2"));
    }

    #[test]
    fn full_markdown_snapshot() {
        let result = make_multi_function_result();
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            false,
            10,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn md_full_table_renders_all_functions_section() {
        let result = make_multi_function_result();
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            true, // --md-full-table
            10,
        );
        // Summary block still leads.
        assert!(out.contains("## Summary"));
        // Full-table escape hatch renders the legacy section.
        assert!(out.contains("## All functions"));
        // Every function row is present (3 in the fixture).
        assert!(out.contains("complex_fn"));
        assert!(out.contains("parse_record"));
        assert!(out.contains("simple_fn"));
        // No spotlight section when full-table is requested.
        assert!(!out.contains("## Failures"));
        assert!(!out.contains("## Top "));
    }

    #[test]
    fn md_full_table_with_breakdown_includes_contributors_and_legend() {
        use crate::domain::types::{AnalysisResult, ComplexityContributor, ContributorKind};
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
            vec![
                ComplexityContributor {
                    kind: ContributorKind::IfBranch,
                    line: 12,
                    column: None,
                    increment: 1,
                    end_line: 12,
                    nesting_depth: 0,
                },
                ComplexityContributor {
                    kind: ContributorKind::Match,
                    line: 18,
                    column: None,
                    increment: 2, // nested → triggers legend
                    end_line: 18,
                    nesting_depth: 1,
                },
            ],
        );
        let result = AnalysisResult {
            functions: vec![verdict.clone()],
            summary: crate::domain::summary::compute_summary(std::slice::from_ref(&verdict)),
            passed: false,
        };
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            true, // --breakdown
            true, // --explain
            true, // --md-full-table
            10,
        );
        assert!(out.contains("## All functions"));
        // Breakdown bullets rendered for the exceeding row.
        assert!(out.contains("L12 if-branch +1"));
        assert!(out.contains("L18 match +2"));
        // Explain legend present (any nested increment > 1).
        assert!(out.contains("Legend:"));
    }

    #[test]
    fn full_markdown_breakdown_snapshot() {
        use crate::domain::types::{AnalysisResult, ComplexityContributor, ContributorKind};
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
            vec![
                ComplexityContributor {
                    kind: ContributorKind::IfBranch,
                    line: 5,
                    column: Some(4),
                    increment: 1,
                    end_line: 5,
                    nesting_depth: 0,
                },
                ComplexityContributor {
                    kind: ContributorKind::ForLoop,
                    line: 10,
                    column: Some(4),
                    increment: 2,
                    end_line: 10,
                    nesting_depth: 1,
                },
            ],
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            true,
            true,
            false,
            10,
        );
        insta::assert_snapshot!(out);
    }

    use crate::domain::types::RiskLevel;

    #[test]
    fn grouped_markdown_has_per_file_header() {
        use crate::domain::view::{self, GroupKey, ViewSpec};
        let result = make_multi_function_result();
        let view = view::apply(
            &result,
            ViewSpec {
                group_by: Some(GroupKey::File),
                ..Default::default()
            },
        );
        let out = format_markdown(&view, None, 8.0, false, false, false, 10);
        assert!(out.contains("| File | Functions | Failing | Avg CRAP | Worst CRAP | Worst Fn |"));
        // Per-function CC/Cov% headers absent in the per-file aggregate table
        assert!(!out.contains("| File | Function | CC |"));
        // Summary block intact (3 functions, 2 above threshold 8)
        assert!(out.contains("**Functions:** 3"));
        assert!(out.contains("**Above threshold (8):** 2"));
    }

    #[test]
    fn grouped_markdown_snapshot() {
        use crate::domain::view::{self, GroupKey, ViewSpec};
        let result = make_multi_function_result();
        let view = view::apply(
            &result,
            ViewSpec {
                group_by: Some(GroupKey::File),
                ..Default::default()
            },
        );
        let out = format_markdown(&view, None, 8.0, false, false, false, 10);
        insta::assert_snapshot!(out);
    }

    // ── Delta scorecard (VS5) ───────────────────────────────────────

    #[test]
    fn delta_scorecard_includes_status_and_counts() {
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = format_markdown(
            &make_view_default(&delta.current),
            Some(&dview),
            8.0,
            false,
            false,
            false,
            10,
        );
        assert!(out.contains("## CRAP Scorecard"));
        // Status reflects the delta gate (new_violations > 0 → FAIL)
        assert!(out.contains("- **Delta status:** FAIL"));
        assert!(out.contains("+1 added, 1 removed, 2 modified"));
        // new_fn is the only new violation; parse_record's baseline already exceeded.
        assert!(out.contains("**New violations:** 1"));
    }

    #[test]
    fn delta_scorecard_renders_regressions_table_when_present() {
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = format_markdown(
            &make_view_default(&delta.current),
            Some(&dview),
            8.0,
            false,
            false,
            false,
            10,
        );
        assert!(out.contains("### Regressions"));
        // parse_record went 15.0 → 22.0
        assert!(out.contains("parse_record"));
        assert!(out.contains("+7.00"));
    }

    #[test]
    fn delta_scorecard_renders_new_violations_table() {
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = format_markdown(
            &make_view_default(&delta.current),
            Some(&dview),
            8.0,
            false,
            false,
            false,
            10,
        );
        assert!(out.contains("### New violations"));
        assert!(out.contains("new_fn"));
    }

    #[test]
    fn no_baseline_means_no_scorecard_block() {
        let result = make_multi_function_result();
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            false,
            10,
        );
        assert!(!out.contains("CRAP Scorecard"));
        assert!(!out.contains("Delta status"));
    }

    #[test]
    fn full_markdown_with_delta_snapshot() {
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = format_markdown(
            &make_view_default(&delta.current),
            Some(&dview),
            8.0,
            false,
            false,
            false,
            10,
        );
        insta::assert_snapshot!(out);
    }
}
