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
pub fn format_html(
    view: &AnalysisView<'_>,
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
        format_html(view, 8.0, &test_meta(), ComplexityMetric::Cognitive)
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
}
