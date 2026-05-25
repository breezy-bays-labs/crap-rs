//! Multi-language composition — combine per-adapter blocks into a
//! unified report context.
//!
//! Pure function. No I/O, no errors. The renderer consumes the
//! resulting [`MultiLangContext`] directly.
//!
//! ## Sort rule (D2d-locked)
//!
//! Combined-view ranking uses risk level descending (per-adapter
//! calibrated), then CRAP/threshold ratio descending within each
//! band. Raw CRAP scores are NOT dimensionally consistent across
//! adapters (cognitive complexity scales differently from cyclomatic),
//! so raw CRAP cannot serve as the primary sort. Per-tier risk and
//! ratio ARE dimensionally consistent — they answer "how far over
//! this adapter's own threshold" — and produce an honest ordering.

use crate::domain::multi_lang::{
    CombinedSummary, LanguageBlock, MultiLangContext, RankedFunction, WorstRatio, risk_level_rank,
    safe_ratio,
};
use crate::domain::types::RiskDistribution;
use std::cmp::Ordering;

/// Compose per-language blocks into a multi-language report context.
///
/// Sort rule: risk level desc, then CRAP/threshold ratio desc within
/// band. Stable secondary tie-break: qualified name asc + file path
/// asc, so identical (risk, ratio) pairs render deterministically.
///
/// Aggregate summary fields sum across blocks where additive
/// (function count, exceeding count, file count, risk-distribution
/// per tier); the `worst_ratio` field carries the single highest
/// CRAP/threshold ratio across all functions (NOT the highest raw
/// CRAP — see the dimensional-consistency note in the module
/// docstring).
pub fn compose_multi_lang<'a>(blocks: Vec<LanguageBlock<'a>>) -> MultiLangContext<'a> {
    let combined = compute_combined_summary(&blocks);
    MultiLangContext {
        languages: blocks,
        combined,
    }
}

fn compute_combined_summary(blocks: &[LanguageBlock<'_>]) -> CombinedSummary {
    let mut total_functions = 0usize;
    let mut total_exceeding = 0usize;
    let mut total_files = 0usize;
    let mut distribution = RiskDistribution::default();
    let mut ordered: Vec<RankedFunction> = Vec::new();
    let mut worst: Option<WorstRatio> = None;

    for block in blocks {
        let summary = &block.view.full.summary;
        total_functions += summary.total_functions;
        total_exceeding += summary.exceeding_threshold;
        total_files += summary.total_files;
        distribution.low += summary.distribution.low;
        distribution.acceptable += summary.distribution.acceptable;
        distribution.moderate += summary.distribution.moderate;
        distribution.high += summary.distribution.high;

        for verdict in &block.view.full.functions {
            let ratio = safe_ratio(verdict.scored.crap.value, verdict.threshold);
            let ranked = RankedFunction {
                language: block.language.clone(),
                adapter_display: block.display_name.clone(),
                identity: verdict.scored.identity.clone(),
                crap: verdict.scored.crap.value,
                threshold: verdict.threshold,
                ratio,
                risk_level: verdict.scored.crap.risk_level,
                coverage_percent: verdict.scored.coverage_percent,
                complexity: verdict.scored.complexity,
            };

            // Track the workspace-worst CRAP/threshold ratio. NOT
            // raw CRAP — the dimensional-consistency rule applies
            // here too (a Rust CRAP=20 and a TS CRAP=20 are not
            // comparable, but ratios over their own thresholds
            // are).
            match &worst {
                None => {
                    worst = Some(WorstRatio {
                        ratio,
                        language: block.language.clone(),
                        adapter_display: block.display_name.clone(),
                        function_name: verdict.scored.identity.qualified_name.clone(),
                    });
                }
                Some(current) if ratio > current.ratio => {
                    worst = Some(WorstRatio {
                        ratio,
                        language: block.language.clone(),
                        adapter_display: block.display_name.clone(),
                        function_name: verdict.scored.identity.qualified_name.clone(),
                    });
                }
                _ => {}
            }

            ordered.push(ranked);
        }
    }

    ordered.sort_by(rank_function_cmp);

    CombinedSummary {
        total_functions,
        total_exceeding,
        total_files,
        worst_ratio: worst,
        distribution,
        ordered_functions: ordered,
    }
}

/// Comparator for [`RankedFunction`]. Risk level desc, then ratio
/// desc, then qualified name asc, then file path asc (stable
/// deterministic order for identical risk + ratio).
fn rank_function_cmp(a: &RankedFunction, b: &RankedFunction) -> Ordering {
    // Risk level descending: higher rank first.
    let risk_cmp = risk_level_rank(b.risk_level).cmp(&risk_level_rank(a.risk_level));
    if risk_cmp != Ordering::Equal {
        return risk_cmp;
    }
    // Ratio descending: bigger fraction first. f64 may include
    // infinity (safe-divide guard for zero threshold) — guard with
    // partial_cmp + Ordering::Equal fallback so NaN doesn't crash
    // the sort.
    let ratio_cmp = b.ratio.partial_cmp(&a.ratio).unwrap_or(Ordering::Equal);
    if ratio_cmp != Ordering::Equal {
        return ratio_cmp;
    }
    // Stable tie-break by qualified name + file path.
    let name_cmp = a.identity.qualified_name.cmp(&b.identity.qualified_name);
    if name_cmp != Ordering::Equal {
        return name_cmp;
    }
    a.identity.file_path.cmp(&b.identity.file_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        AnalysisResult, AnalysisSummary, ComplexityMetric, CrapScore, FunctionIdentity,
        FunctionVerdict, RiskDistribution, RiskLevel, ScoredFunction, SourceSpan,
    };
    use crate::domain::view::{self, ViewSpec};

    /// Build a `LanguageBlock` from a list of (qualified_name, file_path,
    /// crap, threshold, risk_level) tuples. The supporting
    /// `AnalysisResult` is stored on a `Box` so this test helper can
    /// return owned data; the block borrows it via `&'a`.
    struct Fixture {
        result: AnalysisResult,
    }

    impl Fixture {
        fn new(functions: Vec<(&str, &str, f64, f64, RiskLevel, u32, f64)>) -> Self {
            let verdicts: Vec<FunctionVerdict> = functions
                .into_iter()
                .map(
                    |(name, file, crap, threshold, risk, complexity, coverage)| FunctionVerdict {
                        scored: ScoredFunction {
                            identity: FunctionIdentity {
                                qualified_name: name.to_string(),
                                file_path: file.to_string(),
                                span: SourceSpan::new(1, 1, 0, 0),
                            },
                            complexity,
                            complexity_metric: ComplexityMetric::Cognitive,
                            coverage_percent: coverage,
                            branch_coverage_percent: None,
                            crap: CrapScore {
                                value: crap,
                                risk_level: risk,
                            },
                            contributors: Vec::new(),
                        },
                        threshold,
                        exceeds: crap > threshold,
                        diagnostic: None,
                    },
                )
                .collect();

            // Hand-roll a minimal summary so `compose_multi_lang`'s
            // aggregation sees realistic counts without going through
            // the full `compute_summary` path.
            let total_functions = verdicts.len();
            let exceeding_threshold = verdicts.iter().filter(|v| v.exceeds).count();
            let mut distribution = RiskDistribution::default();
            for v in &verdicts {
                match v.scored.crap.risk_level {
                    RiskLevel::Low => distribution.low += 1,
                    RiskLevel::Acceptable => distribution.acceptable += 1,
                    RiskLevel::Moderate => distribution.moderate += 1,
                    RiskLevel::High => distribution.high += 1,
                }
            }
            let summary = AnalysisSummary {
                total_functions,
                total_files: 1,
                exceeding_threshold,
                average_crap: 0.0,
                median_crap: 0.0,
                max_crap: None,
                worst_function: None,
                distribution,
                ..AnalysisSummary::default()
            };

            let result = AnalysisResult {
                functions: verdicts,
                summary,
                passed: exceeding_threshold == 0,
            };
            Self { result }
        }

        fn block<'a>(
            &'a self,
            tool_name: &str,
            display_name: &str,
            language: &str,
            metric: ComplexityMetric,
            threshold: f64,
        ) -> LanguageBlock<'a> {
            LanguageBlock {
                tool_name: tool_name.to_string(),
                display_name: display_name.to_string(),
                language: language.to_string(),
                metric,
                threshold,
                view: view::apply(&self.result, ViewSpec::default()),
                delta: None,
            }
        }
    }

    #[test]
    fn compose_empty_blocks_produces_empty_context() {
        let ctx = compose_multi_lang(Vec::new());
        assert!(ctx.languages.is_empty());
        assert_eq!(ctx.combined.total_functions, 0);
        assert_eq!(ctx.combined.total_exceeding, 0);
        assert_eq!(ctx.combined.total_files, 0);
        assert!(ctx.combined.worst_ratio.is_none());
        assert!(ctx.combined.ordered_functions.is_empty());
    }

    #[test]
    fn compose_single_block_passes_through_counts() {
        let fx = Fixture::new(vec![
            ("a::f1", "src/a.rs", 5.0, 8.0, RiskLevel::Low, 3, 80.0),
            ("a::f2", "src/a.rs", 12.0, 8.0, RiskLevel::Moderate, 7, 50.0),
        ]);
        let block = fx.block("crap4rs", "Rust", "rust", ComplexityMetric::Cognitive, 8.0);
        let ctx = compose_multi_lang(vec![block]);
        assert_eq!(ctx.combined.total_functions, 2);
        assert_eq!(ctx.combined.total_exceeding, 1);
        assert_eq!(ctx.combined.ordered_functions.len(), 2);
    }

    #[test]
    fn compose_sorts_by_risk_level_desc_then_ratio_desc() {
        // Rust High-risk @ ratio 5.625, TS Moderate-risk @ ratio 2.5,
        // Rust Low-risk @ ratio 0.5. Expected order: Rust High, TS
        // Moderate, Rust Low — risk band drives outer ordering.
        let fx_rs = Fixture::new(vec![
            (
                "rs::high_fn",
                "src/h.rs",
                45.0,
                8.0,
                RiskLevel::High,
                20,
                30.0,
            ),
            ("rs::low_fn", "src/l.rs", 4.0, 8.0, RiskLevel::Low, 2, 95.0),
        ]);
        let fx_ts = Fixture::new(vec![(
            "ts::moderate_fn",
            "src/m.ts",
            20.0,
            8.0,
            RiskLevel::Moderate,
            10,
            60.0,
        )]);

        let blocks = vec![
            fx_rs.block("crap4rs", "Rust", "rust", ComplexityMetric::Cognitive, 8.0),
            fx_ts.block(
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
        ];
        let ctx = compose_multi_lang(blocks);
        let names: Vec<&str> = ctx
            .combined
            .ordered_functions
            .iter()
            .map(|f| f.identity.qualified_name.as_str())
            .collect();
        assert_eq!(names, vec!["rs::high_fn", "ts::moderate_fn", "rs::low_fn"]);
    }

    #[test]
    fn compose_within_risk_band_sorts_by_ratio_desc() {
        // Two Moderate-risk functions: one at ratio 3.0, one at ratio
        // 1.5. Higher ratio comes first within band.
        let fx = Fixture::new(vec![
            (
                "a::lower_ratio",
                "src/a.rs",
                12.0,
                8.0,
                RiskLevel::Moderate,
                8,
                60.0,
            ),
            (
                "a::higher_ratio",
                "src/b.rs",
                24.0,
                8.0,
                RiskLevel::Moderate,
                10,
                40.0,
            ),
        ]);
        let ctx = compose_multi_lang(vec![fx.block(
            "crap4rs",
            "Rust",
            "rust",
            ComplexityMetric::Cognitive,
            8.0,
        )]);
        let names: Vec<&str> = ctx
            .combined
            .ordered_functions
            .iter()
            .map(|f| f.identity.qualified_name.as_str())
            .collect();
        assert_eq!(names, vec!["a::higher_ratio", "a::lower_ratio"]);
    }

    #[test]
    fn worst_ratio_picks_highest_ratio_across_adapters() {
        let fx_rs = Fixture::new(vec![(
            "rs::worst",
            "src/h.rs",
            45.0,
            8.0,
            RiskLevel::High,
            20,
            30.0,
        )]);
        let fx_ts = Fixture::new(vec![(
            "ts::lesser",
            "src/m.ts",
            20.0,
            8.0,
            RiskLevel::Moderate,
            10,
            60.0,
        )]);
        let ctx = compose_multi_lang(vec![
            fx_rs.block("crap4rs", "Rust", "rust", ComplexityMetric::Cognitive, 8.0),
            fx_ts.block(
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
        ]);
        let worst = ctx.combined.worst_ratio.expect("worst ratio should be set");
        assert_eq!(worst.language, "rust");
        assert_eq!(worst.function_name, "rs::worst");
        assert!((worst.ratio - 5.625).abs() < 1e-9);
    }

    /// R2 explicit test: N-agnostic composition. Constructs 3 synthetic
    /// blocks (a hypothetical Go adapter joins Rust + TypeScript) and
    /// asserts composition works without code changes outside the test.
    /// Documents that adding a new adapter is purely additive.
    #[test]
    fn compose_multi_lang_three_adapters_test() {
        let fx_rs = Fixture::new(vec![(
            "rs::fn",
            "src/a.rs",
            10.0,
            8.0,
            RiskLevel::Moderate,
            5,
            50.0,
        )]);
        let fx_ts = Fixture::new(vec![(
            "ts::fn",
            "src/b.ts",
            12.0,
            8.0,
            RiskLevel::Moderate,
            6,
            40.0,
        )]);
        let fx_go = Fixture::new(vec![(
            "go::fn",
            "src/c.go",
            9.0,
            8.0,
            RiskLevel::Acceptable,
            4,
            70.0,
        )]);

        let blocks = vec![
            fx_rs.block("crap4rs", "Rust", "rust", ComplexityMetric::Cognitive, 8.0),
            fx_ts.block(
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
            fx_go.block("crap4go", "Go", "go", ComplexityMetric::Cyclomatic, 8.0),
        ];
        let ctx = compose_multi_lang(blocks);
        assert_eq!(ctx.languages.len(), 3);
        assert_eq!(ctx.combined.total_functions, 3);
        assert_eq!(ctx.combined.ordered_functions.len(), 3);

        // All three adapter languages should be represented in the
        // ranked list.
        let languages: std::collections::HashSet<&str> = ctx
            .combined
            .ordered_functions
            .iter()
            .map(|f| f.language.as_str())
            .collect();
        assert!(languages.contains("rust"));
        assert!(languages.contains("typescript"));
        assert!(languages.contains("go"));
    }

    #[test]
    fn worst_ratio_handles_zero_threshold_as_infinity() {
        // Pathological config: threshold=0. safe_ratio returns
        // infinity; the worst-ratio tracker treats the result as
        // the highest possible value.
        let fx = Fixture::new(vec![(
            "fn::any",
            "src/x.rs",
            1.0,
            0.0,
            RiskLevel::Low,
            1,
            100.0,
        )]);
        let ctx = compose_multi_lang(vec![fx.block(
            "crap4rs",
            "Rust",
            "rust",
            ComplexityMetric::Cognitive,
            0.0,
        )]);
        let worst = ctx.combined.worst_ratio.expect("worst ratio should be set");
        assert!(worst.ratio.is_infinite());
    }
}
