use serde::Serialize;
use std::fmt;

// ── Source Location ──────────────────────────────────────────────────

/// Line range in source coordinates.
/// `start_line` is 1-based inclusive, `end_line` is inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SourceSpan {
    pub start_line: usize,
    pub end_line: usize,
}

// ── Complexity Metric ────────────────────────────────────────────────

/// Which complexity metric to use for CRAP score computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ComplexityMetric {
    /// Counts nesting depth + structural complexity (default for Rust).
    #[default]
    Cognitive,
    /// Counts decision points (branches). Classic CRAP metric.
    Cyclomatic,
}

impl fmt::Display for ComplexityMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cognitive => write!(f, "cognitive"),
            Self::Cyclomatic => write!(f, "cyclomatic"),
        }
    }
}

// ── Function Identity & Metrics ─────────────────────────────────────

/// Identifies a function in the source code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FunctionIdentity {
    /// Project-relative file path, forward-slash normalized.
    pub file_path: String,
    /// Qualified name: `Module::Type::method` or `module::function`.
    pub qualified_name: String,
    /// Source location.
    pub span: SourceSpan,
}

/// Complexity data extracted from source code for a single function.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionComplexity {
    pub identity: FunctionIdentity,
    /// Complexity value (>= 1). Interpretation depends on the metric used.
    pub complexity: u32,
    /// Which metric produced this value.
    pub metric: ComplexityMetric,
}

/// Coverage ratio for a function.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CoverageRatio {
    pub covered: usize,
    pub total: usize,
    pub percent: f64,
}

/// Line-level coverage data parsed from LCOV DA entries.
#[derive(Debug, Clone)]
pub struct LineCoverage {
    pub line: usize,
    pub hits: u64,
}

/// Per-function coverage data parsed from LCOV.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionCoverage {
    pub file_path: String,
    pub span: SourceSpan,
    pub line_coverage: CoverageRatio,
}

// ── CRAP Scoring ────────────────────────────────────────────────────

/// Risk classification based on CRAP score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Acceptable,
    Moderate,
    High,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Acceptable => write!(f, "acceptable"),
            Self::Moderate => write!(f, "moderate"),
            Self::High => write!(f, "high"),
        }
    }
}

/// Computed CRAP score with risk classification.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CrapScore {
    /// Rounded to 2 decimal places.
    pub value: f64,
    pub risk_level: RiskLevel,
}

/// A function with all metrics computed.
#[derive(Debug, Clone, Serialize)]
pub struct ScoredFunction {
    pub identity: FunctionIdentity,
    pub complexity: u32,
    pub complexity_metric: ComplexityMetric,
    pub coverage_percent: f64,
    pub crap: CrapScore,
}

/// A scored function compared against a threshold.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionVerdict {
    pub scored: ScoredFunction,
    pub threshold: f64,
    pub exceeds: bool,
}

// ── Analysis Results ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RiskDistribution {
    pub low: usize,
    pub acceptable: usize,
    pub moderate: usize,
    pub high: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisSummary {
    pub total_functions: usize,
    pub total_files: usize,
    pub exceeding_threshold: usize,
    pub average_crap: f64,
    pub median_crap: f64,
    pub max_crap: Option<CrapScore>,
    pub worst_function: Option<FunctionIdentity>,
    pub distribution: RiskDistribution,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    pub functions: Vec<FunctionVerdict>,
    pub summary: AnalysisSummary,
    pub passed: bool,
}

// ── Parse Diagnostics ──────────────────────────────────────────────

/// Non-fatal issues encountered during coverage parsing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParseDiagnostic {
    /// A DA record could not be parsed (bad format, missing fields, invalid values).
    MalformedRecord {
        /// 1-based line number in the LCOV input where the issue occurred.
        line_number: usize,
        /// The raw line content that failed to parse.
        content: String,
    },
    /// An SF record had an empty path.
    EmptySourceFile {
        /// The 1-based line number in the LCOV input.
        line_number: usize,
    },
}

impl fmt::Display for ParseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedRecord {
                line_number,
                content,
            } => write!(f, "line {line_number}: malformed record: {content}"),
            Self::EmptySourceFile { line_number } => {
                write!(f, "line {line_number}: empty SF path")
            }
        }
    }
}

/// Statistics about the analysis process, surfaced by `--verbose`.
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisDiagnostics {
    /// Non-fatal parse issues from the LCOV coverage file.
    pub parse_diagnostics: Vec<ParseDiagnostic>,
    /// Number of source files discovered.
    pub files_found: usize,
    /// Number of source files that failed to parse (skipped).
    pub files_unparseable: usize,
    /// Total functions extracted from source files.
    pub functions_extracted: usize,
    /// Functions matched with coverage data.
    pub functions_matched: usize,
    /// Functions with no LCOV data (0% coverage assumed).
    pub functions_no_coverage: usize,
}

// ── Errors ──────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum CrapError {
    #[error("Invalid complexity value: {0}. Must be >= 1 and finite.")]
    InvalidComplexity(u32),

    #[error("Invalid coverage percentage: {0}. Must be finite.")]
    InvalidCoverage(f64),

    #[error("Failed to parse LCOV data: {0}")]
    LcovParse(String),

    #[error("Failed to parse source file: {0}")]
    SourceParse(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diagnostic_display_malformed_record() {
        let d = ParseDiagnostic::MalformedRecord {
            line_number: 42,
            content: "DA:bad".to_string(),
        };
        assert_eq!(d.to_string(), "line 42: malformed record: DA:bad");
    }

    #[test]
    fn parse_diagnostic_display_empty_source_file() {
        let d = ParseDiagnostic::EmptySourceFile { line_number: 7 };
        assert_eq!(d.to_string(), "line 7: empty SF path");
    }
}
