//! HTML reporter — self-contained HTML report rendered via an askama
//! compile-time template.
//!
//! Produces a single document with inline CSS + inline `<script>` and
//! no external assets — no CDN, no font URLs, no `<link>` to anything
//! external. Layout follows the **Sakura Reports** design system
//! (crap-rs#260): a verdict-stamped header, 4 KPI tiles, a risk
//! distribution bar, up to 4 worst offenders, a `<details>` card per
//! file with a function-level table, and a per-adapter footer carrying
//! metric / coverage / threshold provenance.
//!
//! Color-coded risk levels map onto the Sakura ordinal `--risk-1` …
//! `--risk-4` token ladder (low → high) so light + optional dark mode
//! both work from the same markup. The inline `<script>` handles the
//! theme toggle, file-list filter, and `/` keyboard shortcut — no
//! framework, no build step.
//!
//! Templates live under `crates/crap-core/templates/` and are checked
//! at compile time by the `#[derive(Template)]` macro.

use crate::cli::AdapterMeta;
use crate::domain::delta::{DeltaView, FunctionChange};
use crate::domain::types::{AnalysisSummary, ComplexityMetric, FunctionVerdict, RiskLevel};
use crate::domain::view::AnalysisView;
use askama::Template;
use std::collections::BTreeMap;

/// Format an `AnalysisView` as a self-contained HTML document.
///
/// The output is one full HTML document (`<!doctype html>` …
/// `</html>`). All CSS is inlined; the bundled `<script>` block has
/// no external dependencies. Reporters that want to embed the body in
/// a larger document should consume the structured view directly
/// rather than scraping this output.
///
/// `meta` carries the calling binary's identity, including the
/// `display_name` ("Rust" / "TypeScript") used by the per-adapter
/// footer row. `effective_metric` is the runtime-resolved complexity
/// metric (cognitive vs cyclomatic, post-CLI/config merge); the
/// footer renders this verbatim so users can see which metric a
/// report was scored under.
///
/// The signature widened from `(view, threshold, &str, &str)` to
/// `(view, threshold, &AdapterMeta, ComplexityMetric)` in crap-rs#260
/// (locked plan deviation #1) so the template can render the per-
/// adapter footer without reading domain state — the dispatcher in
/// `cli/mod.rs` already holds both in scope.
///
/// The signature widened again in crap-rs#306 to accept an optional
/// `&DeltaView<'_>`. When `delta` is `None` the output is byte-
/// identical to the v0.5.0 single-tab render (no tabs nav, no second
/// panel — preserves the contract every existing consumer relies on).
/// When `delta` is `Some(_)`, a `<nav class="tabs">` is emitted between
/// the header and the body, and a second `<div class="tab-panel"
/// data-tab="delta">` follows the Current panel with the delta KPI
/// grid + per-category change tables. The Current tab opens by default
/// so first-time users see the Sakura summary; the delta tab is one
/// click away and reachable directly via the `#delta` URL hash (the
/// inline `<script>` hook honors `location.hash` for CI sticky-comment
/// deep links).
pub fn format_html(
    view: &AnalysisView<'_>,
    delta: Option<&DeltaView<'_>>,
    threshold: f64,
    meta: &AdapterMeta,
    effective_metric: ComplexityMetric,
) -> String {
    // `AdapterMeta`'s `&'static str` fields originate from the adapter
    // binaries' compile-time literals. `format_html_inner` takes
    // borrowed `&str` so envelope-loaded callers (e.g. `crap-render`'s
    // single-language passthrough) can pass owned strings sourced from
    // `LanguageBlock` without leaking memory to satisfy a `'static`
    // bound — see `format_html_multi` for the passthrough call site.
    format_html_inner(
        view,
        delta,
        threshold,
        meta.tool_name,
        meta.tool_version,
        meta.display_name,
        effective_metric,
    )
}

fn format_html_inner(
    view: &AnalysisView<'_>,
    delta: Option<&DeltaView<'_>>,
    threshold: f64,
    tool_name: &str,
    tool_version: &str,
    display_name: &str,
    effective_metric: ComplexityMetric,
) -> String {
    let summary = &view.full.summary;
    let title = format!("{} v{} — CRAP score analysis", tool_name, tool_version);

    let metric_label = metric_label(effective_metric);

    let (verdict_class, verdict_label, verdict_glyph) = if view.full.passed {
        ("pass", "PASS", "✓")
    } else {
        ("fail", "FAIL", "✕")
    };

    let is_empty = visible_section_is_empty(view);
    let summary_view = summary_view(summary, threshold);
    let files = if is_empty {
        Vec::new()
    } else {
        file_cards(view, threshold)
    };
    // Worst-offenders enumerates only over the rendered file set. Under
    // `--group-by` truncation, the file cards may already be a proper
    // subset of `view.shown`; including functions from omitted files
    // here would expose data the file-list deliberately hid.
    let worst_offenders = if is_empty {
        Vec::new()
    } else {
        worst_offenders_top4_from_files(&files)
    };
    let exceeding_file_count = files.iter().filter(|f| f.exceeds_count > 0).count();
    let high_file_count = files.iter().filter(|f| f.risk_data == 4).count();
    let file_count = files.len();

    // The delta panel is boxed because it carries several owned
    // `Vec<DeltaRow>` fields. Boxing keeps the `Option::None` arm
    // cheap (the no-baseline byte-identical contract is the dominant
    // path) and mirrors #260's `MarkdownBody::Filled.summary` pattern
    // for the same `large_enum_variant` reason.
    let delta_panel = delta.map(|d| Box::new(build_delta_panel(d)));
    let has_delta = delta_panel.is_some();
    let current_tab_count = if is_empty { 0 } else { summary.total_functions };
    let delta_tab_count = delta_panel
        .as_ref()
        .map(|p| p.summary.added + p.summary.removed + p.summary.modified)
        .unwrap_or(0);
    let delta_has_news = delta_panel
        .as_ref()
        .map(|p| p.summary.regressions > 0 || p.summary.new_violations > 0)
        .unwrap_or(false);

    let tmpl = HtmlReport {
        title,
        tool_name,
        tool_version,
        adapter_display: display_name,
        metric_label,
        verdict_class,
        verdict_label,
        verdict_glyph,
        is_empty,
        summary: summary_view,
        worst_offenders,
        files,
        file_count,
        exceeding_file_count,
        high_file_count,
        has_delta,
        current_tab_count,
        delta_tab_count,
        delta_has_news,
        delta_panel,
    };
    tmpl.render()
        .expect("html template render is total — all fields owned")
}

#[derive(Template)]
#[template(path = "html_report.html")]
struct HtmlReport<'a> {
    title: String,
    tool_name: &'a str,
    tool_version: &'a str,
    adapter_display: &'a str,
    metric_label: &'static str,
    verdict_class: &'static str,
    verdict_label: &'static str,
    verdict_glyph: &'static str,
    is_empty: bool,
    summary: SummaryView,
    worst_offenders: Vec<OffenderRow>,
    files: Vec<FileCard>,
    file_count: usize,
    exceeding_file_count: usize,
    high_file_count: usize,
    /// True when a baseline was supplied and the delta tab should
    /// render. The template gates the `<nav class="tabs">` block + the
    /// second `<div class="tab-panel" data-tab="delta">` on this flag
    /// so the no-baseline case stays byte-identical to v0.5.0.
    has_delta: bool,
    /// Tab-count badge on the "Current" tab. Equal to
    /// `summary.total_functions` for the populated case, 0 for an
    /// empty analysis.
    current_tab_count: usize,
    /// Tab-count badge on the "Delta" tab. Sum of all change kinds —
    /// matches the markdown reporter's "+N added, M removed, K
    /// modified" count.
    delta_tab_count: u32,
    /// True when the delta has at least one regression or new
    /// violation. Drives the inline `<span class="tab-dot">` indicator
    /// next to the Delta tab label — same affordance as the Sakura
    /// mock's "news dot."
    delta_has_news: bool,
    /// Owned per-tab projection of the delta. `None` when no baseline
    /// was supplied. Boxed because the populated case carries several
    /// owned `Vec<DeltaRow>` fields and `large_enum_variant` would
    /// otherwise penalize the dominant `None` arm — same boxing
    /// pattern as the markdown reporter's `MarkdownBody::Filled.summary`.
    delta_panel: Option<Box<DeltaPanel>>,
}

/// Per-tab projection of a `DeltaView` into render-ready row + KPI
/// data. Pure presentation — no domain types leak into the template.
///
/// The four-KPI lock matches the Current-tab convention from the
/// Sakura design (chat1.md trim). The five-tile "Functions" KPI from
/// the mock is dropped: the change-counts already show added /
/// removed / modified inline above the tables, and the per-section
/// counts are visible in each table header.
struct DeltaPanel {
    /// Aggregate counts mirroring `DeltaSummary` (copied so the
    /// template doesn't import a domain type). Drives the verdict
    /// stamp + tile sub-text.
    summary: DeltaPanelSummary,
    /// Display label for the verdict pill (passed/regressed). The
    /// delta verdict is "REGRESSED" when `summary.new_violations > 0`,
    /// "PASSED" otherwise — mirroring `DeltaSummary::passed`.
    verdict_class: &'static str,
    verdict_label: &'static str,
    verdict_glyph: &'static str,
    /// The four KPI tiles, in fixed display order:
    /// (1) Exceeding threshold (2) Max CRAP (3) Average CRAP
    /// (4) Avg coverage. Each carries baseline + current values plus
    /// a signed delta + direction.
    kpis: [DeltaKpi; 4],
    /// Modified-row regressions (positive score delta ≥ 0.005,
    /// matching the markdown reporter's filter threshold to suppress
    /// sub-cell-rendered-precision noise).
    regressions: Vec<DeltaRow>,
    /// Modified-row improvements (negative score delta ≤ −0.005).
    improvements: Vec<DeltaRow>,
    /// New functions — `Added` entries. Includes new violations
    /// (`current.exceeds`); the table marks them with a high-risk
    /// pill so the regression vs. benign distinction is visible per-
    /// row without needing a separate "new violations" table.
    new_functions: Vec<DeltaRow>,
    /// `unchanged_count` is the count of baseline functions whose
    /// identity persists in current and whose CRAP score moved less
    /// than 0.005 (`Modified` with zero-ish delta). Rendered as a
    /// single-line note per the chat1.md trim — no full table.
    unchanged_count: u32,
    /// Display label for the baseline reference. Today this is always
    /// "baseline" because `DeltaView.baseline_ref` is `None` reserved
    /// until F2; once a `--baseline-ref <label>` CLI flag lands, this
    /// field carries the label verbatim (e.g. "main@a1f3c2b").
    baseline_ref: &'static str,
}

#[derive(Clone, Copy)]
struct DeltaPanelSummary {
    added: u32,
    removed: u32,
    modified: u32,
    regressions: u32,
    improvements: u32,
    new_violations: u32,
}

struct DeltaKpi {
    label: &'static str,
    before: String,
    after: String,
    /// Signed change as a chip glyph + value, e.g. "▲ +1" or
    /// "▼ -2.30". Empty string when the delta is exactly zero (the
    /// chip is suppressed; "no change" speaks for itself).
    chip_glyph: &'static str,
    chip_value: String,
    /// One of "up" / "down" / "flat" — drives the chip color via
    /// `data-direction`. Up = current is higher (bad for CRAP / max /
    /// exceeding; good for coverage), Down = current is lower. The
    /// `is_regression` flag below decides whether to paint the chip
    /// red (regression) or green (improvement); direction alone is
    /// not sufficient because higher coverage is an improvement.
    direction: &'static str,
    /// True when the chip should be painted red (worse than baseline
    /// for the metric). For CRAP-style KPIs higher = worse; for
    /// coverage lower = worse.
    is_regression: bool,
    /// Optional sub-text under the chip (e.g. "1 new function broke
    /// the threshold"). Empty string when not applicable.
    note: String,
}

struct DeltaRow {
    file: String,
    qualified_name: String,
    start_line: usize,
    end_line: usize,
    /// Baseline CRAP value, formatted to 2 decimals. Empty string for
    /// `Added` rows (no baseline).
    baseline_crap: String,
    /// Current CRAP value, formatted to 2 decimals. Empty string for
    /// `Removed` rows (no current).
    current_crap: String,
    /// Baseline coverage %, "{:.1}%". Empty for Added.
    baseline_cov: String,
    /// Current coverage %, "{:.1}%". Empty for Removed.
    current_cov: String,
    /// Signed CRAP delta, formatted as a chip. e.g. "+5.20" /
    /// "−8.70". Empty string for Added/Removed (the chip cell renders
    /// a literal "—" instead).
    delta_value: String,
    /// "▲" / "▼" / "" (suppressed when delta_value is empty).
    delta_glyph: &'static str,
    /// "up" / "down" / "flat" — for chip color.
    delta_direction: &'static str,
    /// Baseline risk-pill data-risk value (1..=4). 0 for Added.
    baseline_risk: u8,
    /// Baseline risk-pill text label. Empty for Added.
    baseline_risk_label: &'static str,
    /// Current risk-pill data-risk value (1..=4). 0 for Removed.
    current_risk: u8,
    /// Current risk-pill text label. Empty for Removed.
    current_risk_label: &'static str,
    /// True when the current row exceeds threshold — rendered with a
    /// `data-exceeds="1"` flag for tinting.
    exceeds: bool,
}

struct SummaryView {
    total_functions: usize,
    total_files: usize,
    exceeding_threshold: usize,
    exceeding_pct: String,
    has_max_crap: bool,
    max_crap: String,
    crap_avg: String,
    crap_med: String,
    cov_avg: String,
    cov_avg_risk: u8,
    cx_avg: String,
    cx_med: String,
    cx_max: String,
    dist_low: usize,
    dist_acceptable: usize,
    dist_moderate: usize,
    dist_high: usize,
    threshold: String,
}

struct OffenderRow {
    rank: usize,
    fn_name: String,
    file: String,
    start_line: usize,
    end_line: usize,
    crap: String,
    risk_data: u8,
    risk_label: &'static str,
}

struct FileCard {
    path: String,
    risk_data: u8,
    fn_count: usize,
    exceeds_count: usize,
    max_crap: String,
    open: bool,
    rows: Vec<FileFnRow>,
}

struct FileFnRow {
    fn_name: String,
    loc: usize,
    start_line: usize,
    end_line: usize,
    cc: u32,
    cc_risk: u8,
    cc_bar_pct: u32,
    cov: String,
    cov_risk: u8,
    crap: String,
    risk_data: u8,
    risk_label: &'static str,
    exceeds: bool,
    over_by: String,
}

fn summary_view(summary: &AnalysisSummary, threshold: f64) -> SummaryView {
    let pct = if summary.total_functions == 0 {
        "0.0".to_string()
    } else {
        format!(
            "{:.1}",
            (summary.exceeding_threshold as f64 / summary.total_functions as f64) * 100.0
        )
    };
    let max_crap = summary
        .max_crap
        .as_ref()
        .map(|c| format!("{:.2}", c.value))
        .unwrap_or_else(|| "—".to_string());
    SummaryView {
        total_functions: summary.total_functions,
        total_files: summary.total_files,
        exceeding_threshold: summary.exceeding_threshold,
        exceeding_pct: pct,
        has_max_crap: summary.max_crap.is_some(),
        max_crap,
        crap_avg: format!("{:.2}", summary.average_crap),
        crap_med: format!("{:.2}", summary.median_crap),
        cov_avg: format!("{:.1}", summary.average_coverage),
        cov_avg_risk: coverage_risk_bucket(summary.average_coverage),
        cx_avg: format!("{:.1}", summary.average_complexity),
        cx_med: format!("{:.1}", summary.median_complexity),
        cx_max: format!("{}", summary.max_complexity),
        dist_low: summary.distribution.low,
        dist_acceptable: summary.distribution.acceptable,
        dist_moderate: summary.distribution.moderate,
        dist_high: summary.distribution.high,
        threshold: format!("{:.2}", threshold),
    }
}

fn worst_offenders_top4_from_files(files: &[FileCard]) -> Vec<OffenderRow> {
    // Flatten rendered file rows, sort by CRAP descending, take 4.
    // The file iteration order doesn't matter — we re-sort by CRAP.
    struct FlatRow<'a> {
        file: &'a str,
        row: &'a FileFnRow,
    }
    let mut flat: Vec<FlatRow<'_>> = files
        .iter()
        .flat_map(|f| {
            f.rows.iter().map(move |r| FlatRow {
                file: &f.path,
                row: r,
            })
        })
        .collect();
    flat.sort_by(|a, b| {
        // Parse the formatted CRAP back to f64 for ordering. Cheaper than
        // threading the raw float through FileFnRow just for this sort.
        let av: f64 = a.row.crap.parse().unwrap_or(0.0);
        let bv: f64 = b.row.crap.parse().unwrap_or(0.0);
        bv.partial_cmp(&av).unwrap_or(std::cmp::Ordering::Equal)
    });
    flat.into_iter()
        .take(4)
        .enumerate()
        .map(|(i, fr)| OffenderRow {
            rank: i + 1,
            fn_name: fr.row.fn_name.clone(),
            file: fr.file.to_string(),
            start_line: fr.row.start_line,
            end_line: fr.row.end_line,
            crap: fr.row.crap.clone(),
            risk_data: fr.row.risk_data,
            risk_label: fr.row.risk_label,
        })
        .collect()
}

fn file_cards(view: &AnalysisView<'_>, threshold: f64) -> Vec<FileCard> {
    let fns_by_file = group_by_file(&view.shown);

    // Resolve file order: with grouping, honor the grouped order; else
    // sort files by max-CRAP descending so the worst offenders surface
    // at the top of the file list (matches the Sakura design).
    let file_order: Vec<String> = if let Some(grouped) = view.grouped.as_ref() {
        grouped.files.iter().map(|f| f.file_path.clone()).collect()
    } else {
        let mut paths: Vec<String> = fns_by_file.keys().map(|k| k.to_string()).collect();
        paths.sort_by(|a, b| {
            let ma = fns_by_file
                .get(a.as_str())
                .and_then(|v| {
                    v.iter().map(|f| f.scored.crap.value).fold(None, |acc, x| {
                        Some(match acc {
                            Some(y) if y > x => y,
                            _ => x,
                        })
                    })
                })
                .unwrap_or(f64::NEG_INFINITY);
            let mb = fns_by_file
                .get(b.as_str())
                .and_then(|v| {
                    v.iter().map(|f| f.scored.crap.value).fold(None, |acc, x| {
                        Some(match acc {
                            Some(y) if y > x => y,
                            _ => x,
                        })
                    })
                })
                .unwrap_or(f64::NEG_INFINITY);
            mb.partial_cmp(&ma).unwrap_or(std::cmp::Ordering::Equal)
        });
        paths
    };

    file_order
        .into_iter()
        .filter_map(|file| {
            let fns = fns_by_file.get(file.as_str())?.clone();
            Some(build_file_card(file, &fns, threshold))
        })
        .collect()
}

fn build_file_card(file: String, fns: &[&FunctionVerdict], threshold: f64) -> FileCard {
    let exceeds_count = fns.iter().filter(|f| f.exceeds).count();
    let max_crap_value = fns
        .iter()
        .map(|f| f.scored.crap.value)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_crap = if max_crap_value.is_finite() {
        format!("{:.2}", max_crap_value)
    } else {
        "—".to_string()
    };
    let card_risk = fns
        .iter()
        .map(|f| risk_data(f.scored.crap.risk_level))
        .max()
        .unwrap_or(1);
    let open = exceeds_count > 0;

    let rows: Vec<FileFnRow> = fns.iter().map(|v| file_fn_row(v, threshold)).collect();

    FileCard {
        path: file,
        risk_data: card_risk,
        fn_count: fns.len(),
        exceeds_count,
        max_crap,
        open,
        rows,
    }
}

fn file_fn_row(v: &FunctionVerdict, threshold: f64) -> FileFnRow {
    let span = &v.scored.identity.span;
    let loc = span
        .end_line
        .saturating_sub(span.start_line)
        .saturating_add(1);
    let cov = v.scored.coverage_percent;
    // Bar fill caps at 100% but the complexity number can exceed it; we
    // scale by /20 so a CC of 20 renders as a full bar (matches the
    // Sakura design's exemplar).
    let cc_bar_pct = (v.scored.complexity * 5).min(100);
    let over_by_val = (v.scored.crap.value - threshold).max(0.0);
    FileFnRow {
        fn_name: v.scored.identity.qualified_name.clone(),
        loc,
        start_line: span.start_line,
        end_line: span.end_line,
        cc: v.scored.complexity,
        cc_risk: complexity_risk_bucket(v.scored.complexity),
        cc_bar_pct,
        cov: format!("{:.1}", cov),
        cov_risk: coverage_risk_bucket(cov),
        crap: format!("{:.2}", v.scored.crap.value),
        risk_data: risk_data(v.scored.crap.risk_level),
        risk_label: risk_label(v.scored.crap.risk_level),
        exceeds: v.exceeds,
        over_by: format!("{:.2}", over_by_val),
    }
}

/// True when there are no rows to render. Mirrors the prior reporter's
/// `visible_section_is_empty` semantics: honors grouping, otherwise
/// considers the post-filter `shown` rows.
fn visible_section_is_empty(view: &AnalysisView<'_>) -> bool {
    match view.grouped.as_ref() {
        Some(g) => g.files.is_empty(),
        None => view.shown.is_empty(),
    }
}

fn group_by_file<'a>(rows: &[&'a FunctionVerdict]) -> BTreeMap<&'a str, Vec<&'a FunctionVerdict>> {
    let mut map: BTreeMap<&str, Vec<&FunctionVerdict>> = BTreeMap::new();
    for v in rows {
        map.entry(v.scored.identity.file_path.as_str())
            .or_default()
            .push(v);
    }
    for fns in map.values_mut() {
        fns.sort_by(|a, b| {
            b.scored
                .crap
                .value
                .partial_cmp(&a.scored.crap.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    map
}

fn risk_data(level: RiskLevel) -> u8 {
    match level {
        RiskLevel::Low => 1,
        RiskLevel::Acceptable => 2,
        RiskLevel::Moderate => 3,
        RiskLevel::High => 4,
    }
}

fn risk_label(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Low => "low",
        RiskLevel::Acceptable => "acceptable",
        RiskLevel::Moderate => "moderate",
        RiskLevel::High => "high",
    }
}

fn coverage_risk_bucket(pct: f64) -> u8 {
    // High coverage = low risk. Mirrors the design's color choices.
    if pct >= 80.0 {
        1
    } else if pct >= 60.0 {
        2
    } else if pct >= 40.0 {
        3
    } else {
        4
    }
}

fn complexity_risk_bucket(cc: u32) -> u8 {
    // Loose mapping for the inline bar tint — same risk ladder as
    // CRAP itself but coarser. CC 1–3 low, 4–6 acceptable, 7–10
    // moderate, 11+ high.
    match cc {
        0..=3 => 1,
        4..=6 => 2,
        7..=10 => 3,
        _ => 4,
    }
}

fn metric_label(metric: ComplexityMetric) -> &'static str {
    match metric {
        ComplexityMetric::Cognitive => "cognitive",
        ComplexityMetric::Cyclomatic => "cyclomatic",
    }
}

// ── Multi-language HTML reporter ────────────────────────────────────
//
// Renders a unified report composing multiple per-adapter analyses.
// Single-language input passes through to the existing `format_html`
// path byte-identically (back-compat invariant — every existing
// consumer of `crap4{lang} --format html` sees no change when a
// multi-language renderer is invoked with one envelope).
//
// Multi-language input renders the Sakura-style document with a
// `.segmented` Language nav at the top, a Combined panel as the
// default, per-language panels reachable via JS, and a per-adapter
// footer provenance grid.

/// Options for the multi-language HTML reporter.
///
/// Reserved for future-proofing — additional knobs (custom panel
/// ordering, per-adapter glyph overrides, etc.) land here without
/// widening the `format_html_multi` signature. v0.7.0 carries no
/// fields; consumers pass `HtmlMultiOptions::default()`.
#[derive(Debug, Default, Clone, Copy)]
#[non_exhaustive]
pub struct HtmlMultiOptions {}

/// Render a multi-language unified HTML report.
///
/// Single-language passthrough: when `multi.languages.len() == 1`
/// (single envelope input), delegates to the existing
/// [`format_html`] with that single block's data. The output is
/// byte-identical to the direct `crap4{lang} --format html` path,
/// preserving the back-compat invariant.
///
/// Multi-language path: renders the multi-language template
/// with a Language nav above per-language and Combined panels.
/// Combined panel uses the ranked-CRAP table sorted by risk level
/// + ratio per the [`crate::domain::multi_lang`] dimensional-consistency rule.
///
/// `threshold` is the workspace-wide threshold echoed in the
/// scope banner. Per-language KPI tiles use each adapter's own
/// `block.threshold` (the dominant value the per-adapter footer
/// row also cites).
pub fn format_html_multi(
    multi: &crate::domain::multi_lang::MultiLangContext<'_>,
    threshold: f64,
    _options: HtmlMultiOptions,
) -> String {
    if multi.languages.len() == 1 {
        let block = &multi.languages[0];
        // Single-language passthrough renders byte-identical HTML to
        // the existing single-language `format_html` path. We route
        // through `format_html_inner` (the borrowed-string variant)
        // rather than `format_html` so the owned strings on
        // `LanguageBlock` flow through without a `Box::leak`-style
        // `&'static str` projection — the rendered template only
        // needs the strings to live for the duration of the call,
        // which `&block.tool_name` etc. already guarantee.
        return format_html_inner(
            &block.view,
            block.delta.as_ref(),
            threshold,
            &block.tool_name,
            &block.tool_version,
            &block.display_name,
            block.metric,
        );
    }

    render_multi_lang(multi, threshold)
}

fn render_multi_lang(
    multi: &crate::domain::multi_lang::MultiLangContext<'_>,
    threshold: f64,
) -> String {
    let language_count = multi.languages.len();
    let language_count_plus_one = (language_count + 1).to_string();

    // Verdict: PASS only when every adapter passed. Multi-language
    // verdict semantics intentionally mirror the composite scorecard
    // action's AND-aggregation — a single failing adapter fails the
    // workspace verdict.
    let all_passed = multi.languages.iter().all(|b| b.view.full.passed);
    let (overall_verdict_class, overall_verdict_label, overall_verdict_glyph) = if all_passed {
        ("pass", "PASS", "✓")
    } else {
        ("fail", "FAIL", "✕")
    };

    let language_buttons = build_language_buttons(multi);
    let combined = build_combined_view(multi, threshold);
    let language_panels = build_language_panels(multi, threshold);
    let adapters_footer = build_adapters_footer(multi);
    let combined_delta_panel = crate::core::compose::compose_combined_delta(&multi.languages)
        .map(|cd| Box::new(build_combined_delta_panel(cd)));

    let title = if language_count == 0 {
        "CRAP scorecard — multi-language".to_string()
    } else {
        let displays: Vec<&str> = multi
            .languages
            .iter()
            .map(|b| b.display_name.as_str())
            .collect();
        format!("CRAP scorecard — {}", displays.join(" + "))
    };

    let tmpl = HtmlMultiReport {
        title,
        overall_verdict_class,
        overall_verdict_label,
        overall_verdict_glyph,
        language_count,
        language_count_plus_one,
        language_buttons,
        combined,
        language_panels,
        adapters_footer,
        combined_delta_panel,
    };
    tmpl.render()
        .expect("html_multi template render is total — all fields owned")
}

#[derive(Template)]
#[template(path = "html_multi_report.html")]
struct HtmlMultiReport {
    title: String,
    overall_verdict_class: &'static str,
    overall_verdict_label: &'static str,
    overall_verdict_glyph: &'static str,
    language_count: usize,
    /// Pre-formatted `(language_count + 1)` for the CSS grid-row
    /// `1 / span N` shorthand in the footer. Done in Rust because
    /// askama lacks an arithmetic operator across types.
    language_count_plus_one: String,
    language_buttons: Vec<LangButton>,
    combined: CombinedPanel,
    language_panels: Vec<LangPanel>,
    adapters_footer: Vec<AdapterFooterRow>,
    /// Combined Delta panel: cross-adapter aggregate + ranked
    /// regressions/new violations. `None` when no language supplied a
    /// baseline; the template still renders the View nav, but the
    /// Delta tab is rendered disabled with a no-baseline tooltip so
    /// the affordance stays visible to consumers. Boxed to keep the
    /// dominant no-baseline arm cheap; same `large_enum_variant`
    /// rationale as the single-language `delta_panel`.
    combined_delta_panel: Option<Box<CombinedDeltaPanel>>,
}

struct LangButton {
    key: String,
    label: String,
    /// Sub-count text (e.g. `"42"` for the Rust panel's analyzed-
    /// function total). Empty string for the Combined button.
    count: String,
    is_active: bool,
    /// Stringified boolean for the `aria-pressed` attribute — askama
    /// can't render a `bool` to `"true"`/`"false"` inside an HTML
    /// attribute value without a manual map, so we do it once here.
    aria_pressed: &'static str,
}

struct CombinedPanel {
    total_functions: usize,
    total_exceeding: usize,
    total_files: usize,
    has_worst_ratio: bool,
    /// Formatted worst CRAP/threshold ratio (`"5.72"`). Empty when
    /// `has_worst_ratio == false`.
    worst_ratio_value: String,
    worst_function_name: String,
    worst_adapter_display: String,
    dist_low: usize,
    dist_acceptable: usize,
    dist_moderate: usize,
    dist_high: usize,
    ranked_rows: Vec<RankedRow>,
}

struct RankedRow {
    language: String,
    adapter_display: String,
    /// Single-character adapter glyph (e.g. "R" / "T"). Derived from
    /// `language` so future adapters (Go = "G", Python = "P") drop in
    /// without code changes here.
    badge_glyph: String,
    qualified_name: String,
    file_path: String,
    start_line: usize,
    end_line: usize,
    complexity: u32,
    coverage_percent: String,
    crap: String,
    ratio_display: String,
    risk_data: u8,
    risk_label: &'static str,
    exceeds: bool,
}

struct LangPanel {
    language: String,
    display_name: String,
    metric_label: &'static str,
    threshold_display: String,
    total_functions: usize,
    total_files: usize,
    exceeding_threshold: usize,
    exceeding_pct: String,
    has_max_crap: bool,
    max_crap: String,
    crap_avg: String,
    crap_med: String,
    cov_avg: String,
    cov_avg_risk: u8,
    cx_avg: String,
    cx_med: String,
    cx_max: String,
    dist_low: usize,
    dist_acceptable: usize,
    dist_moderate: usize,
    dist_high: usize,
    files: Vec<FileCard>,
    /// True when this language supplied a baseline. The template
    /// renders the Delta tab as enabled when `true`, disabled +
    /// titled with a no-baseline tooltip when `false`. The Current
    /// tab is always enabled.
    has_delta: bool,
    /// Tab-count badge for the Current tab — total analyzed
    /// functions, mirroring the single-language template's
    /// `current_tab_count`.
    current_tab_count: usize,
    /// Tab-count badge for the Delta tab — sum of all change kinds.
    /// Zero when this language has no baseline.
    delta_tab_count: u32,
    /// Drives the inline news-dot indicator on the Delta tab when
    /// regressions or new violations are present.
    delta_has_news: bool,
    /// Per-language delta panel. Boxed for the same
    /// `large_enum_variant` reason as the single-language reporter
    /// — the dominant no-baseline arm stays cheap.
    delta_panel: Option<Box<DeltaPanel>>,
}

struct AdapterFooterRow {
    display_name: String,
    metric_label: &'static str,
    threshold_display: String,
}

/// Combined Delta panel — cross-adapter aggregate plus a ranked list
/// of regressions + new violations across every language with a
/// baseline.
///
/// Boxed at the `HtmlMultiReport` level to keep the dominant
/// no-baseline arm cheap; same `large_enum_variant` rationale as the
/// per-language reporter's `DeltaPanel`. Built by
/// `build_combined_delta_panel` from a `CombinedDelta` aggregate
/// produced by `compose_combined_delta`.
struct CombinedDeltaPanel {
    /// Aggregate change counts across contributing languages.
    /// Drives the scope-banner copy + delta tab badge counts.
    summary: CombinedDeltaPanelSummary,
    /// Verdict pill class for the Combined Delta hero. "pass" when
    /// every contributing language passed; "fail" when any reported
    /// a new violation (AND-aggregated across blocks). Mirrors the
    /// per-language reporter's verdict polarity exactly.
    verdict_class: &'static str,
    verdict_label: &'static str,
    verdict_glyph: &'static str,
    /// Display labels of languages that contributed a baseline.
    /// Surfaced in the scope-banner copy so reviewers see which
    /// languages this aggregate represents.
    contributing_languages: Vec<String>,
    /// Display labels of languages with no baseline. Rendered as a
    /// scope-banner note ("TypeScript has no baseline yet — provide
    /// one via …") so the asymmetry between Current and Delta views
    /// is visible in-document, not just at the disabled-tab level.
    missing_baseline_languages: Vec<String>,
    /// Workspace-wide ranked rows. Sort: risk band desc, then
    /// CRAP/threshold ratio desc within band; per-row `kind`
    /// distinguishes regressions from new functions.
    ranked_rows: Vec<CombinedDeltaRow>,
}

#[derive(Clone, Copy)]
struct CombinedDeltaPanelSummary {
    added: u32,
    removed: u32,
    modified: u32,
    regressions: u32,
    improvements: u32,
    new_violations: u32,
}

/// One row of the Combined Delta ranked table — same shape as
/// `RankedRow` plus baseline + delta cells so reviewers see the
/// before/after CRAP transition without leaving the row.
struct CombinedDeltaRow {
    language: String,
    adapter_display: String,
    badge_glyph: String,
    qualified_name: String,
    file_path: String,
    start_line: usize,
    end_line: usize,
    /// Render label for the per-row kind: `"regression"` or `"new"`.
    /// Drives the per-row badge so reviewers can distinguish a
    /// modified-with-regression row from a brand-new function at a
    /// glance.
    kind_label: &'static str,
    /// Baseline CRAP value, `"5.20"` formatted, or empty for new
    /// functions.
    baseline_crap: String,
    /// Current CRAP value, `"45.20"` formatted.
    current_crap: String,
    /// CRAP / threshold for the current row, formatted as `"5.65"`
    /// or `"∞"` for the zero-threshold safe-divide guard.
    ratio_display: String,
    /// Signed delta string `"+40.00"`; empty for new functions.
    delta_value: String,
    /// `"▲"` / `"▼"` / `""` mirror of the per-language delta panel
    /// row glyphs. Empty when delta_value is empty.
    delta_glyph: &'static str,
    /// `"up"` / `"down"` / `"flat"` for chip color.
    delta_direction: &'static str,
    /// Current risk-pill data-risk value (1..=4).
    current_risk: u8,
    /// Current risk-pill text label.
    current_risk_label: &'static str,
    /// True when the current row's CRAP exceeds its adapter's
    /// threshold. Drives the `data-exceeds="1"` row-tint.
    exceeds: bool,
}

fn build_language_buttons(
    multi: &crate::domain::multi_lang::MultiLangContext<'_>,
) -> Vec<LangButton> {
    let mut buttons = Vec::with_capacity(multi.languages.len() + 1);
    // Combined first — it's the default active button.
    buttons.push(LangButton {
        key: "combined".to_string(),
        label: "Combined".to_string(),
        count: multi.combined.total_functions.to_string(),
        is_active: true,
        aria_pressed: "true",
    });
    for block in &multi.languages {
        buttons.push(LangButton {
            key: block.language.clone(),
            label: block.display_name.clone(),
            count: block.view.full.summary.total_functions.to_string(),
            is_active: false,
            aria_pressed: "false",
        });
    }
    buttons
}

fn build_combined_view(
    multi: &crate::domain::multi_lang::MultiLangContext<'_>,
    _threshold: f64,
) -> CombinedPanel {
    let combined = &multi.combined;
    let (has_worst, worst_ratio_value, worst_function_name, worst_adapter_display) =
        match combined.worst_ratio.as_ref() {
            Some(w) => (
                true,
                format_ratio_value(w.ratio),
                w.function_name.clone(),
                w.adapter_display.clone(),
            ),
            None => (false, String::new(), String::new(), String::new()),
        };

    let ranked_rows = combined
        .ordered_functions
        .iter()
        .map(|f| RankedRow {
            language: f.language.clone(),
            adapter_display: f.adapter_display.clone(),
            badge_glyph: adapter_glyph(&f.language, &f.adapter_display),
            qualified_name: f.identity.qualified_name.clone(),
            file_path: f.identity.file_path.clone(),
            start_line: f.identity.span.start_line,
            end_line: f.identity.span.end_line,
            complexity: f.complexity,
            coverage_percent: format!("{:.1}", f.coverage_percent),
            crap: format!("{:.2}", f.crap),
            ratio_display: format_ratio_value(f.ratio),
            risk_data: risk_data(f.risk_level),
            risk_label: risk_label(f.risk_level),
            exceeds: f.crap > f.threshold,
        })
        .collect();

    CombinedPanel {
        total_functions: combined.total_functions,
        total_exceeding: combined.total_exceeding,
        total_files: combined.total_files,
        has_worst_ratio: has_worst,
        worst_ratio_value,
        worst_function_name,
        worst_adapter_display,
        dist_low: combined.distribution.low,
        dist_acceptable: combined.distribution.acceptable,
        dist_moderate: combined.distribution.moderate,
        dist_high: combined.distribution.high,
        ranked_rows,
    }
}

fn build_language_panels(
    multi: &crate::domain::multi_lang::MultiLangContext<'_>,
    threshold: f64,
) -> Vec<LangPanel> {
    multi
        .languages
        .iter()
        .map(|block| {
            let summary = &block.view.full.summary;
            // Per-panel threshold: prefer the adapter's own
            // `block.threshold`; fall back to the workspace
            // `threshold` arg if the adapter envelope didn't carry
            // one (zero-default edge case).
            let panel_threshold = if block.threshold > 0.0 {
                block.threshold
            } else {
                threshold
            };
            let summary_view = summary_view(summary, panel_threshold);
            let is_empty = visible_section_is_empty(&block.view);
            let files = if is_empty {
                Vec::new()
            } else {
                file_cards(&block.view, panel_threshold)
            };
            let delta_panel = block.delta.as_ref().map(|d| Box::new(build_delta_panel(d)));
            let has_delta = delta_panel.is_some();
            let current_tab_count = if is_empty { 0 } else { summary.total_functions };
            let delta_tab_count = delta_panel
                .as_ref()
                .map(|p| p.summary.added + p.summary.removed + p.summary.modified)
                .unwrap_or(0);
            let delta_has_news = delta_panel
                .as_ref()
                .map(|p| p.summary.regressions > 0 || p.summary.new_violations > 0)
                .unwrap_or(false);
            LangPanel {
                language: block.language.clone(),
                display_name: block.display_name.clone(),
                metric_label: metric_label(block.metric),
                threshold_display: format!("{:.2}", panel_threshold),
                total_functions: summary_view.total_functions,
                total_files: summary_view.total_files,
                exceeding_threshold: summary_view.exceeding_threshold,
                exceeding_pct: summary_view.exceeding_pct,
                has_max_crap: summary_view.has_max_crap,
                max_crap: summary_view.max_crap,
                crap_avg: summary_view.crap_avg,
                crap_med: summary_view.crap_med,
                cov_avg: summary_view.cov_avg,
                cov_avg_risk: summary_view.cov_avg_risk,
                cx_avg: summary_view.cx_avg,
                cx_med: summary_view.cx_med,
                cx_max: summary_view.cx_max,
                dist_low: summary_view.dist_low,
                dist_acceptable: summary_view.dist_acceptable,
                dist_moderate: summary_view.dist_moderate,
                dist_high: summary_view.dist_high,
                files,
                has_delta,
                current_tab_count,
                delta_tab_count,
                delta_has_news,
                delta_panel,
            }
        })
        .collect()
}

fn build_adapters_footer(
    multi: &crate::domain::multi_lang::MultiLangContext<'_>,
) -> Vec<AdapterFooterRow> {
    multi
        .languages
        .iter()
        .map(|block| AdapterFooterRow {
            display_name: block.display_name.clone(),
            metric_label: metric_label(block.metric),
            threshold_display: format!("{:.2}", block.threshold),
        })
        .collect()
}

/// Derive a single-character glyph for the adapter badge from the
/// adapter identity. Falls back to the first ASCII alphanumeric of
/// the display name (uppercased) so new adapters drop in without
/// code edits.
fn adapter_glyph(language: &str, display_name: &str) -> String {
    if let Some(c) = display_name.chars().find(|c| c.is_ascii_alphanumeric()) {
        return c.to_ascii_uppercase().to_string();
    }
    if let Some(c) = language.chars().find(|c| c.is_ascii_alphanumeric()) {
        return c.to_ascii_uppercase().to_string();
    }
    "?".to_string()
}

/// Format a CRAP/threshold ratio as `"N.NN"`. Infinity (the safe-
/// divide guard from `multi_lang::safe_ratio` for zero-threshold
/// envelopes) renders as `"∞"` so the ranked table still displays a
/// readable value.
fn format_ratio_value(ratio: f64) -> String {
    if ratio.is_infinite() {
        "∞".to_string()
    } else {
        format!("{:.2}", ratio)
    }
}

/// Build the Combined Delta panel template projection from the
/// composed cross-adapter aggregate.
///
/// Verdict polarity mirrors the per-language reporter: any
/// `new_violations` count above zero flips the verdict to
/// REGRESSED; otherwise PASS. Caller has already guaranteed at least
/// one language contributed (the aggregate is wrapped in `Option<>`
/// so the no-baseline arm of the renderer never reaches this code).
fn build_combined_delta_panel(cd: crate::domain::multi_lang::CombinedDelta) -> CombinedDeltaPanel {
    let summary = CombinedDeltaPanelSummary {
        added: cd.summary.added,
        removed: cd.summary.removed,
        modified: cd.summary.modified,
        regressions: cd.summary.regressions,
        improvements: cd.summary.improvements,
        new_violations: cd.summary.new_violations,
    };

    let (verdict_class, verdict_label, verdict_glyph) = if cd.summary.passed {
        ("pass", "PASS", "✓")
    } else {
        ("fail", "REGRESSED", "▲")
    };

    let ranked_rows: Vec<CombinedDeltaRow> = cd
        .ordered_rows
        .into_iter()
        .map(|r| {
            let (kind_label, baseline_crap, delta_value, delta_glyph, delta_direction) =
                match r.kind {
                    crate::domain::multi_lang::RankedDeltaKind::Regression => {
                        let baseline = r
                            .baseline
                            .as_ref()
                            .expect("regressions always carry a baseline snapshot");
                        let delta = r.current.crap - baseline.crap;
                        (
                            "regression",
                            format!("{:.2}", baseline.crap),
                            format!("{:+.2}", delta),
                            signed_glyph_f64(delta),
                            direction_f64(delta),
                        )
                    }
                    crate::domain::multi_lang::RankedDeltaKind::NewFunction => {
                        ("new", String::new(), String::new(), "", "flat")
                    }
                };
            CombinedDeltaRow {
                badge_glyph: adapter_glyph(&r.language, &r.adapter_display),
                language: r.language,
                adapter_display: r.adapter_display,
                qualified_name: r.current.identity.qualified_name.clone(),
                file_path: r.current.identity.file_path.clone(),
                start_line: r.current.identity.span.start_line,
                end_line: r.current.identity.span.end_line,
                kind_label,
                baseline_crap,
                current_crap: format!("{:.2}", r.current.crap),
                ratio_display: format_ratio_value(r.ratio),
                delta_value,
                delta_glyph,
                delta_direction,
                current_risk: risk_data(r.current.risk_level),
                current_risk_label: risk_label(r.current.risk_level),
                exceeds: r.current.exceeds,
            }
        })
        .collect();

    CombinedDeltaPanel {
        summary,
        verdict_class,
        verdict_label,
        verdict_glyph,
        contributing_languages: cd.contributing_languages,
        missing_baseline_languages: cd.missing_baseline_languages,
        ranked_rows,
    }
}

// ── Delta-panel projection ──────────────────────────────────────────
//
// Pulls baseline + current aggregates off the `AnalysisDelta` and
// shapes the four KPI tiles + per-category row lists. The summary on
// `DeltaPanel` mirrors `DeltaSummary` field-for-field (copied so the
// template doesn't import a domain type — same boundary discipline as
// the markdown reporter's `SummaryData`).
//
// The 0.005 cutoff on the regressions / improvements partition matches
// the markdown reporter's filter: a delta below half a cent rounds to
// "+0.00" in `{:.2}` cell output and looks like a false flag. The
// "unchanged" bucket captures every Modified row that doesn't qualify
// as a regression or improvement under that cutoff — including
// genuinely-identical rows that show up as `Modified` with delta = 0.0
// (e.g. when a function's surrounding LOC changed but its body
// didn't).

fn build_delta_panel(view: &DeltaView<'_>) -> DeltaPanel {
    let summary = view.full.summary;
    let baseline_summary = &view.full.baseline.summary;
    let current_summary = &view.full.current.summary;
    let panel_summary = DeltaPanelSummary {
        added: summary.added,
        removed: summary.removed,
        modified: summary.modified,
        regressions: summary.regressions,
        improvements: summary.improvements,
        new_violations: summary.new_violations,
    };

    let (verdict_class, verdict_label, verdict_glyph) = if summary.passed {
        ("pass", "PASS", "✓")
    } else {
        ("fail", "REGRESSED", "▲")
    };

    let kpis = build_delta_kpis(&summary, baseline_summary, current_summary);

    let mut regressions: Vec<DeltaRow> = Vec::new();
    let mut improvements: Vec<DeltaRow> = Vec::new();
    let mut new_functions: Vec<DeltaRow> = Vec::new();

    // The shaped `view.shown` is sort-by-signed-impact descending by
    // default, so we get regressions first → improvements last under
    // the default spec. Within each bucket we preserve that order
    // (largest-impact-first) so the most consequential changes lead.
    for change in view.shown.iter().copied() {
        match change {
            FunctionChange::Added { current } => {
                new_functions.push(added_row(current));
            }
            FunctionChange::Removed { .. } => {
                // v1 design intentionally drops the Removed-zero
                // panel per chat1.md simplification, and the regular
                // case (Removed > 0) isn't surfaced in this iteration
                // either — the chat1.md trim treats removed functions
                // as out of scope for a regression-focused scorecard.
                // Counts still ride in the summary line.
            }
            FunctionChange::Modified { baseline, current } => {
                let delta = current.scored.crap.value - baseline.scored.crap.value;
                if delta >= 0.005 {
                    regressions.push(modified_row(baseline, current));
                } else if delta <= -0.005 {
                    improvements.push(modified_row(baseline, current));
                }
            }
        }
    }

    // Count unchanged from the FULL delta (pre-truncate, pre-sort) so a
    // `--top N` cap doesn't silently lop them off — under the default
    // signed-impact sort, near-zero-delta entries land at the bottom of
    // the list and are the first to drop on truncation. Respect the
    // user's `change_kinds` filter so a deliberate exclusion of Modified
    // entries doesn't get re-surfaced in the footer line.
    let unchanged_count: u32 = view
        .full
        .changes
        .iter()
        .filter(|c| {
            view.spec
                .filters
                .change_kinds
                .as_ref()
                .is_none_or(|kinds| kinds.contains(&c.kind()))
        })
        .filter(|c| match c {
            FunctionChange::Modified { baseline, current } => {
                (current.scored.crap.value - baseline.scored.crap.value).abs() < 0.005
            }
            _ => false,
        })
        .count() as u32;

    DeltaPanel {
        summary: panel_summary,
        verdict_class,
        verdict_label,
        verdict_glyph,
        kpis,
        regressions,
        improvements,
        new_functions,
        unchanged_count,
        // F2 follow-up: when `--baseline-ref <label>` lands, thread
        // the label through `DeltaView.baseline_ref` and surface it
        // here. Until then the honest label is the literal "baseline."
        baseline_ref: "baseline",
    }
}

fn build_delta_kpis(
    summary: &crate::domain::delta::DeltaSummary,
    baseline: &AnalysisSummary,
    current: &AnalysisSummary,
) -> [DeltaKpi; 4] {
    // KPI 1 — exceeding threshold (count integer). Higher = worse.
    let before_exc = baseline.exceeding_threshold;
    let after_exc = current.exceeding_threshold;
    let exc_delta = after_exc as i64 - before_exc as i64;
    let exc_note = if summary.new_violations > 0 {
        format!(
            "{} new {} broke the threshold.",
            summary.new_violations,
            if summary.new_violations == 1 {
                "function"
            } else {
                "functions"
            }
        )
    } else if exc_delta < 0 {
        format!(
            "Threshold breaches dropped by {}.",
            exc_delta.unsigned_abs()
        )
    } else {
        "No new threshold breaches.".to_string()
    };
    let exceeding = DeltaKpi {
        label: "Exceeding threshold",
        before: format!("{}", before_exc),
        after: format!("{}", after_exc),
        chip_glyph: signed_glyph_int(exc_delta),
        chip_value: signed_int_chip(exc_delta),
        direction: direction_int(exc_delta),
        // For exceeding-count, higher = worse (regression).
        is_regression: exc_delta > 0,
        note: exc_note,
    };

    // KPI 2 — Max CRAP. Higher = worse.
    let before_max = baseline.max_crap.as_ref().map(|c| c.value).unwrap_or(0.0);
    let after_max = current.max_crap.as_ref().map(|c| c.value).unwrap_or(0.0);
    let max_delta = after_max - before_max;
    let max_crap = DeltaKpi {
        label: "Max CRAP",
        before: format!("{:.2}", before_max),
        after: format!("{:.2}", after_max),
        chip_glyph: signed_glyph_f64(max_delta),
        chip_value: signed_f64_chip(max_delta),
        direction: direction_f64(max_delta),
        is_regression: max_delta > 0.005,
        note: String::new(),
    };

    // KPI 3 — Average CRAP. Higher = worse.
    let before_avg = baseline.average_crap;
    let after_avg = current.average_crap;
    let avg_delta = after_avg - before_avg;
    let avg_crap = DeltaKpi {
        label: "Average CRAP",
        before: format!("{:.2}", before_avg),
        after: format!("{:.2}", after_avg),
        chip_glyph: signed_glyph_f64(avg_delta),
        chip_value: signed_f64_chip(avg_delta),
        direction: direction_f64(avg_delta),
        is_regression: avg_delta > 0.005,
        note: format!(
            "{} added · {} removed · {} modified",
            summary.added, summary.removed, summary.modified
        ),
    };

    // KPI 4 — Avg coverage. Higher = better (inverted regression
    // polarity vs the CRAP-style KPIs).
    let before_cov = baseline.average_coverage;
    let after_cov = current.average_coverage;
    let cov_delta = after_cov - before_cov;
    let cov_chip_value = if cov_delta.abs() < 0.05 {
        String::new()
    } else {
        format!("{:+.1} pp", cov_delta)
    };
    let avg_cov = DeltaKpi {
        label: "Avg coverage",
        before: format!("{:.1}%", before_cov),
        after: format!("{:.1}%", after_cov),
        chip_glyph: signed_glyph_f64(cov_delta),
        chip_value: cov_chip_value,
        direction: direction_f64(cov_delta),
        // For coverage, lower = worse (regression).
        is_regression: cov_delta < -0.05,
        note: if cov_delta.abs() < 0.05 {
            "Coverage unchanged.".to_string()
        } else if cov_delta > 0.0 {
            "Coverage moved in the right direction.".to_string()
        } else {
            "Coverage dropped.".to_string()
        },
    };

    [exceeding, max_crap, avg_crap, avg_cov]
}

fn modified_row(baseline: &FunctionVerdict, current: &FunctionVerdict) -> DeltaRow {
    let baseline_value = baseline.scored.crap.value;
    let current_value = current.scored.crap.value;
    let delta = current_value - baseline_value;
    let span = &current.scored.identity.span;
    DeltaRow {
        file: current.scored.identity.file_path.clone(),
        qualified_name: current.scored.identity.qualified_name.clone(),
        start_line: span.start_line,
        end_line: span.end_line,
        baseline_crap: format!("{:.2}", baseline_value),
        current_crap: format!("{:.2}", current_value),
        baseline_cov: format!("{:.1}%", baseline.scored.coverage_percent),
        current_cov: format!("{:.1}%", current.scored.coverage_percent),
        delta_value: format!("{:+.2}", delta),
        delta_glyph: signed_glyph_f64(delta),
        delta_direction: direction_f64(delta),
        baseline_risk: risk_data(baseline.scored.crap.risk_level),
        baseline_risk_label: risk_label(baseline.scored.crap.risk_level),
        current_risk: risk_data(current.scored.crap.risk_level),
        current_risk_label: risk_label(current.scored.crap.risk_level),
        exceeds: current.exceeds,
    }
}

fn added_row(current: &FunctionVerdict) -> DeltaRow {
    let span = &current.scored.identity.span;
    DeltaRow {
        file: current.scored.identity.file_path.clone(),
        qualified_name: current.scored.identity.qualified_name.clone(),
        start_line: span.start_line,
        end_line: span.end_line,
        baseline_crap: String::new(),
        current_crap: format!("{:.2}", current.scored.crap.value),
        baseline_cov: String::new(),
        current_cov: format!("{:.1}%", current.scored.coverage_percent),
        delta_value: String::new(),
        delta_glyph: "",
        delta_direction: "flat",
        baseline_risk: 0,
        baseline_risk_label: "",
        current_risk: risk_data(current.scored.crap.risk_level),
        current_risk_label: risk_label(current.scored.crap.risk_level),
        exceeds: current.exceeds,
    }
}

fn signed_glyph_f64(delta: f64) -> &'static str {
    if delta > 0.005 {
        "▲"
    } else if delta < -0.005 {
        "▼"
    } else {
        ""
    }
}

fn signed_glyph_int(delta: i64) -> &'static str {
    if delta > 0 {
        "▲"
    } else if delta < 0 {
        "▼"
    } else {
        ""
    }
}

fn signed_f64_chip(delta: f64) -> String {
    if delta.abs() < 0.005 {
        String::new()
    } else {
        format!("{:+.2}", delta)
    }
}

fn signed_int_chip(delta: i64) -> String {
    if delta == 0 {
        String::new()
    } else {
        format!("{:+}", delta)
    }
}

fn direction_f64(delta: f64) -> &'static str {
    if delta > 0.005 {
        "up"
    } else if delta < -0.005 {
        "down"
    } else {
        "flat"
    }
}

fn direction_int(delta: i64) -> &'static str {
    if delta > 0 {
        "up"
    } else if delta < 0 {
        "down"
    } else {
        "flat"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::{
        TEST_RULE_HELP_URI, TEST_TOOL_INFO_URI, TEST_TOOL_NAME, TEST_TOOL_VERSION,
        make_empty_result, make_multi_function_result, make_single_function_result,
        make_view_default,
    };
    use crate::domain::multi_lang::LanguageBlock;
    use crate::domain::types::RiskLevel;

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
            default_excludes: &[],
            forced_excludes: &[],
            default_metric: ComplexityMetric::Cognitive,
        }
    }

    fn html(view: &AnalysisView<'_>) -> String {
        format_html(view, None, 8.0, &test_meta(), ComplexityMetric::Cognitive)
    }

    fn html_with_delta(view: &AnalysisView<'_>, delta: &DeltaView<'_>) -> String {
        format_html(
            view,
            Some(delta),
            8.0,
            &test_meta(),
            ComplexityMetric::Cognitive,
        )
    }

    #[test]
    fn empty_renders_doctype_and_empty_marker() {
        let result = make_empty_result();
        let out = html(&make_view_default(&result));
        assert!(out.starts_with("<!doctype html>"));
        assert!(out.contains("No functions to display"));
        assert!(out.contains("</html>"));
    }

    #[test]
    fn self_contained_no_external_assets() {
        let result = make_multi_function_result();
        let out = html(&make_view_default(&result));
        // No `<script src=…>` (inline scripts are now permitted per
        // the Sakura design handoff for theme/filter behavior).
        assert!(
            !out.contains("<script src"),
            "html should ship no external scripts"
        );
        // No `<link …>` to any external stylesheet/font/asset.
        assert!(
            !out.contains("<link "),
            "html should ship no <link> elements"
        );
        // No `@import` directives in any inline `<style>` block.
        assert!(
            !out.contains("@import"),
            "html should ship no @import directives"
        );
        // No `src="http…"` or `href="http…"` attributes (the
        // *fetched-asset* patterns). Bare `http://` substrings are
        // allowed because XML namespace declarations like
        // `<svg xmlns='http://www.w3.org/2000/svg'>` use the URL form
        // by spec without triggering any network access — and Sakura's
        // inline-SVG `data:` URI for the search icon contains one.
        for fetched in [
            "src=\"http://",
            "src=\"https://",
            "href=\"http://",
            "href=\"https://",
        ] {
            assert!(
                !out.contains(fetched),
                "html should ship no externally-fetched assets, found `{fetched}`"
            );
        }
    }

    #[test]
    fn passes_when_no_violations() {
        let result =
            make_single_function_result("ok", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let out = html(&make_view_default(&result));
        assert!(out.contains("verdict is-pass"));
        assert!(!out.contains("verdict is-fail"));
    }

    #[test]
    fn fails_when_threshold_exceeded() {
        let result =
            make_single_function_result("bad", "src/lib.rs", 20, 10.0, 45.0, RiskLevel::High, 8.0);
        let out = html(&make_view_default(&result));
        assert!(out.contains("verdict is-fail"));
        // The failing row carries the high risk-pill via data-risk=4.
        assert!(out.contains("data-risk=\"4\""));
        // Exceeding files render pre-expanded.
        assert!(out.contains("severity-card file-card") && out.contains(" open>"));
    }

    #[test]
    fn risk_levels_render_distinct_data_attrs() {
        let result = make_multi_function_result();
        let out = html(&make_view_default(&result));
        assert!(out.contains("data-risk=\"1\""));
        assert!(out.contains("data-risk=\"3\""));
        assert!(out.contains("data-risk=\"4\""));
    }

    #[test]
    fn escapes_html_in_function_names() {
        let result = make_single_function_result(
            "<script>alert('x')</script>",
            "src/lib.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let out = html(&make_view_default(&result));
        assert!(!out.contains("<script>alert"));
        assert!(out.contains("&#60;script&#62;") || out.contains("&lt;script&gt;"));
    }

    #[test]
    fn escapes_html_in_file_paths() {
        let result = make_single_function_result(
            "f",
            "src/<dangerous>.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let out = html(&make_view_default(&result));
        assert!(!out.contains("src/<dangerous>"));
        assert!(out.contains("&#60;dangerous&#62;") || out.contains("&lt;dangerous&gt;"));
    }

    #[test]
    fn groups_functions_by_file() {
        let result = make_multi_function_result();
        let out = html(&make_view_default(&result));
        // Three distinct files in the fixture → three file cards.
        assert_eq!(out.matches("class=\"severity-card file-card\"").count(), 3);
    }

    #[test]
    fn risk_distribution_shows_all_buckets() {
        let result = make_multi_function_result();
        let out = html(&make_view_default(&result));
        for risk in [1u8, 2, 3, 4] {
            let needle = format!("dist-seg\" data-risk=\"{risk}\"");
            assert!(out.contains(&needle), "missing dist-seg for risk {risk}");
        }
    }

    #[test]
    fn doctype_present_and_lang_set() {
        let result = make_empty_result();
        let out = html(&make_view_default(&result));
        assert!(out.contains("<!doctype html>"));
        assert!(out.contains("<html lang=\"en\">"));
        assert!(out.contains("viewport"));
    }

    #[test]
    fn empty_after_filter_renders_empty_marker() {
        use crate::domain::view::{self, CoverageRange, Filters, ViewSpec};
        let result = make_multi_function_result();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(CoverageRange::new(99.0, 100.0).unwrap()),
                ..Filters::default()
            },
            ..ViewSpec::default()
        };
        let view = view::apply(&result, spec);
        assert!(view.shown.is_empty());
        let out = html(&view);
        assert!(out.contains("No functions to display"));
        assert!(!out.contains("Functions by file"));
    }

    #[test]
    fn grouped_view_honors_file_top_n_and_order() {
        use crate::domain::view::{self, GroupKey, SortKey, ViewSpec};
        let result = make_multi_function_result();
        let spec = ViewSpec {
            sort: SortKey::Crap,
            group_by: Some(GroupKey::File),
            limit: Some(1),
            ..ViewSpec::default()
        };
        let view = view::apply(&result, spec);
        assert!(view.grouped.is_some());
        let out = html(&view);
        assert_eq!(
            out.matches("class=\"severity-card file-card\"").count(),
            1,
            "only the top-1 file should be rendered when grouped"
        );
        assert!(out.contains("src/domain/crap.rs"));
        assert!(!out.contains("src/lib.rs"));
        assert!(!out.contains("src/adapters/coverage/mod.rs"));
    }

    #[test]
    fn per_adapter_footer_renders_metric_and_threshold() {
        let result = make_multi_function_result();
        let out = format_html(
            &make_view_default(&result),
            None,
            8.0,
            &test_meta(),
            ComplexityMetric::Cognitive,
        );
        assert!(out.contains("footer-adapters"));
        assert!(
            out.contains(">Test<"),
            "display_name should appear in footer"
        );
        assert!(
            out.contains("cognitive complexity"),
            "footer should show effective metric"
        );
        assert!(out.contains("8.00"), "footer should show threshold");
    }

    #[test]
    fn footer_reflects_cyclomatic_when_effective_metric_is_cyclomatic() {
        let result = make_multi_function_result();
        let out = format_html(
            &make_view_default(&result),
            None,
            10.0,
            &test_meta(),
            ComplexityMetric::Cyclomatic,
        );
        assert!(out.contains("cyclomatic complexity"));
        assert!(out.contains("10.00"));
    }

    #[test]
    fn dark_mode_toggle_present() {
        let result = make_multi_function_result();
        let out = html(&make_view_default(&result));
        assert!(out.contains("id=\"theme-toggle\""));
        assert!(out.contains("data-theme"));
    }

    /// Byte-level snapshot lock for the HTML reporter under the
    /// Sakura design (crap-rs#260).
    #[test]
    fn full_html_snapshot() {
        let result = make_multi_function_result();
        let out = html(&make_view_default(&result));
        insta::assert_snapshot!(out);
    }

    // ── Delta tab (crap-rs#306) ───────────────────────────────────────

    use crate::adapters::reporters::test_fixtures::{make_delta_view_default, make_sample_delta};

    /// Without `--baseline`, the tabs nav and the second `<div
    /// class="tab-panel">` MUST NOT render. This is the byte-identical
    /// contract preservation: the v0.5.0 single-tab output is exactly
    /// what consumers without a baseline still get.
    #[test]
    fn no_tabs_nav_when_delta_is_none() {
        let result = make_multi_function_result();
        let out = html(&make_view_default(&result));
        assert!(
            !out.contains("<nav class=\"tabs\""),
            "no-baseline output must not emit the tabs nav"
        );
        assert!(
            !out.contains("data-tab=\"delta\""),
            "no-baseline output must not emit the delta tab panel"
        );
        assert!(
            !out.contains("data-tab=\"current\""),
            "no-baseline output must not wrap body in current tab panel"
        );
    }

    #[test]
    fn delta_tab_renders_when_delta_is_some() {
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        // Tabs nav present
        assert!(
            out.contains("<nav class=\"tabs\""),
            "tabs nav should render when delta is supplied"
        );
        // Current tab opens by default (default-open lock from
        // orchestrator pre-resolved Discovery #1)
        assert!(
            out.contains("data-tab=\"current\" data-active"),
            "Current tab should open by default"
        );
        // Delta panel present (without data-active)
        assert!(
            out.contains("<div class=\"tab-panel\" data-tab=\"delta\""),
            "delta tab panel should render"
        );
    }

    #[test]
    fn tabs_nav_has_two_tabs_when_delta_supplied() {
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        // Each tab carries a data-tab="X" attribute on a <button>
        assert!(out.contains("data-tab=\"current\""));
        assert!(out.contains("data-tab=\"delta\""));
        // The Delta tab label is anchored on the literal baseline-ref
        // string until F2 introduces `--baseline-ref <label>`.
        assert!(
            out.contains("Delta vs baseline"),
            "delta tab label should anchor on the baseline-ref literal"
        );
    }

    #[test]
    fn delta_panel_has_exactly_4_kpi_tiles() {
        // 4-tile lock from orchestrator pre-resolved Discovery #2 —
        // matches the Current tab's 4-KPI convention. Mirrors the
        // playwright assertion (`.delta-kpi-grid .kpi` count == 4).
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        let count = out.matches("class=\"delta-kpi\"").count();
        assert_eq!(
            count, 4,
            "delta panel must render exactly 4 KPI tiles, got {count}"
        );
    }

    #[test]
    fn delta_kpi_tiles_include_expected_labels() {
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        assert!(out.contains("Exceeding threshold"));
        assert!(out.contains("Max CRAP"));
        assert!(out.contains("Average CRAP"));
        assert!(out.contains("Avg coverage"));
        // The dropped 5th tile ("Functions") MUST NOT appear in the
        // delta panel as a tile (the word can show up elsewhere in
        // the report — we anchor on the tile structure).
        assert!(
            !out.contains("<span class=\"delta-kpi-label\">Functions</span>"),
            "the 5th 'Functions' tile from the mock is intentionally dropped per orchestrator-locked Discovery #2"
        );
    }

    #[test]
    fn delta_panel_renders_regressions_table() {
        // make_sample_delta sets parse_record's CRAP 15.0 → 22.0
        // (Modified with delta = +7.0), which qualifies as a
        // regression under the 0.005 cutoff.
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        assert!(
            out.contains("delta-table regressions"),
            "regressions table should render when at least one positive delta is present"
        );
        assert!(out.contains("parse_record"));
        // The +7.00 chip value should appear in some form (signed).
        assert!(
            out.contains("+7.00"),
            "regression delta chip should render the signed value"
        );
    }

    #[test]
    fn delta_panel_renders_new_functions_table() {
        // make_sample_delta adds new_fn (Added, exceeds=true, CRAP=30.0).
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        assert!(
            out.contains("delta-table new-functions"),
            "new-functions table should render when at least one Added change is present"
        );
        assert!(out.contains("new_fn"));
        // The new violation gets a high-risk pill (data-risk=4).
        assert!(out.contains("data-risk=\"4\""));
    }

    #[test]
    fn delta_panel_improvements_table_absent_when_no_improvements() {
        // make_sample_delta has no improvements (parse_record went
        // up; simple_fn stayed flat at 3.0; complex_fn was removed
        // not modified). So the improvements table should not render.
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        assert!(
            !out.contains("delta-table improvements"),
            "improvements table should be suppressed when there are no improvements"
        );
    }

    #[test]
    fn delta_panel_unchanged_rendered_as_single_line_not_table() {
        // simple_fn is Modified with zero delta (3.0 → 3.0) — it
        // hits the unchanged bucket. The render should be a single
        // <p>...</p>-style note, not a full table.
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        assert!(
            out.contains("delta-unchanged"),
            "unchanged count should render as a single-line note (the chat1.md trim)"
        );
        assert!(
            !out.contains("delta-table unchanged"),
            "unchanged must NOT render as a full table — single-line note only"
        );
    }

    #[test]
    fn delta_panel_unchanged_count_survives_top_truncation() {
        // Regression guard: the unchanged_count footer is computed from
        // the FULL delta, not from `view.shown`. Under a `--top 1`
        // truncation that lops the unchanged Modified row off the tail
        // of the signed-impact sort, the count must still report 1
        // (make_sample_delta has exactly one zero-delta Modified row:
        // `simple_fn`). Truncating to `Some(1)` keeps only the
        // top-ranked regression in `view.shown`, so the old loop-based
        // counter would have read 0.
        use crate::domain::delta::DeltaViewSpec;
        let delta = make_sample_delta();
        let truncated = crate::domain::delta::apply(
            &delta,
            DeltaViewSpec {
                limit: Some(1),
                ..Default::default()
            },
        );
        let out = html_with_delta(&make_view_default(&delta.current), &truncated);
        assert!(
            out.contains("class=\"delta-unchanged\""),
            "unchanged footer must render even when --top truncates the row off view.shown"
        );
        assert!(
            out.contains("1 function"),
            "unchanged_count should be 1 (from full delta) not 0 (from truncated shown subset)"
        );
    }

    #[test]
    fn delta_tab_news_dot_when_regressions_or_new_violations_present() {
        // make_sample_delta has 1 regression + 1 new violation → dot.
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        assert!(
            out.contains("class=\"tab-dot\""),
            "delta tab should render a news dot when there are regressions or new violations"
        );
    }

    #[test]
    fn delta_tab_no_news_dot_when_clean() {
        // A self-vs-self delta has 0 regressions and 0 new violations,
        // so the dot suppression path is exercised.
        let result = make_multi_function_result();
        let delta = crate::domain::delta::compute(result.clone(), result.clone());
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&result), &dview);
        assert!(!out.contains("class=\"tab-dot\""), "no news → no dot");
    }

    #[test]
    fn delta_tab_script_present_when_delta_supplied() {
        // The inline JS hook that switches tabs + restores from #hash
        // MUST ship only when a second panel exists — otherwise it's
        // dead code in the no-baseline output (which fails the
        // byte-identical contract).
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        assert!(
            out.contains("// ── Tab switcher"),
            "tab-switcher IIFE should be present when delta is supplied"
        );
    }

    #[test]
    fn delta_tab_script_absent_when_no_delta() {
        // Companion to the test above — confirms the no-baseline
        // output doesn't carry a dead tab-switcher.
        let result = make_multi_function_result();
        let out = html(&make_view_default(&result));
        assert!(
            !out.contains("// ── Tab switcher"),
            "tab-switcher must be absent when no delta — byte-identical no-baseline contract"
        );
    }

    #[test]
    fn delta_panel_escapes_function_names() {
        use crate::domain::types::{AnalysisResult, AnalysisSummary, RiskDistribution};
        // Build a hand-crafted Added change with a hostile name and
        // file path.
        let evil = crate::adapters::reporters::test_fixtures::make_verdict(
            "<script>alert('x')</script>",
            "src/<dangerous>.rs",
            5,
            50.0,
            45.0,
            RiskLevel::High,
            8.0,
        );
        let baseline = AnalysisResult {
            functions: vec![],
            summary: AnalysisSummary {
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 0,
                    moderate: 0,
                    high: 0,
                },
                ..Default::default()
            },
            passed: true,
        };
        let current_summary = AnalysisSummary {
            total_functions: 1,
            total_files: 1,
            exceeding_threshold: 1,
            average_crap: 45.0,
            median_crap: 45.0,
            distribution: RiskDistribution {
                low: 0,
                acceptable: 0,
                moderate: 0,
                high: 1,
            },
            ..Default::default()
        };
        let current = AnalysisResult {
            functions: vec![evil],
            summary: current_summary,
            passed: false,
        };
        let delta = crate::domain::delta::compute(baseline, current);
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        assert!(
            !out.contains("<script>alert"),
            "hostile fn name must be HTML-escaped"
        );
        assert!(
            !out.contains("src/<dangerous>"),
            "hostile path must be HTML-escaped"
        );
    }

    /// Byte-level snapshot lock for the HTML reporter's delta-tab
    /// render (crap-rs#306).
    #[test]
    fn full_html_with_delta_snapshot() {
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let out = html_with_delta(&make_view_default(&delta.current), &dview);
        insta::assert_snapshot!(out);
    }

    // ── Multi-language reporter tests ──────────────────────────────

    fn build_block<'a>(
        result: &'a crate::domain::types::AnalysisResult,
        tool_name: &str,
        display_name: &str,
        language: &str,
        metric: ComplexityMetric,
        threshold: f64,
    ) -> crate::domain::multi_lang::LanguageBlock<'a> {
        crate::domain::multi_lang::LanguageBlock {
            tool_name: tool_name.to_string(),
            display_name: display_name.to_string(),
            language: language.to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric,
            threshold,
            view: make_view_default(result),
            delta: None,
        }
    }

    #[test]
    fn multi_lang_single_language_passthrough_byte_identical() {
        let result = make_multi_function_result();
        let direct = format_html(
            &make_view_default(&result),
            None,
            8.0,
            &test_meta(),
            ComplexityMetric::Cognitive,
        );

        let block = build_block(
            &result,
            TEST_TOOL_NAME,
            "Test",
            "rust",
            ComplexityMetric::Cognitive,
            8.0,
        );
        let multi = crate::core::compose::compose_multi_lang(vec![block]);
        let unified = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

        // Single-language passthrough must be byte-identical to the
        // single-binary render path — that's the back-compat invariant
        // every existing consumer of `crap4{lang} --format html`
        // relies on.
        assert_eq!(direct, unified);
        assert!(!unified.contains("data-multi-lang"));
        assert!(!unified.contains("lang-nav"));
    }

    #[test]
    fn multi_lang_two_language_renders_segmented_nav_and_combined_panel() {
        let rs_result = make_multi_function_result();
        let ts_result = make_single_function_result(
            "ts::fn",
            "src/x.ts",
            5,
            70.0,
            8.5,
            RiskLevel::Acceptable,
            8.0,
        );

        let blocks = vec![
            build_block(
                &rs_result,
                "crap4rs",
                "Rust",
                "rust",
                ComplexityMetric::Cognitive,
                8.0,
            ),
            build_block(
                &ts_result,
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
        ];
        let multi = crate::core::compose::compose_multi_lang(blocks);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

        // Document carries the multi-language marker + segmented nav.
        assert!(
            out.contains("data-multi-lang"),
            "expected multi-lang body marker"
        );
        assert!(
            out.contains("class=\"lang-nav segmented\""),
            "expected language nav"
        );
        assert!(out.contains("data-lang=\"rust\""));
        assert!(out.contains("data-lang=\"typescript\""));
        assert!(out.contains("data-lang=\"combined\""));
        // Combined panel is the default-active.
        assert!(
            out.contains("<div class=\"lang-panel\" data-lang=\"combined\" data-active>"),
            "Combined panel must render with data-active attribute"
        );
        // Per-language panels exist but are inactive by default.
        assert!(out.contains("<div class=\"lang-panel\" data-lang=\"rust\">"));
        assert!(out.contains("<div class=\"lang-panel\" data-lang=\"typescript\">"));
        // Footer carries the Adapters provenance grid.
        assert!(out.contains("class=\"footer-adapters\""));
    }

    #[test]
    fn multi_lang_combined_view_sorts_high_risk_before_lower_risk() {
        let rs_result = {
            let v_high = crate::adapters::reporters::test_fixtures::make_verdict(
                "rs::high_fn",
                "src/h.rs",
                20,
                30.0,
                45.0,
                RiskLevel::High,
                8.0,
            );
            crate::domain::types::AnalysisResult {
                functions: vec![v_high],
                summary: crate::domain::types::AnalysisSummary {
                    total_functions: 1,
                    total_files: 1,
                    exceeding_threshold: 1,
                    distribution: crate::domain::types::RiskDistribution {
                        low: 0,
                        acceptable: 0,
                        moderate: 0,
                        high: 1,
                    },
                    ..Default::default()
                },
                passed: false,
            }
        };
        let ts_result = make_single_function_result(
            "ts::moderate_fn",
            "src/m.ts",
            10,
            60.0,
            20.0,
            RiskLevel::Moderate,
            8.0,
        );

        let blocks = vec![
            build_block(
                &rs_result,
                "crap4rs",
                "Rust",
                "rust",
                ComplexityMetric::Cognitive,
                8.0,
            ),
            build_block(
                &ts_result,
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
        ];
        let multi = crate::core::compose::compose_multi_lang(blocks);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

        // The Rust High-risk function must appear BEFORE the TS
        // Moderate-risk function in the ranked-CRAP table — the D2d
        // dimensional-consistency-aware sort rule.
        let high_pos = out
            .find("rs::high_fn")
            .expect("Rust high-risk row should render");
        let moderate_pos = out
            .find("ts::moderate_fn")
            .expect("TS moderate-risk row should render");
        assert!(
            high_pos < moderate_pos,
            "expected High-risk before Moderate-risk in ranked table (D2d sort)"
        );
    }

    #[test]
    fn multi_lang_zero_input_renders_empty_combined() {
        let multi = crate::core::compose::compose_multi_lang(Vec::new());
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());
        // Even zero-input renders a doc skeleton + the Combined-empty
        // state — the renderer never panics on edge inputs.
        assert!(out.starts_with("<!doctype html>"));
        assert!(out.contains("No functions analyzed across any language"));
        assert!(out.contains("</html>"));
    }

    /// Combined-view snapshot lock for the 2-language render.
    #[test]
    fn full_html_multi_two_language_snapshot() {
        let rs_result = make_multi_function_result();
        let ts_result = make_single_function_result(
            "parseInvoiceDraft",
            "apps/web/src/composer.ts",
            6,
            55.0,
            12.0,
            RiskLevel::Moderate,
            8.0,
        );
        let blocks = vec![
            build_block(
                &rs_result,
                "crap4rs",
                "Rust",
                "rust",
                ComplexityMetric::Cognitive,
                8.0,
            ),
            build_block(
                &ts_result,
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
        ];
        let multi = crate::core::compose::compose_multi_lang(blocks);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());
        insta::assert_snapshot!(out);
    }

    // ── View axis tests (per-language Current/Delta + Combined Delta) ──

    /// Helper: produce a Rust + TypeScript pair of baselines + current
    /// fixtures that exercises every Delta affordance:
    ///   - Rust regression (CRAP 10 → 22, Acceptable → Moderate)
    ///   - Rust improvement (CRAP 14 → 6, Moderate → Acceptable)
    ///   - Rust new function (Added, exceeds threshold → new violation)
    ///   - TypeScript regression with smaller-band crossing
    ///
    /// Returns four owned `AnalysisResult`s (rs_baseline, rs_current,
    /// ts_baseline, ts_current) so callers can build `AnalysisDelta`s
    /// in place and borrow into `LanguageBlock`s.
    #[allow(clippy::type_complexity)]
    fn two_lang_baseline_current_fixtures() -> (
        crate::domain::types::AnalysisResult,
        crate::domain::types::AnalysisResult,
        crate::domain::types::AnalysisResult,
        crate::domain::types::AnalysisResult,
    ) {
        use crate::adapters::reporters::test_fixtures::make_verdict;
        use crate::domain::types::{AnalysisResult, AnalysisSummary, CrapScore, RiskDistribution};

        // Rust baseline: regressing (10.0, Acceptable), improving (14.0, Moderate)
        let rs_baseline = AnalysisResult {
            functions: vec![
                make_verdict(
                    "rs::regressing",
                    "src/lib.rs",
                    5,
                    70.0,
                    10.0,
                    RiskLevel::Acceptable,
                    8.0,
                ),
                make_verdict(
                    "rs::improving",
                    "src/lib.rs",
                    8,
                    50.0,
                    14.0,
                    RiskLevel::Moderate,
                    8.0,
                ),
            ],
            summary: AnalysisSummary {
                total_functions: 2,
                total_files: 1,
                exceeding_threshold: 1,
                average_crap: 12.0,
                median_crap: 12.0,
                max_crap: Some(CrapScore {
                    value: 14.0,
                    risk_level: RiskLevel::Moderate,
                }),
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 1,
                    moderate: 1,
                    high: 0,
                },
                ..Default::default()
            },
            passed: false,
        };

        // Rust current: regressing → Moderate, improving → Acceptable,
        // plus a brand-new function that exceeds threshold.
        let rs_current = AnalysisResult {
            functions: vec![
                make_verdict(
                    "rs::regressing",
                    "src/lib.rs",
                    8,
                    55.0,
                    22.0,
                    RiskLevel::Moderate,
                    8.0,
                ),
                make_verdict(
                    "rs::improving",
                    "src/lib.rs",
                    4,
                    85.0,
                    6.0,
                    RiskLevel::Acceptable,
                    8.0,
                ),
                make_verdict(
                    "rs::brand_new",
                    "src/new.rs",
                    10,
                    40.0,
                    15.0,
                    RiskLevel::Moderate,
                    8.0,
                ),
            ],
            summary: AnalysisSummary {
                total_functions: 3,
                total_files: 2,
                exceeding_threshold: 2,
                average_crap: 14.33,
                median_crap: 15.0,
                max_crap: Some(CrapScore {
                    value: 22.0,
                    risk_level: RiskLevel::Moderate,
                }),
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 1,
                    moderate: 2,
                    high: 0,
                },
                ..Default::default()
            },
            passed: false,
        };

        // TypeScript baseline: one Acceptable function.
        let ts_baseline = AnalysisResult {
            functions: vec![make_verdict(
                "ts::parser",
                "apps/web/src/parser.ts",
                5,
                75.0,
                7.0,
                RiskLevel::Acceptable,
                8.0,
            )],
            summary: AnalysisSummary {
                total_functions: 1,
                total_files: 1,
                exceeding_threshold: 0,
                average_crap: 7.0,
                median_crap: 7.0,
                max_crap: Some(CrapScore {
                    value: 7.0,
                    risk_level: RiskLevel::Acceptable,
                }),
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 1,
                    moderate: 0,
                    high: 0,
                },
                ..Default::default()
            },
            passed: true,
        };

        // TypeScript current: regression on parser (7.0 → 13.0).
        let ts_current = AnalysisResult {
            functions: vec![make_verdict(
                "ts::parser",
                "apps/web/src/parser.ts",
                7,
                60.0,
                13.0,
                RiskLevel::Moderate,
                8.0,
            )],
            summary: AnalysisSummary {
                total_functions: 1,
                total_files: 1,
                exceeding_threshold: 1,
                average_crap: 13.0,
                median_crap: 13.0,
                max_crap: Some(CrapScore {
                    value: 13.0,
                    risk_level: RiskLevel::Moderate,
                }),
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 0,
                    moderate: 1,
                    high: 0,
                },
                ..Default::default()
            },
            passed: false,
        };

        (rs_baseline, rs_current, ts_baseline, ts_current)
    }

    /// View axis renders per-language Current/Delta tabs and the
    /// Combined panel exposes its own Delta tab when at least one
    /// language has a baseline.
    #[test]
    fn multi_lang_view_axis_renders_when_any_language_has_baseline() {
        let (rs_b, rs_c, ts_b, ts_c) = two_lang_baseline_current_fixtures();
        let rs_delta = crate::domain::delta::compute(rs_b, rs_c.clone());
        let ts_delta = crate::domain::delta::compute(ts_b, ts_c.clone());

        let rs_block = LanguageBlock {
            tool_name: "crap4rs".to_string(),
            display_name: "Rust".to_string(),
            language: "rust".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cognitive,
            threshold: 8.0,
            view: make_view_default(&rs_c),
            delta: Some(crate::domain::delta::apply(
                &rs_delta,
                crate::domain::delta::DeltaViewSpec::default(),
            )),
        };
        let ts_block = LanguageBlock {
            tool_name: "crap4ts".to_string(),
            display_name: "TypeScript".to_string(),
            language: "typescript".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cyclomatic,
            threshold: 8.0,
            view: make_view_default(&ts_c),
            delta: Some(crate::domain::delta::apply(
                &ts_delta,
                crate::domain::delta::DeltaViewSpec::default(),
            )),
        };
        let multi = crate::core::compose::compose_multi_lang(vec![rs_block, ts_block]);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

        // Tab nav present on the Combined panel.
        assert!(
            out.contains(r#"<nav class="tabs" role="tablist" aria-label="Combined views">"#),
            "Combined panel must carry View axis tabs when any language has a baseline"
        );
        // Per-language Delta tabs present and enabled for both languages.
        assert!(
            out.contains(r#"<nav class="tabs" role="tablist" aria-label="Rust views">"#),
            "Rust panel must carry View axis tabs"
        );
        assert!(
            out.contains(r#"<nav class="tabs" role="tablist" aria-label="TypeScript views">"#),
            "TypeScript panel must carry View axis tabs"
        );
        // No disabled Delta tab when both languages have baselines.
        assert!(
            !out.contains(r#"title="no baseline available"#),
            "no language should render the disabled Delta tooltip when both have baselines"
        );
        // Combined Delta hero references both contributing languages.
        assert!(out.contains("Comparing</strong> current run vs baseline across"));
        // Per-language Delta panels carry the per-row regression rows.
        assert!(
            out.contains("rs::regressing"),
            "Rust regression row must surface in the Rust Delta panel"
        );
    }

    /// Mismatched-baseline scenario: only Rust has a baseline. The
    /// TypeScript panel must render the Delta tab DISABLED with the
    /// no-baseline tooltip; the Combined Delta scope-banner must note
    /// TypeScript's missing baseline.
    #[test]
    fn multi_lang_mismatched_baselines_disables_typescript_delta_tab() {
        let (rs_b, rs_c, _ts_b, ts_c) = two_lang_baseline_current_fixtures();
        let rs_delta = crate::domain::delta::compute(rs_b, rs_c.clone());

        let rs_block = LanguageBlock {
            tool_name: "crap4rs".to_string(),
            display_name: "Rust".to_string(),
            language: "rust".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cognitive,
            threshold: 8.0,
            view: make_view_default(&rs_c),
            delta: Some(crate::domain::delta::apply(
                &rs_delta,
                crate::domain::delta::DeltaViewSpec::default(),
            )),
        };
        let ts_block = LanguageBlock {
            tool_name: "crap4ts".to_string(),
            display_name: "TypeScript".to_string(),
            language: "typescript".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cyclomatic,
            threshold: 8.0,
            view: make_view_default(&ts_c),
            delta: None,
        };
        let multi = crate::core::compose::compose_multi_lang(vec![rs_block, ts_block]);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

        // Rust Delta tab is enabled (no disabled marker).
        let rs_nav_start = out
            .find(r#"aria-label="Rust views""#)
            .expect("Rust tabs nav present");
        let next_close = out[rs_nav_start..].find("</nav>").unwrap();
        let rs_nav = &out[rs_nav_start..rs_nav_start + next_close];
        assert!(
            !rs_nav.contains("disabled"),
            "Rust Delta tab must be enabled when Rust has a baseline; got: {rs_nav}"
        );

        // TypeScript Delta tab IS disabled with the no-baseline title.
        let ts_nav_start = out
            .find(r#"aria-label="TypeScript views""#)
            .expect("TypeScript tabs nav present");
        let next_close = out[ts_nav_start..].find("</nav>").unwrap();
        let ts_nav = &out[ts_nav_start..ts_nav_start + next_close];
        assert!(
            ts_nav.contains("disabled"),
            "TypeScript Delta tab must be disabled when TypeScript has no baseline; got: {ts_nav}"
        );
        assert!(
            ts_nav.contains(r#"title="no baseline available for TypeScript""#),
            "Disabled Delta tab must carry the no-baseline tooltip; got: {ts_nav}"
        );

        // Combined Delta scope-banner surfaces the missing-baseline note.
        assert!(
            out.contains(r#"class="missing-baseline-note""#),
            "Combined Delta hero must render the missing-baseline note"
        );
        assert!(
            out.contains("<strong>TypeScript</strong>") && out.contains("has no baseline yet"),
            "missing-baseline-note must name TypeScript"
        );
    }

    /// No-baseline path: when neither language supplies a baseline,
    /// the View axis nav must still render in every panel (Combined +
    /// per-language) with the Delta tab in the disabled state. The
    /// affordance must stay visible so consumers downloading the
    /// rendered HTML in the dominant no-baseline case learn the
    /// feature exists rather than being silently denied a tab nav
    /// they can't see is gated.
    #[test]
    fn multi_lang_no_baselines_renders_view_nav_with_disabled_delta_in_every_panel() {
        let rs_result = make_multi_function_result();
        let ts_result = make_single_function_result(
            "parseInvoiceDraft",
            "apps/web/src/composer.ts",
            6,
            55.0,
            12.0,
            RiskLevel::Moderate,
            8.0,
        );
        let blocks = vec![
            build_block(
                &rs_result,
                "crap4rs",
                "Rust",
                "rust",
                ComplexityMetric::Cognitive,
                8.0,
            ),
            build_block(
                &ts_result,
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
        ];
        let multi = crate::core::compose::compose_multi_lang(blocks);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

        // All three View navs render even though no language supplied
        // a baseline.
        assert!(
            out.contains(r#"<nav class="tabs" role="tablist" aria-label="Combined views">"#),
            "Combined panel must carry View axis tabs even without baselines"
        );
        assert!(
            out.contains(r#"<nav class="tabs" role="tablist" aria-label="Rust views">"#),
            "Rust panel must carry View axis tabs even without a baseline"
        );
        assert!(
            out.contains(r#"<nav class="tabs" role="tablist" aria-label="TypeScript views">"#),
            "TypeScript panel must carry View axis tabs even without a baseline"
        );

        // Three navs total — one per panel.
        let nav_count = out.matches(r#"<nav class="tabs""#).count();
        assert_eq!(
            nav_count, 3,
            "expected exactly three View navs (Combined + Rust + TypeScript); got {nav_count}"
        );

        // The Combined Delta tab is disabled with the cross-adapter
        // no-baseline tooltip.
        let combined_nav_start = out
            .find(r#"aria-label="Combined views""#)
            .expect("Combined tabs nav present");
        let next_close = out[combined_nav_start..]
            .find("</nav>")
            .expect("Combined nav has closing tag");
        let combined_nav = &out[combined_nav_start..combined_nav_start + next_close];
        assert!(
            combined_nav.contains("disabled") && combined_nav.contains("aria-disabled=\"true\""),
            "Combined Delta tab must be disabled when no language supplied a baseline; got: {combined_nav}"
        );
        assert!(
            combined_nav.contains(
                r#"title="no baselines provided — pass --baseline to enable cross-adapter delta""#
            ),
            "Combined Delta disabled tooltip must name the no-baselines cause; got: {combined_nav}"
        );

        // Both per-language Delta tabs are disabled with the per-language tooltip.
        for lang_label in ["Rust", "TypeScript"] {
            let nav_start = out
                .find(&format!(r#"aria-label="{lang_label} views""#))
                .unwrap_or_else(|| panic!("{lang_label} tabs nav present"));
            let close_off = out[nav_start..]
                .find("</nav>")
                .expect("per-language nav has closing tag");
            let nav = &out[nav_start..nav_start + close_off];
            assert!(
                nav.contains("disabled") && nav.contains("aria-disabled=\"true\""),
                "{lang_label} Delta tab must be disabled when {lang_label} has no baseline; got: {nav}"
            );
            assert!(
                nav.contains(&format!(
                    r#"title="no baseline available for {lang_label}""#
                )),
                "{lang_label} Delta disabled tooltip must name the language; got: {nav}"
            );
        }

        // No Delta tab-panel renders for any language; the Current
        // panel is always the only `.tab-panel` per `.lang-panel`.
        // The JS skips clicks on disabled buttons (early return at the
        // tab handler), so the missing Delta panel never gets
        // activated. Lock the absence so a regression that emits an
        // orphan Delta panel without populating it surfaces here.
        // `<div class="tab-panel" data-tab="delta"` is the panel
        // marker; the disabled button uses `<button` so the panel
        // assertion stays orthogonal to the button assertions above.
        assert!(
            !out.contains(r#"<div class="tab-panel" data-tab="delta""#),
            "no-baseline render must not emit a Delta tab-panel for any language"
        );
    }

    /// Combined → Delta ranks regressions and new functions by risk
    /// band desc then ratio desc, mixing rows from both adapters.
    #[test]
    fn multi_lang_combined_delta_ranks_cross_adapter_by_risk_band_then_ratio() {
        use crate::adapters::reporters::test_fixtures::make_verdict;
        use crate::domain::types::{AnalysisResult, AnalysisSummary, CrapScore, RiskDistribution};

        // Rust baseline: low-risk Acceptable function.
        let rs_baseline = AnalysisResult {
            functions: vec![make_verdict(
                "rs::regressing_hard",
                "src/lib.rs",
                4,
                75.0,
                6.0,
                RiskLevel::Acceptable,
                8.0,
            )],
            summary: AnalysisSummary {
                total_functions: 1,
                total_files: 1,
                exceeding_threshold: 0,
                average_crap: 6.0,
                median_crap: 6.0,
                max_crap: Some(CrapScore {
                    value: 6.0,
                    risk_level: RiskLevel::Acceptable,
                }),
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 1,
                    moderate: 0,
                    high: 0,
                },
                ..Default::default()
            },
            passed: true,
        };
        // Rust current: a brutal regression to High risk; ratio ~5.7
        let rs_current = AnalysisResult {
            functions: vec![make_verdict(
                "rs::regressing_hard",
                "src/lib.rs",
                20,
                30.0,
                45.6,
                RiskLevel::High,
                8.0,
            )],
            summary: AnalysisSummary {
                total_functions: 1,
                total_files: 1,
                exceeding_threshold: 1,
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 0,
                    moderate: 0,
                    high: 1,
                },
                ..Default::default()
            },
            passed: false,
        };

        // TypeScript baseline: another low-risk function.
        let ts_baseline = AnalysisResult {
            functions: vec![make_verdict(
                "ts::moderate_change",
                "src/parser.ts",
                4,
                75.0,
                6.0,
                RiskLevel::Acceptable,
                8.0,
            )],
            summary: AnalysisSummary {
                total_functions: 1,
                total_files: 1,
                exceeding_threshold: 0,
                average_crap: 6.0,
                median_crap: 6.0,
                max_crap: Some(CrapScore {
                    value: 6.0,
                    risk_level: RiskLevel::Acceptable,
                }),
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 1,
                    moderate: 0,
                    high: 0,
                },
                ..Default::default()
            },
            passed: true,
        };
        // TypeScript current: regression to Moderate; ratio 2.5
        let ts_current = AnalysisResult {
            functions: vec![make_verdict(
                "ts::moderate_change",
                "src/parser.ts",
                10,
                60.0,
                20.0,
                RiskLevel::Moderate,
                8.0,
            )],
            summary: AnalysisSummary {
                total_functions: 1,
                total_files: 1,
                exceeding_threshold: 1,
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 0,
                    moderate: 1,
                    high: 0,
                },
                ..Default::default()
            },
            passed: false,
        };

        let rs_delta_full = crate::domain::delta::compute(rs_baseline, rs_current.clone());
        let ts_delta_full = crate::domain::delta::compute(ts_baseline, ts_current.clone());

        let blocks = vec![
            LanguageBlock {
                tool_name: "crap4rs".to_string(),
                display_name: "Rust".to_string(),
                language: "rust".to_string(),
                tool_version: TEST_TOOL_VERSION.to_string(),
                metric: ComplexityMetric::Cognitive,
                threshold: 8.0,
                view: make_view_default(&rs_current),
                delta: Some(crate::domain::delta::apply(
                    &rs_delta_full,
                    crate::domain::delta::DeltaViewSpec::default(),
                )),
            },
            LanguageBlock {
                tool_name: "crap4ts".to_string(),
                display_name: "TypeScript".to_string(),
                language: "typescript".to_string(),
                tool_version: TEST_TOOL_VERSION.to_string(),
                metric: ComplexityMetric::Cyclomatic,
                threshold: 8.0,
                view: make_view_default(&ts_current),
                delta: Some(crate::domain::delta::apply(
                    &ts_delta_full,
                    crate::domain::delta::DeltaViewSpec::default(),
                )),
            },
        ];
        let multi = crate::core::compose::compose_multi_lang(blocks);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

        // Locate the Combined Delta tab panel and confirm the Rust
        // High-risk row appears before the TypeScript Moderate-risk
        // row inside it.
        let combined_delta_start = out
            .find(r#"data-tab="delta" role="tabpanel""#)
            .expect("Combined Delta tab-panel must render");
        let combined_delta_section = &out[combined_delta_start..];
        let rs_pos = combined_delta_section
            .find("rs::regressing_hard")
            .expect("Rust High-risk regression must surface in Combined Delta");
        let ts_pos = combined_delta_section
            .find("ts::moderate_change")
            .expect("TypeScript Moderate-risk regression must surface in Combined Delta");
        assert!(
            rs_pos < ts_pos,
            "Rust High-risk regression must rank ahead of TypeScript Moderate-risk regression in Combined Delta (D2d sort: risk band desc, ratio desc)"
        );
    }

    /// Insta snapshot lock for the both-baselines View axis render.
    #[test]
    fn full_html_multi_two_language_with_baselines_snapshot() {
        let (rs_b, rs_c, ts_b, ts_c) = two_lang_baseline_current_fixtures();
        let rs_delta = crate::domain::delta::compute(rs_b, rs_c.clone());
        let ts_delta = crate::domain::delta::compute(ts_b, ts_c.clone());

        let rs_block = LanguageBlock {
            tool_name: "crap4rs".to_string(),
            display_name: "Rust".to_string(),
            language: "rust".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cognitive,
            threshold: 8.0,
            view: make_view_default(&rs_c),
            delta: Some(crate::domain::delta::apply(
                &rs_delta,
                crate::domain::delta::DeltaViewSpec::default(),
            )),
        };
        let ts_block = LanguageBlock {
            tool_name: "crap4ts".to_string(),
            display_name: "TypeScript".to_string(),
            language: "typescript".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cyclomatic,
            threshold: 8.0,
            view: make_view_default(&ts_c),
            delta: Some(crate::domain::delta::apply(
                &ts_delta,
                crate::domain::delta::DeltaViewSpec::default(),
            )),
        };
        let multi = crate::core::compose::compose_multi_lang(vec![rs_block, ts_block]);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());
        insta::assert_snapshot!(out);
    }

    /// Insta snapshot lock for the mismatched-baseline scenario
    /// (Rust has a baseline; TypeScript doesn't).
    #[test]
    fn full_html_multi_mismatched_baseline_snapshot() {
        let (rs_b, rs_c, _ts_b, ts_c) = two_lang_baseline_current_fixtures();
        let rs_delta = crate::domain::delta::compute(rs_b, rs_c.clone());

        let rs_block = LanguageBlock {
            tool_name: "crap4rs".to_string(),
            display_name: "Rust".to_string(),
            language: "rust".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cognitive,
            threshold: 8.0,
            view: make_view_default(&rs_c),
            delta: Some(crate::domain::delta::apply(
                &rs_delta,
                crate::domain::delta::DeltaViewSpec::default(),
            )),
        };
        let ts_block = LanguageBlock {
            tool_name: "crap4ts".to_string(),
            display_name: "TypeScript".to_string(),
            language: "typescript".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cyclomatic,
            threshold: 8.0,
            view: make_view_default(&ts_c),
            delta: None,
        };
        let multi = crate::core::compose::compose_multi_lang(vec![rs_block, ts_block]);
        let out = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());
        insta::assert_snapshot!(out);
    }

    /// Single-language passthrough must remain byte-identical to the
    /// single-binary path even when the language carries a baseline.
    /// Confirms the new View axis plumbing in the multi-lang glue
    /// doesn't leak into the n=1 short-circuit.
    #[test]
    fn multi_lang_single_language_passthrough_byte_identical_with_baseline() {
        use crate::adapters::reporters::test_fixtures::{
            make_delta_view_default, make_sample_delta,
        };

        let delta = make_sample_delta();
        let view = make_view_default(&delta.current);
        let dview = make_delta_view_default(&delta);

        let direct = format_html(
            &view,
            Some(&dview),
            8.0,
            &test_meta(),
            ComplexityMetric::Cognitive,
        );

        let block = LanguageBlock {
            tool_name: TEST_TOOL_NAME.to_string(),
            display_name: "Test".to_string(),
            language: "rust".to_string(),
            tool_version: TEST_TOOL_VERSION.to_string(),
            metric: ComplexityMetric::Cognitive,
            threshold: 8.0,
            view: make_view_default(&delta.current),
            delta: Some(make_delta_view_default(&delta)),
        };
        let multi = crate::core::compose::compose_multi_lang(vec![block]);
        let unified = format_html_multi(&multi, 8.0, HtmlMultiOptions::default());

        assert_eq!(
            direct, unified,
            "single-language passthrough must be byte-identical to format_html WITH a baseline too — the n=1 short-circuit must not gain multi-lang chrome when delta is present"
        );
        assert!(
            !unified.contains("data-multi-lang"),
            "single-language passthrough must not render the multi-lang body marker"
        );
    }
}
