//! Function matching — joins complexity data with coverage data.
//!
//! Uses line-range matching: for each function from the complexity adapter,
//! compute coverage from LCOV DA lines within the function's line range.
//! This is dramatically simpler than crap4ts's span-overlap matching because
//! we bypass function name matching entirely.

use super::types::{
    BranchCoverage, CoverageRatio, FunctionComplexity, FunctionCoverage, LineCoverage, SourceSpan,
};
use std::collections::HashMap;

/// Returns `true` if `span` overlaps any of the `changed_ranges`.
///
/// Both `span` and each range use inclusive start/end lines.
/// An empty `changed_ranges` slice always returns `false`.
pub fn overlaps_any(span: &SourceSpan, changed_ranges: &[SourceSpan]) -> bool {
    changed_ranges
        .iter()
        .any(|range| span.start_line <= range.end_line && range.start_line <= span.end_line)
}

/// Match complexity entries with coverage using line-range overlap.
///
/// For each function in `complexities`, finds DA lines from `line_data`
/// that fall within the function's span, then computes coverage ratio.
/// If `branch_data` is provided, also computes branch coverage per function.
pub fn match_functions(
    complexities: &[FunctionComplexity],
    line_data: &HashMap<String, Vec<LineCoverage>>,
    branch_data: Option<&HashMap<String, Vec<BranchCoverage>>>,
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

        let file_branches =
            branch_data.and_then(|bd| bd.get(&comp.identity.file_path).map(|v| v.as_slice()));
        let coverage = compute_function_coverage(
            &comp.identity.file_path,
            comp.identity.span,
            file_lines,
            file_branches,
        );
        results.push((comp.clone(), coverage));
    }

    results
}

fn compute_function_coverage(
    file_path: &str,
    span: SourceSpan,
    file_lines: &[LineCoverage],
    file_branches: Option<&[BranchCoverage]>,
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

    let branch_coverage =
        file_branches.and_then(|branches| compute_branch_coverage(span, branches));

    FunctionCoverage {
        file_path: file_path.to_string(),
        span,
        line_coverage: CoverageRatio {
            covered,
            total,
            percent,
        },
        branch_coverage,
    }
}

fn compute_branch_coverage(span: SourceSpan, branches: &[BranchCoverage]) -> Option<CoverageRatio> {
    let mut total = 0usize;
    let mut covered = 0usize;

    for branch in branches {
        if branch.line >= span.start_line
            && branch.line <= span.end_line
            && let Some(taken) = branch.taken
        {
            total += 1;
            if taken > 0 {
                covered += 1;
            }
        }
    }

    if total == 0 {
        return None;
    }

    let percent = (covered as f64 / total as f64) * 100.0;
    Some(CoverageRatio {
        covered,
        total,
        percent,
    })
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
        branch_coverage: None,
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
                    start_column: 0,
                    end_column: 0,
                },
            },
            complexity: 1,
            metric: ComplexityMetric::Cognitive,
            contributors: vec![],
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
        let result = match_functions(&[], &line_data, None);
        assert!(result.is_empty());
    }

    #[test]
    fn no_coverage_data_for_file() {
        let comp = make_complexity("a.rs", "foo", 1, 10);
        let line_data = make_line_data(&[("b.rs", &[(1, 5)])]);
        let result = match_functions(&[comp], &line_data, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.line_coverage.percent, 0.0);
        assert_eq!(result[0].1.line_coverage.covered, 0);
        assert_eq!(result[0].1.line_coverage.total, 0);
    }

    #[test]
    fn full_coverage() {
        let comp = make_complexity("a.rs", "foo", 1, 3);
        let line_data = make_line_data(&[("a.rs", &[(1, 1), (2, 3), (3, 7)])]);
        let result = match_functions(&[comp], &line_data, None);
        assert_eq!(result[0].1.line_coverage.covered, 3);
        assert_eq!(result[0].1.line_coverage.total, 3);
        assert_eq!(result[0].1.line_coverage.percent, 100.0);
    }

    #[test]
    fn zero_coverage_all_unhit() {
        let comp = make_complexity("a.rs", "foo", 1, 3);
        let line_data = make_line_data(&[("a.rs", &[(1, 0), (2, 0), (3, 0)])]);
        let result = match_functions(&[comp], &line_data, None);
        assert_eq!(result[0].1.line_coverage.covered, 0);
        assert_eq!(result[0].1.line_coverage.total, 3);
        assert_eq!(result[0].1.line_coverage.percent, 0.0);
    }

    #[test]
    fn partial_coverage() {
        let comp = make_complexity("a.rs", "foo", 1, 3);
        let line_data = make_line_data(&[("a.rs", &[(1, 1), (2, 0), (3, 5)])]);
        let result = match_functions(&[comp], &line_data, None);
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
        let result = match_functions(&[comp], &line_data, None);
        // Only lines 3, 4, 5 should be counted
        assert_eq!(result[0].1.line_coverage.total, 3);
        assert_eq!(result[0].1.line_coverage.covered, 2); // lines 3 and 5
    }

    #[test]
    fn boundary_inclusive_start() {
        let comp = make_complexity("a.rs", "foo", 5, 10);
        let line_data = make_line_data(&[("a.rs", &[(5, 3)])]);
        let result = match_functions(&[comp], &line_data, None);
        assert_eq!(result[0].1.line_coverage.total, 1);
        assert_eq!(result[0].1.line_coverage.covered, 1);
    }

    #[test]
    fn boundary_inclusive_end() {
        let comp = make_complexity("a.rs", "foo", 5, 10);
        let line_data = make_line_data(&[("a.rs", &[(10, 2)])]);
        let result = match_functions(&[comp], &line_data, None);
        assert_eq!(result[0].1.line_coverage.total, 1);
        assert_eq!(result[0].1.line_coverage.covered, 1);
    }

    #[test]
    fn no_instrumentable_lines_100_pct() {
        let comp = make_complexity("a.rs", "foo", 5, 10);
        // No DA lines in the span at all
        let line_data = make_line_data(&[("a.rs", &[(1, 1), (20, 1)])]);
        let result = match_functions(&[comp], &line_data, None);
        assert_eq!(result[0].1.line_coverage.total, 0);
        assert_eq!(result[0].1.line_coverage.percent, 100.0);
    }

    #[test]
    fn multiple_functions_same_file() {
        let comp1 = make_complexity("a.rs", "foo", 1, 5);
        let comp2 = make_complexity("a.rs", "bar", 10, 15);
        let line_data =
            make_line_data(&[("a.rs", &[(1, 1), (2, 0), (3, 1), (10, 0), (11, 0), (12, 0)])]);
        let result = match_functions(&[comp1, comp2], &line_data, None);

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
        let result = match_functions(&[comp_a, comp_b], &line_data, None);

        // a.rs: 3/3 = 100%
        assert_eq!(result[0].1.line_coverage.percent, 100.0);
        // b.rs: 0/2 = 0%
        assert_eq!(result[1].1.line_coverage.percent, 0.0);
    }

    // ── Branch matching tests ───────────────────────────────────────

    type BranchEntry = (usize, Option<u64>);

    fn make_branch_data(
        entries: &[(&str, &[BranchEntry])],
    ) -> HashMap<String, Vec<BranchCoverage>> {
        let mut map = HashMap::new();
        for (file, branches) in entries {
            map.insert(
                file.to_string(),
                branches
                    .iter()
                    .map(|&(line, taken)| BranchCoverage { line, taken })
                    .collect(),
            );
        }
        map
    }

    #[test]
    fn branch_points_within_span() {
        let comp = make_complexity("a.rs", "foo", 5, 15);
        let line_data = make_line_data(&[("a.rs", &[(5, 1)])]);
        let branch_data =
            make_branch_data(&[("a.rs", &[(7, Some(3)), (10, Some(1)), (12, Some(0))])]);
        let result = match_functions(&[comp], &line_data, Some(&branch_data));
        let bc = result[0]
            .1
            .branch_coverage
            .as_ref()
            .expect("should have branch_coverage");
        assert_eq!(bc.total, 3);
    }

    #[test]
    fn branch_points_outside_span() {
        let comp = make_complexity("a.rs", "foo", 5, 15);
        let line_data = make_line_data(&[("a.rs", &[(5, 1)])]);
        let branch_data =
            make_branch_data(&[("a.rs", &[(3, Some(1)), (10, Some(1)), (20, Some(1))])]);
        let result = match_functions(&[comp], &line_data, Some(&branch_data));
        let bc = result[0]
            .1
            .branch_coverage
            .as_ref()
            .expect("should have branch_coverage");
        assert_eq!(bc.total, 1);
    }

    #[test]
    fn branch_boundaries_inclusive() {
        let comp = make_complexity("a.rs", "foo", 5, 15);
        let line_data = make_line_data(&[("a.rs", &[(5, 1)])]);
        let branch_data = make_branch_data(&[("a.rs", &[(5, Some(1)), (15, Some(1))])]);
        let result = match_functions(&[comp], &line_data, Some(&branch_data));
        let bc = result[0]
            .1
            .branch_coverage
            .as_ref()
            .expect("should have branch_coverage");
        assert_eq!(bc.total, 2);
        assert_eq!(bc.covered, 2);
    }

    #[test]
    fn branch_none_excluded_from_ratio() {
        let comp = make_complexity("a.rs", "foo", 5, 15);
        let line_data = make_line_data(&[("a.rs", &[(5, 1)])]);
        let branch_data = make_branch_data(&[("a.rs", &[(7, Some(3)), (10, None), (12, Some(0))])]);
        let result = match_functions(&[comp], &line_data, Some(&branch_data));
        let bc = result[0]
            .1
            .branch_coverage
            .as_ref()
            .expect("should have branch_coverage");
        // None excluded: total = 2 (Some(3) and Some(0)), covered = 1 (Some(3) only)
        assert_eq!(bc.total, 2);
        assert_eq!(bc.covered, 1);
        assert_eq!(bc.percent, 50.0);
    }

    #[test]
    fn no_branch_points_gives_none() {
        let comp = make_complexity("a.rs", "foo", 5, 15);
        let line_data = make_line_data(&[("a.rs", &[(5, 1)])]);
        // Branch data exists but outside span
        let branch_data = make_branch_data(&[("a.rs", &[(20, Some(1))])]);
        let result = match_functions(&[comp], &line_data, Some(&branch_data));
        assert!(result[0].1.branch_coverage.is_none());
    }

    #[test]
    fn branch_no_cross_file_leakage() {
        let comp_a = make_complexity("a.rs", "foo", 1, 10);
        let comp_b = make_complexity("b.rs", "bar", 1, 10);
        let line_data = make_line_data(&[("a.rs", &[(1, 1)]), ("b.rs", &[(1, 1)])]);
        let branch_data = make_branch_data(&[("a.rs", &[(5, Some(1))])]);
        let result = match_functions(&[comp_a, comp_b], &line_data, Some(&branch_data));
        // a.rs has branch coverage
        let a_bc = result[0]
            .1
            .branch_coverage
            .as_ref()
            .expect("a.rs should have branch_coverage");
        assert_eq!(a_bc.total, 1);
        // b.rs has no branch data → None
        assert!(result[1].1.branch_coverage.is_none());
    }
}

#[cfg(test)]
mod overlaps_any_tests {
    use super::*;

    fn span(start: usize, end: usize) -> SourceSpan {
        SourceSpan {
            start_line: start,
            end_line: end,
            start_column: 0,
            end_column: 0,
        }
    }

    #[test]
    fn overlap_partial() {
        assert!(overlaps_any(&span(5, 15), &[span(10, 20)]));
    }

    #[test]
    fn no_overlap() {
        assert!(!overlaps_any(&span(1, 5), &[span(10, 20)]));
    }

    #[test]
    fn adjacent_disjoint() {
        assert!(!overlaps_any(&span(1, 10), &[span(11, 20)]));
    }

    #[test]
    fn touching_boundary() {
        assert!(overlaps_any(&span(1, 10), &[span(10, 20)]));
    }

    #[test]
    fn contained() {
        assert!(overlaps_any(&span(5, 8), &[span(1, 20)]));
    }

    #[test]
    fn exact_match() {
        assert!(overlaps_any(&span(5, 10), &[span(5, 10)]));
    }

    #[test]
    fn single_line_spans() {
        assert!(overlaps_any(&span(5, 5), &[span(5, 5)]));
    }

    #[test]
    fn empty_ranges() {
        assert!(!overlaps_any(&span(1, 100), &[]));
    }

    #[test]
    fn multiple_ranges_hit_second() {
        assert!(overlaps_any(&span(50, 60), &[span(1, 5), span(55, 70)]));
    }
}

#[cfg(test)]
mod overlaps_any_proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_span() -> impl Strategy<Value = SourceSpan> {
        (1..10_000usize, 1..10_000usize).prop_map(|(a, b)| {
            let (start, end) = if a <= b { (a, b) } else { (b, a) };
            SourceSpan {
                start_line: start,
                end_line: end,
                start_column: 0,
                end_column: 0,
            }
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn commutative(a in arb_span(), b in arb_span()) {
            prop_assert_eq!(
                overlaps_any(&a, &[b]),
                overlaps_any(&b, &[a]),
            );
        }

        #[test]
        fn reflexive(s in arb_span()) {
            prop_assert!(overlaps_any(&s, &[s]));
        }

        #[test]
        fn empty_is_always_false(s in arb_span()) {
            prop_assert!(!overlaps_any(&s, &[]));
        }

        #[test]
        fn subset_implies_overlap(
            outer_start in 1..5_000usize,
            outer_len in 10..5_000usize,
            inner_offset in 1..9usize,
        ) {
            let outer_end = outer_start + outer_len;
            let inner_start = outer_start + inner_offset.min(outer_len - 1);
            let inner_end = inner_start.min(outer_end);
            let outer = SourceSpan { start_line: outer_start, end_line: outer_end, start_column: 0, end_column: 0 };
            let inner = SourceSpan { start_line: inner_start, end_line: inner_end, start_column: 0, end_column: 0 };
            prop_assert!(overlaps_any(&inner, &[outer]));
        }

        #[test]
        fn adjacent_disjoint_property(
            start in 1..5_000usize,
            len1 in 1..5_000usize,
            len2 in 1..5_000usize,
        ) {
            let end1 = start + len1;
            let start2 = end1 + 1; // gap of 1
            let end2 = start2 + len2;
            let a = SourceSpan { start_line: start, end_line: end1, start_column: 0, end_column: 0 };
            let b = SourceSpan { start_line: start2, end_line: end2, start_column: 0, end_column: 0 };
            prop_assert!(!overlaps_any(&a, &[b]));
        }
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
                        start_column: 0,
                        end_column: 0,
                    },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
                contributors: vec![],
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

    fn arb_branch_data(
        file: &'static str,
    ) -> impl Strategy<Value = HashMap<String, Vec<BranchCoverage>>> {
        prop::collection::vec((1..1000usize, prop::option::of(0..100u64)), 0..50).prop_map(
            move |entries| {
                let mut map = HashMap::new();
                map.insert(
                    file.to_string(),
                    entries
                        .into_iter()
                        .map(|(line, taken)| BranchCoverage { line, taken })
                        .collect(),
                );
                map
            },
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn coverage_always_0_to_100(
            comp in arb_complexity("test.rs"),
            line_data in arb_line_data("test.rs"),
        ) {
            let result = match_functions(&[comp], &line_data, None);
            for (_, cov) in &result {
                let pct = cov.line_coverage.percent;
                prop_assert!((0.0..=100.0).contains(&pct), "Coverage percent {pct} out of range");
            }
        }

        #[test]
        fn covered_lte_total(
            comp in arb_complexity("test.rs"),
            line_data in arb_line_data("test.rs"),
        ) {
            let result = match_functions(&[comp], &line_data, None);
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
                    span: SourceSpan { start_line: 1, end_line: 100, start_column: 0, end_column: 0 },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
                contributors: vec![],
            };
            let comp_b = FunctionComplexity {
                identity: FunctionIdentity {
                    file_path: "b.rs".to_string(),
                    qualified_name: "bar".to_string(),
                    span: SourceSpan { start_line: 200, end_line: 300, start_column: 0, end_column: 0 },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
                contributors: vec![],
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

            let result = match_functions(&[comp_a, comp_b], &line_data, None);

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
                    span: SourceSpan { start_line: start, end_line: end, start_column: 0, end_column: 0 },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
                contributors: vec![],
            };

            // Place lines at boundary positions
            let mut line_data = HashMap::new();
            line_data.insert("test.rs".to_string(), vec![
                LineCoverage { line: start.saturating_sub(1).max(1), hits: 1 }, // before span
                LineCoverage { line: start, hits: 1 },     // at start (included)
                LineCoverage { line: end, hits: 1 },       // at end (included)
                LineCoverage { line: end + 1, hits: 1 },   // after span (excluded)
            ]);

            let result = match_functions(&[comp], &line_data, None);
            let cov = &result[0].1;

            if start > 1 {
                // start-1 and end+1 should be excluded → total = 2
                prop_assert_eq!(cov.line_coverage.total, 2, "Only start and end should be included");
                prop_assert_eq!(cov.line_coverage.covered, 2);
            }
        }

        #[test]
        fn branch_ratio_always_0_to_100(
            comp in arb_complexity("test.rs"),
            line_data in arb_line_data("test.rs"),
            branch_data in arb_branch_data("test.rs"),
        ) {
            let result = match_functions(&[comp], &line_data, Some(&branch_data));
            for (_, cov) in &result {
                if let Some(bc) = &cov.branch_coverage {
                    prop_assert!(bc.percent >= 0.0 && bc.percent <= 100.0,
                        "Branch percent {} out of range", bc.percent);
                }
            }
        }

        #[test]
        fn branch_covered_lte_total(
            comp in arb_complexity("test.rs"),
            line_data in arb_line_data("test.rs"),
            branch_data in arb_branch_data("test.rs"),
        ) {
            let result = match_functions(&[comp], &line_data, Some(&branch_data));
            for (_, cov) in &result {
                if let Some(bc) = &cov.branch_coverage {
                    prop_assert!(bc.covered <= bc.total,
                        "branch covered ({}) > total ({})", bc.covered, bc.total);
                }
            }
        }

        #[test]
        fn branch_no_cross_file_leakage_proptest(
            a_branches in prop::collection::vec((1..100usize, prop::option::of(0..10u64)), 1..10),
            b_branches in prop::collection::vec((200..300usize, prop::option::of(0..10u64)), 1..10),
        ) {
            let comp_a = FunctionComplexity {
                identity: FunctionIdentity {
                    file_path: "a.rs".to_string(),
                    qualified_name: "foo".to_string(),
                    span: SourceSpan { start_line: 1, end_line: 100, start_column: 0, end_column: 0 },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
                contributors: vec![],
            };
            let comp_b = FunctionComplexity {
                identity: FunctionIdentity {
                    file_path: "b.rs".to_string(),
                    qualified_name: "bar".to_string(),
                    span: SourceSpan { start_line: 200, end_line: 300, start_column: 0, end_column: 0 },
                },
                complexity: 1,
                metric: ComplexityMetric::Cognitive,
                contributors: vec![],
            };

            let line_data = {
                let mut m = HashMap::new();
                m.insert("a.rs".to_string(), vec![LineCoverage { line: 1, hits: 1 }]);
                m.insert("b.rs".to_string(), vec![LineCoverage { line: 200, hits: 1 }]);
                m
            };

            let mut branch_data: HashMap<String, Vec<BranchCoverage>> = HashMap::new();
            branch_data.insert(
                "a.rs".to_string(),
                a_branches.iter().map(|&(l, t)| BranchCoverage { line: l, taken: t }).collect(),
            );
            branch_data.insert(
                "b.rs".to_string(),
                b_branches.iter().map(|&(l, t)| BranchCoverage { line: l, taken: t }).collect(),
            );

            let result = match_functions(&[comp_a, comp_b], &line_data, Some(&branch_data));

            // a.rs branch total should only count branches in [1, 100]
            if let Some(bc) = &result[0].1.branch_coverage {
                let a_in_range = a_branches.iter().filter(|(l, t)| *l <= 100 && t.is_some()).count();
                prop_assert!(bc.total <= a_in_range,
                    "a.rs branch total ({}) exceeds in-range count ({})", bc.total, a_in_range);
            }

            // b.rs branch total should only count branches in [200, 300]
            if let Some(bc) = &result[1].1.branch_coverage {
                let b_in_range = b_branches.iter().filter(|(l, t)| *l >= 200 && *l <= 300 && t.is_some()).count();
                prop_assert!(bc.total <= b_in_range,
                    "b.rs branch total ({}) exceeds in-range count ({})", bc.total, b_in_range);
            }
        }
    }
}
