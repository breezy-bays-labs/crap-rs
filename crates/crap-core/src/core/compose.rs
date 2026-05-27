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

use crate::domain::delta::FunctionChange;
use crate::domain::multi_lang::{
    CombinedDelta, CombinedDeltaSummary, CombinedSummary, DeltaRowSnapshot, LanguageBlock,
    MultiLangContext, RankedDeltaKind, RankedDeltaRow, RankedFunction, WorstRatio, risk_level_rank,
    safe_ratio,
};
use crate::domain::types::{FunctionVerdict, RiskDistribution};
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

/// Compose a cross-adapter Combined Delta aggregate, or `None` when
/// no language supplied a baseline.
///
/// Returns `None` precisely when every block's `delta` field is
/// `None` — the renderer uses that signal to suppress the View axis
/// nav entirely (no language has a Delta to show).
///
/// When at least one language contributed a baseline, this returns
/// `Some(CombinedDelta)` carrying:
/// - Summed change counts across contributing languages
/// - Display labels listing which languages contributed AND which
///   are missing a baseline (so the renderer can surface the
///   asymmetry to reviewers)
/// - A workspace-wide ranked list of regressions + new functions,
///   sorted by risk band desc then CRAP/threshold ratio desc within
///   band (same dimensional-consistency rule as the Current-view
///   Combined ranking)
///
/// Improvements are intentionally not surfaced in the ranked list —
/// the Combined Delta affordance is regression-focused, matching the
/// per-language delta reporter's separate-improvements-table
/// arrangement at the chat1.md trim.
pub fn compose_combined_delta(blocks: &[LanguageBlock<'_>]) -> Option<CombinedDelta> {
    // Detect the no-baseline case up-front so we can short-circuit
    // without allocating any intermediate state.
    if blocks.iter().all(|b| b.delta.is_none()) {
        return None;
    }

    let mut summary = CombinedDeltaSummary::default();
    let mut contributing_languages: Vec<String> = Vec::new();
    let mut missing_baseline_languages: Vec<String> = Vec::new();
    let mut ordered_rows: Vec<RankedDeltaRow> = Vec::new();

    for block in blocks {
        match block.delta.as_ref() {
            Some(delta_view) => {
                summary.fold(&delta_view.full.summary);
                contributing_languages.push(block.display_name.clone());
                collect_ranked_rows(&mut ordered_rows, block, delta_view);
            }
            None => {
                missing_baseline_languages.push(block.display_name.clone());
            }
        }
    }

    ordered_rows.sort_by(rank_delta_row_cmp);

    Some(CombinedDelta {
        summary,
        contributing_languages,
        missing_baseline_languages,
        ordered_rows,
    })
}

fn collect_ranked_rows(
    out: &mut Vec<RankedDeltaRow>,
    block: &LanguageBlock<'_>,
    delta_view: &crate::domain::delta::DeltaView<'_>,
) {
    // Walk the un-truncated change list so a per-language `--top N`
    // doesn't silently drop rows from the workspace ranking. The
    // Combined Delta table applies its own ranking; per-language
    // truncation is presentational, not authoritative.
    for change in &delta_view.full.changes {
        match change {
            FunctionChange::Modified { baseline, current } => {
                let crap_delta = current.scored.crap.value - baseline.scored.crap.value;
                // Regression threshold matches the per-language
                // reporter's 0.005 cutoff — a smaller delta rounds
                // to "+0.00" in the {:.2} cell output and would
                // look like a false flag.
                if crap_delta >= 0.005 {
                    out.push(make_ranked_row(
                        block,
                        Some(baseline),
                        current,
                        RankedDeltaKind::Regression,
                    ));
                }
            }
            FunctionChange::Added { current } => {
                out.push(make_ranked_row(
                    block,
                    None,
                    current,
                    RankedDeltaKind::NewFunction,
                ));
            }
            FunctionChange::Removed { .. } => {
                // v1 design intentionally drops Removed-zero rows
                // here for the same reason the per-language
                // reporter does at chat1.md — Combined Delta is
                // regression-focused; removed functions don't add
                // risk.
            }
        }
    }
}

fn make_ranked_row(
    block: &LanguageBlock<'_>,
    baseline: Option<&FunctionVerdict>,
    current: &FunctionVerdict,
    kind: RankedDeltaKind,
) -> RankedDeltaRow {
    // Ratio uses the per-function threshold (`current.threshold`)
    // rather than `block.threshold` so per-function overrides are
    // respected. Matches the Current-view's `compute_combined_summary`
    // ranking, which keys off `verdict.threshold` for the same
    // reason: the dimensionally-consistent comparand within a risk
    // band is "how far over this row's own threshold."
    let ratio = safe_ratio(current.scored.crap.value, current.threshold);
    RankedDeltaRow {
        language: block.language.clone(),
        adapter_display: block.display_name.clone(),
        kind,
        baseline: baseline.map(snapshot_from_verdict),
        current: snapshot_from_verdict(current),
        threshold: current.threshold,
        ratio,
    }
}

fn snapshot_from_verdict(verdict: &FunctionVerdict) -> DeltaRowSnapshot {
    DeltaRowSnapshot {
        identity: verdict.scored.identity.clone(),
        crap: verdict.scored.crap.value,
        coverage_percent: verdict.scored.coverage_percent,
        risk_level: verdict.scored.crap.risk_level,
        exceeds: verdict.exceeds,
    }
}

/// Comparator for [`RankedDeltaRow`]. Same rule as the Current-view
/// ranking: risk level desc (per-adapter calibrated), then
/// CRAP/threshold ratio desc (dimensionally consistent within band),
/// then stable tie-break by qualified name asc + file path asc.
fn rank_delta_row_cmp(a: &RankedDeltaRow, b: &RankedDeltaRow) -> Ordering {
    let risk_cmp =
        risk_level_rank(b.current.risk_level).cmp(&risk_level_rank(a.current.risk_level));
    if risk_cmp != Ordering::Equal {
        return risk_cmp;
    }
    let ratio_cmp = b.ratio.partial_cmp(&a.ratio).unwrap_or(Ordering::Equal);
    if ratio_cmp != Ordering::Equal {
        return ratio_cmp;
    }
    let name_cmp = a
        .current
        .identity
        .qualified_name
        .cmp(&b.current.identity.qualified_name);
    if name_cmp != Ordering::Equal {
        return name_cmp;
    }
    a.current
        .identity
        .file_path
        .cmp(&b.current.identity.file_path)
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
                tool_version: "0.0.0-test".to_string(),
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

    // ── compose_combined_delta tests (View axis cross-adapter delta) ──

    use crate::domain::delta::{self, DeltaViewSpec};

    /// Build a `LanguageBlock` from a baseline + current pair. The
    /// owned `AnalysisResult` for current is held on the holder; the
    /// `DeltaView` borrows from an `AnalysisDelta` we construct in
    /// the `block` accessor.
    struct DeltaFixture {
        baseline: AnalysisResult,
        current: AnalysisResult,
    }

    impl DeltaFixture {
        fn from_fixtures(baseline: Fixture, current: Fixture) -> Self {
            Self {
                baseline: baseline.result,
                current: current.result,
            }
        }

        fn block_with_baseline<'a>(
            &'a self,
            holder: &'a mut Option<crate::domain::delta::AnalysisDelta>,
            tool_name: &str,
            display_name: &str,
            language: &str,
            metric: ComplexityMetric,
            threshold: f64,
        ) -> LanguageBlock<'a> {
            *holder = Some(delta::compute(self.baseline.clone(), self.current.clone()));
            let analysis_delta = holder.as_ref().expect("just populated");
            LanguageBlock {
                tool_name: tool_name.to_string(),
                display_name: display_name.to_string(),
                language: language.to_string(),
                tool_version: "0.0.0-test".to_string(),
                metric,
                threshold,
                view: view::apply(&self.current, ViewSpec::default()),
                delta: Some(delta::apply(analysis_delta, DeltaViewSpec::default())),
            }
        }
    }

    /// Verifies the no-baseline short-circuit. Composition should
    /// return `None` cheaply so the renderer can suppress the View
    /// axis nav entirely.
    #[test]
    fn compose_combined_delta_returns_none_when_no_block_has_baseline() {
        let fx = Fixture::new(vec![(
            "fn::any",
            "src/x.rs",
            5.0,
            8.0,
            RiskLevel::Low,
            3,
            80.0,
        )]);
        let block = fx.block("crap4rs", "Rust", "rust", ComplexityMetric::Cognitive, 8.0);
        assert!(compose_combined_delta(&[block]).is_none());
    }

    /// Mismatched-baseline scenario: only one language supplied a
    /// baseline. The aggregate must surface it as contributing and
    /// list the other language under `missing_baseline_languages` so
    /// the renderer can paint the disabled Delta tab + scope-banner
    /// asymmetry note.
    #[test]
    fn compose_combined_delta_marks_missing_baselines() {
        let baseline_rs = Fixture::new(vec![(
            "rs::a",
            "src/a.rs",
            4.0,
            8.0,
            RiskLevel::Low,
            3,
            80.0,
        )]);
        let current_rs = Fixture::new(vec![(
            "rs::a",
            "src/a.rs",
            6.0,
            8.0,
            RiskLevel::Acceptable,
            5,
            70.0,
        )]);
        let rs_dfx = DeltaFixture::from_fixtures(baseline_rs, current_rs);
        let mut rs_delta = None;
        let rs_block = rs_dfx.block_with_baseline(
            &mut rs_delta,
            "crap4rs",
            "Rust",
            "rust",
            ComplexityMetric::Cognitive,
            8.0,
        );

        let ts_fx = Fixture::new(vec![(
            "ts::b",
            "src/b.ts",
            3.0,
            8.0,
            RiskLevel::Low,
            3,
            90.0,
        )]);
        let ts_block = ts_fx.block(
            "crap4ts",
            "TypeScript",
            "typescript",
            ComplexityMetric::Cyclomatic,
            8.0,
        );

        let combined = compose_combined_delta(&[rs_block, ts_block])
            .expect("at least one language has a baseline");
        assert_eq!(
            combined.contributing_languages,
            vec!["Rust".to_string()],
            "Only Rust contributed a baseline"
        );
        assert_eq!(
            combined.missing_baseline_languages,
            vec!["TypeScript".to_string()],
            "TypeScript has no baseline; renderer paints disabled Delta tab on its panel"
        );
    }

    /// Cross-adapter regression ranking: a Rust High-risk regression
    /// must rank ahead of a TypeScript Moderate-risk regression, per
    /// the dimensional-consistency rule (risk band desc, then
    /// ratio).
    #[test]
    fn compose_combined_delta_ranks_high_risk_before_moderate_across_adapters() {
        let baseline_rs = Fixture::new(vec![(
            "rs::scary_fn",
            "src/h.rs",
            10.0,
            8.0,
            RiskLevel::Acceptable,
            10,
            60.0,
        )]);
        let current_rs = Fixture::new(vec![(
            "rs::scary_fn",
            "src/h.rs",
            45.6,
            8.0,
            RiskLevel::High,
            20,
            30.0,
        )]);
        let rs_dfx = DeltaFixture::from_fixtures(baseline_rs, current_rs);

        let baseline_ts = Fixture::new(vec![(
            "ts::moderate_change",
            "src/m.ts",
            14.0,
            8.0,
            RiskLevel::Moderate,
            8,
            65.0,
        )]);
        let current_ts = Fixture::new(vec![(
            "ts::moderate_change",
            "src/m.ts",
            20.0,
            8.0,
            RiskLevel::Moderate,
            10,
            60.0,
        )]);
        let ts_dfx = DeltaFixture::from_fixtures(baseline_ts, current_ts);

        let mut rs_delta = None;
        let mut ts_delta = None;
        let blocks = vec![
            rs_dfx.block_with_baseline(
                &mut rs_delta,
                "crap4rs",
                "Rust",
                "rust",
                ComplexityMetric::Cognitive,
                8.0,
            ),
            ts_dfx.block_with_baseline(
                &mut ts_delta,
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
        ];

        let combined =
            compose_combined_delta(&blocks).expect("both languages contributed baselines");
        let names: Vec<&str> = combined
            .ordered_rows
            .iter()
            .map(|r| r.current.identity.qualified_name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["rs::scary_fn", "ts::moderate_change"],
            "High-risk Rust regression must rank ahead of Moderate-risk TS regression"
        );
        // Per-row kind identifies regressions.
        for row in &combined.ordered_rows {
            assert_eq!(row.kind, RankedDeltaKind::Regression);
        }
        // Aggregate summary AND-folds passed across contributing
        // blocks. Neither baseline → current crossed the threshold
        // (both stayed below or scored as regressions but didn't
        // produce a new-violation count of 1), so the fold reflects
        // each block's `DeltaSummary.passed`.
        assert_eq!(combined.summary.regressions, 2);
        assert_eq!(combined.summary.new_violations, 0);
    }

    /// Within-band tie-break: two Moderate-risk regressions from
    /// different adapters must order by CRAP/threshold ratio desc.
    /// Higher ratio first.
    #[test]
    fn compose_combined_delta_orders_within_band_by_ratio_desc() {
        let baseline_rs = Fixture::new(vec![(
            "rs::change",
            "src/r.rs",
            10.0,
            8.0,
            RiskLevel::Acceptable,
            5,
            70.0,
        )]);
        let current_rs = Fixture::new(vec![(
            "rs::change",
            "src/r.rs",
            18.0,
            8.0,
            RiskLevel::Moderate,
            8,
            55.0,
        )]);
        let rs_dfx = DeltaFixture::from_fixtures(baseline_rs, current_rs);
        // ratio_rs = 18.0 / 8.0 = 2.25

        let baseline_ts = Fixture::new(vec![(
            "ts::change",
            "src/t.ts",
            10.0,
            8.0,
            RiskLevel::Acceptable,
            5,
            70.0,
        )]);
        let current_ts = Fixture::new(vec![(
            "ts::change",
            "src/t.ts",
            24.0,
            8.0,
            RiskLevel::Moderate,
            10,
            45.0,
        )]);
        let ts_dfx = DeltaFixture::from_fixtures(baseline_ts, current_ts);
        // ratio_ts = 24.0 / 8.0 = 3.00 — higher ratio than rs

        let mut rs_delta = None;
        let mut ts_delta = None;
        let blocks = vec![
            rs_dfx.block_with_baseline(
                &mut rs_delta,
                "crap4rs",
                "Rust",
                "rust",
                ComplexityMetric::Cognitive,
                8.0,
            ),
            ts_dfx.block_with_baseline(
                &mut ts_delta,
                "crap4ts",
                "TypeScript",
                "typescript",
                ComplexityMetric::Cyclomatic,
                8.0,
            ),
        ];

        let combined = compose_combined_delta(&blocks).expect("both contributed baselines");
        let names: Vec<&str> = combined
            .ordered_rows
            .iter()
            .map(|r| r.current.identity.qualified_name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["ts::change", "rs::change"],
            "Within Moderate band, TS row at ratio 3.0 ranks ahead of RS row at ratio 2.25"
        );
    }

    /// Added (new) functions surface in the ranked list with the
    /// NewFunction kind, alongside regressions. Improvements are
    /// intentionally suppressed — Combined Delta is
    /// regression-focused.
    #[test]
    fn compose_combined_delta_includes_added_and_excludes_improvements() {
        let baseline = Fixture::new(vec![
            (
                "fn::regressing",
                "src/r.rs",
                10.0,
                8.0,
                RiskLevel::Acceptable,
                6,
                70.0,
            ),
            (
                "fn::improving",
                "src/i.rs",
                14.0,
                8.0,
                RiskLevel::Moderate,
                8,
                50.0,
            ),
        ]);
        let current = Fixture::new(vec![
            (
                "fn::regressing",
                "src/r.rs",
                20.0,
                8.0,
                RiskLevel::Moderate,
                10,
                50.0,
            ),
            (
                "fn::improving",
                "src/i.rs",
                6.0,
                8.0,
                RiskLevel::Acceptable,
                4,
                85.0,
            ),
            (
                "fn::brand_new",
                "src/n.rs",
                12.0,
                8.0,
                RiskLevel::Moderate,
                7,
                55.0,
            ),
        ]);
        let dfx = DeltaFixture::from_fixtures(baseline, current);
        let mut delta_holder = None;
        let block = dfx.block_with_baseline(
            &mut delta_holder,
            "crap4rs",
            "Rust",
            "rust",
            ComplexityMetric::Cognitive,
            8.0,
        );

        let combined = compose_combined_delta(&[block]).expect("Rust block contributed a baseline");
        let kinds: Vec<(&str, RankedDeltaKind)> = combined
            .ordered_rows
            .iter()
            .map(|r| (r.current.identity.qualified_name.as_str(), r.kind))
            .collect();
        // Regression + Added surface; improvement is excluded.
        assert!(
            kinds.contains(&("fn::regressing", RankedDeltaKind::Regression)),
            "regression must surface"
        );
        assert!(
            kinds.contains(&("fn::brand_new", RankedDeltaKind::NewFunction)),
            "added (new) function must surface"
        );
        assert!(
            !kinds.iter().any(|(n, _)| *n == "fn::improving"),
            "improvement must NOT surface — Combined Delta is regression-focused"
        );
    }
}
