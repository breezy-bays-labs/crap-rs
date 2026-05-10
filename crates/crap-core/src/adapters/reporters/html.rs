//! HTML reporter — self-contained HTML report for the AnalysisView.
//!
//! Produces a single document with inline CSS and no external assets
//! (no CDN, no fonts). Layout is mobile-responsive via CSS flex/grid
//! and per-file collapsibility uses native `<details>`/`<summary>` —
//! no JavaScript required (issue #71).
//!
//! Color-coded risk levels match the SARIF severity mapping:
//! - High → red
//! - Moderate → orange
//! - Acceptable → yellow
//! - Low → green

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::domain::types::{
    AnalysisSummary, ComplexityContributor, FunctionVerdict, RiskDistribution, RiskLevel,
};
use crate::domain::view::AnalysisView;

/// Format an `AnalysisView` as a self-contained HTML document.
///
/// The output is one full HTML document (`<!DOCTYPE html>` …
/// `</html>`). Reporters that want to embed the body in a larger
/// document should consume the structured view directly rather than
/// scraping this output.
///
/// `tool_version` is threaded from the caller (was `env!("CARGO_PKG_VERSION")`
/// before the S3 relocation; that macro now resolves to `crap-core`'s
/// version, not `crap4rs`'s, so the calling adapter passes its own
/// version explicitly — same parameter pattern `format_sarif` already
/// established).
pub fn format_html(view: &AnalysisView<'_>, threshold: f64, tool_version: &str) -> String {
    let title = format!("crap4rs v{tool_version} — CRAP Score Analysis");

    let mut body = String::new();
    body.push_str(&render_header(&title, threshold));
    body.push_str(&render_summary(&view.full.summary, view.full.passed));

    if visible_section_is_empty(view) {
        body.push_str("<p class=\"empty\">No functions to display.</p>\n");
    } else {
        body.push_str(&render_files_section(view));
    }

    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n");
    out.push_str("<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(out, "<title>{}</title>", escape_html(&title));
    out.push_str("<style>\n");
    out.push_str(STYLES);
    out.push_str("\n</style>\n</head>\n<body>\n");
    out.push_str(&body);
    out.push_str("</body>\n</html>\n");
    out
}

const STYLES: &str = r#"
:root {
  --bg: #ffffff;
  --fg: #1f2328;
  --muted: #57606a;
  --border: #d0d7de;
  --row-alt: #f6f8fa;
  --risk-low: #16a34a;
  --risk-acceptable: #ca8a04;
  --risk-moderate: #ea580c;
  --risk-high: #dc2626;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0d1117;
    --fg: #e6edf3;
    --muted: #8b949e;
    --border: #30363d;
    --row-alt: #161b22;
  }
}
* { box-sizing: border-box; }
body {
  margin: 0;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  color: var(--fg);
  background: var(--bg);
  line-height: 1.45;
  padding: 1.5rem;
  max-width: 1200px;
  margin-inline: auto;
}
header h1 { margin: 0 0 0.25rem 0; font-size: 1.5rem; }
header .threshold { color: var(--muted); font-size: 0.9rem; }
.badge {
  display: inline-block;
  padding: 0.15rem 0.55rem;
  border-radius: 999px;
  font-size: 0.8rem;
  font-weight: 600;
  color: #fff;
}
.badge-pass { background: var(--risk-low); }
.badge-fail { background: var(--risk-high); }
.summary-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 0.75rem;
  margin: 1rem 0;
}
.stat {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 0.75rem;
}
.stat .label { color: var(--muted); font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.05em; }
.stat .value { font-size: 1.4rem; font-weight: 600; margin-top: 0.25rem; }
.distribution {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin: 0.75rem 0 1.5rem 0;
}
.dist-pill {
  padding: 0.3rem 0.7rem;
  border-radius: 4px;
  color: #fff;
  font-size: 0.85rem;
  font-weight: 500;
}
.dist-low { background: var(--risk-low); }
.dist-acceptable { background: var(--risk-acceptable); }
.dist-moderate { background: var(--risk-moderate); }
.dist-high { background: var(--risk-high); }
details.file {
  border: 1px solid var(--border);
  border-radius: 6px;
  margin-bottom: 0.75rem;
  background: var(--bg);
}
details.file > summary {
  cursor: pointer;
  padding: 0.6rem 0.85rem;
  font-weight: 500;
  list-style: none;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.6rem;
  user-select: none;
}
details.file > summary::before {
  content: "▶";
  font-size: 0.7rem;
  color: var(--muted);
  transition: transform 0.15s ease;
}
details.file[open] > summary::before { transform: rotate(90deg); }
details.file > summary .file-path { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
details.file > summary .file-meta { color: var(--muted); font-size: 0.85rem; margin-left: auto; }
table.functions {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.9rem;
}
table.functions th, table.functions td {
  text-align: left;
  padding: 0.45rem 0.65rem;
  border-bottom: 1px solid var(--border);
  vertical-align: top;
}
table.functions th {
  font-weight: 600;
  color: var(--muted);
  background: var(--row-alt);
}
table.functions td.numeric { text-align: right; font-variant-numeric: tabular-nums; }
table.functions td.fn-name { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
.risk {
  display: inline-block;
  padding: 0.1rem 0.5rem;
  border-radius: 4px;
  color: #fff;
  font-size: 0.78rem;
  font-weight: 500;
}
.risk-low { background: var(--risk-low); }
.risk-acceptable { background: var(--risk-acceptable); }
.risk-moderate { background: var(--risk-moderate); }
.risk-high { background: var(--risk-high); }
.exceeds { color: var(--risk-high); font-weight: 600; }
details.contributors {
  margin-top: 0.4rem;
  padding-left: 0.6rem;
  border-left: 2px solid var(--border);
}
details.contributors > summary {
  cursor: pointer;
  font-size: 0.82rem;
  color: var(--muted);
  user-select: none;
}
details.contributors ul {
  margin: 0.4rem 0 0.2rem 0;
  padding-left: 1.2rem;
  font-size: 0.82rem;
}
.empty { color: var(--muted); font-style: italic; }
@media (max-width: 640px) {
  body { padding: 1rem; }
  table.functions th:nth-child(2),
  table.functions td:nth-child(2) { display: none; }
}
"#;

fn render_header(title: &str, threshold: f64) -> String {
    let mut out = String::new();
    out.push_str("<header>\n");
    let _ = writeln!(out, "<h1>{}</h1>", escape_html(title));
    let _ = writeln!(
        out,
        "<p class=\"threshold\">Threshold: <strong>{threshold:.2}</strong></p>"
    );
    out.push_str("</header>\n");
    out
}

fn render_summary(summary: &AnalysisSummary, passed: bool) -> String {
    let mut out = String::new();
    out.push_str("<section class=\"summary\">\n");
    let badge = if passed {
        "<span class=\"badge badge-pass\">PASS</span>"
    } else {
        "<span class=\"badge badge-fail\">FAIL</span>"
    };
    let _ = writeln!(out, "<h2>Summary {badge}</h2>");
    out.push_str("<div class=\"summary-grid\">\n");
    out.push_str(&stat("Functions", &summary.total_functions.to_string()));
    out.push_str(&stat("Files", &summary.total_files.to_string()));
    out.push_str(&stat(
        "Exceeding threshold",
        &summary.exceeding_threshold.to_string(),
    ));
    out.push_str(&stat(
        "Average CRAP",
        &format!("{:.2}", summary.average_crap),
    ));
    out.push_str(&stat("Median CRAP", &format!("{:.2}", summary.median_crap)));
    let max_crap = summary
        .max_crap
        .map(|c| format!("{:.2}", c.value))
        .unwrap_or_else(|| "—".to_string());
    out.push_str(&stat("Max CRAP", &max_crap));
    out.push_str("</div>\n");
    out.push_str(&render_distribution(&summary.distribution));
    out.push_str("</section>\n");
    out
}

fn stat(label: &str, value: &str) -> String {
    format!(
        "<div class=\"stat\"><div class=\"label\">{}</div><div class=\"value\">{}</div></div>\n",
        escape_html(label),
        escape_html(value)
    )
}

fn render_distribution(dist: &RiskDistribution) -> String {
    let mut out = String::new();
    out.push_str("<div class=\"distribution\" aria-label=\"Risk distribution\">\n");
    for (level, count, class) in [
        (RiskLevel::Low, dist.low, "dist-low"),
        (RiskLevel::Acceptable, dist.acceptable, "dist-acceptable"),
        (RiskLevel::Moderate, dist.moderate, "dist-moderate"),
        (RiskLevel::High, dist.high, "dist-high"),
    ] {
        let _ = writeln!(
            out,
            "<span class=\"dist-pill {class}\">{level}: {count}</span>"
        );
    }
    out.push_str("</div>\n");
    out
}

fn render_files_section(view: &AnalysisView<'_>) -> String {
    let mut out = String::new();
    out.push_str("<section class=\"files\">\n<h2>Functions by file</h2>\n");
    // When `--group-by file` is active, `view.grouped.files` carries the
    // file-level sort/Top-N selection; respect that order and restrict
    // the rendered file list to it. Otherwise fall back to the natural
    // file partition over `view.shown`.
    if let Some(grouped) = view.grouped.as_ref() {
        let fns_by_file = group_by_file(&view.shown);
        for file_summary in &grouped.files {
            let fns: Vec<&FunctionVerdict> = fns_by_file
                .get(file_summary.file_path.as_str())
                .cloned()
                .unwrap_or_default();
            out.push_str(&render_file(&file_summary.file_path, &fns));
        }
    } else {
        for (file, fns) in &group_by_file(&view.shown) {
            out.push_str(&render_file(file, fns));
        }
    }
    out.push_str("</section>\n");
    out
}

/// True when there are no rows to render in the file section. Honors
/// the shaped view: with grouping active, considers the post-truncate
/// file list; otherwise considers the post-filter `shown` rows.
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

fn render_file(file: &str, fns: &[&FunctionVerdict]) -> String {
    let exceeds_count = fns.iter().filter(|f| f.exceeds).count();
    let max_crap = fns
        .iter()
        .map(|f| f.scored.crap.value)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_crap_str = if max_crap.is_finite() {
        format!("{max_crap:.2}")
    } else {
        "—".to_string()
    };
    let open_attr = if exceeds_count > 0 { " open" } else { "" };
    let mut out = String::new();
    let _ = write!(
        out,
        "<details class=\"file\"{open_attr}>\n<summary>\
         <span class=\"file-path\">{}</span>\
         <span class=\"file-meta\">{} fn · max CRAP {} · {} over</span>\
         </summary>\n",
        escape_html(file),
        fns.len(),
        max_crap_str,
        exceeds_count
    );
    out.push_str("<table class=\"functions\">\n");
    out.push_str(
        "<thead><tr>\
        <th>Function</th><th>Lines</th><th class=\"numeric\">CC</th>\
        <th class=\"numeric\">Cov %</th><th class=\"numeric\">CRAP</th>\
        <th>Risk</th></tr></thead>\n",
    );
    out.push_str("<tbody>\n");
    for v in fns {
        out.push_str(&render_function_row(v));
    }
    out.push_str("</tbody>\n</table>\n</details>\n");
    out
}

fn render_function_row(v: &FunctionVerdict) -> String {
    let span = &v.scored.identity.span;
    let crap_class = if v.exceeds { " exceeds" } else { "" };
    let crap_cell = format!(
        "<td class=\"numeric{crap_class}\">{:.2}</td>",
        v.scored.crap.value
    );
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<tr>\
         <td class=\"fn-name\">{name}</td>\
         <td>{start}–{end}</td>\
         <td class=\"numeric\">{cc}</td>\
         <td class=\"numeric\">{cov:.1}</td>\
         {crap}\
         <td><span class=\"risk risk-{risk_class}\">{risk}</span></td>\
         </tr>",
        name = escape_html(&v.scored.identity.qualified_name),
        start = span.start_line,
        end = span.end_line,
        cc = v.scored.complexity,
        cov = v.scored.coverage_percent,
        crap = crap_cell,
        risk_class = risk_class_name(v.scored.crap.risk_level),
        risk = v.scored.crap.risk_level,
    );
    if !v.scored.contributors.is_empty() {
        out.push_str("<tr><td colspan=\"6\">\n");
        out.push_str(&render_contributors(&v.scored.contributors));
        out.push_str("</td></tr>\n");
    }
    out
}

fn render_contributors(contributors: &[ComplexityContributor]) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "<details class=\"contributors\">\n<summary>{} contributors</summary>\n<ul>\n",
        contributors.len()
    );
    for c in contributors {
        let _ = writeln!(
            out,
            "<li>line {line}: <code>{kind}</code> (+{inc}, depth {depth})</li>",
            line = c.line,
            kind = escape_html(&c.kind.to_string()),
            inc = c.increment,
            depth = c.nesting_depth,
        );
    }
    out.push_str("</ul>\n</details>\n");
    out
}

fn risk_class_name(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Low => "low",
        RiskLevel::Acceptable => "acceptable",
        RiskLevel::Moderate => "moderate",
        RiskLevel::High => "high",
    }
}

/// Minimal HTML escape for text nodes and attribute values.
fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::{
        make_empty_result, make_multi_function_result, make_single_function_result,
        make_view_default,
    };
    use crate::domain::types::RiskLevel;

    #[test]
    fn empty_renders_doctype_and_empty_marker() {
        let result = make_empty_result();
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("No functions to display"));
        assert!(html.ends_with("</html>\n"));
    }

    #[test]
    fn self_contained_no_external_assets() {
        let result = make_multi_function_result();
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        // No <script>, no <link>, no external font/CDN URLs.
        assert!(!html.contains("<script"), "html should ship no JS for v1");
        assert!(!html.contains("<link"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(!html.contains("@import"));
    }

    #[test]
    fn passes_when_no_violations() {
        let result =
            make_single_function_result("ok", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(html.contains("badge badge-pass\">PASS"));
        assert!(!html.contains("badge badge-fail\">FAIL"));
    }

    #[test]
    fn fails_when_threshold_exceeded() {
        let result =
            make_single_function_result("bad", "src/lib.rs", 20, 10.0, 45.0, RiskLevel::High, 8.0);
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(html.contains("badge badge-fail\">FAIL"));
        assert!(html.contains("risk risk-high"));
        // Exceeding functions show the file pre-expanded.
        assert!(html.contains("<details class=\"file\" open>"));
    }

    #[test]
    fn risk_levels_render_distinct_classes() {
        let result = make_multi_function_result();
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(html.contains("risk-low"));
        assert!(html.contains("risk-moderate"));
        assert!(html.contains("risk-high"));
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
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
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
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(!html.contains("<dangerous>"));
        assert!(html.contains("&lt;dangerous&gt;"));
    }

    #[test]
    fn groups_functions_by_file() {
        let result = make_multi_function_result();
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        // Three distinct files in the fixture.
        assert_eq!(html.matches("<details class=\"file\"").count(), 3);
    }

    #[test]
    fn risk_distribution_shows_all_buckets() {
        let result = make_multi_function_result();
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(html.contains("dist-low"));
        assert!(html.contains("dist-acceptable"));
        assert!(html.contains("dist-moderate"));
        assert!(html.contains("dist-high"));
    }

    #[test]
    fn doctype_present_and_lang_set() {
        let result = make_empty_result();
        let view = make_view_default(&result);
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("viewport"));
    }

    #[test]
    fn empty_after_filter_renders_empty_marker() {
        // The fixture has functions in `full`, but a filter that strips
        // every function from `shown` should produce the empty section
        // — not a stale "Functions by file" header with no contents.
        use crate::domain::view::{self, CoverageRange, Filters, ViewSpec};
        let result = make_multi_function_result();
        // Fixture coverage maxes out at 95% — a 99–100% band drops all.
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(CoverageRange::new(99.0, 100.0).unwrap()),
                ..Filters::default()
            },
            ..ViewSpec::default()
        };
        let view = view::apply(&result, spec);
        assert!(
            view.shown.is_empty(),
            "fixture pre-condition: shown should be empty under this filter"
        );
        let html = format_html(&view, 8.0, "0.4.0");
        assert!(html.contains("No functions to display"));
        assert!(!html.contains("Functions by file"));
    }

    #[test]
    fn grouped_view_honors_file_top_n_and_order() {
        // Under `--group-by file --top 1`, only the worst file should
        // appear in the rendered output. The fixture's worst file
        // (highest CRAP) is `src/domain/crap.rs` (complex_fn @ 45.2);
        // the other two should be omitted.
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
        let html = format_html(&view, 8.0, "0.4.0");
        assert_eq!(
            html.matches("<details class=\"file\"").count(),
            1,
            "only the top-1 file should be rendered when grouped"
        );
        assert!(html.contains("src/domain/crap.rs"));
        assert!(!html.contains("src/lib.rs"));
        assert!(!html.contains("src/adapters/coverage/mod.rs"));
    }
}
