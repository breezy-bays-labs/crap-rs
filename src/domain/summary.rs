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
        if total_functions % 2 == 0 {
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
        max_crap: max_crap.map(|value| super::crap::classify_risk(value)).map(|risk_level| {
            super::types::CrapScore {
                value: max_crap.unwrap(),
                risk_level,
            }
        }),
        worst_function,
        distribution,
    }
}
