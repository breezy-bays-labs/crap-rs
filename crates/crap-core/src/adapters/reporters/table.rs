//! Terminal table reporter — formats an `AnalysisView` as a colored
//! terminal table.
//!
//! The reporter trusts the view's pre-shaped row order (`view.shown`)
//! and derives its summary line from `view.full.summary` (the
//! unshapeable gate). It does not sort, filter, or truncate.

use crate::domain::delta::{DeltaView, FunctionChange};
use crate::domain::types::RiskLevel;
use crate::domain::view::AnalysisView;
use colored::Colorize;
use comfy_table::{ContentArrangement, Table};

/// Format an `AnalysisView` as a colored terminal table.
///
/// Risk-level and coverage coloring; when `threshold` differs across
/// functions (per-path overrides), the summary shows
/// "varied (default: N)" instead of a single number. Returns a
/// ready-to-print `String`.
pub fn format_table(
    view: &AnalysisView<'_>,
    threshold: f64,
    breakdown: bool,
    tool_name: &str,
    tool_version: &str,
) -> String {
    format_table_with_explain(
        view,
        None,
        threshold,
        breakdown,
        false,
        tool_name,
        tool_version,
        None,
        None,
    )
}

/// Format an `AnalysisView` as a colored terminal table with optional
/// breakdown explanation text.
///
/// When `delta` is `Some`, a "Delta vs baseline" block is appended
/// below the analysis table — header line + per-change rows + a
/// summary line. When `delta` is `None`, output is byte-identical to
/// the pre-delta version.
///
/// `tool_name` (the adapter binary's `env!("CARGO_PKG_NAME")`) and `tool_version`
/// are threaded from the caller — `env!("CARGO_PKG_NAME")` and
/// `env!("CARGO_PKG_VERSION")` resolve against `crap-core` here, not
/// the adapter binary, so the calling binary supplies its own identity.
///
/// `title` and `subtitle` are the optional `[output] title` / `subtitle`
/// config labels (crap-rs#352). When `title` is `Some`, it is emitted as
/// a header line above the tool/version line; when `subtitle` is `Some`,
/// it renders on the line beneath the title. Both default to `None`, in
/// which case the header is byte-identical to the unlabeled default — no
/// empty title/subtitle line is emitted.
#[allow(clippy::too_many_arguments)]
pub fn format_table_with_explain(
    view: &AnalysisView<'_>,
    delta: Option<&DeltaView<'_>>,
    threshold: f64,
    breakdown: bool,
    explain: bool,
    tool_name: &str,
    tool_version: &str,
    title: Option<&str>,
    subtitle: Option<&str>,
) -> String {
    let mut output = String::new();

    // Header — the configured title labels the scorecard above the
    // tool/version line; the subtitle renders beneath the title. Each is
    // gated on `Some` independently so the absent path emits no empty
    // line and stays byte-identical to the unlabeled default.
    if let Some(title) = title {
        output.push_str(title);
        output.push('\n');
    }
    if let Some(subtitle) = subtitle {
        output.push_str(subtitle);
        output.push('\n');
    }
    output.push_str(&format!(
        "{tool_name} v{tool_version} — CRAP Score Analysis\n",
    ));

    // Empty guard — derived from the underlying analysis (the gate),
    // not from the shaped view, so an empty `view.shown` resulting from
    // aggressive filtering does not collapse the output the same way
    // an actually-empty analysis does.
    if view.full.functions.is_empty() {
        output.push_str("\nNo functions analyzed\n");
        if let Some(delta_view) = delta {
            append_delta_block(&mut output, delta_view);
        }
        return output;
    }

    // --group-by file: render per-file rows instead of per-function.
    // Summary line still derives from `view.full.summary` (gate keystone).
    if view.grouped.is_some() {
        output.push('\n');
        output.push_str(&format_grouped_table(view));
        output.push('\n');
        append_summary_block(&mut output, view, threshold);
        if let Some(delta_view) = delta {
            append_delta_block(&mut output, delta_view);
        }
        return output;
    }

    let shown: &[&crate::domain::types::FunctionVerdict] = &view.shown;

    // Conditional `Branch%` column (crap-rs#251). The column appears
    // when at least one row in the shaped view carries
    // `branch_coverage_percent: Some(_)` — i.e. the coverage parser
    // produced branch records that fell inside at least one function's
    // span. When no row has it (LCOV runs, jest without branch
    // instrumentation, or branches that all fell outside function
    // spans), the column is omitted entirely so the default Rust /
    // line-only scorecard stays byte-identical to its pre-#251 width.
    let has_branch_coverage = shown
        .iter()
        .any(|v| v.scored.branch_coverage_percent.is_some());

    let table = build_function_table(shown, has_branch_coverage);

    output.push('\n');

    if breakdown {
        output.push_str(&inject_breakdown_subrows(&table.to_string(), shown));
    } else {
        output.push_str(&table.to_string());
    }

    if breakdown
        && explain
        && shown
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

    append_summary_block(&mut output, view, threshold);

    if let Some(delta_view) = delta {
        append_delta_block(&mut output, delta_view);
    }

    output
}

/// Build the per-function `comfy_table::Table` body. Header and row
/// shape are parameterised by `has_branch_coverage` — when true the
/// `Branch%` column is woven in between `Cov%` and `CRAP`; when false
/// the table renders the pre-#251 line-only shape byte-identically.
///
/// Extracted from `format_table_with_explain` so that body stays
/// focused on flow control (grouped-vs-default, breakdown subrows,
/// summary block, delta block); CRAP fingerprint sits well below the
/// strict-mode threshold this way (focus #251 cross-impact note).
fn build_function_table(
    shown: &[&crate::domain::types::FunctionVerdict],
    has_branch_coverage: bool,
) -> Table {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);

    let mut header: Vec<&str> = vec!["File", "Function", "CC", "Cov%"];
    if has_branch_coverage {
        header.push("Branch%");
    }
    header.extend(["CRAP", "Risk"]);
    table.set_header(header);

    for verdict in shown {
        table.add_row(build_function_row(verdict, has_branch_coverage));
    }
    table
}

/// Render a single function verdict as the cell vector its row needs.
/// Pushes the optional `Branch%` cell in the same conditional shape as
/// the header — keeping the two in lockstep means a missing-column /
/// shifted-row bug surface can't open up at the row builder alone.
fn build_function_row(
    verdict: &crate::domain::types::FunctionVerdict,
    has_branch_coverage: bool,
) -> Vec<String> {
    let s = &verdict.scored;
    let cov_str = format!("{:.1}", s.coverage_percent);
    let crap_str = format!("{:.2}", s.crap.value);

    let mut row: Vec<String> = vec![
        s.identity.file_path.clone(),
        s.identity.qualified_name.clone(),
        s.complexity.to_string(),
        coverage_color(s.coverage_percent, &cov_str),
    ];
    if has_branch_coverage {
        row.push(format_branch_cell(s.branch_coverage_percent));
    }
    row.push(crap_color(verdict.exceeds, &crap_str));
    row.push(risk_color(
        &s.crap.risk_level,
        &s.crap.risk_level.to_string(),
    ));
    row
}

/// Format the `Branch%` cell for one row. `Some(p)` ⇒ one-decimal
/// percentage coloured by the same coverage-band gradient line
/// coverage uses (a low branch number reads as red just as a low line
/// number does). `None` ⇒ the `—` (em-dash) sentinel, deliberately
/// distinct from `0.0` so absent data and zero-covered data don't
/// alias visually. Mirrors the grouped table's `worst_fn` "—" fallback.
fn format_branch_cell(branch_coverage_percent: Option<f64>) -> String {
    match branch_coverage_percent {
        Some(p) => coverage_color(p, &format!("{:.1}", p)),
        None => "—".to_string(),
    }
}

/// Append the delta block. Reads `view.full.summary` (the unshapeable
/// delta-gate counts) for the header line and iterates `view.shown`
/// for the per-change rows. The table mirrors the analysis table
/// columns where applicable but adds Kind / Δ for delta semantics.
fn append_delta_block(output: &mut String, view: &DeltaView<'_>) {
    let summary = view.full.summary;
    output.push_str("\nDelta vs baseline:\n");
    output.push_str(&format!(
        "  +{added} added, {removed} removed, {modified} modified · {regressions} regressions, {improvements} improvements, {new_violations} new violations\n",
        added = summary.added,
        removed = summary.removed,
        modified = summary.modified,
        regressions = summary.regressions,
        improvements = summary.improvements,
        new_violations = summary.new_violations,
    ));

    if view.shown.is_empty() {
        output.push_str("  (no changes to display)\n");
        return;
    }

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["Kind", "File", "Function", "Baseline", "Current", "Δ"]);

    for change in view.shown.iter() {
        let kind_str = change.kind().as_str();
        let baseline_str = change
            .baseline_score()
            .map(|s| format!("{s:.2}"))
            .unwrap_or_else(|| "—".to_string());
        let current_str = change
            .current_score()
            .map(|s| format!("{s:.2}"))
            .unwrap_or_else(|| "—".to_string());
        let delta_str = change
            .score_delta()
            .map(|d| {
                let prefix = if d > 0.0 { "+" } else { "" };
                format!("{prefix}{d:.2}")
            })
            .unwrap_or_else(|| "—".to_string());

        let colored_delta = match change {
            FunctionChange::Modified { .. } => delta_color(change.score_delta(), &delta_str),
            _ => delta_str,
        };

        table.add_row(vec![
            kind_str.to_string(),
            change.file_path().to_string(),
            change.qualified_name().to_string(),
            baseline_str,
            current_str,
            colored_delta,
        ]);
    }

    output.push('\n');
    output.push_str(&table.to_string());
    output.push('\n');

    if view.truncated {
        output.push_str(&format!(
            "  (showing {} of {} changes — use --delta-top 0 to disable)\n",
            view.shown.len(),
            view.eligible_count
        ));
    }
}

fn delta_color(score_delta: Option<f64>, s: &str) -> String {
    match score_delta {
        Some(d) if d > 0.0 => s.red().bold().to_string(),
        Some(d) if d < 0.0 => s.green().bold().to_string(),
        _ => s.to_string(),
    }
}

/// Append the two-line summary block. Always derives from
/// `view.full.summary` (gate keystone — unshapeable).
fn append_summary_block(output: &mut String, view: &AnalysisView<'_>, threshold: f64) {
    let summary = &view.full.summary;
    let pass_fail = if view.full.passed {
        "PASS".green().bold().to_string()
    } else {
        "FAIL".red().bold().to_string()
    };
    let worst = summary
        .max_crap
        .map(|c| format!("{:.1}", c.value))
        .unwrap_or_else(|| "N/A".to_string());

    let threshold_display = if has_varied_thresholds(&view.full.functions) {
        format!("varied (default: {})", threshold)
    } else {
        format!("{}", threshold)
    };

    output.push_str(&format!(
        "\nSummary: {} functions | {} above threshold ({}) | worst: {} | {}\n",
        summary.total_functions, summary.exceeding_threshold, threshold_display, worst, pass_fail,
    ));
    let d = &summary.distribution;
    output.push_str(&format!(
        "         avg: {:.1} | median: {:.1} | low: {} | acceptable: {} | moderate: {} | high: {}\n",
        summary.average_crap,
        summary.median_crap,
        d.low,
        d.acceptable,
        d.moderate,
        d.high,
    ));
}

/// Build the per-file grouped table body. Caller appends the summary
/// block. Coloring matches the per-function table for visual continuity:
/// `Worst CRAP` is colored by risk_level via `risk_color`.
fn format_grouped_table(view: &AnalysisView<'_>) -> String {
    let grouped = view
        .grouped
        .as_ref()
        .expect("format_grouped_table called without grouped block");

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "File",
        "Functions",
        "Failing",
        "Avg CRAP",
        "Worst CRAP",
        "Worst Fn",
    ]);

    for f in &grouped.files {
        let avg_str = format!("{:.2}", f.average_crap);
        let worst_str = f
            .max_crap
            .as_ref()
            .map(|c| {
                let s = format!("{:.2}", c.value);
                risk_color(&c.risk_level, &s)
            })
            .unwrap_or_else(|| "N/A".to_string());
        let worst_fn = f
            .worst_function
            .as_ref()
            .map(|id| id.qualified_name.clone())
            .unwrap_or_else(|| "—".to_string());

        table.add_row(vec![
            f.file_path.clone(),
            f.function_count.to_string(),
            f.exceeding_count.to_string(),
            avg_str,
            worst_str,
            worst_fn,
        ]);
    }

    table.to_string()
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
///
/// Acquired via `acquire_color_lock()`, which recovers from
/// `PoisonError` by extracting the inner guard rather than panicking.
/// Without recovery, a single test panicking inside the locked region
/// (historically: comfy-table width detection under non-PTY
/// `cargo test`) cascades into 43+ unrelated failures as every
/// subsequent `lock().unwrap()` re-panics on the poisoned mutex. The
/// lock guards a flag flip, not invariant state — a poison from the
/// previous holder doesn't leave behind inconsistent data, so
/// recovering is safe and breaks the cascade. (crap-rs#202.)
#[cfg(test)]
static COLOR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
fn acquire_color_lock() -> std::sync::MutexGuard<'static, ()> {
    COLOR_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;
    use crate::domain::types::{AnalysisResult, AnalysisSummary, RiskLevel};

    // ── Snapshot tests (color disabled) ────────────────────────────────

    #[test]
    fn test_empty_shows_no_functions() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_empty_result();
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.contains(&format!("{TEST_TOOL_NAME} v")));
        assert!(output.contains("No functions analyzed"));
        // No table header should be present
        assert!(!output.contains("File"));
    }

    #[test]
    fn test_sorted_by_crap_descending() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
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
        let _guard = acquire_color_lock();
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
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.contains("File"));
        assert!(output.contains("Function"));
        assert!(output.contains("CC"));
        assert!(output.contains("Cov%"));
        assert!(output.contains("CRAP"));
        assert!(output.contains("Risk"));
    }

    #[test]
    fn test_function_details_in_columns() {
        let _guard = acquire_color_lock();
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
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.contains("src/adapters/coverage/mod.rs"));
        assert!(output.contains("parse_record"));
        assert!(output.contains("6"));
        assert!(output.contains("72.5"));
        assert!(output.contains("8.13"));
    }

    #[test]
    fn test_crap_two_decimal_places() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 5.0, RiskLevel::Low, 8.0);
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.contains("5.00"));
    }

    #[test]
    fn test_coverage_one_decimal_place() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 85.0, 1.0, RiskLevel::Low, 8.0);
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.contains("85.0"));
    }

    // ── Branch% column (crap-rs#251) ──────────────────────────────────

    /// The Rust / line-only path stays byte-identical: when no verdict
    /// carries `branch_coverage_percent`, the `Branch%` column is
    /// absent from the header entirely.
    #[test]
    fn branch_column_omitted_when_no_branch_data() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_single_function_result(
            "no_branches",
            "src/lib.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(
            !output.contains("Branch%"),
            "expected no `Branch%` column when no verdict has branch data; got:\n{output}",
        );
    }

    /// When at least one verdict has `branch_coverage_percent = Some(_)`
    /// the `Branch%` column appears and renders the value to one
    /// decimal — the same precision as the line `Cov%` column.
    #[test]
    fn branch_column_present_when_branch_data_some() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let mut result = make_single_function_result(
            "classify",
            "src/lib.rs",
            3,
            100.0,
            3.0,
            RiskLevel::Low,
            8.0,
        );
        // Mutate the single verdict to carry branch data — mirrors what
        // a real branch-rich Istanbul fixture would produce after
        // `score_and_summarize` lifts `cov.branch_coverage.percent`
        // onto `ScoredFunction.branch_coverage_percent`.
        result.functions[0].scored.branch_coverage_percent = Some(66.7);

        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(
            output.contains("Branch%"),
            "expected `Branch%` column when a verdict has branch data; got:\n{output}",
        );
        assert!(
            output.contains("66.7"),
            "expected branch percentage `66.7` in the row; got:\n{output}",
        );
    }

    /// Mixed view (some functions have branch data, others don't): the
    /// column appears once at the header, and rows without branch data
    /// show the `—` sentinel — visibly distinct from a genuine `0.0`.
    #[test]
    fn branch_column_dash_for_rows_without_branch_data() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);

        // Two verdicts: one carries branch coverage, one doesn't.
        let v_with = {
            let mut v = make_verdict(
                "with_branches",
                "src/with.rs",
                3,
                100.0,
                3.0,
                RiskLevel::Low,
                8.0,
            );
            v.scored.branch_coverage_percent = Some(100.0);
            v
        };
        let v_without = make_verdict(
            "without_branches",
            "src/without.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let result = AnalysisResult {
            functions: vec![v_with, v_without],
            summary: AnalysisSummary {
                total_functions: 2,
                total_files: 2,
                exceeding_threshold: 0,
                average_crap: 2.0,
                median_crap: 2.0,
                ..Default::default()
            },
            passed: true,
        };

        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(
            output.contains("Branch%"),
            "expected `Branch%` column when any verdict has branch data; got:\n{output}",
        );
        // The `—` (em-dash) sentinel marks rows with no branch data so
        // they're visibly distinct from a genuine zero — see the row
        // builder in `format_table_with_explain`.
        assert!(
            output.contains("—"),
            "expected `—` sentinel in the branch column for the verdict without branch data; got:\n{output}",
        );
    }

    #[test]
    fn test_version_header() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_empty_result();
        // `tool_name` and `tool_version` are caller-supplied. In-crate
        // tests pass the synthetic placeholders `TEST_TOOL_NAME` /
        // `TEST_TOOL_VERSION` so crap-core source is decoupled from
        // any adapter's name.
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.starts_with(&format!("{TEST_TOOL_NAME} v{TEST_TOOL_VERSION}")));
    }

    // ── [output] title / subtitle labeling (crap-rs#352) ────────────────

    #[test]
    fn title_labels_the_scorecard_header() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table_with_explain(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            Some("Acme Coverage Report"),
            None,
        );
        // The configured title labels the header — emitted on the first
        // line, above the tool/version line.
        assert!(
            output.starts_with("Acme Coverage Report\n"),
            "expected the title on the first header line; got:\n{output}",
        );
        assert!(output.contains(&format!("{TEST_TOOL_NAME} v{TEST_TOOL_VERSION}")));
    }

    #[test]
    fn subtitle_renders_beneath_the_title() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table_with_explain(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            Some("Acme Coverage Report"),
            Some("nightly build"),
        );
        assert!(
            output.starts_with("Acme Coverage Report\nnightly build\n"),
            "expected the subtitle beneath the title; got:\n{output}",
        );
    }

    #[test]
    fn absent_title_and_subtitle_emit_no_extra_lines() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        // Absent title/subtitle must be byte-identical to the unlabeled
        // default — `format_table` is the `None, None` wrapper, so both
        // paths must produce the exact same bytes (no empty header line).
        let labeled = format_table_with_explain(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            None,
        );
        let default = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert_eq!(
            labeled, default,
            "absent title/subtitle must be byte-identical to the unlabeled default",
        );
        assert!(
            default.starts_with(&format!("{TEST_TOOL_NAME} v{TEST_TOOL_VERSION}")),
            "default header must lead with the tool/version line, no empty line above",
        );
    }

    #[test]
    fn subtitle_without_title_renders_above_tool_line() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table_with_explain(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            Some("nightly build"),
        );
        // A subtitle set without a title still renders (gated
        // independently), above the tool/version line, with no empty
        // title line preceding it.
        assert!(
            output.starts_with("nightly build\n"),
            "expected the subtitle on the first line when no title is set; got:\n{output}",
        );
    }

    #[test]
    fn test_summary_line_contents() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.contains("3 functions"));
        assert!(output.contains("2 above threshold (8)"));
        assert!(output.contains("worst: 45.2"));
        assert!(output.contains("FAIL"));
    }

    #[test]
    fn test_summary_pass_variant() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.contains("PASS"));
        assert!(!output.contains("FAIL"));
    }

    #[test]
    fn test_summary_distribution() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
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
        ]
    }

    fn make_nested_contributor() -> Vec<crate::domain::types::ComplexityContributor> {
        use crate::domain::types::{ComplexityContributor, ContributorKind};
        vec![ComplexityContributor {
            kind: ContributorKind::Match,
            line: 3,
            column: Some(4),
            increment: 3,
            end_line: 3,
            nesting_depth: 2,
        }]
    }

    #[test]
    fn test_breakdown_off_no_sub_rows() {
        let _guard = acquire_color_lock();
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
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
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
        let _guard = acquire_color_lock();
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
        let output = format_table(
            &make_view_default(&result),
            8.0,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
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
        let _guard = acquire_color_lock();
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
        let output = format_table(
            &make_view_default(&result),
            8.0,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(
            !output.contains("├─"),
            "Non-exceeding fn should not show sub-rows even with breakdown=true: {output}"
        );
    }

    #[test]
    fn test_breakdown_tree_chars_last_uses_corner() {
        let _guard = acquire_color_lock();
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
        let output = format_table(
            &make_view_default(&result),
            8.0,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(output.contains("├─"), "Should have branch char: {output}");
        assert!(output.contains("└─"), "Should have corner char: {output}");
        // Make sure corner appears after branch
        let branch_pos = output.find("├─").unwrap();
        let corner_pos = output.find("└─").unwrap();
        assert!(branch_pos < corner_pos, "├─ should appear before └─");
    }

    #[test]
    fn test_breakdown_nesting_suffix() {
        let _guard = acquire_color_lock();
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
        let output = format_table(
            &make_view_default(&result),
            8.0,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(
            output.contains("(nested)"),
            "increment > 1 should show '(nested)': {output}"
        );
        assert!(output.contains("+3"), "Should show +3 increment: {output}");
    }

    #[test]
    fn test_explain_adds_legend_for_nested_breakdown() {
        let _guard = acquire_color_lock();
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
        let output = format_table_with_explain(
            &make_view_default(&result),
            None,
            8.0,
            true,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            None,
        );
        assert!(output.contains("Legend: +1 = base structural increment."));
        assert!(output.contains("+N (nested) = +1 base plus +(N-1)"));
        assert!(output.contains("if/else branches, match arms"));
        assert!(output.contains("while/for/loop bodies"));
        assert!(output.contains("let-else diverging branches, and closures"));
    }

    #[test]
    fn test_explain_without_breakdown_is_inert() {
        let _guard = acquire_color_lock();
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
        let output = format_table_with_explain(
            &make_view_default(&result),
            None,
            8.0,
            false,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            None,
        );
        assert!(!output.contains("Legend:"));
        assert!(!output.contains("line 3: match (+3 (nested))"));
    }

    #[test]
    fn test_explain_suppressed_without_nested_contributors() {
        let _guard = acquire_color_lock();
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
                end_line: 3,
                nesting_depth: 0,
            }],
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table_with_explain(
            &make_view_default(&result),
            None,
            8.0,
            true,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            None,
        );
        assert!(!output.contains("Legend:"));
    }

    #[test]
    fn test_breakdown_sorted_by_line() {
        use crate::domain::types::{ComplexityContributor, ContributorKind};
        let _guard = acquire_color_lock();
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
                    end_line: 5,
                    nesting_depth: 0,
                },
                ComplexityContributor {
                    kind: ContributorKind::ForLoop,
                    line: 20,
                    column: Some(4),
                    increment: 1,
                    end_line: 20,
                    nesting_depth: 0,
                },
            ],
        );
        let result = AnalysisResult {
            functions: vec![verdict],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(
            &make_view_default(&result),
            8.0,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
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
        let _guard = acquire_color_lock();
        colored::control::set_override(false);

        let v1 = make_verdict_with_contributors(
            make_verdict("parse", "src/lib.rs", 5, 30.0, 45.0, RiskLevel::High, 8.0),
            vec![ComplexityContributor {
                kind: ContributorKind::IfBranch,
                line: 3,
                column: Some(4),
                increment: 1,
                end_line: 3,
                nesting_depth: 0,
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
                end_line: 7,
                nesting_depth: 0,
            }],
        );
        let result = AnalysisResult {
            functions: vec![v1, v2],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(
            &make_view_default(&result),
            8.0,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );

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
        let _guard = acquire_color_lock();
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
                end_line: 3,
                nesting_depth: 0,
            }],
        );
        let result = AnalysisResult {
            functions: vec![v],
            summary: make_multi_function_result().summary,
            passed: false,
        };
        let output = format_table(
            &make_view_default(&result),
            8.0,
            true,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(
            output.contains("if-branch"),
            "Sub-row should appear even when function name is in file path: {output}"
        );
    }

    // ── Varied threshold tests ──────────────────────────────────────────

    #[test]
    fn test_varied_thresholds_detected() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);

        let mut result = make_multi_function_result();
        // Set different thresholds on different functions
        result.functions[0].threshold = 5.0;
        result.functions[1].threshold = 10.0;
        result.functions[2].threshold = 8.0;

        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(
            output.contains("varied (default: 8)"),
            "Should show varied threshold: {output}"
        );
    }

    #[test]
    fn test_uniform_thresholds_not_varied() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);

        let result = make_multi_function_result();
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(
            !output.contains("varied"),
            "Uniform thresholds should not show 'varied': {output}"
        );
    }

    #[test]
    fn test_single_function_not_varied() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);

        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        assert!(!output.contains("varied"));
    }

    // ── Color helper tests (force color on for ANSI assertions) ───────

    #[test]
    fn test_risk_color_low_green() {
        let _guard = acquire_color_lock();
        colored::control::set_override(true);
        let out = risk_color(&RiskLevel::Low, "low");
        assert!(out.contains("\x1b[32m"), "Expected green ANSI: {out:?}");
    }

    #[test]
    fn test_risk_color_acceptable_no_color() {
        let _guard = acquire_color_lock();
        colored::control::set_override(true);
        let out = risk_color(&RiskLevel::Acceptable, "acceptable");
        assert!(!out.contains("\x1b["), "Expected no ANSI escapes: {out:?}");
        assert_eq!(out, "acceptable");
    }

    #[test]
    fn test_risk_color_moderate_yellow() {
        let _guard = acquire_color_lock();
        colored::control::set_override(true);
        let out = risk_color(&RiskLevel::Moderate, "moderate");
        assert!(out.contains("\x1b[33m"), "Expected yellow ANSI: {out:?}");
    }

    #[test]
    fn test_risk_color_high_bold_red() {
        let _guard = acquire_color_lock();
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
        let _guard = acquire_color_lock();
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
        let _guard = acquire_color_lock();
        colored::control::set_override(true);
        let at_50 = coverage_color(50.0, "50.0");
        assert!(
            at_50.contains("\x1b[33m"),
            "Expected yellow at exactly 50%: {at_50:?}"
        );
    }

    #[test]
    fn test_coverage_color_boundary_80() {
        let _guard = acquire_color_lock();
        colored::control::set_override(true);
        let at_80 = coverage_color(80.0, "80.0");
        assert!(
            at_80.contains("\x1b[32m"),
            "Expected green at exactly 80%: {at_80:?}"
        );
    }

    #[test]
    fn test_crap_exceeding_bold_red() {
        let _guard = acquire_color_lock();
        colored::control::set_override(true);
        let out = crap_color(true, "15.00");
        assert!(
            out.contains("\x1b[1;31m"),
            "Expected bold+red ANSI: {out:?}"
        );
    }

    #[test]
    fn test_crap_within_no_emphasis() {
        let _guard = acquire_color_lock();
        colored::control::set_override(true);
        let out = crap_color(false, "5.00");
        assert!(!out.contains("\x1b["), "Expected no ANSI: {out:?}");
        assert_eq!(out, "5.00");
    }

    #[test]
    fn test_full_table_snapshot() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table(
            &make_view_default(&result),
            8.0,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
        );
        insta::assert_snapshot!(output);
    }

    #[test]
    fn grouped_table_has_per_file_header() {
        use crate::domain::view::{self, GroupKey, ViewSpec};
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let view = view::apply(
            &result,
            ViewSpec {
                group_by: Some(GroupKey::File),
                ..Default::default()
            },
        );
        let output = format_table(&view, 8.0, false, TEST_TOOL_NAME, TEST_TOOL_VERSION);
        // Per-file column header
        assert!(output.contains("File"));
        assert!(output.contains("Functions"));
        assert!(output.contains("Failing"));
        assert!(output.contains("Avg CRAP"));
        assert!(output.contains("Worst CRAP"));
        assert!(output.contains("Worst Fn"));
        // The per-function `CC` and `Cov%` columns are absent — those
        // are function-level fields and the grouped table shifts to
        // per-file rows.
        assert!(
            !output.contains("Cov%"),
            "per-function Cov% header leaked: {output}"
        );
        assert!(
            !output.contains(" CC "),
            "per-function CC header leaked: {output}"
        );
        // Summary line still derives from view.full
        assert!(output.contains("Summary: 3 functions"));
    }

    #[test]
    fn grouped_table_snapshot() {
        use crate::domain::view::{self, GroupKey, ViewSpec};
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let view = view::apply(
            &result,
            ViewSpec {
                group_by: Some(GroupKey::File),
                ..Default::default()
            },
        );
        let output = format_table(&view, 8.0, false, TEST_TOOL_NAME, TEST_TOOL_VERSION);
        insta::assert_snapshot!(output);
    }

    // ── Delta block (VS5) ─────────────────────────────────────────────

    #[test]
    fn delta_block_renders_header_and_summary_line() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let output = format_table_with_explain(
            &make_view_default(&delta.current),
            Some(&dview),
            8.0,
            false,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            None,
        );
        assert!(
            output.contains("Delta vs baseline:"),
            "missing delta header: {output}"
        );
        assert!(
            output.contains("+1 added, 1 removed, 2 modified"),
            "missing delta summary counts: {output}"
        );
        // new_fn is the only new violation; parse_record's baseline already exceeded.
        assert!(
            output.contains("1 new violations"),
            "missing new violations: {output}"
        );
    }

    #[test]
    fn delta_block_includes_change_rows_with_kind_column() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let output = format_table_with_explain(
            &make_view_default(&delta.current),
            Some(&dview),
            8.0,
            false,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            None,
        );
        // Per-change rows present
        assert!(output.contains("added"));
        assert!(output.contains("removed"));
        assert!(output.contains("modified"));
        assert!(output.contains("new_fn"));
        assert!(output.contains("complex_fn")); // removed
        assert!(output.contains("parse_record")); // modified
    }

    #[test]
    fn no_baseline_means_no_delta_block() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let result = make_multi_function_result();
        let output = format_table_with_explain(
            &make_view_default(&result),
            None,
            8.0,
            false,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            None,
        );
        assert!(!output.contains("Delta vs baseline"));
    }

    #[test]
    fn delta_block_full_snapshot() {
        let _guard = acquire_color_lock();
        colored::control::set_override(false);
        let delta = make_sample_delta();
        let dview = make_delta_view_default(&delta);
        let output = format_table_with_explain(
            &make_view_default(&delta.current),
            Some(&dview),
            8.0,
            false,
            false,
            TEST_TOOL_NAME,
            TEST_TOOL_VERSION,
            None,
            None,
        );
        insta::assert_snapshot!(output);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::{
        TEST_TOOL_NAME, TEST_TOOL_VERSION, make_view_default,
    };
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
                                start_column: 0,
                                end_column: 0,
                            },
                        },
                        complexity,
                        complexity_metric: crate::domain::types::ComplexityMetric::Cognitive,
                        coverage_percent: coverage,
                        // The table-reporter proptest strategies focus
                        // on the line-coverage rendering path; branch
                        // coverage gets dedicated coverage in the
                        // `branch_column_*` unit tests below.
                        branch_coverage_percent: None,
                        crap: CrapScore {
                            value: crap_value,
                            risk_level: risk,
                        },
                        contributors: vec![],
                    },
                    threshold,
                    exceeds: crap_value > threshold,
                    diagnostic: None,
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
                    ..Default::default()
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
                end_line: line,
                nesting_depth: 0,
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_format_table_never_panics(result in arb_analysis_result()) {
            let _guard = super::acquire_color_lock();
            colored::control::set_override(false);
            let _ = format_table(&make_view_default(&result), 8.0, false, TEST_TOOL_NAME, TEST_TOOL_VERSION);
        }

        #[test]
        fn prop_format_table_with_breakdown_never_panics(
            mut result in arb_analysis_result(),
            contributors in prop::collection::vec(arb_contributor(), 0..5),
        ) {
            let _guard = super::acquire_color_lock();
            colored::control::set_override(false);
            // Inject contributors into the first exceeding function (if any)
            if let Some(v) = result.functions.iter_mut().find(|v| v.exceeds) {
                v.scored.contributors = contributors;
            }
            // Must not panic regardless of contributor content
            let _ = format_table(&make_view_default(&result), 8.0, true, TEST_TOOL_NAME, TEST_TOOL_VERSION);
        }

        #[test]
        fn prop_format_table_row_count(result in arb_analysis_result()) {
            let _guard = super::acquire_color_lock();
            colored::control::set_override(false);
            let output = format_table(&make_view_default(&result), 8.0, false, TEST_TOOL_NAME, TEST_TOOL_VERSION);
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
