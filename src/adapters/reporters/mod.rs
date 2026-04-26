//! Output reporters — terminal table and JSON.

pub mod csv;
pub mod json;
pub mod markdown;
pub mod table;

pub use csv::format_csv;
pub use json::{JsonConfig, format_json};
pub use markdown::format_markdown;
pub use table::{format_table, format_table_with_explain};

#[cfg(test)]
pub(crate) mod test_fixtures {
    use crate::domain::delta::{self, AnalysisDelta, DeltaView, DeltaViewSpec};
    use crate::domain::types::{
        AnalysisResult, AnalysisSummary, ComplexityContributor, CrapScore, FunctionIdentity,
        FunctionVerdict, RiskDistribution, RiskLevel, ScoredFunction, SourceSpan,
    };
    use crate::domain::view::{self, AnalysisView, ViewSpec};

    /// Build a default-spec view from the given analysis. Convenience for
    /// reporter tests that just want to assert on the default-spec output
    /// (the V1a walking-skeleton invariant).
    pub fn make_view_default(result: &AnalysisResult) -> AnalysisView<'_> {
        view::apply(result, ViewSpec::default())
    }

    /// Build a default-spec delta view. Mirrors `make_view_default` for
    /// the delta sibling pipeline.
    pub fn make_delta_view_default(delta: &AnalysisDelta) -> DeltaView<'_> {
        delta::apply(delta, DeltaViewSpec::default())
    }

    /// Sample delta covering all three change kinds plus a regression
    /// and a new violation. Used by reporter snapshot tests.
    ///
    /// Baseline → current edits:
    /// - `simple_fn` (low) → unchanged identity, score 3.0 → 3.0 (Modified, zero delta)
    /// - `parse_record` (moderate) → score 15.0 → 22.0 (Modified, regression)
    /// - `complex_fn` (high) baseline only (Removed)
    /// - `new_fn` (current only) score 30.0, exceeds threshold → Added + new violation
    pub fn make_sample_delta() -> AnalysisDelta {
        let baseline = {
            let mut r = make_multi_function_result();
            // Drop complex_fn from current, but keep in baseline; alter parse_record CRAP
            r.functions[1].scored.crap.value = 15.0;
            r
        };
        let current = {
            let v_simple =
                make_verdict("simple_fn", "src/lib.rs", 2, 95.0, 3.0, RiskLevel::Low, 8.0);
            let mut v_parse = make_verdict(
                "parse_record",
                "src/adapters/coverage/mod.rs",
                6,
                60.0,
                22.0,
                RiskLevel::High,
                8.0,
            );
            v_parse.exceeds = true;
            let mut v_new = make_verdict(
                "new_fn",
                "src/adapters/baseline.rs",
                10,
                40.0,
                30.0,
                RiskLevel::High,
                8.0,
            );
            v_new.exceeds = true;
            AnalysisResult {
                functions: vec![v_simple, v_parse, v_new],
                summary: AnalysisSummary {
                    total_functions: 3,
                    total_files: 3,
                    exceeding_threshold: 2,
                    average_crap: 18.33,
                    median_crap: 22.0,
                    max_crap: Some(CrapScore {
                        value: 30.0,
                        risk_level: RiskLevel::High,
                    }),
                    worst_function: Some(FunctionIdentity {
                        file_path: "src/adapters/baseline.rs".to_string(),
                        qualified_name: "new_fn".to_string(),
                        span: SourceSpan {
                            start_line: 1,
                            end_line: 10,
                        },
                    }),
                    distribution: RiskDistribution {
                        low: 1,
                        acceptable: 0,
                        moderate: 0,
                        high: 2,
                    },
                    ..Default::default()
                },
                passed: false,
            }
        };
        delta::compute(baseline, current)
    }

    pub fn make_verdict(
        name: &str,
        file: &str,
        complexity: u32,
        coverage_pct: f64,
        crap_value: f64,
        risk: RiskLevel,
        threshold: f64,
    ) -> FunctionVerdict {
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
                complexity,
                complexity_metric: crate::domain::types::ComplexityMetric::Cognitive,
                coverage_percent: coverage_pct,
                crap: CrapScore {
                    value: crap_value,
                    risk_level: risk,
                },
                contributors: vec![],
            },
            threshold,
            exceeds: crap_value > threshold,
        }
    }

    pub fn make_verdict_with_contributors(
        verdict: FunctionVerdict,
        contributors: Vec<ComplexityContributor>,
    ) -> FunctionVerdict {
        let mut v = verdict;
        v.scored.contributors = contributors;
        v
    }

    pub fn make_empty_result() -> AnalysisResult {
        AnalysisResult {
            functions: vec![],
            summary: AnalysisSummary {
                total_functions: 0,
                total_files: 0,
                exceeding_threshold: 0,
                average_crap: 0.0,
                median_crap: 0.0,
                max_crap: None,
                worst_function: None,
                distribution: RiskDistribution {
                    low: 0,
                    acceptable: 0,
                    moderate: 0,
                    high: 0,
                },
                ..Default::default()
            },
            passed: true,
        }
    }

    pub fn make_single_function_result(
        name: &str,
        file: &str,
        complexity: u32,
        coverage_pct: f64,
        crap_value: f64,
        risk: RiskLevel,
        threshold: f64,
    ) -> AnalysisResult {
        let verdict = make_verdict(
            name,
            file,
            complexity,
            coverage_pct,
            crap_value,
            risk,
            threshold,
        );
        let exceeds = verdict.exceeds;
        AnalysisResult {
            functions: vec![verdict],
            summary: AnalysisSummary {
                total_functions: 1,
                total_files: 1,
                exceeding_threshold: if exceeds { 1 } else { 0 },
                average_crap: crap_value,
                median_crap: crap_value,
                max_crap: Some(CrapScore {
                    value: crap_value,
                    risk_level: risk,
                }),
                worst_function: Some(FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                    },
                }),
                distribution: RiskDistribution {
                    low: if risk == RiskLevel::Low { 1 } else { 0 },
                    acceptable: if risk == RiskLevel::Acceptable { 1 } else { 0 },
                    moderate: if risk == RiskLevel::Moderate { 1 } else { 0 },
                    high: if risk == RiskLevel::High { 1 } else { 0 },
                },
                ..Default::default()
            },
            passed: !exceeds,
        }
    }

    /// Three functions spanning Low, Moderate, and High risk levels.
    /// Scores: Low=3.0, Moderate=15.0, High=45.2 — threshold 8.0.
    pub fn make_multi_function_result() -> AnalysisResult {
        let v1 = make_verdict("simple_fn", "src/lib.rs", 2, 95.0, 3.0, RiskLevel::Low, 8.0);
        let v2 = make_verdict(
            "parse_record",
            "src/adapters/coverage/mod.rs",
            6,
            72.5,
            15.0,
            RiskLevel::Moderate,
            8.0,
        );
        let v3 = make_verdict(
            "complex_fn",
            "src/domain/crap.rs",
            20,
            30.0,
            45.2,
            RiskLevel::High,
            8.0,
        );

        let functions = vec![v1, v2, v3];
        let summary = crate::domain::summary::compute_summary(&functions);
        AnalysisResult {
            functions,
            summary,
            passed: false,
        }
    }
}
