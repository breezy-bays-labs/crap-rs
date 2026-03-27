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
#[derive(Debug, Clone, PartialEq)]
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
