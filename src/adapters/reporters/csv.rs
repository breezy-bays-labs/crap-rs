//! CSV reporter — RFC 4180 row-per-function output.
//!
//! No header summary. One row per function shown by the view; the
//! envelope's full analysis is the gate, but CSV is data-only.

use std::borrow::Cow;

use crate::domain::types::{ComplexityMetric, FunctionVerdict};
use crate::domain::view::AnalysisView;

/// Format an `AnalysisView` as RFC 4180 CSV.
///
/// Header row is fixed and stable for downstream tools. The
/// `complexity_metric` column reflects the analysis-wide metric, not
/// per-function — every row carries the same value.
pub fn format_csv(view: &AnalysisView<'_>, metric: ComplexityMetric) -> String {
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
        let out = format_csv(&make_view_default(&result), ComplexityMetric::Cognitive);
        assert_eq!(
            out,
            "file,function,start_line,end_line,complexity,complexity_metric,coverage_percent,crap_score,risk_level,exceeds_threshold\n"
        );
    }

    #[test]
    fn one_row_per_function() {
        let result = make_multi_function_result();
        let out = format_csv(&make_view_default(&result), ComplexityMetric::Cognitive);
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
        let out = format_csv(&make_view_default(&result), ComplexityMetric::Cognitive);
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
        let out = format_csv(&make_view_default(&result), ComplexityMetric::Cognitive);
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
        let out = format_csv(&make_view_default(&result), ComplexityMetric::Cognitive);
        assert!(
            out.contains("\"two\nlines\""),
            "expected quoted newline: {out}"
        );
    }

    #[test]
    fn metric_column_reflects_arg() {
        let result =
            make_single_function_result("f", "src/lib.rs", 1, 100.0, 1.0, RiskLevel::Low, 8.0);
        let out_cog = format_csv(&make_view_default(&result), ComplexityMetric::Cognitive);
        let out_cyc = format_csv(&make_view_default(&result), ComplexityMetric::Cyclomatic);
        assert!(out_cog.contains(",cognitive,"));
        assert!(out_cyc.contains(",cyclomatic,"));
    }

    #[test]
    fn full_csv_snapshot() {
        let result = make_multi_function_result();
        let out = format_csv(&make_view_default(&result), ComplexityMetric::Cognitive);
        insta::assert_snapshot!(out);
    }
}
