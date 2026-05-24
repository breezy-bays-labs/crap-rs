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
    let summary = &view.full.summary;
    let title = format!(
        "{} v{} — CRAP score analysis",
        meta.tool_name, meta.tool_version
    );

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
        tool_name: meta.tool_name,
        tool_version: meta.tool_version,
        adapter_display: meta.display_name,
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
    let mut unchanged_count: u32 = 0;

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
                } else {
                    unchanged_count += 1;
                }
            }
        }
    }

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
            config_file_name: "test-adapter.toml",
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
}
