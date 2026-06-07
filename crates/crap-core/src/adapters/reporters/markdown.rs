//! Markdown reporter — formats an `AnalysisView` as GitHub-flavored
//! Markdown with a pipe-syntax table and a readable summary block.
//!
//! No ANSI. Suitable for piping into PR comments, issue bodies, or
//! documentation.
//!
//! Rendering goes through an askama compile-time template
//! (`crates/crap-core/templates/markdown_report.txt`).
//! Width-aligned numeric fields are pre-formatted in Rust because
//! askama's `{{ }}` interpolation does not honor Rust format
//! specifiers; the template is composition-only.

use crate::cli::AdapterMeta;
use crate::domain::delta::{DeltaView, FunctionChange, change_is_new_violation};
use crate::domain::types::{ComplexityMetric, FunctionVerdict};
use crate::domain::view::AnalysisView;
use askama::Template;

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
///
/// `meta` carries the calling binary's identity (the literal
/// `env!("CARGO_PKG_NAME")` value resolves to `crap-core` here, not
/// the adapter binary's name — so the binary supplies its own). `meta`
/// is the full `&AdapterMeta` bundle: the markdown reporter only
/// consumes `tool_name` and `tool_version`, but takes the whole bundle
/// for signature symmetry with `format_html` (which threads
/// `display_name` and `default_metric` into the HTML per-adapter
/// footer). `effective_metric` is the runtime-resolved metric
/// (post-CLI/config merge); see `EffectiveInputs.metric`.
///
/// `title` and `subtitle` are the optional `[output] title` / `subtitle`
/// config labels. When `title` is `Some`, it renders as a
/// `## <title>` line above the tool/version H1; when `subtitle` is
/// `Some`, it renders on the line beneath. Both default to `None`, in
/// which case the output is byte-identical to the unlabeled default — no
/// empty title/subtitle line is emitted (the template ws-strips the
/// absent arms).
#[allow(clippy::too_many_arguments)]
pub fn format_markdown(
    view: &AnalysisView<'_>,
    delta: Option<&DeltaView<'_>>,
    threshold: f64,
    breakdown: bool,
    explain: bool,
    full_table: bool,
    top_n: usize,
    meta: &AdapterMeta,
    _effective_metric: ComplexityMetric,
    title: Option<&str>,
    subtitle: Option<&str>,
) -> String {
    let body = if view.full.functions.is_empty() {
        MarkdownBody::Empty
    } else {
        let summary = Box::new(summary_data(view, threshold));
        let section = if let Some(grouped) = view.grouped.as_ref() {
            BodySection::Grouped {
                rows: grouped_rows(grouped),
            }
        } else if full_table {
            full_table_section(view, breakdown, explain)
        } else {
            spotlight_section(view, threshold, top_n, breakdown, explain)
        };
        MarkdownBody::Filled { summary, section }
    };

    let delta_block = delta.map(format_markdown_delta);

    let tmpl = MarkdownReport {
        title,
        subtitle,
        tool_name: meta.tool_name,
        tool_version: meta.tool_version,
        body,
        delta: delta_block,
    };
    let mut out = tmpl
        .render()
        .expect("markdown template render is total — all fields owned");
    // POSIX text files end with `\n`. askama's `{%-` ws operator strips
    // the trailing newline in the template, and `insta` snapshot
    // assertions trim trailing whitespace on compare so the drift is
    // invisible to in-process tests. The composite scorecard action's
    // `cat <file>` + `echo "<EOF>"` heredoc emission relies on the
    // trailing `\n` to place the EOF delimiter on its own line — a
    // missing newline collides with the heredoc terminator and breaks
    // GH Actions' `$GITHUB_OUTPUT` parsing. Restore the trailing
    // newline here so the contract holds across all consumers.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[derive(Template)]
#[template(path = "markdown_report.txt", escape = "none")]
struct MarkdownReport<'a> {
    /// Configured scorecard title (`[output] title`), rendered as a
    /// `## <title>` line above the tool/version H1. `None` emits nothing.
    title: Option<&'a str>,
    /// Configured scorecard subtitle (`[output] subtitle`), rendered
    /// beneath the title. `None` emits nothing.
    subtitle: Option<&'a str>,
    tool_name: &'a str,
    tool_version: &'a str,
    body: MarkdownBody,
    delta: Option<String>,
}

enum MarkdownBody {
    Empty,
    /// `summary` is boxed because `SummaryData` carries ~14 owned
    /// `String` fields — boxing matches the clippy
    /// `large_enum_variant` recommendation and keeps the
    /// `MarkdownBody::Empty` discriminant cheap.
    Filled {
        summary: Box<SummaryData>,
        section: BodySection,
    },
}

struct SummaryData {
    pass_fail: &'static str,
    total_functions: usize,
    threshold_display: String,
    exceeding_threshold: usize,
    crap_max: String,
    crap_avg: String,
    crap_med: String,
    cx_max: String,
    cx_avg: String,
    cx_med: String,
    cov_min: String,
    cov_avg: String,
    cov_med: String,
    dist_low: usize,
    dist_acceptable: usize,
    dist_moderate: usize,
    dist_high: usize,
}

enum BodySection {
    Grouped {
        rows: Vec<GroupedRow>,
    },
    FullTable {
        rows: Vec<FunctionRow>,
        legend: Option<&'static str>,
    },
    Spotlight {
        header: String,
        rows: Vec<FunctionRow>,
        legend: Option<&'static str>,
        footnote: Option<&'static str>,
    },
    /// All summary-displayed, no body table. Used when a clean run has
    /// zero shown rows (e.g. `--only-failing` strips everything).
    None,
}

struct GroupedRow {
    file_path: String,
    function_count: usize,
    exceeding_count: usize,
    average_crap: String,
    worst_crap: String,
    worst_fn: String,
}

struct FunctionRow {
    file: String,
    function: String,
    cc: u32,
    cov: String,
    crap: String,
    risk: String,
    breakdown_bullets: Vec<String>,
}

fn summary_data(view: &AnalysisView<'_>, threshold: f64) -> SummaryData {
    let summary = &view.full.summary;
    let pass_fail = if view.full.passed { "PASS" } else { "FAIL" };
    let crap_max = summary
        .max_crap
        .as_ref()
        .map(|c| format!("{:.2}", c.value))
        .unwrap_or_else(|| "—".to_string());
    let d = &summary.distribution;
    SummaryData {
        pass_fail,
        total_functions: summary.total_functions,
        threshold_display: format_threshold(view, threshold),
        exceeding_threshold: summary.exceeding_threshold,
        crap_max,
        crap_avg: format!("{:>7.2}", summary.average_crap),
        crap_med: format!("{:>6.2}", summary.median_crap),
        cx_max: format!("{:>5}", summary.max_complexity),
        cx_avg: format!("{:>7.1}", summary.average_complexity),
        cx_med: format!("{:>6.1}", summary.median_complexity),
        cov_min: format!("{:>4.1}%", summary.min_coverage),
        cov_avg: format!("{:>6.1}%", summary.average_coverage),
        cov_med: format!("{:>5.1}%", summary.median_coverage),
        dist_low: d.low,
        dist_acceptable: d.acceptable,
        dist_moderate: d.moderate,
        dist_high: d.high,
    }
}

fn grouped_rows(grouped: &crate::domain::view::GroupedView) -> Vec<GroupedRow> {
    grouped
        .files
        .iter()
        .map(|f| {
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
            GroupedRow {
                file_path: escape_cell(&f.file_path),
                function_count: f.function_count,
                exceeding_count: f.exceeding_count,
                average_crap: format!("{:.2}", f.average_crap),
                worst_crap,
                worst_fn,
            }
        })
        .collect()
}

fn full_table_section(view: &AnalysisView<'_>, breakdown: bool, explain: bool) -> BodySection {
    let rows: Vec<FunctionRow> = view
        .shown
        .iter()
        .map(|v| function_row(v, breakdown))
        .collect();
    BodySection::FullTable {
        rows,
        legend: legend_if_needed(view, breakdown, explain),
    }
}

fn spotlight_section(
    view: &AnalysisView<'_>,
    threshold: f64,
    top_n: usize,
    breakdown: bool,
    explain: bool,
) -> BodySection {
    let summary = &view.full.summary;

    if summary.exceeding_threshold == 0 {
        let worst = top_n_by_crap(view.shown.iter().copied(), top_n);
        if worst.is_empty() {
            return BodySection::None;
        }
        let header = format!("## Top {} worst by CRAP", worst.len());
        let rows: Vec<FunctionRow> = worst.iter().map(|v| function_row(v, breakdown)).collect();
        return BodySection::Spotlight {
            header,
            rows,
            legend: legend_if_needed(view, breakdown, explain),
            footnote: Some("\n_All functions are within threshold._"),
        };
    }

    let shown_failures: Vec<&FunctionVerdict> =
        top_n_by_crap(view.shown.iter().copied().filter(|v| v.exceeds), top_n);
    let header = if summary.exceeding_threshold > shown_failures.len() {
        format!(
            "## Failures (top {} of {} above threshold {})",
            shown_failures.len(),
            summary.exceeding_threshold,
            format_threshold(view, threshold),
        )
    } else {
        format!(
            "## Failures ({} above threshold {})",
            summary.exceeding_threshold,
            format_threshold(view, threshold),
        )
    };
    let rows: Vec<FunctionRow> = shown_failures
        .iter()
        .map(|v| function_row(v, breakdown))
        .collect();
    BodySection::Spotlight {
        header,
        rows,
        legend: legend_if_needed(view, breakdown, explain),
        footnote: None,
    }
}

fn function_row(verdict: &FunctionVerdict, breakdown: bool) -> FunctionRow {
    let s = &verdict.scored;
    let bullets = breakdown_bullets(verdict, breakdown);
    FunctionRow {
        file: escape_cell(&s.identity.file_path),
        function: escape_cell(&s.identity.qualified_name),
        cc: s.complexity,
        cov: format!("{:.1}", s.coverage_percent),
        crap: format!("{:.2}", s.crap.value),
        risk: s.crap.risk_level.to_string(),
        breakdown_bullets: bullets,
    }
}

fn breakdown_bullets(verdict: &FunctionVerdict, breakdown: bool) -> Vec<String> {
    if !breakdown || !verdict.exceeds || verdict.scored.contributors.is_empty() {
        return Vec::new();
    }
    verdict
        .scored
        .contributors
        .iter()
        .map(|c| format!("  - L{} {} +{}", c.line, c.kind, c.increment))
        .collect()
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

const LEGEND: &str = "_Legend: +1 = base structural increment. +N (nested) = +1 base plus +(N-1) from active nesting depth (if/else, match arms, while/for/loop, let-else diverging branches, closures)._";

fn legend_if_needed(
    view: &AnalysisView<'_>,
    breakdown: bool,
    explain: bool,
) -> Option<&'static str> {
    if breakdown && explain && needs_legend(view) {
        Some(LEGEND)
    } else {
        None
    }
}

fn needs_legend(view: &AnalysisView<'_>) -> bool {
    view.shown
        .iter()
        .filter(|v| v.exceeds)
        .flat_map(|v| v.scored.contributors.iter())
        .any(|c| c.increment > 1)
}

fn format_threshold(view: &AnalysisView<'_>, threshold: f64) -> String {
    if has_varied_thresholds(&view.full.functions) {
        format!("varied (default: {})", threshold)
    } else {
        format!("{}", threshold)
    }
}

fn has_varied_thresholds(functions: &[FunctionVerdict]) -> bool {
    let mut iter = functions.iter().map(|v| v.threshold);
    let Some(first) = iter.next() else {
        return false;
    };
    iter.any(|t| (t - first).abs() > f64::EPSILON)
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
        "- **Changes:** +{added} added, {removed} removed, {modified} modified, {renamed} renamed\n",
        added = summary.added,
        removed = summary.removed,
        modified = summary.modified,
        renamed = summary.renamed,
    ));
    out.push_str(&format!(
        "- **Regressions:** {regressions} · **Improvements:** {improvements} · **New violations:** {new_violations}\n",
        regressions = summary.regressions,
        improvements = summary.improvements,
        new_violations = summary.new_violations,
    ));
    // Surfaced only when the border-band epsilon actually suppressed
    // something — a zero count is noise on the common (epsilon-off) path.
    if summary.border_jitter_suppressed > 0 {
        out.push_str(&format!(
            "- **Border-jitter suppressed:** {n} (threshold crossings within ±epsilon, not counted as new violations)\n",
            n = summary.border_jitter_suppressed,
        ));
    }

    push_regressions_table(&mut out, view);
    push_new_violations_table(&mut out, view);

    out
}

/// True when a change is a `Modified` whose CRAP rose by at least 0.005.
///
/// The 0.005 cutoff matches the `{:.2}` cell-rendering precision: a
/// delta below it rounds to "+0.00" in the table and looks like a
/// falsely-flagged regression. (CrapScore values are themselves
/// 2-decimal rounded, so this gate rarely fires in practice — but float
/// arithmetic can produce sub-0.005 noise on identity comparisons.)
fn is_md_regression(change: &FunctionChange) -> bool {
    matches!(
        change,
        FunctionChange::Modified { .. } | FunctionChange::Renamed { .. }
    ) && change.score_delta().unwrap_or(0.0) >= 0.005
}

fn push_regressions_table(out: &mut String, view: &DeltaView<'_>) {
    let regressions: Vec<&FunctionChange> = view
        .shown
        .iter()
        .copied()
        .filter(|c| is_md_regression(c))
        .collect();
    if regressions.is_empty() {
        return;
    }
    out.push_str("\n### Regressions\n\n");
    out.push_str("| File | Function | Baseline CRAP | Current CRAP | Δ |\n");
    out.push_str("|------|----------|--------------:|-------------:|--:|\n");
    for change in regressions {
        out.push_str(&format!(
            "| {} | {} | {:.2} | {:.2} | +{:.2} |\n",
            escape_cell(change.file_path()),
            escape_cell(change.qualified_name()),
            change.baseline_score().unwrap_or(0.0),
            change.current_score().unwrap_or(0.0),
            change.score_delta().unwrap_or(0.0),
        ));
    }
}

fn push_new_violations_table(out: &mut String, view: &DeltaView<'_>) {
    // Route through the shared domain predicate with the run's effective
    // epsilon (the same one the summary tally used) so this table can
    // never disagree with `summary.new_violations` — a border-jitter
    // suppressed crossing is absent from both.
    let epsilon = view.full.epsilon;
    let new_violations: Vec<&FunctionChange> = view
        .shown
        .iter()
        .copied()
        .filter(|c| change_is_new_violation(c, epsilon))
        .collect();
    if new_violations.is_empty() {
        return;
    }
    out.push_str("\n### New violations\n\n");
    out.push_str("| File | Function | Current CRAP |\n");
    out.push_str("|------|----------|-------------:|\n");
    for change in new_violations {
        out.push_str(&format!(
            "| {} | {} | {:.2} |\n",
            escape_cell(change.file_path()),
            escape_cell(change.qualified_name()),
            change.current_score().unwrap_or(0.0),
        ));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;

    /// Build a synthetic `AdapterMeta` for reporter tests. Mirrors the
    /// in-crate `fake_meta` pattern from `cli/mod.rs` but stays local
    /// to the reporter module so tests don't reach across module
    /// boundaries.
    fn test_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: TEST_TOOL_NAME,
            display_name: "Test",
            tool_version: TEST_TOOL_VERSION,
            long_version: TEST_TOOL_VERSION,
            about: "test",
            long_about: "test",
            after_help: "",
            coverage_hint: "test",
            extensions: &["rs"],
            tool_info_uri: TEST_TOOL_INFO_URI,
            rule_help_uri: TEST_RULE_HELP_URI,
            config_file_names: &["test-adapter.toml"],
            config_lang_key: "test",
            default_excludes: &[],
            forced_excludes: &[],
            default_metric: ComplexityMetric::Cognitive,
        }
    }

    fn md(view: &AnalysisView<'_>) -> String {
        format_markdown(
            view,
            None,
            8.0,
            false,
            false,
            false,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        )
    }

    // ── [output] title / subtitle labeling ─────────────────────────────

    fn md_labeled(view: &AnalysisView<'_>, title: Option<&str>, subtitle: Option<&str>) -> String {
        format_markdown(
            view,
            None,
            8.0,
            false,
            false,
            false,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            title,
            subtitle,
        )
    }

    #[test]
    fn title_labels_the_markdown_header() {
        let result = make_multi_function_result();
        let out = md_labeled(
            &make_view_default(&result),
            Some("Acme Coverage Report"),
            None,
        );
        // The configured title is the prominent headline — it takes the
        // `#` H1, and the tool/version line demotes to `##` attribution
        // (prominence consistent with the table + html reporters).
        assert!(
            out.starts_with("# Acme Coverage Report\n"),
            "expected the title as the H1 headline at the top; got:\n{out}",
        );
        assert!(out.contains(&format!(
            "## {TEST_TOOL_NAME} v{TEST_TOOL_VERSION} — CRAP Score Analysis"
        )));
        // The tool line must NOT remain a top-level `#` H1 when a title
        // labels the scorecard.
        assert!(!out.contains(&format!(
            "\n# {TEST_TOOL_NAME} v{TEST_TOOL_VERSION} — CRAP Score Analysis"
        )));
    }

    #[test]
    fn subtitle_renders_beneath_the_markdown_title() {
        let result = make_multi_function_result();
        let out = md_labeled(
            &make_view_default(&result),
            Some("Acme Coverage Report"),
            Some("nightly build"),
        );
        assert!(
            out.starts_with("# Acme Coverage Report\n\nnightly build\n"),
            "expected the subtitle on its own paragraph beneath the title; got:\n{out}",
        );
    }

    #[test]
    fn subtitle_without_title_renders_above_markdown_tool_line() {
        let result = make_multi_function_result();
        let out = md_labeled(&make_view_default(&result), None, Some("nightly build"));
        // A subtitle set without a title renders as a plain line above
        // the tool/version line, which stays the `#` H1 (no title to
        // demote it). Exercises the template's `{%- else -%}` branch's
        // inner subtitle arm.
        assert!(
            out.starts_with("nightly build\n\n# "),
            "expected the subtitle above the tool H1; got:\n{out}",
        );
        assert!(out.contains(&format!(
            "# {TEST_TOOL_NAME} v{TEST_TOOL_VERSION} — CRAP Score Analysis"
        )));
    }

    #[test]
    fn absent_markdown_title_subtitle_is_byte_identical() {
        let result = make_multi_function_result();
        // Absent title/subtitle must be byte-identical to the default —
        // the template ws-strips the absent arms, so no empty line drift.
        let labeled = md_labeled(&make_view_default(&result), None, None);
        let default = md(&make_view_default(&result));
        assert_eq!(
            labeled, default,
            "absent title/subtitle must produce the unlabeled markdown verbatim",
        );
        assert!(
            default.starts_with(&format!(
                "# {TEST_TOOL_NAME} v{TEST_TOOL_VERSION} — CRAP Score Analysis"
            )),
            "default header must lead with the tool/version H1, no empty line above",
        );
    }

    #[test]
    fn header_row_pipes_and_columns() {
        let result = make_multi_function_result();
        let out = md(&make_view_default(&result));
        assert!(out.contains("| File | Function | CC | Cov% | CRAP | Risk |"));
        assert!(out.contains("|------|"));
    }

    #[test]
    fn empty_analysis_says_no_functions() {
        let result = make_empty_result();
        let out = md(&make_view_default(&result));
        assert!(out.contains("No functions analyzed"));
        assert!(!out.contains("| File |"));
    }

    #[test]
    fn pipe_in_function_name_is_escaped() {
        let result =
            make_single_function_result("a|b", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let out = md(&make_view_default(&result));
        assert!(out.contains("a\\|b"), "expected escaped pipe in: {out}");
    }

    #[test]
    fn summary_reflects_full_analysis_not_view() {
        let result = make_multi_function_result();
        let out = md(&make_view_default(&result));
        assert!(out.contains("**Result:** FAIL"));
        assert!(out.contains("**Functions:** 3"));
        assert!(out.contains("**Above threshold (8):** 2"));
    }

    #[test]
    fn full_markdown_snapshot() {
        let result = make_multi_function_result();
        let out = md(&make_view_default(&result));
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
            true,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        assert!(out.contains("## Summary"));
        assert!(out.contains("## All functions"));
        assert!(out.contains("complex_fn"));
        assert!(out.contains("parse_record"));
        assert!(out.contains("simple_fn"));
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
                    increment: 2,
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
            true,
            true,
            true,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        assert!(out.contains("## All functions"));
        assert!(out.contains("L12 if-branch +1"));
        assert!(out.contains("L18 match +2"));
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
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
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
        let out = md(&view);
        assert!(out.contains("| File | Functions | Failing | Avg CRAP | Worst CRAP | Worst Fn |"));
        assert!(!out.contains("| File | Function | CC |"));
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
        let out = md(&view);
        insta::assert_snapshot!(out);
    }

    // ── Delta scorecard ─────────────────────────────────────────────

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
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        assert!(out.contains("## CRAP Scorecard"));
        assert!(out.contains("- **Delta status:** FAIL"));
        assert!(out.contains("+1 added, 1 removed, 2 modified"));
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
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        assert!(out.contains("### Regressions"));
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
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        assert!(out.contains("### New violations"));
        assert!(out.contains("new_fn"));
    }

    #[test]
    fn no_baseline_means_no_scorecard_block() {
        let result = make_multi_function_result();
        let out = md(&make_view_default(&result));
        assert!(!out.contains("CRAP Scorecard"));
        assert!(!out.contains("Delta status"));
    }

    #[test]
    fn delta_scorecard_border_jitter_suppressed_is_consistent_with_summary() {
        // A function oscillating across threshold 8.0 within epsilon 0.5
        // (7.8 → 8.2) is suppressed. The summary must show 0 new
        // violations + 1 border-jitter suppressed, AND the "New
        // violations" table must be ABSENT — the table re-derives via the
        // SAME shared predicate (with view.full.epsilon), so it cannot
        // disagree with the count (#277, the tally-vs-reporter axis that
        // bit #274).
        let baseline =
            make_single_function_result("f", "a.rs", 5, 50.0, 7.8, RiskLevel::Acceptable, 8.0);
        let current =
            make_single_function_result("f", "a.rs", 5, 48.0, 8.2, RiskLevel::Acceptable, 8.0);
        let delta = crate::domain::delta::compute_with_epsilon(baseline, current, 0.5);
        assert_eq!(delta.summary.new_violations, 0);
        assert_eq!(delta.summary.border_jitter_suppressed, 1);
        let dview = make_delta_view_default(&delta);
        let out = format_markdown(
            &make_view_default(&delta.current),
            Some(&dview),
            8.0,
            false,
            false,
            false,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        assert!(
            out.contains("**New violations:** 0"),
            "summary must show 0 new violations:\n{out}"
        );
        assert!(
            out.contains("Border-jitter suppressed:** 1"),
            "summary must surface the suppressed count:\n{out}"
        );
        assert!(
            !out.contains("### New violations"),
            "a border-jitter suppressed crossing must NOT appear in the new-violations table:\n{out}"
        );
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
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        insta::assert_snapshot!(out);
    }
}
