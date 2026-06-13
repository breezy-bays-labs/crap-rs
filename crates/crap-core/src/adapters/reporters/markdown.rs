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
/// The first output line is always a hidden HTML comment marker,
/// `<!-- {tool_name}:scorecard -->` — invisible in rendered GFM, but a
/// stable dedupe anchor for sticky-PR-comment tooling. It carries the
/// calling adapter's `tool_name`, so each adapter's scorecard can
/// sticky to its own comment on the same PR.
///
/// `breakdown` collects the per-line complexity contributors of every
/// above-threshold function into one `<details><summary>Show
/// breakdown</summary>` collapsible rendered BELOW the table (spotlight
/// or full table) so the default PR-comment view stays compact and the
/// GFM table stays intact — an inline `<details>` would terminate it.
/// `breakdown` ALSO collapses the all-clear "Top N worst" spotlight
/// TABLE into a `<details>` (crap-rs#400): when every function is within
/// threshold the worst-by-CRAP list is low-priority detail on a green
/// sticky, so it folds away while a failures spotlight always stays
/// visible. Both collapsibles are GFM-safe (blank line after
/// `</summary>` and before `</details>` so the table renders inside).
/// `explain` adds a trailing legend describing increment semantics
/// (only meaningful when `breakdown` is set).
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
        /// Per-function complexity breakdowns, rendered in one
        /// collapsible BELOW the table (crap-rs#397). Empty unless
        /// `--breakdown` is active and at least one shown function
        /// exceeds the threshold with contributors.
        breakdowns: Vec<FunctionBreakdown>,
        legend: Option<&'static str>,
    },
    Spotlight {
        /// The section label, WITHOUT a `##` prefix — the template adds
        /// `## ` for the visible heading, or uses it bare as the
        /// `<summary>` text when `collapsible` (crap-rs#400).
        header: String,
        rows: Vec<FunctionRow>,
        breakdowns: Vec<FunctionBreakdown>,
        legend: Option<&'static str>,
        footnote: Option<&'static str>,
        /// crap-rs#400: when true, wrap the spotlight table in a collapsed
        /// `<details>` (the all-clear top-worst under `--breakdown`). The
        /// failures case stays `false` — failures must be visible.
        collapsible: bool,
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
}

/// One function's complexity-contributor breakdown, rendered inside the
/// single collapsible below the table (crap-rs#397). `function` and
/// `file` are passed through `code_span_safe` (not pipe-escaped like the
/// table cells) because the template renders them as markdown code
/// spans: inside backticks GFM renders `<`/`>` literally, so a TS
/// `<arrow>` survives where a bare table cell drops it — but a literal
/// backtick or newline would break the span, so those are neutralized.
struct FunctionBreakdown {
    function: String,
    file: String,
    bullets: Vec<String>,
}

/// Make an identity string safe to drop inside a markdown code span.
/// Code spans are how the breakdown header renders `<`/`>` literally
/// (a TS `<arrow>`), but a literal backtick closes the span early and a
/// newline breaks the line. Neither occurs in a Rust path, but a
/// TypeScript string-literal module/property name reaches
/// `qualified_name` verbatim (e.g. `module "a`b" { … }`), so collapse
/// both: backtick → `'`, CR/LF → space (mirroring `escape_cell`'s
/// newline handling).
fn code_span_safe(s: &str) -> String {
    s.replace(['\n', '\r'], " ").replace('`', "'")
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
    let rows: Vec<FunctionRow> = view.shown.iter().map(|v| function_row(v)).collect();
    BodySection::FullTable {
        breakdowns: function_breakdowns(view.shown.iter().copied(), breakdown),
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
        let header = format!("Top {} worst by CRAP", worst.len());
        let rows: Vec<FunctionRow> = worst.iter().map(|v| function_row(v)).collect();
        return BodySection::Spotlight {
            header,
            // No function exceeds here, so `function_breakdowns`
            // resolves to empty (breakdowns gate on `exceeds`); the
            // breakdown sub-collapsible never renders in the all-clear
            // case. The top-worst TABLE itself collapses under
            // `--breakdown` (crap-rs#400) — low-priority detail on a green
            // sticky — via the `collapsible` flag below.
            breakdowns: function_breakdowns(worst.iter().copied(), breakdown),
            rows,
            legend: legend_if_needed(view, breakdown, explain),
            footnote: Some("\n_All functions are within threshold._"),
            collapsible: breakdown,
        };
    }

    let shown_failures: Vec<&FunctionVerdict> =
        top_n_by_crap(view.shown.iter().copied().filter(|v| v.exceeds), top_n);
    let header = if summary.exceeding_threshold > shown_failures.len() {
        format!(
            "Failures (top {} of {} above threshold {})",
            shown_failures.len(),
            summary.exceeding_threshold,
            format_threshold(view, threshold),
        )
    } else {
        format!(
            "Failures ({} above threshold {})",
            summary.exceeding_threshold,
            format_threshold(view, threshold),
        )
    };
    let rows: Vec<FunctionRow> = shown_failures.iter().map(|v| function_row(v)).collect();
    BodySection::Spotlight {
        header,
        breakdowns: function_breakdowns(shown_failures.iter().copied(), breakdown),
        rows,
        legend: legend_if_needed(view, breakdown, explain),
        footnote: None,
        // Failures must stay VISIBLE — never collapse them. (Their
        // complexity breakdown still collapses below, via `breakdowns`.)
        collapsible: false,
    }
}

fn function_row(verdict: &FunctionVerdict) -> FunctionRow {
    let s = &verdict.scored;
    FunctionRow {
        file: escape_cell(&s.identity.file_path),
        function: escape_cell(&s.identity.qualified_name),
        cc: s.complexity,
        cov: format!("{:.1}", s.coverage_percent),
        crap: format!("{:.2}", s.crap.value),
        risk: s.crap.risk_level.to_string(),
    }
}

/// Build the per-function breakdown list rendered in the single
/// collapsible below the table (crap-rs#397). Iterates the SAME verdict
/// slice the table rows came from (so order + membership match the
/// table) and keeps only functions whose `breakdown_bullets` are
/// non-empty — i.e. `--breakdown` is active AND the function exceeds the
/// threshold AND it has contributors. Returns empty otherwise, so the
/// template omits the collapsible entirely.
fn function_breakdowns<'a, I>(verdicts: I, breakdown: bool) -> Vec<FunctionBreakdown>
where
    I: IntoIterator<Item = &'a FunctionVerdict>,
{
    verdicts
        .into_iter()
        .filter_map(|v| {
            let bullets = breakdown_bullets(v, breakdown);
            (!bullets.is_empty()).then(|| FunctionBreakdown {
                function: code_span_safe(&v.scored.identity.qualified_name),
                file: code_span_safe(&v.scored.identity.file_path),
                bullets,
            })
        })
        .collect()
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
    // Shown whenever the border band was active, so an opt-in run always
    // confirms the band (even "0 suppressed" reassures the operator nothing
    // slipped through). Keyed off the serialized `border_jitter_active` flag
    // so every reporter shares one display rule (crap-rs#379); on this
    // in-memory path the flag equals `epsilon > 0.0`, so the output is
    // byte-identical. `|| > 0` is a belt-and-suspenders if a count ever
    // outlives the flag. The epsilon-off path (inactive, 0 count) stays silent.
    if summary.border_jitter_active || summary.border_jitter_suppressed > 0 {
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
        // `#` H1 (right after the hidden dedupe marker), and the
        // tool/version line demotes to `##` attribution (prominence
        // consistent with the table + html reporters).
        assert!(
            out.starts_with(&format!(
                "<!-- {TEST_TOOL_NAME}:scorecard -->\n\n# Acme Coverage Report\n"
            )),
            "expected the title as the H1 headline beneath the marker; got:\n{out}",
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
            out.starts_with(&format!(
                "<!-- {TEST_TOOL_NAME}:scorecard -->\n\n# Acme Coverage Report\n\nnightly build\n"
            )),
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
            out.starts_with(&format!(
                "<!-- {TEST_TOOL_NAME}:scorecard -->\n\nnightly build\n\n# "
            )),
            "expected the subtitle above the tool H1 (beneath the marker); got:\n{out}",
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
                "<!-- {TEST_TOOL_NAME}:scorecard -->\n\n# {TEST_TOOL_NAME} v{TEST_TOOL_VERSION} — CRAP Score Analysis"
            )),
            "default header must lead with the marker, a blank line, then \
             the tool/version H1 — no extra line drift",
        );
    }

    // ── sticky-comment marker + breakdown collapsibles ──────────────

    #[test]
    fn markdown_leads_with_hidden_sticky_marker() {
        let result = make_multi_function_result();
        let out = md(&make_view_default(&result));
        assert!(
            out.starts_with(&format!(
                "<!-- {TEST_TOOL_NAME}:scorecard -->\n\n# {TEST_TOOL_NAME}"
            )),
            "expected the hidden dedupe marker as the first line, a blank \
             line, then the tool H1; got:\n{out}"
        );
    }

    #[test]
    fn marker_carries_the_adapter_tool_name() {
        // Distinct per adapter so crap4rs and crap4ts can sticky to
        // separate comments on the same PR.
        let result = make_multi_function_result();
        let mut meta = test_meta();
        meta.tool_name = "other-adapter";
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            false,
            10,
            &meta,
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        assert!(
            out.starts_with("<!-- other-adapter:scorecard -->\n"),
            "expected the marker to carry the calling adapter's tool_name; got:\n{out}"
        );
    }

    #[test]
    fn marker_precedes_configured_title() {
        // The marker is a machine dedupe anchor — it must stay the first
        // line even when a configured title takes the H1 headline.
        let result = make_multi_function_result();
        let out = md_labeled(
            &make_view_default(&result),
            Some("Acme Coverage Report"),
            None,
        );
        assert!(
            out.starts_with(&format!(
                "<!-- {TEST_TOOL_NAME}:scorecard -->\n\n# Acme Coverage Report\n"
            )),
            "expected marker first, then the configured title H1; got:\n{out}"
        );
    }

    #[test]
    fn marker_present_on_empty_analysis() {
        let result = make_empty_result();
        let out = md(&make_view_default(&result));
        assert!(
            out.starts_with(&format!("<!-- {TEST_TOOL_NAME}:scorecard -->\n")),
            "the dedupe marker must lead even an empty analysis; got:\n{out}"
        );
        assert!(out.contains("No functions analyzed"));
    }

    #[test]
    fn breakdown_bullets_collapse_into_details() {
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
            false,
            true,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        let open = out
            .find("<details><summary>Show breakdown</summary>")
            .unwrap_or_else(|| panic!("missing <details> wrapper in:\n{out}"));
        let close = out
            .find("</details>")
            .unwrap_or_else(|| panic!("missing </details> in:\n{out}"));
        assert!(open < close, "malformed details block in:\n{out}");
        let inner = &out[open..close];
        assert!(
            inner.contains("L12 if-branch +1") && inner.contains("L18 match +2"),
            "contributor bullets must sit inside the collapsible; got:\n{out}"
        );
        // GFM only renders markdown inside an HTML block after a blank
        // line — the bullets must not butt up against the <summary> tag.
        assert!(
            out.contains("</summary>\n\n"),
            "expected a blank line after </summary> so the bullet list renders; got:\n{out}"
        );
    }

    #[test]
    fn no_details_block_without_breakdown() {
        let result = make_multi_function_result();
        let out = md(&make_view_default(&result));
        assert!(
            !out.contains("<details>"),
            "no collapsible should render when breakdown is inactive; got:\n{out}"
        );
    }

    /// Regression for crap-rs#397. #275 emitted a `<details>` block
    /// directly after each table row; in GFM a `<details>` is an HTML
    /// block that TERMINATES the table, so every row after the first
    /// rendered as literal pipe-text ("pipe soup") — verified against
    /// GitHub's own `/markdown` renderer. The single-function fixtures
    /// the other breakdown tests use could never catch it. The
    /// breakdowns now sit in ONE collapsible BELOW the whole table so
    /// the table stays contiguous.
    #[test]
    fn multi_row_breakdown_keeps_table_contiguous() {
        use crate::domain::types::{AnalysisResult, ComplexityContributor, ContributorKind};
        let contributors = || {
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
            ]
        };
        let functions = vec![
            make_verdict_with_contributors(
                make_verdict("alpha_fn", "src/a.rs", 9, 40.0, 30.0, RiskLevel::High, 8.0),
                contributors(),
            ),
            make_verdict_with_contributors(
                make_verdict("beta_fn", "src/b.rs", 7, 50.0, 20.0, RiskLevel::High, 8.0),
                contributors(),
            ),
        ];
        let result = AnalysisResult {
            summary: crate::domain::summary::compute_summary(&functions),
            functions,
            passed: false,
        };
        // Spotlight arm (full_table = false) — the shape the scorecard
        // action renders on its sticky comments.
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            true,
            false,
            false,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );

        // The whole table must precede the first collapsible: both data
        // rows live in the region above `<details>`. Under the #275 bug
        // `beta_fn` landed AFTER alpha's `<details>` and rendered as
        // pipe soup — so it would be absent from this region.
        let details_at = out
            .find("<details>")
            .unwrap_or_else(|| panic!("missing <details>; got:\n{out}"));
        let table_region = &out[..details_at];
        assert!(
            table_region.contains("alpha_fn") && table_region.contains("beta_fn"),
            "both rows must sit in the contiguous table above the collapsible; got:\n{out}"
        );

        // Exactly ONE collapsible wraps every breakdown (not one per row).
        assert_eq!(
            out.matches("<details>").count(),
            1,
            "expected a single breakdown collapsible below the table; got:\n{out}"
        );

        // GFM needs a blank line between the table and the HTML block,
        // and after `</summary>`, or the table/list silently fail to
        // render. A table row ends with `|`; assert the blank line
        // separates it from `<details>`, and the blank after summary.
        assert!(
            out.contains("|\n\n<details><summary>Show breakdown</summary>\n\n"),
            "need a blank line before <details> and after </summary> for GFM; got:\n{out}"
        );

        // Each function's breakdown sits inside the wrapper, keyed by a
        // code span so names with angle brackets (e.g. a TS `<arrow>`)
        // render literally instead of being eaten as an HTML tag.
        let open = out
            .find("<details><summary>Show breakdown</summary>")
            .unwrap();
        let close = out.find("</details>").unwrap();
        let inner = &out[open..close];
        assert!(
            inner.contains("`alpha_fn`") && inner.contains("`beta_fn`"),
            "both function headers must sit inside the collapsible as code spans; got:\n{out}"
        );
        assert!(
            inner.contains("L12 if-branch +1") && inner.contains("L18 match +2"),
            "contributor bullets must sit inside the collapsible; got:\n{out}"
        );
    }

    /// crap-rs#400. On a green sticky the "Top N worst by CRAP" spotlight
    /// is low-priority detail, so under `--breakdown` it collapses into a
    /// `<details>` (mirroring the failure-breakdown collapsible). The
    /// header becomes the `<summary>` label (no `##`), the table renders
    /// inside (GFM-safe blank line after `</summary>` and before
    /// `</details>`), and the all-clear footnote stays BELOW the
    /// collapsible.
    #[test]
    fn all_clear_top_worst_collapses_under_breakdown() {
        use crate::domain::types::AnalysisResult;
        let functions = vec![
            make_verdict("calc_total", "src/a.rs", 3, 95.0, 5.0, RiskLevel::Low, 8.0),
            make_verdict("merge_in", "src/b.rs", 2, 90.0, 4.0, RiskLevel::Low, 8.0),
        ];
        let result = AnalysisResult {
            summary: crate::domain::summary::compute_summary(&functions),
            functions,
            passed: true,
        };
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            true,
            false,
            false,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );

        // The label is the <summary>, with the GFM-required blank line
        // before the table renders inside the HTML block.
        assert!(
            out.contains("<details><summary>Top 2 worst by CRAP</summary>\n\n| File |"),
            "green top-worst must open a <details> with the label as summary + blank line before the table; got:\n{out}"
        );
        // The header is no longer a `##` heading — it moved into <summary>.
        assert!(
            !out.contains("## Top 2 worst by CRAP"),
            "the `##` heading must be gone in collapsed mode; got:\n{out}"
        );
        // Both worst rows sit inside the collapsible (above </details>).
        let open = out
            .find("<details><summary>Top 2 worst by CRAP</summary>")
            .unwrap();
        let close = out.find("</details>").unwrap();
        let inner = &out[open..close];
        assert!(
            inner.contains("calc_total") && inner.contains("merge_in"),
            "both worst rows must sit inside the collapsible; got:\n{out}"
        );
        // GFM-safe close: a blank line between the last table row and the
        // `</details>` (a row ends with `|`).
        assert!(
            out.contains("|\n\n</details>"),
            "need a blank line between the last table row and </details>; got:\n{out}"
        );
        // The all-clear footnote stays OUTSIDE/below the collapsible.
        let footnote_at = out.find("_All functions are within threshold._").unwrap();
        assert!(
            footnote_at > close,
            "the all-clear footnote must render below the collapsible; got:\n{out}"
        );
    }

    /// crap-rs#400. Without `--breakdown` the all-clear spotlight is
    /// byte-identical to before the collapsible existed: a plain `##`
    /// heading + table, no `<details>`.
    #[test]
    fn all_clear_top_worst_uncollapsed_without_breakdown() {
        use crate::domain::types::AnalysisResult;
        let functions = vec![make_verdict(
            "calc_total",
            "src/a.rs",
            3,
            95.0,
            5.0,
            RiskLevel::Low,
            8.0,
        )];
        let result = AnalysisResult {
            summary: crate::domain::summary::compute_summary(&functions),
            functions,
            passed: true,
        };
        let out = format_markdown(
            &make_view_default(&result),
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
        );
        assert!(
            out.contains("## Top 1 worst by CRAP"),
            "uncollapsed green keeps the `##` heading; got:\n{out}"
        );
        assert!(
            !out.contains("<details>"),
            "no collapsible without breakdown; got:\n{out}"
        );
    }

    /// A backtick in an identity string (reachable from crap4ts, which
    /// maps a string-literal module/property name verbatim into
    /// `qualified_name`) must NOT leak into the markdown code span, where
    /// it would close the span early and garble the breakdown header.
    /// `code_span_safe` neutralizes it to an apostrophe.
    #[test]
    fn backtick_in_name_does_not_break_the_code_span() {
        use crate::domain::types::{AnalysisResult, ComplexityContributor, ContributorKind};
        let verdict = make_verdict_with_contributors(
            make_verdict(
                "ns`weird.fn",
                "src/a`b.ts",
                9,
                30.0,
                30.0,
                RiskLevel::High,
                8.0,
            ),
            vec![ComplexityContributor {
                kind: ContributorKind::IfBranch,
                line: 12,
                column: None,
                increment: 1,
                end_line: 12,
                nesting_depth: 0,
            }],
        );
        let result = AnalysisResult {
            summary: crate::domain::summary::compute_summary(std::slice::from_ref(&verdict)),
            functions: vec![verdict],
            passed: false,
        };
        let out = format_markdown(
            &make_view_default(&result),
            None,
            8.0,
            true,
            false,
            false,
            10,
            &test_meta(),
            ComplexityMetric::Cognitive,
            None,
            None,
        );
        let open = out
            .find("<details><summary>Show breakdown</summary>")
            .unwrap();
        let close = out.find("</details>").unwrap();
        let inner = &out[open..close];
        // The header renders as a balanced code span with the backtick
        // neutralized — never the raw backtick that would break it.
        assert!(
            inner.contains("`ns'weird.fn` — `src/a'b.ts`"),
            "backtick must be neutralized inside the code-span header; got:\n{out}"
        );
        assert!(
            !inner.contains("ns`weird"),
            "raw backtick must not leak into the code span; got:\n{out}"
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
    fn border_jitter_line_shown_at_zero_when_epsilon_is_set() {
        // An opt-in epsilon run with NO crossing still surfaces the line
        // (count 0) — confirming the band is active and nothing slipped
        // through. A function that stays well under threshold on both
        // sides: no crossing, nothing suppressed, but epsilon > 0.
        let baseline = make_single_function_result("f", "a.rs", 5, 90.0, 4.0, RiskLevel::Low, 12.0);
        let current = make_single_function_result("f", "a.rs", 5, 90.0, 4.0, RiskLevel::Low, 12.0);
        let delta = crate::domain::delta::compute_with_epsilon(baseline, current, 0.5);
        assert_eq!(delta.summary.border_jitter_suppressed, 0);
        let dview = make_delta_view_default(&delta);
        let out = format_markdown(
            &make_view_default(&delta.current),
            Some(&dview),
            12.0,
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
            out.contains("Border-jitter suppressed:** 0"),
            "an active epsilon run shows the line even at 0:\n{out}"
        );
    }

    #[test]
    fn border_jitter_line_absent_when_epsilon_off() {
        // The common path: no epsilon, no suppression → no line (output
        // byte-identical to the pre-#277 report).
        let baseline = make_single_function_result("f", "a.rs", 5, 90.0, 4.0, RiskLevel::Low, 12.0);
        let current = make_single_function_result("f", "a.rs", 5, 90.0, 4.0, RiskLevel::Low, 12.0);
        let delta = crate::domain::delta::compute(baseline, current);
        let dview = make_delta_view_default(&delta);
        let out = format_markdown(
            &make_view_default(&delta.current),
            Some(&dview),
            12.0,
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
            !out.contains("Border-jitter suppressed"),
            "epsilon-off reports must not mention border jitter:\n{out}"
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
