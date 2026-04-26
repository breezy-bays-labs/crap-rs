//! CSV reporter — RFC 4180 row-per-function output.
//!
//! No header summary. One row per function shown by the view; the
//! envelope's full analysis is the gate, but CSV is data-only.

use std::borrow::Cow;

use crate::domain::delta::{DeltaView, FunctionChange};
use crate::domain::summary::FileSummary;
use crate::domain::types::{ComplexityMetric, FunctionVerdict};
use crate::domain::view::AnalysisView;

/// Format an `AnalysisView` as RFC 4180 CSV.
///
/// Header row is fixed and stable for downstream tools. The
/// `complexity_metric` column reflects the analysis-wide metric, not
/// per-function — every row carries the same value.
///
/// When `delta` is `Some`, the schema mode-switches to row-per-change
/// (one row per `FunctionChange`) with a `change_kind` column. The
/// per-function schema is unchanged when `delta` is `None`. CSV
/// consumers must pin their expected schema based on whether
/// `--baseline` was passed.
pub fn format_csv(
    view: &AnalysisView<'_>,
    delta: Option<&DeltaView<'_>>,
    metric: ComplexityMetric,
) -> String {
    if let Some(delta_view) = delta {
        return format_csv_delta(delta_view, metric);
    }

    if let Some(grouped) = view.grouped.as_ref() {
        return format_csv_grouped(&grouped.files);
    }

    let mut out = String::new();
    out.push_str(
        "file,function,start_line,end_line,complexity,complexity_metric,coverage_percent,crap_score,risk_level,exceeds_threshold\n",
    );

    for verdict in view.shown.iter() {
        out.push_str(&row_for(verdict, metric));
        out.push('\n');
    }

    out
}

/// Per-change CSV. Header carries side-by-side baseline / current
/// scores so downstream renderers can sort or filter without re-pairing
/// against the original analyses. Empty cells signal "not applicable"
/// for the change's kind (Added has no baseline_*; Removed has no
/// current_*).
fn format_csv_delta(view: &DeltaView<'_>, metric: ComplexityMetric) -> String {
    let mut out = String::new();
    out.push_str(
        "change_kind,file,function,baseline_complexity,baseline_coverage_percent,baseline_crap,current_complexity,current_coverage_percent,current_crap,score_delta,complexity_metric\n",
    );
    for change in view.shown.iter() {
        out.push_str(&row_for_change(change, metric));
        out.push('\n');
    }
    out
}

fn row_for_change(change: &FunctionChange, metric: ComplexityMetric) -> String {
    let baseline_cells = match change {
        FunctionChange::Removed { baseline } | FunctionChange::Modified { baseline, .. } => {
            let s = &baseline.scored;
            format!(
                "{},{:.1},{:.2}",
                s.complexity, s.coverage_percent, s.crap.value
            )
        }
        FunctionChange::Added { .. } => ",,".to_string(),
    };
    let current_cells = match change {
        FunctionChange::Added { current } | FunctionChange::Modified { current, .. } => {
            let s = &current.scored;
            format!(
                "{},{:.1},{:.2}",
                s.complexity, s.coverage_percent, s.crap.value
            )
        }
        FunctionChange::Removed { .. } => ",,".to_string(),
    };
    let delta_cell = change
        .score_delta()
        .map(|d| format!("{d:.2}"))
        .unwrap_or_default();
    format!(
        "{},{},{},{},{},{},{}",
        change.kind().as_str(),
        quote_csv_field(change.file_path()),
        quote_csv_field(change.qualified_name()),
        baseline_cells,
        current_cells,
        delta_cell,
        metric,
    )
}

/// Per-file CSV output. The header schema differs from the
/// per-function output by design (Cycle 7 / shaping doc Q4): per-file
/// rows aggregate, so positional columns differ. Documented in
/// `--help` and CHANGELOG. Anyone scripting on CSV column position
/// should pin their flags.
fn format_csv_grouped(files: &[FileSummary]) -> String {
    let mut out = String::new();
    out.push_str(
        "file,function_count,exceeding_count,average_crap,max_crap,worst_function,distribution_low,distribution_acceptable,distribution_moderate,distribution_high\n",
    );
    for f in files {
        out.push_str(&row_for_file_summary(f));
        out.push('\n');
    }
    out
}

fn row_for_file_summary(f: &FileSummary) -> String {
    let max_crap = f
        .max_crap
        .as_ref()
        .map(|c| format!("{:.2}", c.value))
        .unwrap_or_default();
    let worst_fn = f
        .worst_function
        .as_ref()
        .map(|id| quote_csv_field(&id.qualified_name).into_owned())
        .unwrap_or_default();
    let d = &f.distribution;
    format!(
        "{},{},{},{:.2},{},{},{},{},{},{}",
        quote_csv_field(&f.file_path),
        f.function_count,
        f.exceeding_count,
        f.average_crap,
        max_crap,
        worst_fn,
        d.low,
        d.acceptable,
        d.moderate,
        d.high,
    )
}

fn row_for(verdict: &FunctionVerdict, metric: ComplexityMetric) -> String {
    let s = &verdict.scored;
    format!(
        "{},{},{},{},{},{},{:.1},{:.2},{},{}",
        quote_csv_field(&s.identity.file_path),
        quote_csv_field(&s.identity.qualified_name),
        s.identity.span.start_line,
        s.identity.span.end_line,
        s.complexity,
        metric,
        s.coverage_percent,
        s.crap.value,
        s.crap.risk_level,
        verdict.exceeds,
    )
}

/// Apply RFC 4180 quoting: wrap in `"..."` if the field contains a
/// comma, quote, CR, or LF; double inner quotes. Borrowed when no
/// quoting is needed.
fn quote_csv_field(s: &str) -> Cow<'_, str> {
    let needs_quoting = s
        .as_bytes()
        .iter()
        .any(|&b| b == b',' || b == b'"' || b == b'\r' || b == b'\n');
    if !needs_quoting {
        return Cow::Borrowed(s);
    }
    let mut quoted = String::with_capacity(s.len() + 2);
    quoted.push('"');
    for ch in s.chars() {
        if ch == '"' {
            quoted.push_str("\"\"");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('"');
    Cow::Owned(quoted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::reporters::test_fixtures::*;
    use crate::domain::types::{ComplexityMetric, RiskLevel};

    #[test]
    fn header_is_exact() {
        let result = make_empty_result();
        let out = format_csv(
            &make_view_default(&result),
            None,
            ComplexityMetric::Cognitive,
        );
        assert_eq!(
            out,
            "file,function,start_line,end_line,complexity,complexity_metric,coverage_percent,crap_score,risk_level,exceeds_threshold\n"
        );
    }

    #[test]
    fn one_row_per_function() {
        let result = make_multi_function_result();
        let out = format_csv(
            &make_view_default(&result),
            None,
            ComplexityMetric::Cognitive,
        );
        // 1 header + 3 data rows + trailing newline → 4 lines
        assert_eq!(out.lines().count(), 4);
    }

    #[test]
    fn comma_in_function_name_is_quoted() {
        let result = make_single_function_result(
            "weird,name",
            "src/lib.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let out = format_csv(
            &make_view_default(&result),
            None,
            ComplexityMetric::Cognitive,
        );
        assert!(
            out.contains("\"weird,name\""),
            "expected quoted comma: {out}"
        );
    }

    #[test]
    fn quote_in_function_name_is_doubled() {
        let result = make_single_function_result(
            "say\"hi",
            "src/lib.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let out = format_csv(
            &make_view_default(&result),
            None,
            ComplexityMetric::Cognitive,
        );
        assert!(
            out.contains("\"say\"\"hi\""),
            "expected doubled quote: {out}"
        );
    }

    #[test]
    fn newline_in_field_is_quoted() {
        let result = make_single_function_result(
            "two\nlines",
            "src/lib.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let out = format_csv(
            &make_view_default(&result),
            None,
            ComplexityMetric::Cognitive,
        );
        assert!(
            out.contains("\"two\nlines\""),
            "expected quoted newline: {out}"
        );
    }

    #[test]
    fn metric_column_reflects_arg() {
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let out_cog = format_csv(
            &make_view_default(&result),
            None,
            ComplexityMetric::Cognitive,
        );
        let out_cyc = format_csv(
            &make_view_default(&result),
            None,
            ComplexityMetric::Cyclomatic,
        );
        assert!(out_cog.contains(",cognitive,"));
        assert!(out_cyc.contains(",cyclomatic,"));
    }

    #[test]
    fn full_csv_snapshot() {
        let result = make_multi_function_result();
        let out = format_csv(
            &make_view_default(&result),
            None,
            ComplexityMetric::Cognitive,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn grouped_csv_header_shifts() {
        use crate::domain::view::{self, GroupKey, ViewSpec};
        let result = make_multi_function_result();
        let view = view::apply(
            &result,
            ViewSpec {
                group_by: Some(GroupKey::File),
                ..Default::default()
            },
        );
        let out = format_csv(&view, None, ComplexityMetric::Cognitive);
        // 10-column per-file header
        let expected_header = "file,function_count,exceeding_count,average_crap,max_crap,worst_function,distribution_low,distribution_acceptable,distribution_moderate,distribution_high";
        let first_line = out.lines().next().unwrap();
        assert_eq!(first_line, expected_header);
        // Per-function header MUST NOT appear
        assert!(
            !out.contains("complexity_metric"),
            "per-function header leaked: {out}"
        );
        // Three files in the fixture → 1 header + 3 data rows + trailing newline.
        assert_eq!(out.lines().count(), 4);
    }

    #[test]
    fn grouped_csv_quotes_commas_in_path() {
        use crate::adapters::reporters::test_fixtures::make_verdict;
        use crate::domain::types::{AnalysisResult, AnalysisSummary, RiskDistribution};
        use crate::domain::view::{self, GroupKey, ViewSpec};
        let v = make_verdict(
            "fn1",
            "src/weird,path.rs",
            1,
            100.0,
            1.0,
            RiskLevel::Low,
            8.0,
        );
        let result = AnalysisResult {
            functions: vec![v],
            summary: AnalysisSummary {
                total_functions: 1,
                total_files: 1,
                exceeding_threshold: 0,
                average_crap: 1.0,
                median_crap: 1.0,
                max_crap: None,
                worst_function: None,
                distribution: RiskDistribution {
                    low: 1,
                    acceptable: 0,
                    moderate: 0,
                    high: 0,
                },
            },
            passed: true,
        };
        let view = view::apply(
            &result,
            ViewSpec {
                group_by: Some(GroupKey::File),
                ..Default::default()
            },
        );
        let out = format_csv(&view, None, ComplexityMetric::Cognitive);
        assert!(
            out.contains("\"src/weird,path.rs\""),
            "expected quoted path: {out}"
        );
    }

    #[test]
    fn grouped_csv_snapshot() {
        use crate::domain::view::{self, GroupKey, ViewSpec};
        let result = make_multi_function_result();
        let view = view::apply(
            &result,
            ViewSpec {
                group_by: Some(GroupKey::File),
                ..Default::default()
            },
        );
        let out = format_csv(&view, None, ComplexityMetric::Cognitive);
        insta::assert_snapshot!(out);
    }

    // ── Delta CSV (VS5) ─────────────────────────────────────────────

    #[test]
    fn delta_csv_header_carries_change_kind_and_side_by_side_columns() {
        let delta = make_sample_delta();
        let view = make_delta_view_default(&delta);
        let out = format_csv(
            &make_view_default(&delta.current),
            Some(&view),
            ComplexityMetric::Cognitive,
        );
        let header = out.lines().next().expect("at least header");
        assert_eq!(
            header,
            "change_kind,file,function,baseline_complexity,baseline_coverage_percent,baseline_crap,current_complexity,current_coverage_percent,current_crap,score_delta,complexity_metric"
        );
    }

    #[test]
    fn delta_csv_added_row_has_empty_baseline_columns() {
        let delta = make_sample_delta();
        let view = make_delta_view_default(&delta);
        let out = format_csv(
            &make_view_default(&delta.current),
            Some(&view),
            ComplexityMetric::Cognitive,
        );
        let added_line = out
            .lines()
            .find(|l| l.starts_with("added,"))
            .expect("added row present");
        // Columns: change_kind,file,function,baseline_complexity,baseline_coverage,baseline_crap,...
        let cells: Vec<&str> = added_line.split(',').collect();
        assert_eq!(cells[3], ""); // baseline_complexity empty
        assert_eq!(cells[4], ""); // baseline_coverage empty
        assert_eq!(cells[5], ""); // baseline_crap empty
        assert_eq!(cells[9], ""); // score_delta empty for Added
    }

    #[test]
    fn delta_csv_removed_row_has_empty_current_columns() {
        let delta = make_sample_delta();
        let view = make_delta_view_default(&delta);
        let out = format_csv(
            &make_view_default(&delta.current),
            Some(&view),
            ComplexityMetric::Cognitive,
        );
        let removed_line = out
            .lines()
            .find(|l| l.starts_with("removed,"))
            .expect("removed row present");
        let cells: Vec<&str> = removed_line.split(',').collect();
        assert_eq!(cells[6], ""); // current_complexity empty
        assert_eq!(cells[7], ""); // current_coverage empty
        assert_eq!(cells[8], ""); // current_crap empty
        assert_eq!(cells[9], ""); // score_delta empty for Removed
    }

    #[test]
    fn delta_csv_modified_row_carries_score_delta() {
        let delta = make_sample_delta();
        let view = make_delta_view_default(&delta);
        let out = format_csv(
            &make_view_default(&delta.current),
            Some(&view),
            ComplexityMetric::Cognitive,
        );
        // parse_record went 15.0 → 22.0, score_delta = 7.00
        let modified_line = out
            .lines()
            .find(|l| l.starts_with("modified,") && l.contains("parse_record"))
            .expect("modified parse_record row");
        assert!(
            modified_line.contains(",7.00,"),
            "score_delta column missing: {modified_line}"
        );
    }

    #[test]
    fn delta_csv_full_snapshot() {
        let delta = make_sample_delta();
        let view = make_delta_view_default(&delta);
        let out = format_csv(
            &make_view_default(&delta.current),
            Some(&view),
            ComplexityMetric::Cognitive,
        );
        insta::assert_snapshot!(out);
    }
}
