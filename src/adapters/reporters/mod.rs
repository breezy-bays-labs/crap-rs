//! Output reporters — terminal table and JSON.

pub mod json;
pub mod table;

pub use json::{JsonConfig, format_json};
pub use table::format_table;

#[cfg(test)]
pub(crate) mod test_fixtures {
    use crate::domain::types::{
        AnalysisResult, AnalysisSummary, ComplexityContributor, CrapScore, FunctionIdentity,
        FunctionVerdict, RiskDistribution, RiskLevel, ScoredFunction, SourceSpan,
    };

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
        name: &str,
        file: &str,
        complexity: u32,
        coverage_pct: f64,
        crap_value: f64,
        risk: RiskLevel,
        threshold: f64,
        contributors: Vec<ComplexityContributor>,
    ) -> FunctionVerdict {
        let mut v = make_verdict(
            name,
            file,
            complexity,
            coverage_pct,
            crap_value,
            risk,
            threshold,
        );
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

        AnalysisResult {
            functions: vec![v1, v2, v3],
            summary: AnalysisSummary {
                total_functions: 3,
                total_files: 3,
                exceeding_threshold: 2,
                average_crap: 21.07,
                median_crap: 15.0,
                max_crap: Some(CrapScore {
                    value: 45.2,
                    risk_level: RiskLevel::High,
                }),
                worst_function: Some(FunctionIdentity {
                    file_path: "src/domain/crap.rs".to_string(),
                    qualified_name: "complex_fn".to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                    },
                }),
                distribution: RiskDistribution {
                    low: 1,
                    acceptable: 0,
                    moderate: 1,
                    high: 1,
                },
            },
            passed: false,
        }
    }
}
