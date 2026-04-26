use serde::Serialize;

use super::types::{
    AnalysisSummary, CrapScore, FunctionIdentity, FunctionVerdict, RiskDistribution, RiskLevel,
};

/// Compute an `AnalysisSummary` from any iterable of `&FunctionVerdict`.
///
/// Accepting an `IntoIterator` instead of a slice lets callers pass either
/// `&[FunctionVerdict]` (the typical core path) or `&[&FunctionVerdict]`
/// (the `view::apply` path, where `shown` is already a vec of borrows).
/// This avoids the deep clone that would otherwise be required to
/// materialise an owned slice on every `view::apply` invocation.
pub fn compute_summary<'a, I>(verdicts: I) -> AnalysisSummary
where
    I: IntoIterator<Item = &'a FunctionVerdict>,
{
    let mut distribution = RiskDistribution {
        low: 0,
        acceptable: 0,
        moderate: 0,
        high: 0,
    };

    let mut scores: Vec<f64> = Vec::new();
    let mut files: std::collections::HashSet<&'a String> = std::collections::HashSet::new();
    let mut exceeding: usize = 0;
    let mut max_crap = None;
    let mut worst_function = None;

    for v in verdicts {
        let score = v.scored.crap.value;
        scores.push(score);
        files.insert(&v.scored.identity.file_path);

        if v.exceeds {
            exceeding += 1;
        }

        match v.scored.crap.risk_level {
            RiskLevel::Low => distribution.low += 1,
            RiskLevel::Acceptable => distribution.acceptable += 1,
            RiskLevel::Moderate => distribution.moderate += 1,
            RiskLevel::High => distribution.high += 1,
        }

        if max_crap.is_none() || score > max_crap.unwrap_or(0.0) {
            max_crap = Some(score);
            worst_function = Some(v.scored.identity.clone());
        }
    }

    let total_functions = scores.len();
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let average_crap = if total_functions > 0 {
        scores.iter().sum::<f64>() / total_functions as f64
    } else {
        0.0
    };

    let median_crap = if total_functions > 0 {
        if total_functions.is_multiple_of(2) {
            (scores[total_functions / 2 - 1] + scores[total_functions / 2]) / 2.0
        } else {
            scores[total_functions / 2]
        }
    } else {
        0.0
    };

    AnalysisSummary {
        total_functions,
        total_files: files.len(),
        exceeding_threshold: exceeding,
        average_crap,
        median_crap,
        max_crap: max_crap.map(super::crap::classify_risk).map(|risk_level| {
            super::types::CrapScore {
                value: max_crap.unwrap(),
                risk_level,
            }
        }),
        worst_function,
        distribution,
    }
}

// ── Per-file aggregation (issue #64 — `--group-by file`) ────────────

/// Per-file aggregate over a `FunctionVerdict` partition.
///
/// The partition key is `FunctionIdentity::file_path`. Aggregates are
/// pure: no `syn`, no LCOV, no `PathBuf` — just integers, floats, and
/// the already-domain identity string. Ships unchanged into `crap-core`.
#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    pub file_path: String,
    pub function_count: usize,
    pub exceeding_count: usize,
    pub average_crap: f64,
    pub median_crap: f64,
    pub max_crap: Option<CrapScore>,
    pub worst_function: Option<FunctionIdentity>,
    pub distribution: RiskDistribution,
}

/// Group `verdicts` by `file_path` and compute per-file aggregates.
///
/// Order of the returned vec is undefined — callers (e.g. the View)
/// must apply their own sort. Empty input returns an empty vec.
///
/// Mirrors `compute_summary`'s `IntoIterator<Item = &'a FunctionVerdict>`
/// signature so it composes with `view::apply` without forcing a clone.
pub fn compute_file_summaries<'a, I>(verdicts: I) -> Vec<FileSummary>
where
    I: IntoIterator<Item = &'a FunctionVerdict>,
{
    // Stable insertion order keeps the output deterministic for fixture
    // tests; callers that want a specific order sort downstream.
    let mut order: Vec<&'a String> = Vec::new();
    let mut buckets: std::collections::HashMap<&'a String, Vec<&'a FunctionVerdict>> =
        std::collections::HashMap::new();

    for v in verdicts {
        let path = &v.scored.identity.file_path;
        if !buckets.contains_key(path) {
            order.push(path);
        }
        buckets.entry(path).or_default().push(v);
    }

    order
        .into_iter()
        .map(|path| {
            let bucket = buckets
                .remove(path)
                .expect("bucket present for ordered key");
            file_summary_for(path.clone(), &bucket)
        })
        .collect()
}

fn file_summary_for(file_path: String, verdicts: &[&FunctionVerdict]) -> FileSummary {
    let function_count = verdicts.len();
    let mut distribution = RiskDistribution {
        low: 0,
        acceptable: 0,
        moderate: 0,
        high: 0,
    };
    let mut exceeding_count: usize = 0;
    let mut sum_crap: f64 = 0.0;
    let mut max_crap_value: Option<f64> = None;
    let mut worst_function: Option<FunctionIdentity> = None;
    let mut scores: Vec<f64> = Vec::with_capacity(function_count);

    for v in verdicts {
        let score = v.scored.crap.value;
        scores.push(score);
        sum_crap += score;
        if v.exceeds {
            exceeding_count += 1;
        }
        match v.scored.crap.risk_level {
            RiskLevel::Low => distribution.low += 1,
            RiskLevel::Acceptable => distribution.acceptable += 1,
            RiskLevel::Moderate => distribution.moderate += 1,
            RiskLevel::High => distribution.high += 1,
        }
        // Strict-greater: first verdict at the max wins (matches
        // `compute_summary`'s "first wins" semantics on ties).
        let beats = match max_crap_value {
            None => true,
            Some(curr) => score > curr,
        };
        if beats {
            max_crap_value = Some(score);
            worst_function = Some(v.scored.identity.clone());
        }
    }

    let average_crap = if function_count > 0 {
        sum_crap / function_count as f64
    } else {
        0.0
    };

    let median_crap = median_of(&mut scores);

    let max_crap = max_crap_value.map(|value| CrapScore {
        value,
        risk_level: super::crap::classify_risk(value),
    });

    FileSummary {
        file_path,
        function_count,
        exceeding_count,
        average_crap,
        median_crap,
        max_crap,
        worst_function,
        distribution,
    }
}

/// Sort-stable median for a non-empty score vector. NaN handling mirrors
/// `compute_summary`: `partial_cmp` falls back to `Equal` so NaN does
/// not panic. Empty input returns `0.0` to match `compute_summary`.
fn median_of(scores: &mut [f64]) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = scores.len();
    if n.is_multiple_of(2) {
        (scores[n / 2 - 1] + scores[n / 2]) / 2.0
    } else {
        scores[n / 2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        ComplexityMetric, CrapScore, FunctionIdentity, ScoredFunction, SourceSpan,
    };

    fn make_verdict(file: &str, name: &str, crap_value: f64, threshold: f64) -> FunctionVerdict {
        let risk_level = super::super::crap::classify_risk(crap_value);
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                    },
                },
                complexity: 1,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: 100.0,
                crap: CrapScore {
                    value: crap_value,
                    risk_level,
                },
                contributors: vec![],
            },
            threshold,
            exceeds: crap_value > threshold,
        }
    }

    #[test]
    fn empty_verdicts() {
        let summary = compute_summary(&[]);
        assert_eq!(summary.total_functions, 0);
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.exceeding_threshold, 0);
        assert_eq!(summary.average_crap, 0.0);
        assert_eq!(summary.median_crap, 0.0);
        assert!(summary.max_crap.is_none());
        assert!(summary.worst_function.is_none());
        assert_eq!(summary.distribution.low, 0);
        assert_eq!(summary.distribution.acceptable, 0);
        assert_eq!(summary.distribution.moderate, 0);
        assert_eq!(summary.distribution.high, 0);
    }

    #[test]
    fn single_verdict() {
        let v = make_verdict("a.rs", "foo", 3.0, 30.0);
        let summary = compute_summary(&[v]);
        assert_eq!(summary.total_functions, 1);
        assert_eq!(summary.total_files, 1);
        assert_eq!(summary.exceeding_threshold, 0);
        assert_eq!(summary.average_crap, 3.0);
        assert_eq!(summary.median_crap, 3.0);
        assert_eq!(summary.max_crap.unwrap().value, 3.0);
        assert_eq!(
            summary.worst_function.as_ref().unwrap().qualified_name,
            "foo"
        );
    }

    #[test]
    fn odd_count_median() {
        let verdicts = vec![
            make_verdict("a.rs", "a", 1.0, 30.0),
            make_verdict("a.rs", "b", 5.0, 30.0),
            make_verdict("a.rs", "c", 9.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.median_crap, 5.0);
    }

    #[test]
    fn even_count_median() {
        let verdicts = vec![
            make_verdict("a.rs", "a", 2.0, 30.0),
            make_verdict("a.rs", "b", 4.0, 30.0),
            make_verdict("a.rs", "c", 6.0, 30.0),
            make_verdict("a.rs", "d", 8.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        // Median of [2, 4, 6, 8] = (4 + 6) / 2 = 5.0
        assert_eq!(summary.median_crap, 5.0);
    }

    #[test]
    fn distribution_counting() {
        let verdicts = vec![
            make_verdict("a.rs", "low", 2.0, 30.0),        // Low (<=5)
            make_verdict("a.rs", "acceptable", 6.0, 30.0), // Acceptable (<=8)
            make_verdict("a.rs", "moderate", 15.0, 30.0),  // Moderate (<=30)
            make_verdict("a.rs", "high", 50.0, 30.0),      // High (>30)
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.distribution.low, 1);
        assert_eq!(summary.distribution.acceptable, 1);
        assert_eq!(summary.distribution.moderate, 1);
        assert_eq!(summary.distribution.high, 1);
    }

    #[test]
    fn max_crap_and_worst_function() {
        let verdicts = vec![
            make_verdict("a.rs", "small", 2.0, 30.0),
            make_verdict("b.rs", "big", 50.0, 30.0),
            make_verdict("c.rs", "medium", 10.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.max_crap.unwrap().value, 50.0);
        assert_eq!(
            summary.worst_function.as_ref().unwrap().qualified_name,
            "big"
        );
    }

    #[test]
    fn file_deduplication() {
        let verdicts = vec![
            make_verdict("a.rs", "foo", 2.0, 30.0),
            make_verdict("a.rs", "bar", 3.0, 30.0),
            make_verdict("b.rs", "baz", 4.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.total_functions, 3);
        assert_eq!(summary.total_files, 2);
    }

    #[test]
    fn exceeding_threshold_count() {
        let verdicts = vec![
            make_verdict("a.rs", "ok", 5.0, 10.0),     // below
            make_verdict("a.rs", "bad", 15.0, 10.0),   // exceeds
            make_verdict("a.rs", "worse", 50.0, 10.0), // exceeds
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.exceeding_threshold, 2);
    }

    #[test]
    fn average_calculation() {
        let verdicts = vec![
            make_verdict("a.rs", "a", 3.0, 30.0),
            make_verdict("a.rs", "b", 6.0, 30.0),
            make_verdict("a.rs", "c", 9.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(summary.average_crap, 6.0);
    }

    #[test]
    fn tied_scores_first_wins_worst_function() {
        let verdicts = vec![
            make_verdict("a.rs", "first", 10.0, 30.0),
            make_verdict("a.rs", "second", 10.0, 30.0),
        ];
        let summary = compute_summary(&verdicts);
        assert_eq!(
            summary.worst_function.as_ref().unwrap().qualified_name,
            "first"
        );
    }
}

#[cfg(test)]
mod file_summary_tests {
    use super::*;
    use crate::domain::types::{
        ComplexityMetric, CrapScore, FunctionIdentity, ScoredFunction, SourceSpan,
    };

    fn vrd(file: &str, name: &str, crap_value: f64, threshold: f64) -> FunctionVerdict {
        let risk_level = super::super::crap::classify_risk(crap_value);
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                    },
                },
                complexity: 1,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: 100.0,
                crap: CrapScore {
                    value: crap_value,
                    risk_level,
                },
                contributors: vec![],
            },
            threshold,
            exceeds: crap_value > threshold,
        }
    }

    #[test]
    fn empty_input_returns_empty_vec() {
        // P8 — empty robustness.
        let result = compute_file_summaries(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn single_file_single_function() {
        let v = vrd("a.rs", "foo", 3.0, 25.0);
        let summaries = compute_file_summaries(&[v]);
        assert_eq!(summaries.len(), 1);
        let f = &summaries[0];
        assert_eq!(f.file_path, "a.rs");
        assert_eq!(f.function_count, 1);
        assert_eq!(f.exceeding_count, 0);
        assert_eq!(f.average_crap, 3.0);
        assert_eq!(f.median_crap, 3.0);
        assert_eq!(f.max_crap.unwrap().value, 3.0);
        assert_eq!(f.worst_function.as_ref().unwrap().qualified_name, "foo");
    }

    #[test]
    fn partition_completeness() {
        // P1: sum(file.function_count) == verdicts.len()
        let verdicts = vec![
            vrd("a.rs", "a1", 1.0, 25.0),
            vrd("a.rs", "a2", 2.0, 25.0),
            vrd("b.rs", "b1", 30.0, 25.0),
            vrd("c.rs", "c1", 4.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let total: usize = summaries.iter().map(|f| f.function_count).sum();
        assert_eq!(total, verdicts.len());
    }

    #[test]
    fn distinct_files_are_grouped() {
        // P3: len == distinct file paths
        let verdicts = vec![
            vrd("a.rs", "a1", 1.0, 25.0),
            vrd("a.rs", "a2", 2.0, 25.0),
            vrd("b.rs", "b1", 30.0, 25.0),
            vrd("c.rs", "c1", 4.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        assert_eq!(summaries.len(), 3);
        let mut paths: Vec<&str> = summaries.iter().map(|f| f.file_path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, vec!["a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn exceeding_count_aggregates_per_file() {
        // P2 (per-file).
        let verdicts = vec![
            vrd("a.rs", "ok", 5.0, 10.0),
            vrd("a.rs", "bad1", 15.0, 10.0),
            vrd("a.rs", "bad2", 50.0, 10.0),
            vrd("b.rs", "ok2", 3.0, 10.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let a = summaries.iter().find(|f| f.file_path == "a.rs").unwrap();
        let b = summaries.iter().find(|f| f.file_path == "b.rs").unwrap();
        assert_eq!(a.exceeding_count, 2);
        assert_eq!(b.exceeding_count, 0);
    }

    #[test]
    fn exceeding_count_sum_matches_total() {
        // P2 (global): sum(file.exceeding_count) == verdicts.iter().filter(|v| v.exceeds).count().
        let verdicts = vec![
            vrd("a.rs", "ok", 5.0, 10.0),
            vrd("a.rs", "bad1", 15.0, 10.0),
            vrd("b.rs", "bad2", 50.0, 10.0),
            vrd("c.rs", "ok2", 3.0, 10.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let sum: usize = summaries.iter().map(|f| f.exceeding_count).sum();
        let manual = verdicts.iter().filter(|v| v.exceeds).count();
        assert_eq!(sum, manual);
    }

    #[test]
    fn max_crap_per_file_correct() {
        // P4: per-file max_crap matches manual max.
        let verdicts = vec![
            vrd("a.rs", "low", 5.0, 25.0),
            vrd("a.rs", "high", 50.0, 25.0),
            vrd("b.rs", "med", 12.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let a = summaries.iter().find(|f| f.file_path == "a.rs").unwrap();
        let b = summaries.iter().find(|f| f.file_path == "b.rs").unwrap();
        assert_eq!(a.max_crap.unwrap().value, 50.0);
        assert_eq!(a.worst_function.as_ref().unwrap().qualified_name, "high");
        assert_eq!(b.max_crap.unwrap().value, 12.0);
    }

    #[test]
    fn average_crap_per_file_correct() {
        // P5: per-file average_crap matches manual mean.
        let verdicts = vec![
            vrd("a.rs", "a1", 4.0, 25.0),
            vrd("a.rs", "a2", 6.0, 25.0),
            vrd("a.rs", "a3", 14.0, 25.0),
            vrd("b.rs", "b1", 3.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let a = summaries.iter().find(|f| f.file_path == "a.rs").unwrap();
        let b = summaries.iter().find(|f| f.file_path == "b.rs").unwrap();
        // (4 + 6 + 14) / 3 = 8.0
        assert!((a.average_crap - 8.0).abs() < 1e-9);
        assert!((b.average_crap - 3.0).abs() < 1e-9);
    }

    #[test]
    fn median_per_file_odd_and_even() {
        let verdicts = vec![
            vrd("a.rs", "a1", 1.0, 25.0),
            vrd("a.rs", "a2", 5.0, 25.0),
            vrd("a.rs", "a3", 9.0, 25.0),
            vrd("b.rs", "b1", 2.0, 25.0),
            vrd("b.rs", "b2", 8.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        let a = summaries.iter().find(|f| f.file_path == "a.rs").unwrap();
        let b = summaries.iter().find(|f| f.file_path == "b.rs").unwrap();
        assert_eq!(a.median_crap, 5.0);
        assert_eq!(b.median_crap, 5.0); // (2+8)/2
    }

    #[test]
    fn distribution_per_file() {
        let verdicts = vec![
            vrd("a.rs", "low", 2.0, 25.0),        // Low (<=5)
            vrd("a.rs", "acceptable", 6.0, 25.0), // Acceptable (<=8)
            vrd("a.rs", "moderate", 15.0, 25.0),  // Moderate (<=30)
            vrd("a.rs", "high", 50.0, 25.0),      // High (>30)
        ];
        let summaries = compute_file_summaries(&verdicts);
        assert_eq!(summaries.len(), 1);
        let d = &summaries[0].distribution;
        assert_eq!(d.low, 1);
        assert_eq!(d.acceptable, 1);
        assert_eq!(d.moderate, 1);
        assert_eq!(d.high, 1);
    }

    #[test]
    fn nan_coverage_does_not_panic() {
        // P8 (NaN). Coverage NaN does not affect file aggregation since
        // file aggregates are over CRAP not coverage; assert no panic.
        let v = FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: "a.rs".to_string(),
                    qualified_name: "f".to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                    },
                },
                complexity: 1,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: f64::NAN,
                crap: CrapScore {
                    value: 5.0,
                    risk_level: RiskLevel::Low,
                },
                contributors: vec![],
            },
            threshold: 25.0,
            exceeds: false,
        };
        let summaries = compute_file_summaries(&[v]);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].function_count, 1);
    }

    #[test]
    fn tied_max_first_wins() {
        // Two functions in the same file at the same CRAP — first wins.
        let verdicts = vec![
            vrd("a.rs", "first", 10.0, 25.0),
            vrd("a.rs", "second", 10.0, 25.0),
        ];
        let summaries = compute_file_summaries(&verdicts);
        assert_eq!(
            summaries[0].worst_function.as_ref().unwrap().qualified_name,
            "first"
        );
    }
}

#[cfg(test)]
mod file_summary_proptests {
    use super::*;
    use crate::test_strategies::arb_verdict;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// P1: partition completeness.
        #[test]
        fn prop_partition_completeness(
            verdicts in prop::collection::vec(arb_verdict(), 0..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            let total: usize = summaries.iter().map(|f| f.function_count).sum();
            prop_assert_eq!(total, verdicts.len());
        }

        /// P2: aggregation faithfulness for `exceeding_count`.
        #[test]
        fn prop_exceeding_count_aggregation(
            verdicts in prop::collection::vec(arb_verdict(), 0..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            let sum: usize = summaries.iter().map(|f| f.exceeding_count).sum();
            let manual = verdicts.iter().filter(|v| v.exceeds).count();
            prop_assert_eq!(sum, manual);
        }

        /// P3: one row per distinct file path.
        #[test]
        fn prop_one_row_per_distinct_file(
            verdicts in prop::collection::vec(arb_verdict(), 0..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            let distinct: std::collections::HashSet<&str> = verdicts
                .iter()
                .map(|v| v.scored.identity.file_path.as_str())
                .collect();
            prop_assert_eq!(summaries.len(), distinct.len());
            // and the file_paths in summaries match the distinct set
            let summary_paths: std::collections::HashSet<&str> =
                summaries.iter().map(|f| f.file_path.as_str()).collect();
            prop_assert_eq!(summary_paths, distinct);
        }

        /// P4: per-file max_crap is the max within the file.
        #[test]
        fn prop_max_crap_correct(
            verdicts in prop::collection::vec(arb_verdict(), 1..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            for f in &summaries {
                let in_file: Vec<f64> = verdicts
                    .iter()
                    .filter(|v| v.scored.identity.file_path == f.file_path)
                    .map(|v| v.scored.crap.value)
                    .collect();
                let manual_max = in_file
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                let got = f.max_crap.unwrap().value;
                prop_assert!((got - manual_max).abs() < 1e-9,
                    "file {} max_crap mismatch: got {got}, expected {manual_max}",
                    f.file_path);
            }
        }

        /// P5: per-file average_crap is the mean within the file.
        #[test]
        fn prop_average_crap_correct(
            verdicts in prop::collection::vec(arb_verdict(), 1..50)
        ) {
            let summaries = compute_file_summaries(&verdicts);
            for f in &summaries {
                let in_file: Vec<f64> = verdicts
                    .iter()
                    .filter(|v| v.scored.identity.file_path == f.file_path)
                    .map(|v| v.scored.crap.value)
                    .collect();
                let manual_mean = in_file.iter().sum::<f64>() / in_file.len() as f64;
                prop_assert!((f.average_crap - manual_mean).abs() < 1e-6,
                    "file {} average_crap mismatch: got {}, expected {}",
                    f.file_path, f.average_crap, manual_mean);
            }
        }

        /// P8: never panics on empty / arbitrary inputs.
        #[test]
        fn prop_never_panics(
            verdicts in prop::collection::vec(arb_verdict(), 0..50)
        ) {
            let _ = compute_file_summaries(&verdicts);
        }
    }
}
