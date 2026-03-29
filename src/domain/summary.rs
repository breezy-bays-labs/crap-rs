use super::types::{AnalysisSummary, FunctionVerdict, RiskDistribution, RiskLevel};

pub fn compute_summary(verdicts: &[FunctionVerdict]) -> AnalysisSummary {
    let total_functions = verdicts.len();
    let exceeding = verdicts.iter().filter(|v| v.exceeds).count();

    let mut distribution = RiskDistribution {
        low: 0,
        acceptable: 0,
        moderate: 0,
        high: 0,
    };

    let mut scores: Vec<f64> = Vec::with_capacity(total_functions);
    let mut files = std::collections::HashSet::new();
    let mut max_crap = None;
    let mut worst_function = None;

    for v in verdicts {
        let score = v.scored.crap.value;
        scores.push(score);
        files.insert(&v.scored.identity.file_path);

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
