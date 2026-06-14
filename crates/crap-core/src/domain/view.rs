//! Domain-level View abstraction — the canonical pure-domain shaping
//! primitive over `AnalysisResult`.
//!
//! ```text
//! ViewSpec → view::apply(&result, spec) → AnalysisView<'_>
//! ```
//!
//! # Input contract
//!
//! `apply` borrows an `AnalysisResult` (already-thresholded function
//! verdicts plus a precomputed summary and gate verdict) and a
//! by-value `ViewSpec` (filter set, sort key, optional row limit,
//! optional group-by key). Both inputs are pure domain types — no I/O
//! happens inside the View layer.
//!
//! # Pipeline order
//!
//! `apply` runs phases in a fixed order:
//!
//! ```text
//! filter → group? → sort → truncate
//! ```
//!
//! 1. **Filter** — `Filters` AND-compose; `apply_filters` returns the
//!    eligible (post-filter, pre-shape) borrow vector.
//! 2. **Group** (optional) — when `spec.group_by.is_some()`, eligible
//!    rows fan into `GroupedView::files` (file-level aggregates that
//!    are independently sorted and truncated). Function-level `shown`
//!    retains the un-truncated eligible set under grouping for
//!    drill-down ergonomics.
//! 3. **Sort** — function-level (or file-level under grouping) by
//!    `SortKey`. `sort_by` is *stable* — callers rely on input-order
//!    preservation on tied keys.
//! 4. **Truncate** — `limit` applies to whichever level was sorted in
//!    step 3. `Some(0)` and `None` are treated identically as "no limit"
//!    (`--top 0` ergonomic).
//!
//! # Gate keystone — unshapeable
//!
//! **The gate is unshapeable; only the display is shapeable.**
//! `view.full` always borrows the original, unfiltered `AnalysisResult`,
//! and exit-code logic must derive from `view.full.passed`, never from
//! the post-shape `view.shown` or `view.shown_summary`. This invariant
//! lets reporters ship `--top`, `--only-failing`, `--coverage-range`,
//! and `--group-by` without ever changing CI's verdict.
//!
//! # Display predicate
//!
//! [`should_render_view_line`] returns true iff the shaped view
//! materially differs from the underlying analysis (rows filtered out,
//! function-level rows truncated, or grouped files truncated). Reporters
//! consult this predicate to decide whether to emit a "View:" subtitle
//! line — sort-only or default-spec invocations skip the subtitle.
//!
//! # `#[non_exhaustive]` extension policy
//!
//! Every public type in this module is `#[non_exhaustive]` so additive
//! extensions (new `SortKey` variants, new `GroupKey` aggregations, new
//! `Filters` predicates, new `AnalysisView` aggregates) ship as minor
//! version bumps without requiring downstream consumers to update match
//! arms or struct literals. Construct with `Default` + struct-update
//! syntax (`ViewSpec { sort: SortKey::Coverage, ..Default::default() }`)
//! to stay forward-compatible.
//!
//! # `crap-core` extraction
//!
//! Pure domain code — no I/O, no external crates beyond `serde` and
//! `thiserror` (mirrors `domain::types`). LSP, web, and agent
//! consumers all flow through `view::apply` as the canonical CRAP
//! shaping surface.

use crate::domain::summary::{FileSummary, compute_file_summaries, compute_summary};
use crate::domain::types::{AnalysisResult, AnalysisSummary, FunctionVerdict};
use serde::Serialize;

// ── Spec types ───────────────────────────────────────────────────────

/// Caller-supplied shape for the View pipeline.
///
/// `Default::default()` produces a no-op spec: no filtering, CRAP
/// descending, no row limit, no grouping. Construct with struct-update
/// syntax to stay forward-compatible with future fields:
///
/// ```ignore
/// let spec = ViewSpec {
///     filters: Filters { only_failing: true, ..Default::default() },
///     sort: SortKey::Coverage,
///     limit: Some(10),
///     ..Default::default()
/// };
/// ```
///
/// `#[non_exhaustive]` reserves namespace for additive fields (e.g.,
/// future `min_complexity`, `risk_floor`, secondary sort) without
/// breaking downstream callers.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize)]
pub struct ViewSpec {
    /// Eligibility predicates — AND-composed. See [`Filters`].
    pub filters: Filters,
    /// Ordering key. See [`SortKey`].
    pub sort: SortKey,
    /// Maximum rows after sort. `None` and `Some(0)` mean "no limit"
    /// (the `--top 0` ergonomic). When `group_by` is set, `limit`
    /// shifts to the file level — function-level rows are not truncated.
    pub limit: Option<usize>,
    /// When set, the View carries a parallel per-key aggregation
    /// (`AnalysisView::grouped`). The function-level row list
    /// (`shown`) is *not* truncated under grouping — `limit` shifts to
    /// the file level and applies to `grouped.files`. Today only
    /// `Some(GroupKey::File)` is supported; `#[non_exhaustive]`
    /// reserves namespace for `Risk` / `Module`.
    pub group_by: Option<GroupKey>,
}

/// Aggregation key for the optional grouped block of an
/// `AnalysisView`.
///
/// Only `File` is supported today. `#[non_exhaustive]`
/// reserves namespace for `Risk` and `Module` as listed in the
/// shaping doc — adding variants is additive on `crap-core`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKey {
    /// Aggregate by `FunctionIdentity::file_path`.
    File,
}

impl GroupKey {
    /// Canonical wire string — equal to the serde JSON representation
    /// (sans quotes). See
    /// `crate::domain::types::ContributorKind::as_wire_str` for the
    /// rationale; equality with serde is pinned in
    /// `tests::wire_str_matches_serde`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::File => "file",
        }
    }
}

/// Eligibility predicates over a `FunctionVerdict`. AND-composed: a
/// verdict is eligible iff every active filter admits it.
///
/// `Default::default()` admits all verdicts (no filtering).
/// `#[non_exhaustive]` reserves namespace for future predicates
/// (`min_complexity`, `risk_floor`, `path_glob`, etc.) without breaking
/// downstream construction.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize)]
pub struct Filters {
    /// When true, retain only verdicts where `exceeds == true`
    /// (CRAP score strictly exceeds the threshold).
    pub only_failing: bool,
    /// Inclusive coverage band. Verdicts with `coverage_percent` outside
    /// the band are excluded; non-finite coverage (NaN, ±∞) is excluded
    /// regardless of the band.
    pub coverage_range: Option<CoverageRange>,
}

/// Inclusive coverage range filter.
///
/// Both endpoints are validated to be in `[0.0, 100.0]` and `min <= max`
/// at construction time; downstream consumers can rely on these
/// invariants without re-checking. Construct via [`CoverageRange::new`]
/// — direct field initialization is intentionally blocked by
/// `#[non_exhaustive]`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CoverageRange {
    /// Lower bound (inclusive), in `[0.0, 100.0]`, finite, `<= max`.
    pub min: f64,
    /// Upper bound (inclusive), in `[0.0, 100.0]`, finite, `>= min`.
    pub max: f64,
}

impl CoverageRange {
    /// Construct a validated range. Returns [`CoverageRangeError`] when
    /// either endpoint is outside `[0.0, 100.0]` (including non-finite),
    /// or when `min > max`.
    pub fn new(min: f64, max: f64) -> Result<Self, CoverageRangeError> {
        if !is_in_unit_percent(min) {
            return Err(CoverageRangeError::OutOfRange { value: min });
        }
        if !is_in_unit_percent(max) {
            return Err(CoverageRangeError::OutOfRange { value: max });
        }
        if min > max {
            return Err(CoverageRangeError::MinExceedsMax { min, max });
        }
        Ok(Self { min, max })
    }
}

fn is_in_unit_percent(v: f64) -> bool {
    v.is_finite() && (0.0..=100.0).contains(&v)
}

/// Tag-only error type — variants carry numeric context but no prose.
/// The CLI translates these to user-facing messages so the domain stays
/// language-agnostic for `crap-core` extraction.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq)]
pub enum CoverageRangeError {
    #[error("coverage value out of range: {value}")]
    OutOfRange { value: f64 },
    #[error("min ({min}) exceeds max ({max})")]
    MinExceedsMax { min: f64, max: f64 },
}

/// Ordering key for the View pipeline's sort phase.
///
/// All sorts are *stable* (`sort_by`, not `sort_unstable_by`) so input
/// order is preserved on tied keys. NaN-bearing keys (CRAP value,
/// coverage percent) sort last under their respective orientation —
/// non-NaN winners take the descending positions.
///
/// File-level interpretation under `--group-by file`:
///
/// | Variant       | File-level meaning              |
/// |---------------|---------------------------------|
/// | `Crap`        | `average_crap` descending       |
/// | `Coverage`    | `average_coverage` ascending    |
/// | `Complexity`  | `max_complexity` descending     |
/// | `Path`        | `file_path` ascending           |
///
/// `#[non_exhaustive]` reserves namespace for future keys
/// (e.g., risk-bucket, function-name) without forcing match-arm churn
/// downstream.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    /// CRAP score descending (matches the legacy table reporter's order).
    #[default]
    Crap,
    /// Coverage ascending (low-coverage shown first — investigator's interest).
    Coverage,
    /// Complexity descending.
    Complexity,
    /// Alphabetical by `file_path`, then CRAP descending within file.
    Path,
}

impl SortKey {
    /// Canonical wire string — see `GroupKey::as_wire_str`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Crap => "crap",
            Self::Coverage => "coverage",
            Self::Complexity => "complexity",
            Self::Path => "path",
        }
    }
}

// ── View output ──────────────────────────────────────────────────────

/// The shaped result of applying a `ViewSpec` to an `AnalysisResult`.
///
/// `full` is borrow-only and elided from JSON output (the envelope's
/// `result` field already carries the same data). All shaping happens
/// over `shown`; `eligible_count` is the post-filter, pre-truncate
/// count; `truncated` records whether `limit` reduced the row set.
///
/// **Gate keystone:** exit-code logic must derive from
/// `view.full.passed`, never from `view.shown` or
/// `view.shown_summary`. Reporters consult [`should_render_view_line`]
/// to decide whether to emit a "View:" subtitle for the shaped output.
///
/// `#[non_exhaustive]` reserves namespace for future per-view aggregates
/// (e.g., per-risk-bucket counts, per-module fan-in) without breaking
/// downstream pattern matches.
#[non_exhaustive]
#[derive(Debug, Serialize)]
pub struct AnalysisView<'a> {
    /// Borrows the original analysis. `#[serde(skip)]` because the
    /// envelope's `result` already serializes the full analysis.
    /// **Gate source of truth** — exit-code logic uses `full.passed`.
    #[serde(skip)]
    pub full: &'a AnalysisResult,
    /// The spec that produced this view (echoed for JSON consumers).
    pub spec: ViewSpec,
    /// Post-filter, pre-truncate row count. When grouping is active,
    /// this is the function-level eligible count; the file-level
    /// equivalent lives in [`GroupedView::eligible_count`].
    pub eligible_count: usize,
    /// True iff `limit` dropped function-level rows. When grouping is
    /// active, this is forced false (the function-level row list is
    /// not truncated under grouping); see [`GroupedView::truncated`].
    pub truncated: bool,
    /// Borrow vector over the shaped function rows. Order, count, and
    /// truncation depend on `spec`.
    pub shown: Vec<&'a FunctionVerdict>,
    /// Summary computed over `shown` only — useful for reporters that
    /// want a "selected subset" header. **Not** the gate source: use
    /// `full.summary` and `full.passed` for verdict logic.
    pub shown_summary: AnalysisSummary,
    /// Optional parallel grouping. Present iff `spec.group_by.is_some()`.
    /// When set, `shown` retains the *un-truncated* eligible function
    /// rows (drill-down ergonomics) and `grouped.files` carries the
    /// post-sort, post-truncate file list.
    pub grouped: Option<GroupedView>,
}

/// File-level shaping over a `--group-by` view.
///
/// `eligible_count` and `truncated` mirror the function-level analogs
/// but at the file level so consumers can render headers like
/// "Showing 10 of 45 files" without recomputing.
///
/// `#[non_exhaustive]` reserves namespace for future per-group
/// aggregates (e.g., risk-bucket totals, complexity histograms) without
/// breaking downstream pattern matches.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize)]
pub struct GroupedView {
    /// The key this view was grouped by (today: always `GroupKey::File`).
    pub key: GroupKey,
    /// Distinct files surviving the function-level filter pass —
    /// before `limit` truncates the file list.
    pub eligible_count: usize,
    /// True iff `limit` reduced the file list.
    pub truncated: bool,
    /// Per-file aggregates, sorted and truncated per `spec.sort` and
    /// `spec.limit` at the file level.
    pub files: Vec<FileSummary>,
}

// ── apply: filter → sort → truncate ──────────────────────────────────

/// Apply a `ViewSpec` to an `AnalysisResult`, producing the shaped
/// `AnalysisView`.
///
/// Phases run in fixed order: **filter → group? → sort → truncate**.
/// See the module-level docs for the full pipeline contract.
///
/// The returned view borrows from `result`; `view.full == &result` is
/// guaranteed (pointer-equal). The gate verdict (`view.full.passed`,
/// `view.full.summary`) is *unshapeable* — it always reflects the
/// pre-shape analysis, regardless of how aggressively the spec
/// filters or truncates.
///
/// Stable sort: input order is preserved on tied sort keys. NaN-bearing
/// keys sort last under their orientation.
///
/// `apply` is total — it never panics, even on NaN coverage or empty
/// inputs. The unit tests and the `proptests` module below pin the full
/// behavioral contract: the order, identity, summary, and display
/// invariants over arbitrary `AnalysisResult`s.
pub fn apply<'a>(result: &'a AnalysisResult, spec: ViewSpec) -> AnalysisView<'a> {
    let eligible: Vec<&'a FunctionVerdict> = apply_filters(&result.functions, &spec.filters);
    let eligible_count = eligible.len();

    // Order of ops:
    //   filter → group? → sort+truncate (function-level OR file-level)
    //
    // When grouping is active, `shown` carries the *un-truncated*
    // eligible function rows for drill-down (the JSON consumer's
    // `view.shown[] | select(...)` flow), and the function-level
    // `truncated` flag is forced false because no function-level
    // truncation took place. The file list carries its own
    // `truncated` flag inside `GroupedView`.
    let grouped = apply_grouping(&eligible, &spec);

    let (shown, truncated) = if grouped.is_some() {
        (eligible, false)
    } else {
        let mut shown = eligible;
        sort_in_place(&mut shown, spec.sort);
        let truncated = truncate_to(&mut shown, spec.limit);
        (shown, truncated)
    };

    // `compute_summary` accepts any `IntoIterator<Item = &FunctionVerdict>`,
    // so we feed it the borrowed `shown` directly — no per-`apply()` clone.
    let shown_summary = compute_summary(shown.iter().copied());

    AnalysisView {
        full: result,
        spec,
        eligible_count,
        truncated,
        shown,
        shown_summary,
        grouped,
    }
}

/// Build the optional `GroupedView` from the eligible (post-filter) row set.
///
/// Returns `None` iff `spec.group_by.is_none()` — the biconditional that
/// keeps reporters' branching decisions simple. The returned files are
/// sorted by the `spec.sort` key at the *file* level and truncated to
/// `spec.limit` (if any). The function-level row list and gate are
/// untouched: `view.shown` and `view.full.passed` still describe the
/// underlying analysis.
fn apply_grouping(eligible: &[&FunctionVerdict], spec: &ViewSpec) -> Option<GroupedView> {
    let key = spec.group_by?;
    let mut files = compute_file_summaries(eligible.iter().copied());
    let eligible_count = files.len();
    sort_files_in_place(&mut files, spec.sort);
    let truncated = truncate_files_to(&mut files, spec.limit);
    Some(GroupedView {
        key,
        eligible_count,
        truncated,
        files,
    })
}

/// File-level sort. Mirrors the function-level `SortKey` semantics but
/// applied to per-file aggregates:
///
/// | SortKey    | File-level interpretation                     |
/// |------------|-----------------------------------------------|
/// | `Crap`     | `average_crap` descending                     |
/// | `Coverage` | `average_coverage` ascending                  |
/// | `Complexity` | `max_complexity` descending                 |
/// | `Path`     | `file_path` ascending                         |
fn sort_files_in_place(files: &mut [FileSummary], key: SortKey) {
    match key {
        SortKey::Crap => files.sort_by(cmp_files_by_avg_crap),
        SortKey::Coverage => files.sort_by(cmp_files_by_avg_coverage),
        SortKey::Complexity => files.sort_by_key(|f| std::cmp::Reverse(f.max_complexity)),
        SortKey::Path => files.sort_by(|a, b| a.file_path.cmp(&b.file_path)),
    }
}

fn cmp_files_by_avg_crap(a: &FileSummary, b: &FileSummary) -> std::cmp::Ordering {
    let (ax, bx) = (a.average_crap, b.average_crap);
    match (ax.is_nan(), bx.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => bx.partial_cmp(&ax).expect("non-NaN partial_cmp infallible"),
    }
}

fn cmp_files_by_avg_coverage(a: &FileSummary, b: &FileSummary) -> std::cmp::Ordering {
    let (ax, bx) = (a.average_coverage, b.average_coverage);
    match (ax.is_nan(), bx.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => ax.partial_cmp(&bx).expect("non-NaN partial_cmp infallible"),
    }
}

fn truncate_files_to(files: &mut Vec<FileSummary>, limit: Option<usize>) -> bool {
    match limit {
        Some(n) if n > 0 && files.len() > n => {
            files.truncate(n);
            true
        }
        _ => false,
    }
}

/// Filter pass — returns a vector of references that match every active filter.
///
/// AND-composes filters: a verdict is eligible iff every active filter
/// admits it. The coverage-range branch uses `is_finite()` so non-finite
/// coverage is excluded — NaN in practice, and
/// also ±∞ defensively. LCOV-derived percentages should never be infinite,
/// but the wider check costs nothing and keeps the comparator total.
fn apply_filters<'a>(
    verdicts: &'a [FunctionVerdict],
    filters: &Filters,
) -> Vec<&'a FunctionVerdict> {
    verdicts
        .iter()
        .filter(|v| !filters.only_failing || v.exceeds)
        .filter(|v| match &filters.coverage_range {
            Some(range) => matches_coverage_range(v.scored.coverage_percent, range),
            None => true,
        })
        .collect()
}

fn matches_coverage_range(cov: f64, range: &CoverageRange) -> bool {
    cov.is_finite() && cov >= range.min && cov <= range.max
}

// ── Sort dispatch + comparators ──────────────────────────────────────

fn sort_in_place(shown: &mut [&FunctionVerdict], key: SortKey) {
    // sort_by — stable. NOT sort_unstable_by: callers rely on input-order
    // preservation for tied keys.
    match key {
        SortKey::Crap => shown.sort_by(cmp_by_crap),
        SortKey::Coverage => shown.sort_by(cmp_by_coverage),
        SortKey::Complexity => shown.sort_by(cmp_by_complexity),
        SortKey::Path => shown.sort_by(cmp_by_path),
    }
}

/// CRAP descending. f64-bearing — NaN sorts last under the descending
/// order (i.e., comparator returns `Less` for non-NaN vs NaN-second so
/// non-NaN wins the descending position; symmetrically for NaN-first).
fn cmp_by_crap(a: &&FunctionVerdict, b: &&FunctionVerdict) -> std::cmp::Ordering {
    let (ax, bx) = (a.scored.crap.value, b.scored.crap.value);
    match (ax.is_nan(), bx.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => bx.partial_cmp(&ax).expect("non-NaN partial_cmp infallible"),
    }
}

/// Coverage ascending. NaN sorts last.
fn cmp_by_coverage(a: &&FunctionVerdict, b: &&FunctionVerdict) -> std::cmp::Ordering {
    let (ax, bx) = (a.scored.coverage_percent, b.scored.coverage_percent);
    match (ax.is_nan(), bx.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => ax.partial_cmp(&bx).expect("non-NaN partial_cmp infallible"),
    }
}

/// Complexity descending. `u32` is `Ord`; no NaN concerns.
fn cmp_by_complexity(a: &&FunctionVerdict, b: &&FunctionVerdict) -> std::cmp::Ordering {
    b.scored.complexity.cmp(&a.scored.complexity)
}

/// Path alphabetical ascending; ties broken by CRAP descending.
fn cmp_by_path(a: &&FunctionVerdict, b: &&FunctionVerdict) -> std::cmp::Ordering {
    match a
        .scored
        .identity
        .file_path
        .cmp(&b.scored.identity.file_path)
    {
        std::cmp::Ordering::Equal => cmp_by_crap(a, b),
        ord => ord,
    }
}

// ── Truncate ─────────────────────────────────────────────────────────

/// Truncate `shown` to `limit` entries. Returns whether any rows were
/// dropped. `Some(0)` and `None` are treated identically as "no limit"
/// (the `--top 0` semantics).
fn truncate_to(shown: &mut Vec<&FunctionVerdict>, limit: Option<usize>) -> bool {
    match limit {
        Some(n) if n > 0 && shown.len() > n => {
            shown.truncate(n);
            true
        }
        _ => false,
    }
}

// ── Display predicate ────────────────────────────────────────────────

/// True iff the shaped view materially differs from the underlying
/// analysis. Returns `true` when any of:
///
/// - Filtering reduced the eligible row count
///   (`eligible_count < full.functions.len()`),
/// - The function-level `limit` truncated rows (`view.truncated`),
/// - Grouping is active and the file-level `limit` truncated files.
///
/// Reporters use this to decide whether to emit a "View:" subtitle
/// line. Default `ViewSpec` over a non-empty result returns `false` —
/// the walking-skeleton invariant. Sort-only invocations also return
/// `false`: changing order doesn't change information content.
pub fn should_render_view_line(view: &AnalysisView<'_>) -> bool {
    view.eligible_count < view.full.functions.len()
        || view.truncated
        || view.grouped.as_ref().is_some_and(|g| g.truncated)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        AnalysisSummary, ComplexityMetric, CrapScore, FunctionIdentity, FunctionVerdict,
        RiskDistribution, ScoredFunction, SourceSpan,
    };

    // ── Fixture helpers ────────────────────────────────────────────

    fn mk_verdict(
        name: &str,
        file: &str,
        complexity: u32,
        coverage: f64,
        crap_value: f64,
        threshold: f64,
    ) -> FunctionVerdict {
        let risk_level = crate::domain::crap::classify_risk(crap_value);
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 10,
                        start_column: 0,
                        end_column: 0,
                    },
                },
                complexity,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: coverage,
                // View-layer test fixtures don't exercise branch
                // coverage today; branch_coverage_percent stays None so
                // sort / filter / truncate invariants are isolated from
                // the new field.
                branch_coverage_percent: None,
                crap: CrapScore {
                    value: crap_value,
                    risk_level,
                },
                contributors: vec![],
            },
            threshold,
            exceeds: crap_value > threshold,
            diagnostic: None,
        }
    }

    /// Shared 6-function fixture at threshold 25.0 (two rows fail).
    fn background_fixture() -> AnalysisResult {
        let verdicts = vec![
            mk_verdict("parse_lcov", "src/adapters/lcov.rs", 12, 100.0, 12.00, 25.0),
            mk_verdict("walk_ast", "src/adapters/syn.rs", 18, 75.0, 23.06, 25.0),
            mk_verdict(
                "render_table",
                "src/adapters/table.rs",
                9,
                60.0,
                14.18,
                25.0,
            ),
            mk_verdict(
                "apply_threshold",
                "src/domain/threshold.rs",
                4,
                100.0,
                4.00,
                25.0,
            ),
            mk_verdict(
                "sort_verdicts",
                "src/adapters/table.rs",
                6,
                0.0,
                42.00,
                25.0,
            ),
            mk_verdict("parse_args", "src/cli/mod.rs", 22, 50.0, 63.50, 25.0),
        ];
        let summary = compute_summary(&verdicts);
        let passed = verdicts.iter().all(|v| !v.exceeds);
        AnalysisResult {
            functions: verdicts,
            summary,
            passed,
        }
    }

    fn empty_result() -> AnalysisResult {
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

    // ── Default-spec invariants (Order, Identity, Summary, immutability) ───

    #[test]
    fn default_spec_is_noop_on_fixture() {
        // Default spec produces a no-op view in CRAP-descending order:
        // shown contains every function; eligible_count equals total;
        // truncated false; CRAP desc.
        let r = background_fixture();
        let view = apply(&r, ViewSpec::default());
        assert_eq!(view.shown.len(), r.functions.len());
        assert_eq!(view.eligible_count, r.functions.len());
        assert!(!view.truncated);
        // Pointer equality on `view.full` (no PartialEq derive needed)
        assert!(std::ptr::eq(view.full, &r));
        // Order: CRAP descending
        for w in view.shown.windows(2) {
            assert!(
                w[0].scored.crap.value >= w[1].scored.crap.value,
                "expected CRAP descending; got {} then {}",
                w[0].scored.crap.value,
                w[1].scored.crap.value
            );
        }
    }

    #[test]
    fn default_spec_empty_input_is_empty_view() {
        let r = empty_result();
        let view = apply(&r, ViewSpec::default());
        assert!(view.shown.is_empty());
        assert_eq!(view.eligible_count, 0);
        assert!(!view.truncated);
        assert!(view.full.passed);
    }

    #[test]
    fn view_full_immutability_after_apply() {
        let r = background_fixture();
        let crap_before: Vec<f64> = r.functions.iter().map(|v| v.scored.crap.value).collect();
        let view = apply(&r, ViewSpec::default());
        // view.full points at r
        assert!(std::ptr::eq(view.full, &r));
        // r itself is unchanged
        let crap_after: Vec<f64> = r.functions.iter().map(|v| v.scored.crap.value).collect();
        assert_eq!(crap_before, crap_after);
    }

    #[test]
    fn default_spec_preserves_identity_set() {
        let r = background_fixture();
        let view = apply(&r, ViewSpec::default());
        let shown_names: std::collections::HashSet<&String> = view
            .shown
            .iter()
            .map(|v| &v.scored.identity.qualified_name)
            .collect();
        let original_names: std::collections::HashSet<&String> = r
            .functions
            .iter()
            .map(|v| &v.scored.identity.qualified_name)
            .collect();
        assert_eq!(shown_names, original_names);
    }

    // ── CoverageRange constructor: 7-row table ─────────────────────

    #[test]
    fn coverage_range_new_validation_table() {
        type Case = (f64, f64, Result<(f64, f64), ()>);
        let cases: &[Case] = &[
            (0.0, 100.0, Ok((0.0, 100.0))),
            (50.0, 50.0, Ok((50.0, 50.0))),
            (1.0, 90.0, Ok((1.0, 90.0))),
            (-0.1, 50.0, Err(())),
            (50.0, 100.1, Err(())),
            (90.0, 50.0, Err(())),
            (100.0, 0.0, Err(())),
        ];
        for (min, max, expect) in cases {
            let got = CoverageRange::new(*min, *max);
            match (got, expect) {
                (Ok(r), Ok((emin, emax))) => {
                    assert!(
                        (r.min - emin).abs() < 1e-9 && (r.max - emax).abs() < 1e-9,
                        "min={min}, max={max}: got {r:?}, expected ({emin}, {emax})"
                    );
                }
                (Err(_), Err(())) => {} // good
                (got, expect) => panic!("min={min}, max={max}: got {got:?}, expected {expect:?}"),
            }
        }
    }

    #[test]
    fn coverage_range_error_variants() {
        // out-of-range vs min-exceeds-max are distinct, tag-only variants.
        let oor = CoverageRange::new(-1.0, 50.0).unwrap_err();
        assert!(matches!(oor, CoverageRangeError::OutOfRange { .. }));
        let mxm = CoverageRange::new(80.0, 20.0).unwrap_err();
        assert!(matches!(mxm, CoverageRangeError::MinExceedsMax { .. }));
    }

    // ── Sort stability — the surgical mutation killer ───────────

    #[test]
    fn sort_stability_on_tied_crap() {
        // Catches the `sort_by → sort_unstable_by` mutation.
        // Hand-built deterministic [foo, bar] both at CRAP=12.0.
        let foo = mk_verdict("foo", "src/a.rs", 5, 80.0, 12.0, 25.0);
        let bar = mk_verdict("bar", "src/a.rs", 5, 80.0, 12.0, 25.0);
        let r = AnalysisResult {
            functions: vec![foo, bar],
            summary: empty_result().summary, // unused
            passed: true,
        };
        let view = apply(&r, ViewSpec::default());
        // Input order [foo, bar] preserved on tied keys.
        assert_eq!(
            view.shown[0].scored.identity.qualified_name,
            "foo",
            "stable sort must preserve input order on ties; got {:?}",
            view.shown
                .iter()
                .map(|v| &v.scored.identity.qualified_name)
                .collect::<Vec<_>>()
        );
        assert_eq!(view.shown[1].scored.identity.qualified_name, "bar");
    }

    // ── Filters ────────────────────────────────────────────────────

    #[test]
    fn only_failing_filter_retains_only_exceeds_true() {
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.iter().all(|v| v.exceeds));
        // And every shown CRAP exceeds threshold
        for v in &view.shown {
            assert!(v.scored.crap.value > v.threshold);
        }
    }

    #[test]
    fn coverage_range_filter_inclusive() {
        let r = background_fixture();
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.iter().all(|v| {
            let cov = v.scored.coverage_percent;
            cov.is_finite() && (50.0..=90.0).contains(&cov)
        }));
        let manual_count = r
            .functions
            .iter()
            .filter(|v| v.scored.coverage_percent.is_finite())
            .filter(|v| (50.0..=90.0).contains(&v.scored.coverage_percent))
            .count();
        assert_eq!(view.eligible_count, manual_count);
    }

    #[test]
    fn coverage_range_boundary_inclusive_50_low() {
        // cov=50.0 in 50..=90 → appears (inclusive low boundary).
        let v = mk_verdict("at50", "src/a.rs", 1, 50.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
    }

    #[test]
    fn coverage_range_boundary_inclusive_90_high() {
        // row 2: cov=90.0 in 50..=90 → appears.
        let v = mk_verdict("at90", "src/a.rs", 1, 90.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
    }

    #[test]
    fn coverage_range_boundary_inclusive_below_low() {
        // row 3: cov=49.9 in 50..=90 → absent.
        let v = mk_verdict("just_under", "src/a.rs", 1, 49.9, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.is_empty());
    }

    #[test]
    fn coverage_range_boundary_inclusive_above_high() {
        // row 4: cov=90.1 in 50..=90 → absent.
        let v = mk_verdict("just_over", "src/a.rs", 1, 90.1, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(50.0, 90.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.is_empty());
    }

    #[test]
    fn coverage_range_boundary_inclusive_zero_singleton() {
        // row 5: cov=0.0 in 0..=0 → appears.
        let v = mk_verdict("zero", "src/a.rs", 1, 0.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(0.0, 0.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
    }

    #[test]
    fn coverage_range_boundary_inclusive_hundred_singleton() {
        // row 6: cov=100.0 in 100..=100 → appears.
        let v = mk_verdict("full", "src/a.rs", 1, 100.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(100.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
    }

    #[test]
    fn nan_coverage_excluded_from_range_filter() {
        let v = mk_verdict("zero_lines", "src/a.rs", 1, f64::NAN, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(0.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.is_empty());
    }

    // ── Sort ───────────────────────────────────────────────────────

    #[test]
    fn sort_by_crap_descending() {
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Crap,
            ..Default::default()
        };
        let view = apply(&r, spec);
        for w in view.shown.windows(2) {
            assert!(w[0].scored.crap.value >= w[1].scored.crap.value);
        }
    }

    #[test]
    fn sort_by_coverage_ascending() {
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Coverage,
            ..Default::default()
        };
        let view = apply(&r, spec);
        for w in view.shown.windows(2) {
            assert!(w[0].scored.coverage_percent <= w[1].scored.coverage_percent);
        }
    }

    #[test]
    fn sort_by_complexity_descending() {
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Complexity,
            ..Default::default()
        };
        let view = apply(&r, spec);
        for w in view.shown.windows(2) {
            assert!(w[0].scored.complexity >= w[1].scored.complexity);
        }
    }

    #[test]
    fn sort_by_path_alphabetical_then_crap() {
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Path,
            ..Default::default()
        };
        let view = apply(&r, spec);
        // Primary: file_path ascending.
        for w in view.shown.windows(2) {
            let (a, b) = (
                &w[0].scored.identity.file_path,
                &w[1].scored.identity.file_path,
            );
            assert!(a <= b, "files not in ascending order: {a} then {b}");
        }
        // Within each file: CRAP descending.
        for w in view.shown.windows(2) {
            if w[0].scored.identity.file_path == w[1].scored.identity.file_path {
                assert!(
                    w[0].scored.crap.value >= w[1].scored.crap.value,
                    "within file {}: CRAP not descending: {} then {}",
                    w[0].scored.identity.file_path,
                    w[0].scored.crap.value,
                    w[1].scored.crap.value
                );
            }
        }
    }

    #[test]
    fn sort_by_path_secondary_multi_file() {
        // 3 files, 5 verdicts: src/a.rs (5, 30), src/b.rs (10),
        // src/c.rs (1, 50). Expected: a.rs::30, a.rs::5, b.rs::10,
        // c.rs::50, c.rs::1.
        let verdicts = vec![
            mk_verdict("a_low", "src/a.rs", 1, 50.0, 5.0, 25.0),
            mk_verdict("a_high", "src/a.rs", 1, 50.0, 30.0, 25.0),
            mk_verdict("b_only", "src/b.rs", 1, 50.0, 10.0, 25.0),
            mk_verdict("c_low", "src/c.rs", 1, 50.0, 1.0, 25.0),
            mk_verdict("c_high", "src/c.rs", 1, 50.0, 50.0, 25.0),
        ];
        let r = AnalysisResult {
            functions: verdicts,
            summary: empty_result().summary,
            passed: true,
        };
        let spec = ViewSpec {
            sort: SortKey::Path,
            ..Default::default()
        };
        let view = apply(&r, spec);
        let names: Vec<&str> = view
            .shown
            .iter()
            .map(|v| v.scored.identity.qualified_name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["a_high", "a_low", "b_only", "c_high", "c_low"],
            "path sort with secondary CRAP-desc order wrong"
        );
    }

    #[test]
    fn nan_coverage_sorts_last_under_coverage_ascending() {
        // Coverages: [10.0, NaN, 50.0, NaN, 90.0].
        let verdicts = vec![
            mk_verdict("c10", "src/a.rs", 1, 10.0, 1.0, 25.0),
            mk_verdict("nan1", "src/a.rs", 1, f64::NAN, 1.0, 25.0),
            mk_verdict("c50", "src/a.rs", 1, 50.0, 1.0, 25.0),
            mk_verdict("nan2", "src/a.rs", 1, f64::NAN, 1.0, 25.0),
            mk_verdict("c90", "src/a.rs", 1, 90.0, 1.0, 25.0),
        ];
        let r = AnalysisResult {
            functions: verdicts,
            summary: empty_result().summary,
            passed: true,
        };
        let spec = ViewSpec {
            sort: SortKey::Coverage,
            ..Default::default()
        };
        let view = apply(&r, spec);
        // First 3 are non-NaN ascending; last 2 are NaN.
        let coverages: Vec<f64> = view
            .shown
            .iter()
            .map(|v| v.scored.coverage_percent)
            .collect();
        assert_eq!(coverages[0], 10.0);
        assert_eq!(coverages[1], 50.0);
        assert_eq!(coverages[2], 90.0);
        assert!(coverages[3].is_nan());
        assert!(coverages[4].is_nan());
    }

    // ── Truncate ───────────────────────────────────────────────────

    #[test]
    fn limit_truncates() {
        // Background has 6 functions; limit=3.
        let r = background_fixture();
        let spec = ViewSpec {
            limit: Some(3),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 3);
        assert_eq!(view.eligible_count, 6);
        assert!(view.truncated);
    }

    #[test]
    fn limit_greater_than_eligible() {
        let r = background_fixture();
        let spec = ViewSpec {
            limit: Some(100),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 6);
        assert_eq!(view.eligible_count, 6);
        assert!(!view.truncated);
    }

    #[test]
    fn limit_none() {
        let r = background_fixture();
        let spec = ViewSpec {
            limit: None,
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), view.eligible_count);
        assert!(!view.truncated);
    }

    #[test]
    fn limit_zero_treated_as_no_limit() {
        // --top 0 ⇒ limit = None semantics. Construct directly with
        // Some(0); the code treats it as no-limit.
        let r = background_fixture();
        let spec = ViewSpec {
            limit: Some(0),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), view.eligible_count);
        assert!(!view.truncated);
    }

    #[test]
    fn limit_equal_to_eligible_does_not_mark_truncated() {
        // Boundary: shown.len() == limit. Mutation-killer for `>` vs `>=`
        // in `truncate_to`. The data is unchanged either way, but
        // `truncated` MUST stay false when nothing was actually dropped.
        let r = background_fixture();
        assert_eq!(r.functions.len(), 6, "background fixture sanity");
        let spec = ViewSpec {
            limit: Some(6),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 6);
        assert_eq!(view.eligible_count, 6);
        assert!(!view.truncated, "limit == eligible must NOT mark truncated");
    }

    // ── Order of operations ────────────────────────────────────────

    #[test]
    fn order_filter_then_sort_then_truncate() {
        // only_failing AND sort=Coverage AND limit=2.
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            sort: SortKey::Coverage,
            limit: Some(2),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 2);
        for v in &view.shown {
            assert!(v.exceeds);
        }
        // Coverage ascending
        assert!(view.shown[0].scored.coverage_percent <= view.shown[1].scored.coverage_percent);
        // eligible_count = total failing functions before truncation
        let total_failing = r.functions.iter().filter(|v| v.exceeds).count();
        assert_eq!(view.eligible_count, total_failing);
    }

    #[test]
    fn truncation_does_not_change_gate() {
        // Analysis with 3 functions exceeding threshold; construct that
        // fixture explicitly.
        let verdicts = vec![
            mk_verdict("ok", "src/a.rs", 1, 100.0, 1.0, 25.0),
            mk_verdict("fail1", "src/a.rs", 1, 0.0, 50.0, 25.0),
            mk_verdict("fail2", "src/a.rs", 1, 0.0, 60.0, 25.0),
            mk_verdict("fail3", "src/a.rs", 1, 0.0, 70.0, 25.0),
        ];
        let summary = compute_summary(&verdicts);
        let passed = verdicts.iter().all(|v| !v.exceeds);
        let r = AnalysisResult {
            functions: verdicts,
            summary,
            passed,
        };
        let spec = ViewSpec {
            limit: Some(1),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown.len(), 1);
        // gate = view.full.passed (false) and exceeding count unchanged.
        assert!(!view.full.passed);
        assert_eq!(view.full.summary.exceeding_threshold, 3);
    }

    #[test]
    fn filtering_does_not_change_gate() {
        // Analysis with 3 exceeding; filter excludes all of them.
        let verdicts = vec![
            mk_verdict("ok", "src/a.rs", 1, 100.0, 1.0, 25.0),
            mk_verdict("fail1", "src/a.rs", 1, 0.0, 50.0, 25.0),
            mk_verdict("fail2", "src/a.rs", 1, 0.0, 60.0, 25.0),
            mk_verdict("fail3", "src/a.rs", 1, 0.0, 70.0, 25.0),
        ];
        let summary = compute_summary(&verdicts);
        let r = AnalysisResult {
            functions: verdicts,
            summary,
            passed: false,
        };
        // Range [99, 100] keeps "ok" only; excludes the 3 failing ones.
        let range = CoverageRange::new(99.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.iter().all(|v| !v.exceeds));
        assert!(!view.full.passed);
        assert_eq!(view.full.summary.exceeding_threshold, 3);
    }

    // ── shown_summary ──────────────────────────────────────────────

    #[test]
    fn shown_summary_over_shown_subset() {
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.shown_summary.total_functions, view.shown.len());
        assert_eq!(
            view.shown_summary.exceeding_threshold,
            view.shown.len(),
            "every shown row exceeds, so shown_summary should report all"
        );
        // Manual check: avg
        let manual_avg: f64 =
            view.shown.iter().map(|v| v.scored.crap.value).sum::<f64>() / view.shown.len() as f64;
        assert!((view.shown_summary.average_crap - manual_avg).abs() < 1e-9);
    }

    #[test]
    fn shown_summary_differs_from_full() {
        // Analysis with 6 functions, 3 exceeding.
        let verdicts = vec![
            mk_verdict("ok1", "src/a.rs", 1, 100.0, 1.0, 25.0),
            mk_verdict("ok2", "src/a.rs", 1, 100.0, 2.0, 25.0),
            mk_verdict("ok3", "src/a.rs", 1, 100.0, 3.0, 25.0),
            mk_verdict("fail1", "src/a.rs", 1, 0.0, 50.0, 25.0),
            mk_verdict("fail2", "src/a.rs", 1, 0.0, 60.0, 25.0),
            mk_verdict("fail3", "src/a.rs", 1, 0.0, 70.0, 25.0),
        ];
        let summary = compute_summary(&verdicts);
        let r = AnalysisResult {
            functions: verdicts,
            summary,
            passed: false,
        };
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.full.summary.total_functions, 6);
        assert_eq!(view.shown_summary.total_functions, 3);
    }

    // ── Edge cases ─────────────────────────────────────────────────

    #[test]
    fn all_filtered_out_produces_empty_shown() {
        let v = mk_verdict("low_cov", "src/a.rs", 1, 50.0, 1.0, 25.0);
        let r = AnalysisResult {
            functions: vec![v],
            summary: empty_result().summary,
            passed: true,
        };
        let range = CoverageRange::new(95.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(view.shown.is_empty());
        assert_eq!(view.eligible_count, 0);
        assert!(!view.truncated);
    }

    // ── should_render_view_line — display predicate ────────────────

    #[test]
    fn display_predicate_default_spec_is_false() {
        // Default invocation: no filtering, no truncation.
        let r = background_fixture();
        let view = apply(&r, ViewSpec::default());
        assert!(!should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_sort_only_is_false() {
        // Sort-only invocation: still false (no rows reduced).
        let r = background_fixture();
        let spec = ViewSpec {
            sort: SortKey::Coverage,
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(!should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_top_truncating_is_true() {
        // --top truncating: true.
        let r = background_fixture();
        let spec = ViewSpec {
            limit: Some(2),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_coverage_filter_excluding_is_true() {
        // Coverage filter that excludes: true.
        let r = background_fixture();
        let range = CoverageRange::new(99.0, 100.0).unwrap();
        let spec = ViewSpec {
            filters: Filters {
                coverage_range: Some(range),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_only_failing_reducing_is_true() {
        // --only-failing with some passing rows: true.
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(should_render_view_line(&view));
    }

    // ── Grouping (`--group-by file`) ────────────────────────────────

    #[test]
    fn no_group_by_means_no_grouped_block() {
        // Biconditional half: spec.group_by.is_none() ⇒ view.grouped.is_none()
        let r = background_fixture();
        let view = apply(&r, ViewSpec::default());
        assert!(view.grouped.is_none());
    }

    #[test]
    fn group_by_file_populates_grouped_block() {
        // Biconditional half: spec.group_by.is_some() ⇒ view.grouped.is_some()
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            ..Default::default()
        };
        let view = apply(&r, spec);
        let grouped = view.grouped.as_ref().expect("grouped block expected");
        assert_eq!(grouped.key, GroupKey::File);
        // Background fixture: 5 distinct files (parse_args is in cli/mod.rs;
        // table.rs has two functions; lcov, syn, threshold one each).
        assert_eq!(grouped.files.len(), 5);
        assert_eq!(grouped.eligible_count, 5);
        assert!(!grouped.truncated);
    }

    #[test]
    fn group_by_file_does_not_truncate_function_shown() {
        // Function-level shown is the un-truncated eligible set under grouping.
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            limit: Some(2),
            ..Default::default()
        };
        let view = apply(&r, spec);
        // limit=2 truncates files, not functions.
        assert_eq!(view.shown.len(), r.functions.len());
        assert!(!view.truncated);
        let grouped = view.grouped.as_ref().unwrap();
        assert_eq!(grouped.files.len(), 2);
        assert!(grouped.truncated);
        assert_eq!(grouped.eligible_count, 5);
    }

    #[test]
    fn group_by_file_keeps_gate_unchanged() {
        // P6 (gate-vs-display): grouping does not change view.full.passed
        // or view.full.summary.
        let r = background_fixture();
        let baseline_passed = r.passed;
        let baseline_total = r.summary.total_functions;
        let baseline_exceeding = r.summary.exceeding_threshold;
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            limit: Some(1),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert_eq!(view.full.passed, baseline_passed);
        assert_eq!(view.full.summary.total_functions, baseline_total);
        assert_eq!(view.full.summary.exceeding_threshold, baseline_exceeding);
    }

    #[test]
    fn group_by_file_default_sort_is_avg_crap_desc() {
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            ..Default::default()
        };
        let view = apply(&r, spec);
        let files = &view.grouped.as_ref().unwrap().files;
        for w in files.windows(2) {
            assert!(
                w[0].average_crap >= w[1].average_crap,
                "files not in average_crap descending order"
            );
        }
    }

    #[test]
    fn group_by_file_sort_by_coverage_ascending() {
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            sort: SortKey::Coverage,
            ..Default::default()
        };
        let view = apply(&r, spec);
        let files = &view.grouped.as_ref().unwrap().files;
        for w in files.windows(2) {
            assert!(w[0].average_coverage <= w[1].average_coverage);
        }
    }

    #[test]
    fn group_by_file_sort_by_complexity_descending() {
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            sort: SortKey::Complexity,
            ..Default::default()
        };
        let view = apply(&r, spec);
        let files = &view.grouped.as_ref().unwrap().files;
        for w in files.windows(2) {
            assert!(w[0].max_complexity >= w[1].max_complexity);
        }
    }

    #[test]
    fn group_by_file_sort_by_path_alphabetical() {
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            sort: SortKey::Path,
            ..Default::default()
        };
        let view = apply(&r, spec);
        let files = &view.grouped.as_ref().unwrap().files;
        for w in files.windows(2) {
            assert!(w[0].file_path <= w[1].file_path);
        }
    }

    #[test]
    fn group_by_file_truncate_files() {
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            limit: Some(3),
            ..Default::default()
        };
        let view = apply(&r, spec);
        let grouped = view.grouped.as_ref().unwrap();
        assert_eq!(grouped.files.len(), 3);
        assert!(grouped.truncated);
        assert_eq!(grouped.eligible_count, 5);
    }

    #[test]
    fn group_by_file_filters_compose_before_grouping() {
        // only_failing + group_by file: grouped.files reflect only files
        // that have a failing function.
        let r = background_fixture();
        let spec = ViewSpec {
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            group_by: Some(GroupKey::File),
            ..Default::default()
        };
        let view = apply(&r, spec);
        let grouped = view.grouped.as_ref().unwrap();
        // Background fixture: failing functions are sort_verdicts (table.rs)
        // CRAP=42 and parse_args (cli/mod.rs) CRAP=63.5 → 2 distinct files.
        assert_eq!(grouped.files.len(), 2);
        // Every file has at least one exceeding function.
        for f in &grouped.files {
            assert!(f.exceeding_count >= 1);
        }
    }

    #[test]
    fn group_by_file_empty_input_produces_empty_files() {
        let r = empty_result();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            ..Default::default()
        };
        let view = apply(&r, spec);
        let grouped = view.grouped.as_ref().unwrap();
        assert!(grouped.files.is_empty());
        assert_eq!(grouped.eligible_count, 0);
        assert!(!grouped.truncated);
    }

    #[test]
    fn display_predicate_group_by_only_default_input_is_false() {
        // Grouping without filtering or truncating: all distinct files
        // appear in the grouped block, so the predicate returns false
        // (no rows reduced, no files reduced).
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(!should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_group_by_truncating_files_is_true() {
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            limit: Some(2),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(should_render_view_line(&view));
    }

    // ── Mutation killers for truncate_files_to ─────────────────────
    //
    // truncate_files_to has a tight guard: `Some(n) if n > 0 && files.len() > n`.
    // The three tests below pin each clause:
    //  - L265:22 `n > 0`        — proven by `--top 0` non-empty case
    //  - L265:41 `files.len() > n` — proven by `files.len() == n` case
    //  - L265:26 `&&` operator   — proven by `--top 0` non-empty case
    //  - L265:20 whole guard    — proven by both cases above

    #[test]
    fn group_by_file_top_zero_is_no_limit() {
        // limit=Some(0) with non-empty files MUST NOT truncate; truncated=false.
        // Mirrors the `--top 0` ergonomic where 0 means "no limit".
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            limit: Some(0),
            ..Default::default()
        };
        let view = apply(&r, spec);
        let grouped = view.grouped.as_ref().expect("grouping active");
        assert!(!grouped.truncated);
        // All 5 distinct files must be present.
        assert_eq!(grouped.files.len(), 5);
    }

    #[test]
    fn group_by_file_limit_equal_to_file_count_is_not_truncated() {
        // When limit exactly matches file count, truncated MUST be false.
        // Distinguishes `files.len() > n` (correct) from `files.len() >= n`
        // (would set truncated=true for an effectively no-op truncate).
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            limit: Some(5),
            ..Default::default()
        };
        let view = apply(&r, spec);
        let grouped = view.grouped.as_ref().expect("grouping active");
        assert!(!grouped.truncated);
        assert_eq!(grouped.files.len(), 5);
    }

    // ── Mutation killers for distinct_files ────────────────────────
    //
    // `should_render_view_line` calls `distinct_files(view.full)` to decide
    // whether grouping reduced the file count. Mutants replacing the body
    // with `0` or `1` constants are killed by these tests:
    //  - replace -> 0: filtering excludes some files; eligible_count < distinct
    //                  must NOT trigger when all files survive (background = 5)
    //  - replace -> 1: with 5 distinct files, predicate must reflect that

    #[test]
    fn display_predicate_full_grouping_no_reduction_is_false() {
        // With grouping active but no filter/truncate, view line MUST NOT
        // render. This requires distinct_files == eligible_count == 5
        // (replace-with-0 would make distinct=0, predicate fires; killed.)
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(!should_render_view_line(&view));
    }

    #[test]
    fn display_predicate_grouping_reduces_files_is_true() {
        // Filter excludes 4 of 5 files; eligible_count=1 < distinct=5.
        // Predicate must fire. Replace-with-1 would yield distinct=1=eligible
        // and predicate would NOT fire — killed.
        let r = background_fixture();
        let spec = ViewSpec {
            group_by: Some(GroupKey::File),
            filters: Filters {
                only_failing: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&r, spec);
        assert!(should_render_view_line(&view));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::test_strategies::{arb_analysis_result, arb_verdict_with_nan_coverage};
    use proptest::prelude::*;

    /// Mirror the legacy `format_table` sort: CRAP descending with stable
    /// fallback to input order on ties.
    fn legacy_sort_order(result: &AnalysisResult) -> Vec<&FunctionVerdict> {
        let mut sorted: Vec<&FunctionVerdict> = result.functions.iter().collect();
        sorted.sort_by(|a, b| {
            b.scored
                .crap
                .value
                .partial_cmp(&a.scored.crap.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Invariant 1 (Order): `apply(r, ViewSpec::default()).shown` matches
        /// the legacy sort: CRAP descending, stable on ties.
        ///
        /// Both sides borrow from `result.functions`, so pointer equality is
        /// the strictest possible witness of stable-sort agreement and is
        /// immune to any duplicate `qualified_name` the strategy might
        /// produce (CodeRabbit CR-N7).
        #[test]
        fn prop_default_spec_order_matches_legacy_sort(result in arb_analysis_result()) {
            let view = apply(&result, ViewSpec::default());
            let legacy = legacy_sort_order(&result);
            prop_assert_eq!(view.shown.len(), legacy.len());
            for (a, b) in view.shown.iter().zip(legacy.iter()) {
                prop_assert!(std::ptr::eq(*a, *b));
            }
        }

        /// Invariant 2 (Identity): the set of identities is preserved.
        #[test]
        fn prop_default_spec_preserves_identity(result in arb_analysis_result()) {
            let view = apply(&result, ViewSpec::default());
            let shown_identities: std::collections::HashSet<&crate::domain::types::FunctionIdentity> =
                view.shown.iter().map(|v| &v.scored.identity).collect();
            let original_identities: std::collections::HashSet<&crate::domain::types::FunctionIdentity> =
                result.functions.iter().map(|v| &v.scored.identity).collect();
            prop_assert_eq!(shown_identities, original_identities);
        }

        /// Invariant 3 (Summary): `view.full` borrows the original result.
        /// Stronger than equals — pointer equality.
        #[test]
        fn prop_default_spec_preserves_summary(result in arb_analysis_result()) {
            let view = apply(&result, ViewSpec::default());
            prop_assert!(std::ptr::eq(view.full, &result));
            // and shape: total_functions agrees
            prop_assert_eq!(view.full.summary.total_functions, result.summary.total_functions);
        }

        /// Invariant 4 (Display): biconditional of the predicate.
        #[test]
        fn prop_display_predicate_biconditional(result in arb_analysis_result()) {
            // Default spec: predicate must be false (no rows reduced).
            let view = apply(&result, ViewSpec::default());
            let computed = should_render_view_line(&view);
            let expected = view.eligible_count < view.full.functions.len() || view.truncated;
            prop_assert_eq!(computed, expected);
            // Default invocation reduces nothing → both sides false.
            prop_assert!(!computed);
        }

        /// `apply` never panics on NaN-coverage inputs.
        #[test]
        fn prop_apply_never_panics_with_nan_coverage(
            verdicts in prop::collection::vec(arb_verdict_with_nan_coverage(), 0..50)
        ) {
            let result = AnalysisResult {
                functions: verdicts.clone(),
                summary: crate::domain::types::AnalysisSummary {
                    total_functions: verdicts.len(),
                    total_files: verdicts.len(),
                    exceeding_threshold: 0,
                    average_crap: 0.0,
                    median_crap: 0.0,
                    max_crap: None,
                    worst_function: None,
                    distribution: crate::domain::types::RiskDistribution {
                        low: 0, acceptable: 0, moderate: 0, high: 0,
                    },
                    ..Default::default()
                },
                passed: true,
            };
            // Try every SortKey + a coverage-range filter.
            for sort in [SortKey::Crap, SortKey::Coverage, SortKey::Complexity, SortKey::Path] {
                let spec = ViewSpec {
                    filters: Filters {
                        coverage_range: Some(CoverageRange::new(0.0, 100.0).unwrap()),
                        ..Default::default()
                    },
                    sort,
                    limit: Some(10),
                    ..Default::default()
                };
                let _ = apply(&result, spec);
            }
        }

        /// Filters AND-compose into the *intersection* of each filter's
        /// result set: `apply` with both `only_failing` and a
        /// `coverage_range` active yields exactly the functions that pass
        /// `only_failing` alone AND those that pass the range alone.
        ///
        /// Strictly stronger than an example check of `shown ⊆ both`: this
        /// is full set-equality (no false inclusions AND no false
        /// exclusions) over arbitrary inputs and arbitrary filter
        /// combinations. Identity sets are unambiguous because
        /// `arb_analysis_result` dedups `(file_path, qualified_name)`.
        #[test]
        fn prop_filters_and_compose(
            result in arb_analysis_result(),
            only_failing in any::<bool>(),
            band in prop::option::of((0.0..=100.0f64, 0.0..=100.0f64)),
        ) {
            use crate::domain::types::FunctionIdentity;
            use std::collections::HashSet;

            let coverage_range = band.map(|(a, b)| {
                let (min, max) = if a <= b { (a, b) } else { (b, a) };
                CoverageRange::new(min, max).expect("min <= max within [0,100] is valid")
            });

            let view_both = apply(
                &result,
                ViewSpec {
                    filters: Filters { only_failing, coverage_range },
                    ..Default::default()
                },
            );
            let view_failing = apply(
                &result,
                ViewSpec {
                    filters: Filters { only_failing, coverage_range: None },
                    ..Default::default()
                },
            );
            let view_range = apply(
                &result,
                ViewSpec {
                    filters: Filters { only_failing: false, coverage_range },
                    ..Default::default()
                },
            );

            let both: HashSet<&FunctionIdentity> =
                view_both.shown.iter().map(|v| &v.scored.identity).collect();
            let failing_only: HashSet<&FunctionIdentity> =
                view_failing.shown.iter().map(|v| &v.scored.identity).collect();
            let range_only: HashSet<&FunctionIdentity> =
                view_range.shown.iter().map(|v| &v.scored.identity).collect();
            let intersection: HashSet<&FunctionIdentity> =
                failing_only.intersection(&range_only).copied().collect();

            prop_assert_eq!(both, intersection);
        }

        /// Result-block immutability under ANY spec: the `result` block —
        /// its function set, summary, and pass/fail verdict — is identical
        /// no matter how aggressively the spec filters, sorts, truncates,
        /// or groups. `apply` borrows the analysis immutably and never
        /// replaces it, so `view.full` is pointer-equal to the input (the
        /// strictest witness); the scalar checks document the user-facing
        /// promise that a JSON consumer's `result.*` reads the same whether
        /// or not a view was shaped. Generalizes
        /// `prop_default_spec_preserves_summary` from the default spec to
        /// the full spec space (arbitrary filter, sort, limit, and
        /// grouping).
        #[test]
        fn prop_result_block_invariant_under_any_spec(
            result in arb_analysis_result(),
            only_failing in any::<bool>(),
            band in prop::option::of((0.0..=100.0f64, 0.0..=100.0f64)),
            sort in prop_oneof![
                Just(SortKey::Crap),
                Just(SortKey::Coverage),
                Just(SortKey::Complexity),
                Just(SortKey::Path),
            ],
            limit in prop::option::of(0usize..20),
            group_by in prop::option::of(Just(GroupKey::File)),
        ) {
            let coverage_range = band.map(|(a, b)| {
                let (min, max) = if a <= b { (a, b) } else { (b, a) };
                CoverageRange::new(min, max).expect("min <= max within [0,100] is valid")
            });
            // Capture the result block BEFORE `apply` borrows it.
            let baseline_total = result.summary.total_functions;
            let baseline_exceeding = result.summary.exceeding_threshold;
            let baseline_passed = result.passed;

            let view = apply(
                &result,
                ViewSpec {
                    filters: Filters { only_failing, coverage_range },
                    sort,
                    limit,
                    group_by,
                },
            );

            // The result block is the unmutated original — pointer identity
            // proves the function set, summary, and verdict are all untouched.
            prop_assert!(std::ptr::eq(view.full, &result));
            // Scalar witnesses of the same invariant, for readability.
            prop_assert_eq!(view.full.summary.total_functions, baseline_total);
            prop_assert_eq!(view.full.summary.exceeding_threshold, baseline_exceeding);
            prop_assert_eq!(view.full.passed, baseline_passed);
        }
    }
}
