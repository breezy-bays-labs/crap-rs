//! Function matching — joins complexity data with coverage data.
//!
//! Uses line-range matching: for each function from the complexity adapter,
//! compute coverage from LCOV DA lines within the function's line range.
//! This is dramatically simpler than crap4ts's span-overlap matching because
//! we bypass function name matching entirely.

use super::types::{CoverageRatio, FunctionComplexity, FunctionCoverage, LineCoverage, SourceSpan};
use std::collections::HashMap;

/// Match complexity entries with coverage using line-range overlap.
///
/// For each function in `complexities`, finds DA lines from `line_data`
/// that fall within the function's span, then computes coverage ratio.
pub fn match_functions(
    complexities: &[FunctionComplexity],
    line_data: &HashMap<String, Vec<LineCoverage>>,
) -> Vec<(FunctionComplexity, FunctionCoverage)> {
    let mut results = Vec::new();

    for comp in complexities {
        let file_lines = match line_data.get(&comp.identity.file_path) {
            Some(lines) => lines,
            None => {
                // No coverage data for this file — report as 0% coverage
                results.push((
                    comp.clone(),
                    zero_coverage(&comp.identity.file_path, comp.identity.span),
                ));
                continue;
            }
        };

        let coverage =
            compute_function_coverage(&comp.identity.file_path, comp.identity.span, file_lines);
        results.push((comp.clone(), coverage));
    }

    results
}

fn compute_function_coverage(
    file_path: &str,
    span: SourceSpan,
    file_lines: &[LineCoverage],
) -> FunctionCoverage {
    let mut total = 0usize;
    let mut covered = 0usize;

    for line in file_lines {
        if line.line >= span.start_line && line.line <= span.end_line {
            total += 1;
            if line.hits > 0 {
                covered += 1;
            }
        }
    }

    let percent = if total > 0 {
        (covered as f64 / total as f64) * 100.0
    } else {
        100.0 // No instrumentable lines = trivially covered
    };

    FunctionCoverage {
        file_path: file_path.to_string(),
        span,
        line_coverage: CoverageRatio {
            covered,
            total,
            percent,
        },
    }
}

fn zero_coverage(file_path: &str, span: SourceSpan) -> FunctionCoverage {
    FunctionCoverage {
        file_path: file_path.to_string(),
        span,
        line_coverage: CoverageRatio {
            covered: 0,
            total: 0,
            percent: 0.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{ComplexityMetric, FunctionIdentity};
    use pretty_assertions::assert_eq;

    fn make_complexity(file: &str, name: &str, start: usize, end: usize) -> FunctionComplexity {
        FunctionComplexity {
            identity: FunctionIdentity {
                file_path: file.to_string(),
                qualified_name: name.to_string(),
                span: SourceSpan {
                    start_line: start,
                    end_line: end,
                },
            },
            complexity: 1,
            metric: ComplexityMetric::Cognitive,
        }
    }

    fn make_line_data(entries: &[(&str, &[(usize, u64)])]) -> HashMap<String, Vec<LineCoverage>> {
        let mut map = HashMap::new();
        for (file, lines) in entries {
            map.insert(
                file.to_string(),
                lines
                    .iter()
                    .map(|&(line, hits)| LineCoverage { line, hits })
                    .collect(),
            );
        }
        map
    }

    #[test]
    fn empty_complexities_returns_empty() {
        let line_data = make_line_data(&[("a.rs", &[(1, 5)])]);
        let result = match_functions(&[], &line_data);
        assert!(result.is_empty());
    }

    #[test]
    fn no_coverage_data_for_file() {
        let comp = make_complexity("a.rs", "foo", 1, 10);
        let line_data = make_line_data(&[("b.rs", &[(1, 5)])]);
        let result = match_functions(&[comp], &line_data);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.line_coverage.percent, 0.0);
        assert_eq!(result[0].1.line_coverage.covered, 0);
        assert_eq!(result[0].1.line_coverage.total, 0);
    }

    #[test]
    fn full_coverage() {
        let comp = make_complexity("a.rs", "foo", 1, 3);
        let line_data = make_line_data(&[("a.rs", &[(1, 1), (2, 3), (3, 7)])]);
        let result = match_functions(&[comp], &line_data);
        assert_eq!(result[0].1.line_coverage.covered, 3);
        assert_eq!(result[0].1.line_coverage.total, 3);
        assert_eq!(result[0].1.line_coverage.percent, 100.0);
    }

    #[test]
    fn zero_coverage_all_unhit() {
        let comp = make_complexity("a.rs", "foo", 1, 3);
        let line_data = make_line_data(&[("a.rs", &[(1, 0), (2, 0), (3, 0)])]);
        let result = match_functions(&[comp], &line_data);
        assert_eq!(result[0].1.line_coverage.covered, 0);
        assert_eq!(result[0].1.line_coverage.total, 3);
        assert_eq!(result[0].1.line_coverage.percent, 0.0);
    }

    #[test]
    fn partial_coverage() {
        let comp = make_complexity("a.rs", "foo", 1, 3);
        let line_data = make_line_data(&[("a.rs", &[(1, 1), (2, 0), (3, 5)])]);
        let result = match_functions(&[comp], &line_data);
        assert_eq!(result[0].1.line_coverage.covered, 2);
        assert_eq!(result[0].1.line_coverage.total, 3);
        let pct = result[0].1.line_coverage.percent;
        assert!((pct - 66.66666666666667).abs() < 0.001);
    }

    #[test]
    fn lines_outside_span_excluded() {
        let comp = make_complexity("a.rs", "foo", 3, 5);
        let line_data =
            make_line_data(&[("a.rs", &[(1, 1), (2, 1), (3, 1), (4, 0), (5, 1), (6, 1)])]);
        let result = match_functions(&[comp], &line_data);
        // Only lines 3, 4, 5 should be counted
        assert_eq!(result[0].1.line_coverage.total, 3);
        assert_eq!(result[0].1.line_coverage.covered, 2); // lines 3 and 5
    }

    #[test]
    fn boundary_inclusive_start() {
        let comp = make_complexity("a.rs", "foo", 5, 10);
        let line_data = make_line_data(&[("a.rs", &[(5, 3)])]);
        let result = match_functions(&[comp], &line_data);
        assert_eq!(result[0].1.line_coverage.total, 1);
        assert_eq!(result[0].1.line_coverage.covered, 1);
    }

    #[test]
    fn boundary_inclusive_end() {
        let comp = make_complexity("a.rs", "foo", 5, 10);
        let line_data = make_line_data(&[("a.rs", &[(10, 2)])]);
        let result = match_functions(&[comp], &line_data);
        assert_eq!(result[0].1.line_coverage.total, 1);
        assert_eq!(result[0].1.line_coverage.covered, 1);
    }

    #[test]
    fn no_instrumentable_lines_100_pct() {
        let comp = make_complexity("a.rs", "foo", 5, 10);
        // No DA lines in the span at all
        let line_data = make_line_data(&[("a.rs", &[(1, 1), (20, 1)])]);
        let result = match_functions(&[comp], &line_data);
        assert_eq!(result[0].1.line_coverage.total, 0);
        assert_eq!(result[0].1.line_coverage.percent, 100.0);
    }

    #[test]
    fn multiple_functions_same_file() {
        let comp1 = make_complexity("a.rs", "foo", 1, 5);
        let comp2 = make_complexity("a.rs", "bar", 10, 15);
        let line_data =
            make_line_data(&[("a.rs", &[(1, 1), (2, 0), (3, 1), (10, 0), (11, 0), (12, 0)])]);
        let result = match_functions(&[comp1, comp2], &line_data);

        // foo: 2/3 covered
        assert_eq!(result[0].1.line_coverage.covered, 2);
        assert_eq!(result[0].1.line_coverage.total, 3);

        // bar: 0/3 covered
        assert_eq!(result[1].1.line_coverage.covered, 0);
        assert_eq!(result[1].1.line_coverage.total, 3);
    }

    #[test]
    fn file_scoped_no_leakage() {
        let comp_a = make_complexity("a.rs", "foo", 1, 10);
        let comp_b = make_complexity("b.rs", "bar", 1, 10);
        let line_data = make_line_data(&[
            ("a.rs", &[(1, 5), (2, 5), (3, 5)]),
            ("b.rs", &[(1, 0), (2, 0)]),
        ]);
        let result = match_functions(&[comp_a, comp_b], &line_data);

        // a.rs: 3/3 = 100%
        assert_eq!(result[0].1.line_coverage.percent, 100.0);
        // b.rs: 0/2 = 0%
        assert_eq!(result[1].1.line_coverage.percent, 0.0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::domain::types::{ComplexityMetric, FunctionIdentity};
    use proptest::prelude::*;

    fn arb_complexity(file: &'static str) -> impl Strategy<Value = FunctionComplexity> {
        (1..500usize, 1..500usize).prop_map(move |(start, len)| {
            let end = start + len;
            FunctionComplexity {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: format!("fn_{start}"),
                    span: SourceSpan {
                        start_line: start,
                        end_line: end,
                    },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
            }
        })
    }

    fn arb_line_data(
        file: &'static str,
    ) -> impl Strategy<Value = HashMap<String, Vec<LineCoverage>>> {
        prop::collection::vec((1..1000usize, 0..100u64), 0..50).prop_map(move |entries| {
            let mut map = HashMap::new();
            map.insert(
                file.to_string(),
                entries
                    .into_iter()
                    .map(|(line, hits)| LineCoverage { line, hits })
                    .collect(),
            );
            map
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn coverage_always_0_to_100(
            comp in arb_complexity("test.rs"),
            line_data in arb_line_data("test.rs"),
        ) {
            let result = match_functions(&[comp], &line_data);
            for (_, cov) in &result {
                let pct = cov.line_coverage.percent;
                prop_assert!(pct >= 0.0 && pct <= 100.0, "Coverage percent {pct} out of range");
            }
        }

        #[test]
        fn covered_lte_total(
            comp in arb_complexity("test.rs"),
            line_data in arb_line_data("test.rs"),
        ) {
            let result = match_functions(&[comp], &line_data);
            for (_, cov) in &result {
                prop_assert!(
                    cov.line_coverage.covered <= cov.line_coverage.total,
                    "covered ({}) > total ({})", cov.line_coverage.covered, cov.line_coverage.total
                );
            }
        }

        #[test]
        fn no_cross_file_leakage(
            a_lines in prop::collection::vec((1..100usize, 0..10u64), 1..10),
            b_lines in prop::collection::vec((200..300usize, 0..10u64), 1..10),
        ) {
            let comp_a = FunctionComplexity {
                identity: FunctionIdentity {
                    file_path: "a.rs".to_string(),
                    qualified_name: "foo".to_string(),
                    span: SourceSpan { start_line: 1, end_line: 100 },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
            };
            let comp_b = FunctionComplexity {
                identity: FunctionIdentity {
                    file_path: "b.rs".to_string(),
                    qualified_name: "bar".to_string(),
                    span: SourceSpan { start_line: 200, end_line: 300 },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
            };

            let mut line_data: HashMap<String, Vec<LineCoverage>> = HashMap::new();
            line_data.insert(
                "a.rs".to_string(),
                a_lines.iter().map(|&(l, h)| LineCoverage { line: l, hits: h }).collect(),
            );
            line_data.insert(
                "b.rs".to_string(),
                b_lines.iter().map(|&(l, h)| LineCoverage { line: l, hits: h }).collect(),
            );

            let result = match_functions(&[comp_a, comp_b], &line_data);

            // a.rs function should only have lines < 200
            let a_cov = &result[0].1;
            // Verify by checking that total <= number of a_lines entries in range
            prop_assert!(
                a_cov.line_coverage.total <= a_lines.len(),
                "a.rs total ({}) exceeds a_lines count ({})", a_cov.line_coverage.total, a_lines.len()
            );

            // b.rs function should only have lines >= 200
            let b_cov = &result[1].1;
            prop_assert!(
                b_cov.line_coverage.total <= b_lines.len(),
                "b.rs total ({}) exceeds b_lines count ({})", b_cov.line_coverage.total, b_lines.len()
            );
        }

        #[test]
        fn boundary_precision(
            start in 10..100usize,
            len in 5..50usize,
        ) {
            let end = start + len;
            let comp = FunctionComplexity {
                identity: FunctionIdentity {
                    file_path: "test.rs".to_string(),
                    qualified_name: "fn_test".to_string(),
                    span: SourceSpan { start_line: start, end_line: end },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
            };

            // Place lines at boundary positions
            let mut line_data = HashMap::new();
            line_data.insert("test.rs".to_string(), vec![
                LineCoverage { line: start.saturating_sub(1).max(1), hits: 1 }, // before span
                LineCoverage { line: start, hits: 1 },     // at start (included)
                LineCoverage { line: end, hits: 1 },       // at end (included)
                LineCoverage { line: end + 1, hits: 1 },   // after span (excluded)
            ]);

            let result = match_functions(&[comp], &line_data);
            let cov = &result[0].1;

            if start > 1 {
                // start-1 and end+1 should be excluded → total = 2
                prop_assert_eq!(cov.line_coverage.total, 2, "Only start and end should be included");
                prop_assert_eq!(cov.line_coverage.covered, 2);
            }
        }
    }
}
