//! Shared proptest strategies for crate-wide property tests.
//!
//! Lives at the crate root (not inside an adapter module) so that any
//! layer with `#[cfg(test)]` access — including `domain::view` — can
//! consume the same strategies without violating the dependency rule.
//!
//! `arb_verdict` and `arb_analysis_result` were originally module-private
//! to `src/adapters/reporters/json.rs`; they are reused (unmodified in
//! shape) here, with the analysis-result vec bound widened to `0..50`
//! so property tests probe empty + small-N + N>limit cases.

use crate::domain::types::{
    AnalysisResult, AnalysisSummary, ComplexityMetric, CrapScore, FunctionIdentity,
    FunctionVerdict, RiskDistribution, RiskLevel, ScoredFunction, SourceSpan,
};
use proptest::prelude::*;

pub fn arb_risk_level() -> impl Strategy<Value = RiskLevel> {
    prop_oneof![
        Just(RiskLevel::Low),
        Just(RiskLevel::Acceptable),
        Just(RiskLevel::Moderate),
        Just(RiskLevel::High),
    ]
}

pub fn arb_verdict() -> impl Strategy<Value = FunctionVerdict> {
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
                        },
                    },
                    complexity,
                    complexity_metric: ComplexityMetric::Cognitive,
                    coverage_percent: coverage,
                    crap: CrapScore {
                        value: crap_value,
                        risk_level: risk,
                    },
                    contributors: vec![],
                },
                threshold,
                exceeds: crap_value > threshold,
            },
        )
}

/// Verdict generator that mixes finite coverage and `f64::NAN` coverage.
///
/// Used to exercise NaN-aware filter and sort paths in `domain::view`.
/// Roughly half the verdicts will have `coverage_percent = NaN`.
pub fn arb_verdict_with_nan_coverage() -> impl Strategy<Value = FunctionVerdict> {
    (
        "[a-z_]{1,20}",
        "src/[a-z/]{1,30}\\.rs",
        1..100u32,
        prop_oneof![(0.0..=100.0f64).prop_map(Some), Just(None::<f64>),],
        1.0..200.0f64,
        arb_risk_level(),
        1.0..100.0f64,
    )
        .prop_map(
            |(name, file, complexity, maybe_cov, crap_value, risk, threshold)| {
                let coverage = maybe_cov.unwrap_or(f64::NAN);
                FunctionVerdict {
                    scored: ScoredFunction {
                        identity: FunctionIdentity {
                            file_path: file,
                            qualified_name: name,
                            span: SourceSpan {
                                start_line: 1,
                                end_line: 10,
                            },
                        },
                        complexity,
                        complexity_metric: ComplexityMetric::Cognitive,
                        coverage_percent: coverage,
                        crap: CrapScore {
                            value: crap_value,
                            risk_level: risk,
                        },
                        contributors: vec![],
                    },
                    threshold,
                    exceeds: crap_value > threshold,
                }
            },
        )
}

/// Build an `AnalysisResult` with a hand-constructed summary.
///
/// Summary values are structurally valid but not guaranteed semantically
/// precise — call sites that need a faithful summary should pass the
/// vector through `domain::summary::compute_summary` instead.
///
/// Vec bound is `0..50` so tests probe empty, small-N, and N>typical-limit.
pub fn arb_analysis_result() -> impl Strategy<Value = AnalysisResult> {
    prop::collection::vec(arb_verdict(), 0..50).prop_map(|verdicts| {
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
            },
            passed,
        }
    })
}
