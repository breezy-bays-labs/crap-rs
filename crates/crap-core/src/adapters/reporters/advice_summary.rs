//! Stderr summary reporter for `--format advice`.
//!
//! Format:
//! `[crap=N.NN] file:line-line qualified::name [actions: <kinds>]`
//!
//! One line per over-threshold function in `view.shown[]` order so
//! humans glancing at CI logs see a deterministic, grep-friendly
//! summary alongside the JSON envelope on stdout.
//!
//! Plain text rather than `comfy-table`: lines are not aligned across
//! rows because column widths would shift the moment a long
//! `qualified_name` appeared. Tools like `awk` and `grep` parse this
//! stream more reliably without padding.

use std::io::{self, Write};

use crate::domain::types::FunctionVerdict;
use crate::domain::view::AnalysisView;

/// Render the stderr summary for `--format advice`. Writes one line
/// per over-threshold verdict in `view.shown` order. Returns `Ok(())`
/// even when no verdicts exceed (callers can always invoke this).
pub fn render_summary(view: &AnalysisView<'_>, sink: &mut impl Write) -> io::Result<()> {
    for verdict in &view.shown {
        if !verdict.exceeds {
            continue;
        }
        writeln!(sink, "{}", format_line(verdict))?;
    }
    Ok(())
}

fn format_line(verdict: &FunctionVerdict) -> String {
    let identity = &verdict.scored.identity;
    let span = &identity.span;
    let actions = format_actions(verdict);
    format!(
        "[crap={crap:.2}] {file}:{start}-{end} {name} [actions: {actions}]",
        crap = verdict.scored.crap.value,
        file = identity.file_path,
        start = span.start_line,
        end = span.end_line,
        name = identity.qualified_name,
        actions = actions,
    )
}

fn format_actions(verdict: &FunctionVerdict) -> String {
    let Some(diag) = verdict.diagnostic.as_deref() else {
        // No diagnostic populated (caller didn't request advice). Emit
        // a sentinel so the line is still well-formed for grep/awk.
        return "none".to_string();
    };
    if diag.suggested_actions.is_empty() {
        return "none".to_string();
    }
    diag.suggested_actions
        .iter()
        .map(action_kind_label)
        .collect::<Vec<_>>()
        .join(",")
}

fn action_kind_label(action: &crate::domain::diagnostic::SuggestedAction) -> &'static str {
    use crate::domain::diagnostic::SuggestedAction::*;
    // `SuggestedAction` is `#[non_exhaustive]` paused per D10 amendment
    // (#147 restores at v1.0). Now that this reporter lives in crap-core
    // alongside the enum, the match is in-crate and exhaustive — no
    // wildcard arm needed. v1.0 new variants will require explicit
    // arms here; #147 covers that update path.
    match action {
        AddTestsForLines { .. } => "add_tests_for_lines",
        ExtractFunction { .. } => "extract_function",
        SimplifyBranching { .. } => "simplify_branching",
        AcceptInherentComplexity { .. } => "accept_inherent_complexity",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagnostic::{
        Applicability, Diagnostic, LineRange, ProposedSplit, RootCause, SplitKind, SuggestedAction,
    };
    use crate::domain::types::{
        AnalysisResult, AnalysisSummary, ComplexityMetric, ContributorKind, CrapScore,
        FunctionIdentity, RiskLevel, ScoredFunction, SourceSpan,
    };
    use crate::domain::view::{self, ViewSpec};

    fn make_result(verdicts: Vec<FunctionVerdict>) -> AnalysisResult {
        AnalysisResult {
            passed: verdicts.iter().all(|v| !v.exceeds),
            functions: verdicts,
            summary: AnalysisSummary::default(),
        }
    }

    fn make_verdict(name: &str, file: &str, line_start: usize, line_end: usize) -> FunctionVerdict {
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: line_start,
                        end_line: line_end,
                        start_column: 0,
                        end_column: 0,
                    },
                },
                complexity: 5,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: 50.0,
                crap: CrapScore {
                    value: 12.34,
                    risk_level: RiskLevel::Moderate,
                },
                contributors: vec![],
            },
            threshold: 5.0,
            exceeds: true,
            diagnostic: None,
        }
    }

    #[test]
    fn render_summary_emits_one_line_per_exceeding_verdict() {
        let mut v = make_verdict("foo", "src/lib.rs", 10, 20);
        v.diagnostic = Some(Box::new(Diagnostic {
            coverage_gaps: vec![LineRange::new(11, 13)],
            complexity_drivers: vec![],
            suggested_actions: vec![
                SuggestedAction::AddTestsForLines {
                    lines: vec![LineRange::new(11, 13)],
                    applicability: Applicability::default(),
                },
                SuggestedAction::ExtractFunction {
                    candidates: vec![ProposedSplit {
                        line_range: LineRange::new(11, 19),
                        complexity_contribution: 4,
                        branch_path: "if-branch".to_string(),
                        kind: SplitKind::DeepestNesting,
                        recommended: true,
                    }],
                    applicability: Applicability::default(),
                },
            ],
            root_cause: RootCause::Both,
        }));
        let result = make_result(vec![v]);
        let view = view::apply(&result, ViewSpec::default());
        let mut sink = Vec::new();
        render_summary(&view, &mut sink).unwrap();
        let out = String::from_utf8(sink).unwrap();
        assert_eq!(
            out,
            "[crap=12.34] src/lib.rs:10-20 foo [actions: add_tests_for_lines,extract_function]\n"
        );
    }

    #[test]
    fn render_summary_skips_passing_verdicts() {
        let mut v = make_verdict("under", "src/lib.rs", 1, 5);
        v.exceeds = false;
        let result = make_result(vec![v]);
        let view = view::apply(&result, ViewSpec::default());
        let mut sink = Vec::new();
        render_summary(&view, &mut sink).unwrap();
        assert!(sink.is_empty());
    }

    #[test]
    fn render_summary_emits_none_when_diagnostic_absent() {
        // Defensive — `--format advice` always populates, but the
        // formatter should still be well-formed if a caller skips the
        // gate (e.g., direct API consumer).
        let v = make_verdict("naked", "src/lib.rs", 1, 9);
        let result = make_result(vec![v]);
        let view = view::apply(&result, ViewSpec::default());
        let mut sink = Vec::new();
        render_summary(&view, &mut sink).unwrap();
        let out = String::from_utf8(sink).unwrap();
        assert_eq!(out, "[crap=12.34] src/lib.rs:1-9 naked [actions: none]\n");
    }

    #[test]
    fn render_summary_preserves_view_shown_order() {
        let mut a = make_verdict("a_first", "src/lib.rs", 1, 5);
        a.scored.crap.value = 10.0;
        let mut b = make_verdict("b_second", "src/lib.rs", 6, 10);
        b.scored.crap.value = 30.0;
        // Default sort = CRAP descending, so b should appear first.
        let result = make_result(vec![a, b]);
        let view = view::apply(&result, ViewSpec::default());
        let mut sink = Vec::new();
        render_summary(&view, &mut sink).unwrap();
        let out = String::from_utf8(sink).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("b_second"));
        assert!(lines[1].contains("a_first"));
    }

    /// Byte-level snapshot lock for the advice_summary reporter
    /// (#142). The format is plain text with no ANSI / no
    /// terminal-width dependency, so the snapshot is fully
    /// deterministic by construction.
    ///
    /// Covers all four `SuggestedAction` variants (the entire
    /// `action_kind_label` match) plus the `[actions: none]` branch
    /// (empty suggested_actions vec) so any future variant
    /// addition that misses an arm or relabels surfaces here.
    /// CRAP values are distinct and spread across `RiskLevel`s so
    /// the default-sort ordering is also pinned by the snapshot.
    #[test]
    fn advice_summary_snapshot() {
        // Pin threshold to 8.0 across all verdicts to match the
        // `make_multi_function_result` convention used by sibling
        // reporter tests. The value does not appear in
        // advice_summary's output (`format_line` only includes
        // `crap`, `file`, `lines`, `name`, `actions`), so the
        // snapshot is unaffected — but pinning here keeps future
        // test edits honest (e.g., a future passing-score case
        // would otherwise quietly exceed the default 5.0).
        let mut high = make_verdict("complex_handler", "src/cli/mod.rs", 600, 720);
        high.threshold = 8.0;
        high.scored.crap.value = 45.20;
        high.scored.crap.risk_level = RiskLevel::High;
        high.diagnostic = Some(Box::new(Diagnostic {
            coverage_gaps: vec![LineRange::new(605, 612), LineRange::new(640, 660)],
            complexity_drivers: vec![],
            suggested_actions: vec![
                SuggestedAction::AddTestsForLines {
                    lines: vec![LineRange::new(605, 612)],
                    applicability: Applicability::default(),
                },
                SuggestedAction::ExtractFunction {
                    candidates: vec![ProposedSplit {
                        line_range: LineRange::new(640, 700),
                        complexity_contribution: 12,
                        branch_path: "match-arms".to_string(),
                        kind: SplitKind::DeepestNesting,
                        recommended: true,
                    }],
                    applicability: Applicability::default(),
                },
            ],
            root_cause: RootCause::Both,
        }));

        let mut moderate = make_verdict("parse_record", "src/adapters/coverage/mod.rs", 80, 145);
        moderate.threshold = 8.0;
        moderate.scored.crap.value = 15.00;
        moderate.scored.crap.risk_level = RiskLevel::Moderate;
        moderate.diagnostic = Some(Box::new(Diagnostic {
            coverage_gaps: vec![],
            complexity_drivers: vec![],
            suggested_actions: vec![
                SuggestedAction::SimplifyBranching {
                    drivers: vec![ContributorKind::Match, ContributorKind::MatchArm],
                    applicability: Applicability::default(),
                },
                SuggestedAction::AcceptInherentComplexity {
                    applicability: Applicability::default(),
                },
            ],
            root_cause: RootCause::HighComplexity,
        }));

        let mut bare = make_verdict("opaque_helper", "src/util.rs", 30, 42);
        bare.threshold = 8.0;
        bare.scored.crap.value = 8.50;
        bare.scored.crap.risk_level = RiskLevel::Acceptable;
        // No diagnostic populated — exercises the `none` sentinel branch.

        let result = make_result(vec![high, moderate, bare]);
        let view = view::apply(&result, ViewSpec::default());
        let mut sink = Vec::new();
        render_summary(&view, &mut sink).unwrap();
        let out = String::from_utf8(sink).unwrap();
        insta::assert_snapshot!(out);
    }
}
