use serde::{Deserialize, Serialize};
use std::fmt;

// ── Source Location ──────────────────────────────────────────────────

/// Line range in source coordinates.
///
/// `start_line` is 1-based inclusive; `end_line` is 1-based inclusive.
/// `start_column` and `end_column` are 1-based inclusive when known; `0`
/// signals "column unknown" (e.g., diff hunks parse line ranges only and
/// have no column data). Adapters that lack column information must emit
/// `0` rather than fabricating a value, so reporters can decide whether
/// to surface the columns (SARIF emits them only when both are nonzero).
/// SARIF reporters convert inclusive → exclusive end at serialization
/// time; consumers of `SourceSpan` directly get the intuitive bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
// `#[non_exhaustive]` paused for v0.5: cli/core constructors live in
// crap4rs through S3/S4 and need cross-crate struct-literal init.
// Restored at v1.0 once those constructors land in crap-core.
pub struct SourceSpan {
    pub start_line: usize,
    pub end_line: usize,
    #[serde(default)]
    pub start_column: usize,
    #[serde(default)]
    pub end_column: usize,
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

impl ContributorKind {
    /// Canonical wire string for this variant — equal to the serde JSON
    /// representation (sans surrounding quotes). Pinned so reporters,
    /// diff tools, and adapters that build raw payloads bypass the
    /// `serde_json::to_value(...)` round-trip without risking drift.
    /// `tests::wire_str_matches_serde` asserts equality variant-by-variant.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
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
        }
    }
}

impl fmt::Display for ContributorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

/// A single construct that contributed to a function's complexity score.
///
/// `end_line` and `nesting_depth` are populated by walkers that have access
/// to AST structure; deserialization tolerates older payloads via
/// `#[serde(default)]` (missing → `0`). Reporters and helpers that consume
/// these fields treat `end_line == 0` as "use `line` instead", since a
/// real source position is always >= 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// `#[non_exhaustive]` paused for v0.5 (see SourceSpan). Restored at v1.0.
pub struct ComplexityContributor {
    pub kind: ContributorKind,
    /// 1-based line number of the construct's start (or signal token).
    pub line: usize,
    /// 1-based inclusive column of the construct's start, if available.
    /// `None` when the source adapter has no column data (e.g., diff
    /// hunks parse line ranges only). Aligned with `SourceSpan::start_column`
    /// (also 1-based) and SARIF region semantics so consumers correlating
    /// contributor positions with span positions see one convention.
    pub column: Option<u32>,
    /// How much this contributor added to the total.
    /// Cognitive: `1 + nesting_depth`. Cyclomatic: always 1.
    pub increment: u32,
    /// 1-based inclusive end line of the construct. For atomic constructs
    /// (`?`, `break`, `continue`, single logical operator), equals `line`.
    /// For compound constructs (`if`, `match`, `for`, etc.), covers the
    /// full span including bodies.
    #[serde(default)]
    pub end_line: usize,
    /// How deeply this construct is nested under other complexity-bearing
    /// constructs (0 = top-level statement of the function body).
    #[serde(default)]
    pub nesting_depth: u32,
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

impl ComplexityMetric {
    /// Canonical wire string — see `ContributorKind::as_wire_str` for
    /// the rationale. Variant-by-variant equality with serde is pinned
    /// in `tests::wire_str_matches_serde`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Cognitive => "cognitive",
            Self::Cyclomatic => "cyclomatic",
        }
    }
}

impl fmt::Display for ComplexityMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

// ── Function Identity & Metrics ─────────────────────────────────────

/// Identifies a function in the source code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
// `#[non_exhaustive]` paused for v0.5 (see SourceSpan). Restored at v1.0.
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

impl CoverageMetric {
    /// Canonical wire string — see `ContributorKind::as_wire_str`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Branch => "branch",
        }
    }
}

impl fmt::Display for CoverageMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
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

impl RiskLevel {
    /// Canonical wire string — see `ContributorKind::as_wire_str`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Acceptable => "acceptable",
            Self::Moderate => "moderate",
            Self::High => "high",
        }
    }
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

/// Computed CRAP score with risk classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
// `#[non_exhaustive]` paused for v0.5 (see SourceSpan). Restored at v1.0.
pub struct CrapScore {
    /// Rounded to 2 decimal places.
    pub value: f64,
    pub risk_level: RiskLevel,
}

/// A function with all metrics computed.
#[derive(Debug, Clone, Serialize, Deserialize)]
// `#[non_exhaustive]` paused for v0.5 (see SourceSpan). Restored at v1.0.
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
// `#[non_exhaustive]` paused for v0.5 (see SourceSpan). Restored at v1.0.
pub struct FunctionVerdict {
    pub scored: ScoredFunction,
    pub threshold: f64,
    pub exceeds: bool,
    /// Structured remediation hint, populated when `--format advice` or
    /// `--format sarif` is requested. Boxed so `FunctionVerdict` (and
    /// `FunctionChange::Modified`, which carries two of them) stay small
    /// — most verdicts have no diagnostic, and the boxed pointer keeps
    /// `Option<…>` at 8 bytes. `Box<T>` is transparent to serde, so the
    /// JSON shape is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Box<Diagnostic>>,
}

// Re-exported here so `domain::types::Diagnostic` paths keep resolving.
pub use crate::domain::diagnostic::Diagnostic;

// ── Analysis Results ────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
// `#[non_exhaustive]` paused for v0.5 (see SourceSpan). Restored at v1.0.
pub struct RiskDistribution {
    pub low: usize,
    pub acceptable: usize,
    pub moderate: usize,
    pub high: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
// `#[non_exhaustive]` paused for v0.5 (see SourceSpan). Restored at v1.0.
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
// `#[non_exhaustive]` paused for v0.5 (see SourceSpan). Restored at v1.0.
pub struct AnalysisResult {
    pub functions: Vec<FunctionVerdict>,
    pub summary: AnalysisSummary,
    pub passed: bool,
}

// ── Analysis Diagnostics ───────────────────────────────────────────

/// Statistics about the analysis process, surfaced by `--verbose`.
///
/// Generic over `P: ParseDiagnostic` so adapter-specific parse
/// diagnostics (`LcovParseDiagnostic` in `crap4rs`, future
/// `IstanbulParseDiagnostic` in `crap4ts`) thread through one shared
/// shape. The numeric counts (`files_found`, `files_unparseable`,
/// `functions_extracted`, `functions_matched`, `functions_no_coverage`,
/// `files_analyzed`, `files_zero_coverage`) are language-agnostic; only
/// the `parse_diagnostics` payload varies by adapter. Decomposition
/// per CAO B2 + ADR D9 — see ADR D4 amendment (2026-05-09).
///
/// `#[serde(bound = "")]` suppresses serde's auto-generated
/// `P: Serialize`/`P: Deserialize<'de>` bounds — the trait bound
/// `P: ParseDiagnostic` already provides the equivalent (`Serialize +
/// DeserializeOwned`); the auto-generated bounds conflict with the
/// owned-deserialize requirement and trip up trait resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "")]
// `#[non_exhaustive]` paused for v0.5: cli/core in crap4rs construct
// `AnalysisDiagnostics<LcovParseDiagnostic>` via struct literal until
// they relocate to crap-core in S3/S4. Restored at v1.0.
pub struct AnalysisDiagnostics<P: crate::ports::ParseDiagnostic> {
    /// Non-fatal parse issues from the coverage parser. Concrete shape
    /// is adapter-specific (LCOV-flavored for crap4rs).
    pub parse_diagnostics: Vec<P>,
    /// Number of source files discovered.
    pub files_found: usize,
    /// Number of source files that failed to parse (skipped).
    pub files_unparseable: usize,
    /// Total functions extracted from source files.
    pub functions_extracted: usize,
    /// Functions matched with coverage data.
    pub functions_matched: usize,
    /// Functions with no coverage data (0% coverage assumed).
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
            end_line: 50,
            nesting_depth: 1,
        };
        assert_eq!(c.kind, ContributorKind::Match);
        assert_eq!(c.line, 42);
        assert_eq!(c.column, Some(4));
        assert_eq!(c.increment, 2);
        assert_eq!(c.end_line, 50);
        assert_eq!(c.nesting_depth, 1);
    }

    #[test]
    fn complexity_contributor_no_column() {
        let c = ComplexityContributor {
            kind: ContributorKind::Break,
            line: 10,
            column: None,
            increment: 1,
            end_line: 10,
            nesting_depth: 2,
        };
        assert!(c.column.is_none());
        assert_eq!(c.increment, 1);
        assert_eq!(c.nesting_depth, 2);
    }

    #[test]
    fn complexity_contributor_deserializes_without_new_fields() {
        // Pins the additive convention: older v0.3.0 JSON payloads (which did
        // not carry `end_line` / `nesting_depth`) must still deserialize so
        // schema_version=1 stays compatible across the v0.3.x series.
        let json = r#"{
            "kind": "if-branch",
            "line": 7,
            "column": null,
            "increment": 1
        }"#;
        let parsed: ComplexityContributor = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.kind, ContributorKind::IfBranch);
        assert_eq!(parsed.line, 7);
        assert_eq!(parsed.end_line, 0); // sentinel = "unknown / fall back to line"
        assert_eq!(parsed.nesting_depth, 0);
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
    use crate::test_strategies::DummyParseDiagnostic;

    #[test]
    fn analysis_diagnostics_has_zero_coverage_fields() {
        let diag: AnalysisDiagnostics<DummyParseDiagnostic> = AnalysisDiagnostics {
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
    fn coverage_metric_display() {
        assert_eq!(CoverageMetric::Line.to_string(), "line");
        assert_eq!(CoverageMetric::Branch.to_string(), "branch");
    }

    #[test]
    fn coverage_metric_default_is_line() {
        assert_eq!(CoverageMetric::default(), CoverageMetric::Line);
    }
}
