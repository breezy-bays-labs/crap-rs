use serde::{Deserialize, Serialize};
use std::fmt;

// ── Source Location ──────────────────────────────────────────────────

/// Line range in source coordinates.
/// `start_line` is 1-based inclusive, `end_line` is inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_line: usize,
    pub end_line: usize,
}

// ── Complexity Contributors ──────────────────────────────────────────

/// Identifies the kind of construct that contributed to a complexity score.
///
/// Universal variants apply to all languages. Rust-specific variants are
/// only emitted by the Rust adapter. TypeScript-specific variants are
/// reserved for the future and never emitted by the Rust adapter.
///
/// `#[non_exhaustive]` ensures forward-compatibility as new languages
/// are added to the unified crap monorepo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ContributorKind {
    // Universal
    IfBranch,
    ForLoop,
    WhileLoop,
    DoWhileLoop,
    Catch,
    LogicalOperator,
    // Rust-specific
    Match,
    MatchArm,
    Try,
    LetElse,
    Loop,
    Break,
    Continue,
    /// Intentionally not emitted — unsafe blocks are not yet counted.
    Unsafe,
    // TypeScript-specific (never emitted by Rust adapter)
    Switch,
    CaseBranch,
    Ternary,
    OptionalChain,
}

impl fmt::Display for ContributorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::IfBranch => "if-branch",
            Self::ForLoop => "for-loop",
            Self::WhileLoop => "while-loop",
            Self::DoWhileLoop => "do-while-loop",
            Self::Catch => "catch",
            Self::LogicalOperator => "logical-operator",
            Self::Match => "match",
            Self::MatchArm => "match-arm",
            Self::Try => "try",
            Self::LetElse => "let-else",
            Self::Loop => "loop",
            Self::Break => "break",
            Self::Continue => "continue",
            // Intentionally not emitted — unsafe blocks not yet counted.
            Self::Unsafe => "unsafe",
            Self::Switch => "switch",
            Self::CaseBranch => "case-branch",
            Self::Ternary => "ternary",
            Self::OptionalChain => "optional-chain",
        };
        f.write_str(s)
    }
}

/// A single construct that contributed to a function's complexity score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplexityContributor {
    pub kind: ContributorKind,
    /// 1-based line number of the construct.
    pub line: usize,
    /// 0-based column offset from syn Span, if available.
    pub column: Option<u32>,
    /// How much this contributor added to the total.
    /// Cognitive: `1 + nesting_depth`. Cyclomatic: always 1.
    pub increment: u32,
}

// ── Complexity Metric ────────────────────────────────────────────────

/// Which complexity metric to use for CRAP score computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Individual constructs that contributed to the complexity score.
    /// Sorted by (line, column). Empty when complexity == 1.
    pub contributors: Vec<ComplexityContributor>,
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

/// Branch-level coverage data parsed from LCOV BRDA entries.
/// Language-agnostic: only line position and execution count.
/// Format-specific identifiers (block, branch IDs) stay in the adapter.
#[derive(Debug, Clone)]
pub struct BranchCoverage {
    pub line: usize,
    pub taken: Option<u64>,
}

// ── Coverage Metric ─────────────────────────────────────────────────

/// Which coverage metric to use for analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CoverageMetric {
    /// Line-level coverage from DA records (default).
    #[default]
    Line,
    /// Branch-level coverage from BRDA records.
    Branch,
}

impl fmt::Display for CoverageMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Line => write!(f, "line"),
            Self::Branch => write!(f, "branch"),
        }
    }
}

/// Per-function coverage data parsed from LCOV.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionCoverage {
    pub file_path: String,
    pub span: SourceSpan,
    pub line_coverage: CoverageRatio,
    /// Branch coverage ratio within this function's span.
    /// `None` means no branch points exist in the span (not the same as zero).
    pub branch_coverage: Option<CoverageRatio>,
}

// ── CRAP Scoring ────────────────────────────────────────────────────

/// Risk classification based on CRAP score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CrapScore {
    /// Rounded to 2 decimal places.
    pub value: f64,
    pub risk_level: RiskLevel,
}

/// A function with all metrics computed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredFunction {
    pub identity: FunctionIdentity,
    pub complexity: u32,
    pub complexity_metric: ComplexityMetric,
    pub coverage_percent: f64,
    pub crap: CrapScore,
    /// Individual constructs that contributed to the complexity score.
    /// Always present; empty when complexity == 1.
    pub contributors: Vec<ComplexityContributor>,
}

/// A scored function compared against a threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionVerdict {
    pub scored: ScoredFunction,
    pub threshold: f64,
    pub exceeds: bool,
}

// ── Analysis Results ────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RiskDistribution {
    pub low: usize,
    pub acceptable: usize,
    pub moderate: usize,
    pub high: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_functions: usize,
    pub total_files: usize,
    pub exceeding_threshold: usize,
    pub average_crap: f64,
    pub median_crap: f64,
    pub max_crap: Option<CrapScore>,
    pub worst_function: Option<FunctionIdentity>,
    pub distribution: RiskDistribution,
    /// Highest complexity across analyzed functions. `0` when input is empty.
    #[serde(default)]
    pub max_complexity: u32,
    /// Mean complexity across analyzed functions. `0.0` when input is empty.
    #[serde(default)]
    pub average_complexity: f64,
    /// Median complexity across analyzed functions. `0.0` when input is empty.
    #[serde(default)]
    pub median_complexity: f64,
    /// Minimum of finite `coverage_percent` values. `0.0` when no finite
    /// values exist (NaN-only input or empty).
    #[serde(default)]
    pub min_coverage: f64,
    /// Mean of finite `coverage_percent` values; NaN inputs excluded from
    /// both numerator and denominator. `0.0` when no finite values exist.
    #[serde(default)]
    pub average_coverage: f64,
    /// Median of finite `coverage_percent` values. `0.0` when no finite
    /// values exist.
    #[serde(default)]
    pub median_coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub functions: Vec<FunctionVerdict>,
    pub summary: AnalysisSummary,
    pub passed: bool,
}

// ── Parse Diagnostics ──────────────────────────────────────────────

/// Non-fatal issues encountered during coverage parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Files with at least one analyzed function.
    pub files_analyzed: usize,
    /// Files where every analyzed function has 0% line coverage.
    pub files_zero_coverage: usize,
}

// ── Diff Types ─────────────────────────────────────────────────────

/// Describes how a file changed relative to a diff ref.
///
/// Files absent from the change map are unchanged and should be excluded.
/// Invariant: `Modified` always contains at least one span — deletion-only
/// hunks (zero new lines) are filtered out by the adapter, so a file with
/// only deletions never appears in the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChangeKind {
    /// Entirely new file — include all functions.
    NewFile,
    /// Modified file — include functions overlapping these spans.
    Modified(Vec<SourceSpan>),
}

// ── Errors ──────────────────────────────────────────────────────────

#[cfg(test)]
mod contributor_tests {
    use super::*;

    #[test]
    fn contributor_kind_serializes_as_kebab_case() {
        assert_eq!(
            serde_json::to_string(&ContributorKind::IfBranch).unwrap(),
            "\"if-branch\""
        );
        assert_eq!(
            serde_json::to_string(&ContributorKind::ForLoop).unwrap(),
            "\"for-loop\""
        );
        assert_eq!(
            serde_json::to_string(&ContributorKind::MatchArm).unwrap(),
            "\"match-arm\""
        );
        assert_eq!(
            serde_json::to_string(&ContributorKind::LogicalOperator).unwrap(),
            "\"logical-operator\""
        );
        assert_eq!(
            serde_json::to_string(&ContributorKind::LetElse).unwrap(),
            "\"let-else\""
        );
        assert_eq!(
            serde_json::to_string(&ContributorKind::WhileLoop).unwrap(),
            "\"while-loop\""
        );
        assert_eq!(
            serde_json::to_string(&ContributorKind::DoWhileLoop).unwrap(),
            "\"do-while-loop\""
        );
        assert_eq!(
            serde_json::to_string(&ContributorKind::CaseBranch).unwrap(),
            "\"case-branch\""
        );
        assert_eq!(
            serde_json::to_string(&ContributorKind::OptionalChain).unwrap(),
            "\"optional-chain\""
        );
    }

    #[test]
    fn contributor_kind_display_matches_serde() {
        // Display output must equal the JSON string (sans quotes)
        assert_eq!(ContributorKind::IfBranch.to_string(), "if-branch");
        assert_eq!(ContributorKind::ForLoop.to_string(), "for-loop");
        assert_eq!(ContributorKind::DoWhileLoop.to_string(), "do-while-loop");
        assert_eq!(
            ContributorKind::LogicalOperator.to_string(),
            "logical-operator"
        );
        assert_eq!(ContributorKind::MatchArm.to_string(), "match-arm");
        assert_eq!(ContributorKind::LetElse.to_string(), "let-else");
        assert_eq!(ContributorKind::OptionalChain.to_string(), "optional-chain");
        assert_eq!(ContributorKind::CaseBranch.to_string(), "case-branch");
    }

    #[test]
    fn complexity_contributor_fields_accessible() {
        let c = ComplexityContributor {
            kind: ContributorKind::Match,
            line: 42,
            column: Some(4),
            increment: 2,
        };
        assert_eq!(c.kind, ContributorKind::Match);
        assert_eq!(c.line, 42);
        assert_eq!(c.column, Some(4));
        assert_eq!(c.increment, 2);
    }

    #[test]
    fn complexity_contributor_no_column() {
        let c = ComplexityContributor {
            kind: ContributorKind::Break,
            line: 10,
            column: None,
            increment: 1,
        };
        assert!(c.column.is_none());
        assert_eq!(c.increment, 1);
    }
}

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

    #[error("Failed to compute diff: {0}")]
    DiffCompute(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_diagnostics_has_zero_coverage_fields() {
        let diag = AnalysisDiagnostics {
            parse_diagnostics: vec![],
            files_found: 10,
            files_unparseable: 0,
            functions_extracted: 20,
            functions_matched: 18,
            functions_no_coverage: 2,
            files_analyzed: 8,
            files_zero_coverage: 3,
        };
        assert_eq!(diag.files_analyzed, 8);
        assert_eq!(diag.files_zero_coverage, 3);
    }

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

    #[test]
    fn coverage_metric_display() {
        assert_eq!(CoverageMetric::Line.to_string(), "line");
        assert_eq!(CoverageMetric::Branch.to_string(), "branch");
    }

    #[test]
    fn coverage_metric_default_is_line() {
        assert_eq!(CoverageMetric::default(), CoverageMetric::Line);
    }
}
