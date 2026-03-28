use super::types::{CrapError, CrapScore, RiskLevel};

/// Compute the CRAP score for a function.
///
/// Formula: complexity² × (1 - coverage/100)³ + complexity
///
/// The formula is agnostic to which complexity metric (cognitive or cyclomatic)
/// produced the input value.
pub fn compute_crap(complexity: u32, coverage_percent: f64) -> Result<CrapScore, CrapError> {
    if complexity < 1 {
        return Err(CrapError::InvalidComplexity(complexity));
    }
    if !coverage_percent.is_finite() {
        return Err(CrapError::InvalidCoverage(coverage_percent));
    }

    let clamped = coverage_percent.clamp(0.0, 100.0);
    let uncovered = 1.0 - clamped / 100.0;
    let comp = f64::from(complexity);
    let value = round_to_2(comp * comp * uncovered.powi(3) + comp);

    Ok(CrapScore {
        value,
        risk_level: classify_risk(value),
    })
}

fn round_to_2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub fn classify_risk(score: f64) -> RiskLevel {
    if score <= 5.0 {
        RiskLevel::Low
    } else if score <= 8.0 {
        RiskLevel::Acceptable
    } else if score <= 30.0 {
        RiskLevel::Moderate
    } else {
        RiskLevel::High
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivial_function_fully_covered() {
        let score = compute_crap(1, 100.0).unwrap();
        assert_eq!(score.value, 1.0);
        assert_eq!(score.risk_level, RiskLevel::Low);
    }

    #[test]
    fn trivial_function_zero_coverage() {
        let score = compute_crap(1, 0.0).unwrap();
        assert_eq!(score.value, 2.0);
        assert_eq!(score.risk_level, RiskLevel::Low);
    }

    #[test]
    fn complex_function_fully_covered() {
        let score = compute_crap(10, 100.0).unwrap();
        assert_eq!(score.value, 10.0);
        assert_eq!(score.risk_level, RiskLevel::Moderate);
    }

    #[test]
    fn complex_function_zero_coverage() {
        let score = compute_crap(10, 0.0).unwrap();
        assert_eq!(score.value, 110.0);
        assert_eq!(score.risk_level, RiskLevel::High);
    }

    #[test]
    fn moderate_complexity_partial_coverage() {
        // CC=6, 80% coverage => 6² × 0.2³ + 6 = 36 × 0.008 + 6 = 6.288 → 6.29
        let score = compute_crap(6, 80.0).unwrap();
        assert_eq!(score.value, 6.29);
        assert_eq!(score.risk_level, RiskLevel::Acceptable);
    }

    #[test]
    fn threshold_boundary_low_acceptable() {
        // Score exactly 5.0 should be Low
        assert_eq!(classify_risk(5.0), RiskLevel::Low);
        assert_eq!(classify_risk(5.01), RiskLevel::Acceptable);
    }

    #[test]
    fn threshold_boundary_acceptable_moderate() {
        assert_eq!(classify_risk(8.0), RiskLevel::Acceptable);
        assert_eq!(classify_risk(8.01), RiskLevel::Moderate);
    }

    #[test]
    fn threshold_boundary_moderate_high() {
        assert_eq!(classify_risk(30.0), RiskLevel::Moderate);
        assert_eq!(classify_risk(30.01), RiskLevel::High);
    }

    #[test]
    fn rejects_zero_complexity() {
        assert!(compute_crap(0, 50.0).is_err());
    }

    #[test]
    fn rejects_infinite_coverage() {
        assert!(compute_crap(5, f64::INFINITY).is_err());
    }

    #[test]
    fn rejects_nan_coverage() {
        assert!(compute_crap(5, f64::NAN).is_err());
    }

    #[test]
    fn rejects_neg_infinite_coverage() {
        assert!(compute_crap(5, f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn clamps_coverage_above_100() {
        let score = compute_crap(5, 150.0).unwrap();
        assert_eq!(score.value, 5.0); // Same as 100%
    }

    #[test]
    fn clamps_coverage_below_zero() {
        let score = compute_crap(5, -10.0).unwrap();
        assert_eq!(score.value, 30.0); // Same as 0%
    }

    // ── crap4ts oracle cross-validation ────────────────────────────────
    // These values must match crap4ts exactly (CLAUDE.md reference table).

    #[test]
    fn crap4ts_oracle_moderate_half_covered() {
        let score = compute_crap(5, 50.0).unwrap();
        assert_eq!(score.value, 8.13);
        assert_eq!(score.risk_level, RiskLevel::Moderate);
    }

    #[test]
    fn crap4ts_oracle_high_complexity_mostly_covered() {
        let score = compute_crap(15, 90.0).unwrap();
        assert_eq!(score.value, 15.23);
        assert_eq!(score.risk_level, RiskLevel::Moderate);
    }

}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn full_coverage_equals_complexity(complexity in 1..1000u32) {
            let score = compute_crap(complexity, 100.0).unwrap();
            assert_eq!(score.value, f64::from(complexity));
        }

        #[test]
        fn zero_coverage_equals_c_squared_plus_c(complexity in 1..1000u32) {
            let c = f64::from(complexity);
            let expected = round_to_2(c * c + c);
            let score = compute_crap(complexity, 0.0).unwrap();
            assert_eq!(score.value, expected);
        }

        #[test]
        fn score_always_gte_one(complexity in 1..1000u32, coverage in 0.0..=100.0f64) {
            let score = compute_crap(complexity, coverage).unwrap();
            prop_assert!(score.value >= 1.0, "CRAP score {} < 1.0", score.value);
        }

        #[test]
        fn monotonic_in_coverage(
            complexity in 1..100u32,
            cov_lo in 0.0..50.0f64,
            cov_delta in 0.01..50.0f64,
        ) {
            let cov_hi = (cov_lo + cov_delta).min(100.0);
            let score_lo = compute_crap(complexity, cov_lo).unwrap();
            let score_hi = compute_crap(complexity, cov_hi).unwrap();
            prop_assert!(
                score_hi.value <= score_lo.value,
                "Higher coverage ({cov_hi}%) should give lower CRAP, got {} vs {}",
                score_hi.value, score_lo.value
            );
        }

        #[test]
        fn monotonic_in_complexity(
            comp_lo in 1..500u32,
            comp_delta in 1..500u32,
            coverage in 0.0..=100.0f64,
        ) {
            let comp_hi = comp_lo.saturating_add(comp_delta);
            let score_lo = compute_crap(comp_lo, coverage).unwrap();
            let score_hi = compute_crap(comp_hi, coverage).unwrap();
            prop_assert!(
                score_hi.value >= score_lo.value,
                "Higher complexity ({comp_hi}) should give higher CRAP, got {} vs {}",
                score_hi.value, score_lo.value
            );
        }
    }
}
