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

use crate::domain::summary::compute_summary;
use crate::domain::types::{
    AnalysisResult, ComplexityMetric, CrapScore, FunctionIdentity, FunctionVerdict, RiskLevel,
    ScoredFunction, SourceSpan,
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

/// Build an `AnalysisResult` with a faithful summary derived from the
/// generated verdicts via `domain::summary::compute_summary`.
///
/// Faithful means `summary.worst_function`, `summary.distribution`, and
/// `summary.total_files` are consistent with the `functions` vector —
/// not the previous skewed hand-rolled defaults (CR-N1).
///
/// Vec bound is `0..50` so tests probe empty, small-N, and N>typical-limit.
pub fn arb_analysis_result() -> impl Strategy<Value = AnalysisResult> {
    prop::collection::vec(arb_verdict(), 0..50).prop_map(|verdicts| {
        // Real `AnalysisResult` instances have unique
        // `(file_path, qualified_name)` per function — a syn walker can
        // only emit one verdict per source location. The strategy can
        // and does generate duplicates (low-cardinality regex), so dedup
        // here to keep the generator faithful to the production
        // invariant. Without this, delta tests collapse duplicate
        // baseline entries via `HashMap` overwrite and produce
        // pathological Added/Modified mixes that don't exist in real
        // input.
        let mut seen = std::collections::HashSet::new();
        let verdicts: Vec<FunctionVerdict> = verdicts
            .into_iter()
            .filter(|v| {
                seen.insert((
                    v.scored.identity.file_path.clone(),
                    v.scored.identity.qualified_name.clone(),
                ))
            })
            .collect();
        let passed = !verdicts.iter().any(|v| v.exceeds);
        let summary = compute_summary(&verdicts);
        AnalysisResult {
            functions: verdicts,
            summary,
            passed,
        }
    })
}
