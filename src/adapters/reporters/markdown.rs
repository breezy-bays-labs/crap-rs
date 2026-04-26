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
/// `breakdown` injects an indented bullet list of complexity
/// contributors under each exceeding function. `explain` adds a
/// trailing legend describing increment semantics (only meaningful
/// when `breakdown` is set).
///
/// When `delta` is `Some`, a `## CRAP Scorecard` section is appended
/// after the analysis summary — designed for PR-comment rendering.
/// When `delta` is `None`, output is byte-identical to today.
pub fn format_markdown(
    view: &AnalysisView<'_>,
    delta: Option<&DeltaView<'_>>,
    threshold: f64,
    breakdown: bool,
    explain: bool,
) -> String {
    let mut out = format_markdown_body(view, threshold, breakdown, explain);
    if let Some(delta_view) = delta {
        out.push('\n');
        out.push_str(&format_markdown_delta(delta_view));
    }
    out
}

/// Render the analysis-only body (no delta block). Branches on
/// empty / grouped / per-function paths; the delta block is appended
/// once by the caller.
fn format_markdown_body(
    view: &AnalysisView<'_>,
    threshold: f64,
    breakdown: bool,
    explain: bool,
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

    if view.grouped.is_some() {
        out.push_str(&format_grouped_table_md(view));
        out.push('\n');
        out.push_str(&summary_block(view, threshold));
        return out;
    }

    out.push_str("| File | Function | CC | Cov% | CRAP | Risk |\n");
    out.push_str("|------|----------|----|------|------|------|\n");
    for verdict in view.shown.iter() {
        out.push_str(&row_for(verdict));
        out.push('\n');
        append_breakdown_bullets(&mut out, verdict, breakdown);
    }

    if breakdown && explain && needs_legend(view) {
        out.push_str(
            "\n_Legend: +1 = base structural increment. +N (nested) = +1 base plus +(N-1) from active nesting depth (if/else, match arms, while/for/loop, let-else diverging branches, closures)._\n",
        );
    }

    out.push('\n');
    out.push_str(&summary_block(view, threshold));
    out
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

    let regressions: Vec<&FunctionChange> = view
        .shown
        .iter()
        .copied()
        .filter(|c| {
            matches!(c, FunctionChange::Modified { .. }) && c.score_delta().unwrap_or(0.0) > 0.0
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
    let worst = summary
        .max_crap
        .map(|c| format!("{:.1}", c.value))
        .unwrap_or_else(|| "N/A".to_string());

    let threshold_display = if has_varied_thresholds(&view.full.functions) {
        format!("varied (default: {})", threshold)
    } else {
        format!("{}", threshold)
    };

    let d = &summary.distribution;
    format!(
        "## Summary\n\n\
         - **Result:** {pass_fail}\n\
         - **Functions:** {} ({} above threshold {})\n\
         - **Worst CRAP:** {worst}\n\
         - **Average CRAP:** {:.1}\n\
         - **Median CRAP:** {:.1}\n\
         - **Distribution:** low {} · acceptable {} · moderate {} · high {}\n",
        summary.total_functions,
        summary.exceeding_threshold,
        threshold_display,
        summary.average_crap,
        summary.median_crap,
        d.low,
        d.acceptable,
        d.moderate,
        d.high,
    )
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
        let out = format_markdown(&make_view_default(&result), None, 8.0, false, false);
        assert!(out.contains("| File | Function | CC | Cov% | CRAP | Risk |"));
        assert!(out.contains("|------|"));
    }

    #[test]
    fn empty_analysis_says_no_functions() {
        let result = make_empty_result();
        let out = format_markdown(&make_view_default(&result), None, 8.0, false, false);
        assert!(out.contains("No functions analyzed"));
        assert!(!out.contains("| File |"));
    }

    #[test]
    fn pipe_in_function_name_is_escaped() {
        let result =
            make_single_function_result("a|b", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let out = format_markdown(&make_view_default(&result), None, 8.0, false, false);
        assert!(out.contains("a\\|b"), "expected escaped pipe in: {out}");
    }

    #[test]
    fn summary_reflects_full_analysis_not_view() {
        // Even if the view is filtered, summary derives from view.full
        // (the gate keystone).
        let result = make_multi_function_result();
        let out = format_markdown(&make_view_default(&result), None, 8.0, false, false);
        assert!(out.contains("- **Result:** FAIL"));
        assert!(out.contains("3 (2 above threshold 8)"));
    }

    #[test]
    fn full_markdown_snapshot() {
        let result = make_multi_function_result();
        let out = format_markdown(&make_view_default(&result), None, 8.0, false, false);
        insta::assert_snapshot!(out);
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
                },
                ComplexityContributor {
                    kind: ContributorKind::ForLoop,
                    line: 10,
                    column: Some(4),
                    increment: 2,
                },
            ],
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let out = format_markdown(&make_view_default(&result), None, 8.0, true, true);
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
        let out = format_markdown(&view, None, 8.0, false, false);
        assert!(out.contains("| File | Functions | Failing | Avg CRAP | Worst CRAP | Worst Fn |"));
        // Per-function CC/Cov% absent
        assert!(!out.contains("| CC |"));
        assert!(!out.contains("| Cov% |"));
        // Summary block intact (3 functions, 2 above threshold)
        assert!(out.contains("3 (2 above threshold 8)"));
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
        let out = format_markdown(&view, None, 8.0, false, false);
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
        );
        assert!(out.contains("### New violations"));
        assert!(out.contains("new_fn"));
    }

    #[test]
    fn no_baseline_means_no_scorecard_block() {
        let result = make_multi_function_result();
        let out = format_markdown(&make_view_default(&result), None, 8.0, false, false);
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
        );
        insta::assert_snapshot!(out);
    }
}
