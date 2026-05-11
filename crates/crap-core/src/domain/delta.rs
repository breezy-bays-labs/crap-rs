//! Domain-level Delta abstraction — pure-domain comparison primitive
//! between two `AnalysisResult` values (baseline + current).
//!
//! ```text
//! delta::compute(baseline, current) → AnalysisDelta
//! ```
//!
//! Sibling to [`crate::domain::view`] (ADR D7 §DeltaView). The Delta
//! identifies functions Added / Removed / Modified across two analyses
//! and computes summary aggregates including a *new-violations* count
//! that surfaces only the threshold breaches this delta introduced —
//! pre-existing debt is not double-counted as a new regression.
//!
//! Pure domain code — no I/O, no `syn`, no `PathBuf` semantics. Future
//! `crap-core` extraction takes this module whole.

use crate::domain::types::{AnalysisResult, FunctionIdentity, FunctionVerdict};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

// ── Change kinds ─────────────────────────────────────────────────────

/// Classification of a single function across the baseline → current
/// transition.
///
/// Tag-only enum used in `DeltaSummary` and the JSON envelope. The
/// per-function payload lives on [`FunctionChange`]; this enum is for
/// shaping (filter / sort) and presentation. `#[non_exhaustive]`
/// reserves namespace for future variants like `Renamed` (ADR D7
/// §Future Compat — rename detection is a v0.3.0 candidate).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

impl ChangeKind {
    /// All known variants, in canonical order. Used by CLI parsers and
    /// serializers that enumerate the universe.
    pub const ALL: [ChangeKind; 3] = [ChangeKind::Added, ChangeKind::Removed, ChangeKind::Modified];

    pub fn as_str(&self) -> &'static str {
        self.as_wire_str()
    }

    /// Canonical wire string — equal to the serde JSON representation
    /// (sans quotes). See `ContributorKind::as_wire_str` for the
    /// rationale; equality with serde is pinned in
    /// `tests::wire_str_matches_serde`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
            ChangeKind::Modified => "modified",
        }
    }
}

// ── Per-function change payload ──────────────────────────────────────

/// A single function's change between baseline and current.
///
/// Variants carry the full [`FunctionVerdict`] for both sides where
/// applicable so reporters can render baseline / current scores side by
/// side without re-querying the original `AnalysisResult`s. The
/// [`Modified`](FunctionChange::Modified) variant is the dominant case
/// for established codebases; `Added` and `Removed` track structural
/// drift.
///
/// `Unchanged` is *not* a variant. The View pipeline only surfaces
/// changes; if downstream tooling needs to enumerate all current
/// functions it consumes `current.functions` directly.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FunctionChange {
    Added {
        current: FunctionVerdict,
    },
    Removed {
        baseline: FunctionVerdict,
    },
    Modified {
        baseline: FunctionVerdict,
        current: FunctionVerdict,
    },
}

impl FunctionChange {
    pub fn kind(&self) -> ChangeKind {
        match self {
            FunctionChange::Added { .. } => ChangeKind::Added,
            FunctionChange::Removed { .. } => ChangeKind::Removed,
            FunctionChange::Modified { .. } => ChangeKind::Modified,
        }
    }

    pub fn current_score(&self) -> Option<f64> {
        match self {
            FunctionChange::Added { current } => Some(current.scored.crap.value),
            FunctionChange::Modified { current, .. } => Some(current.scored.crap.value),
            FunctionChange::Removed { .. } => None,
        }
    }

    pub fn baseline_score(&self) -> Option<f64> {
        match self {
            FunctionChange::Removed { baseline } => Some(baseline.scored.crap.value),
            FunctionChange::Modified { baseline, .. } => Some(baseline.scored.crap.value),
            FunctionChange::Added { .. } => None,
        }
    }

    /// `current - baseline` for `Modified`; `None` for `Added` / `Removed`.
    pub fn score_delta(&self) -> Option<f64> {
        match self {
            FunctionChange::Modified { baseline, current } => {
                Some(current.scored.crap.value - baseline.scored.crap.value)
            }
            _ => None,
        }
    }

    /// File path associated with this change. For Modified, baseline
    /// and current share the same path (matching keys on file_path).
    pub fn file_path(&self) -> &str {
        match self {
            FunctionChange::Added { current } => &current.scored.identity.file_path,
            FunctionChange::Removed { baseline } => &baseline.scored.identity.file_path,
            FunctionChange::Modified { current, .. } => &current.scored.identity.file_path,
        }
    }

    /// Qualified name associated with this change.
    pub fn qualified_name(&self) -> &str {
        match self {
            FunctionChange::Added { current } => &current.scored.identity.qualified_name,
            FunctionChange::Removed { baseline } => &baseline.scored.identity.qualified_name,
            FunctionChange::Modified { current, .. } => &current.scored.identity.qualified_name,
        }
    }
}

// ── Summary ──────────────────────────────────────────────────────────

/// Aggregate counts over a set of [`FunctionChange`]s.
///
/// `passed` is the **delta gate**: true iff `new_violations == 0`. The
/// CLI gates exit code on this only when `--delta-gate` is passed;
/// otherwise the delta is informational and `result.passed` alone
/// drives the exit code.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct DeltaSummary {
    pub added: u32,
    pub removed: u32,
    pub modified: u32,
    /// Modified rows where `score_delta > 0` (current got worse).
    pub regressions: u32,
    /// Modified rows where `score_delta < 0` (current got better).
    pub improvements: u32,
    /// Threshold breaches *introduced* by this delta:
    /// - `Added` rows whose `current.exceeds == true`
    /// - `Modified` rows where `baseline.exceeds == false` AND `current.exceeds == true`
    ///
    /// Pre-existing violations (Modified rows where `baseline.exceeds`
    /// was already true) do NOT contribute. This distinction matters
    /// for the delta gate — we want to fail PRs that *introduce* risk,
    /// not PRs that merely touch already-failing functions.
    pub new_violations: u32,
    /// `new_violations == 0`. Drives the optional `--delta-gate`.
    pub passed: bool,
}

impl DeltaSummary {
    pub fn compute(changes: &[FunctionChange]) -> Self {
        let mut summary = Self::default();
        for change in changes {
            tally(&mut summary, change);
        }
        summary.passed = summary.new_violations == 0;
        summary
    }
}

fn tally(summary: &mut DeltaSummary, change: &FunctionChange) {
    match change {
        FunctionChange::Added { current } => {
            summary.added += 1;
            if current.exceeds {
                summary.new_violations += 1;
            }
        }
        FunctionChange::Removed { .. } => {
            summary.removed += 1;
        }
        FunctionChange::Modified { baseline, current } => {
            summary.modified += 1;
            tally_modified(summary, baseline, current);
        }
    }
}

fn tally_modified(
    summary: &mut DeltaSummary,
    baseline: &FunctionVerdict,
    current: &FunctionVerdict,
) {
    let delta = current.scored.crap.value - baseline.scored.crap.value;
    if delta > 0.0 {
        summary.regressions += 1;
    } else if delta < 0.0 {
        summary.improvements += 1;
    }
    if !baseline.exceeds && current.exceeds {
        summary.new_violations += 1;
    }
}

// ── AnalysisDelta ────────────────────────────────────────────────────

/// The product of comparing two [`AnalysisResult`]s.
///
/// `baseline` and `current` are owned (consumed by [`compute`]) so
/// downstream borrows remain valid for the whole `AnalysisDelta`
/// lifetime. The `changes` vector contains every Added / Removed /
/// Modified function — never `Unchanged`. Reporter consumers iterate
/// `changes`; the delta gate consumes `summary` (via
/// [`DeltaSummary::compute`]) once the changes are known.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisDelta {
    /// Owned baseline analysis. `#[serde(skip)]` because the JSON
    /// envelope's `result_baseline` (future work) or the existing
    /// `result` block carry the canonical baseline / current data;
    /// double-emitting it inside `delta` would bloat the payload.
    /// In-memory consumers (reporters) borrow it.
    #[serde(skip)]
    pub baseline: AnalysisResult,
    #[serde(skip)]
    pub current: AnalysisResult,
    pub changes: Vec<FunctionChange>,
    /// Aggregate counts over `changes`. Computed once at construction
    /// (in [`compute`]) so reporters and the delta gate share a
    /// single source of truth — pre-shape, view-independent.
    pub summary: DeltaSummary,
}

// ── compute: pair → classify ────────────────────────────────────────

/// Identity tuple used to match functions across the baseline → current
/// transition. **Span (line range) is intentionally excluded** — line
/// numbers shift when surrounding code is edited, and we want a
/// minor edit to `foo` to register as `Modified`, not `Removed` +
/// `Added`. Renames within a file (different `qualified_name`) and
/// moves across files (different `file_path`) both fall through as
/// `Removed` + `Added` until rename detection ships (v0.3.0+).
type IdentityKey<'a> = (&'a str, &'a str);

fn identity_key(identity: &FunctionIdentity) -> IdentityKey<'_> {
    (&identity.file_path, &identity.qualified_name)
}

/// Compare two analyses, classifying every function as Added, Removed,
/// or Modified. Stable: `current`'s order is preserved for matched +
/// added rows, then baseline-only (`Removed`) rows trail.
///
/// Decomposed into a private helper `pair_identities` to keep the
/// public surface declarative and to localize the HashMap construction
/// for testing.
pub fn compute(baseline: AnalysisResult, current: AnalysisResult) -> AnalysisDelta {
    let changes = pair_identities(&baseline, &current);
    let summary = DeltaSummary::compute(&changes);
    AnalysisDelta {
        baseline,
        current,
        changes,
        summary,
    }
}

fn pair_identities(baseline: &AnalysisResult, current: &AnalysisResult) -> Vec<FunctionChange> {
    // Index baseline by identity key, single pass. We track which
    // baseline entries we've matched so the leftover can be emitted as
    // `Removed` rows after the current sweep.
    let mut baseline_index: HashMap<IdentityKey<'_>, &FunctionVerdict> =
        HashMap::with_capacity(baseline.functions.len());
    for verdict in &baseline.functions {
        baseline_index.insert(identity_key(&verdict.scored.identity), verdict);
    }

    let mut changes: Vec<FunctionChange> =
        Vec::with_capacity(current.functions.len() + baseline.functions.len());

    for current_verdict in &current.functions {
        let key = identity_key(&current_verdict.scored.identity);
        match baseline_index.remove(&key) {
            Some(baseline_verdict) => changes.push(FunctionChange::Modified {
                baseline: baseline_verdict.clone(),
                current: current_verdict.clone(),
            }),
            None => changes.push(FunctionChange::Added {
                current: current_verdict.clone(),
            }),
        }
    }

    // Leftover baseline entries are Removed. `HashMap` iteration order
    // is unspecified, so we sort by identity key before emission —
    // otherwise consumers that iterate `delta.changes` directly (or
    // apply a sort that doesn't break ties on identity) observe
    // run-to-run flakiness. Identity-key sort is cheap, deterministic,
    // and gives a stable presentation order that mirrors the lexical
    // ordering most operators expect.
    let mut leftover: Vec<&FunctionVerdict> = baseline_index.into_values().collect();
    leftover
        .sort_by(|a, b| identity_key(&a.scored.identity).cmp(&identity_key(&b.scored.identity)));
    for baseline_verdict in leftover {
        changes.push(FunctionChange::Removed {
            baseline: baseline_verdict.clone(),
        });
    }

    changes
}

// ── DeltaView (filter / sort / truncate) ─────────────────────────────

/// Spec describing how to shape the per-change row list for display.
///
/// Mirrors [`crate::domain::view::ViewSpec`] in structure but with
/// delta-specific filter and sort dimensions. The summary on the
/// underlying [`AnalysisDelta`] is *not* re-derived from the shaped
/// row list — it always reflects the unshaped change set so the gate
/// keystone holds.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeltaViewSpec {
    pub filters: DeltaFilters,
    pub sort: DeltaSortKey,
    pub limit: Option<usize>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeltaFilters {
    /// When `Some`, retain only changes whose [`ChangeKind`] is in the
    /// set. `None` means "all kinds." A `Some(empty)` set retains
    /// nothing — that's a valid (if pointless) configuration the
    /// shape pipeline doesn't second-guess.
    pub change_kinds: Option<BTreeSet<ChangeKind>>,
    /// Inclusive lower bound on `score_delta`. `None` = no bound.
    /// Only applies to [`FunctionChange::Modified`] entries —
    /// `Added` / `Removed` have no score_delta and pass the bound
    /// check unconditionally.
    pub min_score_delta: Option<f64>,
    /// Inclusive upper bound on `score_delta`. Same conventions as
    /// `min_score_delta`.
    pub max_score_delta: Option<f64>,
}

/// Sort key for the displayed delta view.
///
/// `ScoreDelta` (default) ranks rows by *signed impact*, descending —
/// regressions first. `Modified` uses `current - baseline`; `Added`
/// uses `+current.crap` (a new function exists where there was none —
/// pure load); `Removed` uses `-baseline.crap` (a function went
/// away — pure relief). Sort descending: regressions and risky
/// additions land at the top of the scorecard, improvements and
/// benign removals at the bottom.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaSortKey {
    /// Magnitude of change descending (regressions first).
    #[default]
    ScoreDelta,
    /// Current CRAP score descending. `Removed` rows (no current
    /// score) sort last.
    CurrentCrap,
    /// Baseline CRAP score descending. `Added` rows sort last.
    BaselineCrap,
    /// Alphabetical by `file_path`, then `qualified_name`.
    Path,
}

impl DeltaSortKey {
    /// Canonical wire string — see `ContributorKind::as_wire_str`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::ScoreDelta => "score_delta",
            Self::CurrentCrap => "current_crap",
            Self::BaselineCrap => "baseline_crap",
            Self::Path => "path",
        }
    }
}

/// Shaped view over an [`AnalysisDelta`].
///
/// `full` borrows the parent delta — the gate keystone: shaping never
/// mutates the underlying delta, and the summary surfaced through
/// reporters always derives from `full.summary`, not `shown`.
#[non_exhaustive]
#[derive(Debug, Serialize)]
pub struct DeltaView<'a> {
    /// Borrow of the parent. Skipped in serialization — the JSON
    /// envelope already carries the summary + full change list under
    /// the `delta` block.
    #[serde(skip)]
    pub full: &'a AnalysisDelta,
    pub spec: DeltaViewSpec,
    /// Post-filter, pre-truncate count. Combined with `truncated`,
    /// lets consumers render "Showing N of M (filtered from K)".
    pub eligible_count: usize,
    pub truncated: bool,
    pub shown: Vec<&'a FunctionChange>,
}

/// Shape an [`AnalysisDelta`] into a [`DeltaView`].
///
/// Order of operations: filter → sort → truncate. Mirrors the
/// `view::apply` pattern. The full delta and its summary are
/// untouched.
pub fn apply<'a>(delta: &'a AnalysisDelta, spec: DeltaViewSpec) -> DeltaView<'a> {
    let mut shown: Vec<&'a FunctionChange> = apply_filters(&delta.changes, &spec.filters);
    let eligible_count = shown.len();
    sort_in_place(&mut shown, spec.sort);
    let truncated = truncate_to(&mut shown, spec.limit);
    DeltaView {
        full: delta,
        spec,
        eligible_count,
        truncated,
        shown,
    }
}

fn apply_filters<'a>(
    changes: &'a [FunctionChange],
    filters: &DeltaFilters,
) -> Vec<&'a FunctionChange> {
    changes
        .iter()
        .filter(|c| {
            filters
                .change_kinds
                .as_ref()
                .is_none_or(|kinds| kinds.contains(&c.kind()))
        })
        .filter(|c| matches_score_delta_range(c, filters))
        .collect()
}

fn matches_score_delta_range(change: &FunctionChange, filters: &DeltaFilters) -> bool {
    let Some(delta) = change.score_delta() else {
        // Added / Removed have no score_delta — pass through both
        // bounds (filtering them out is the operator's job via
        // `change_kinds`).
        return true;
    };
    let bounded = filters.min_score_delta.is_some() || filters.max_score_delta.is_some();
    if bounded && !delta.is_finite() {
        // Reject non-finite deltas only when a bound is in play —
        // otherwise a corrupt baseline silently disappears from the
        // delta view while still counting in the summary. Without
        // bounds, pass everything through; with bounds, the
        // comparison would be undefined anyway.
        return false;
    }
    if filters.min_score_delta.is_some_and(|min| delta < min) {
        return false;
    }
    if filters.max_score_delta.is_some_and(|max| delta > max) {
        return false;
    }
    true
}

fn sort_in_place(shown: &mut [&FunctionChange], key: DeltaSortKey) {
    match key {
        DeltaSortKey::ScoreDelta => shown.sort_by(cmp_by_score_delta_desc),
        DeltaSortKey::CurrentCrap => shown.sort_by(cmp_by_current_crap_desc),
        DeltaSortKey::BaselineCrap => shown.sort_by(cmp_by_baseline_crap_desc),
        DeltaSortKey::Path => shown.sort_by(cmp_by_path),
    }
}

/// Signed-impact ordering: regressions and risky additions sort to the
/// top, improvements and benign removals to the bottom. `Modified`
/// uses `current - baseline`; `Added` is treated as `+current.crap`
/// (introducing load); `Removed` is treated as `-baseline.crap`
/// (shedding load).
fn cmp_by_score_delta_desc(a: &&FunctionChange, b: &&FunctionChange) -> Ordering {
    cmp_f64_desc(signed_impact(a), signed_impact(b))
}

fn signed_impact(change: &FunctionChange) -> f64 {
    match change {
        FunctionChange::Modified { baseline, current } => {
            current.scored.crap.value - baseline.scored.crap.value
        }
        FunctionChange::Added { current } => current.scored.crap.value,
        FunctionChange::Removed { baseline } => -baseline.scored.crap.value,
    }
}

fn cmp_by_current_crap_desc(a: &&FunctionChange, b: &&FunctionChange) -> Ordering {
    // Removed entries (no current score) sort last under any
    // current-crap ordering. `Option::None` < `Some(_)` ascending,
    // so we invert by mapping Some → 0 (front) and None → 1 (back).
    let (rank_a, score_a) = current_score_rank(a);
    let (rank_b, score_b) = current_score_rank(b);
    rank_a.cmp(&rank_b).then(cmp_f64_desc(score_a, score_b))
}

fn current_score_rank(change: &FunctionChange) -> (u8, f64) {
    match change.current_score() {
        Some(s) => (0, s),
        None => (1, 0.0),
    }
}

fn cmp_by_baseline_crap_desc(a: &&FunctionChange, b: &&FunctionChange) -> Ordering {
    let (rank_a, score_a) = baseline_score_rank(a);
    let (rank_b, score_b) = baseline_score_rank(b);
    rank_a.cmp(&rank_b).then(cmp_f64_desc(score_a, score_b))
}

fn baseline_score_rank(change: &FunctionChange) -> (u8, f64) {
    match change.baseline_score() {
        Some(s) => (0, s),
        None => (1, 0.0),
    }
}

fn cmp_by_path(a: &&FunctionChange, b: &&FunctionChange) -> Ordering {
    a.file_path()
        .cmp(b.file_path())
        .then_with(|| a.qualified_name().cmp(b.qualified_name()))
}

/// Total f64 ordering, descending. NaN sorts last so non-finite
/// scores never break the comparator.
fn cmp_f64_desc(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => b.partial_cmp(&a).expect("non-NaN partial_cmp infallible"),
    }
}

fn truncate_to(shown: &mut Vec<&FunctionChange>, limit: Option<usize>) -> bool {
    match limit {
        Some(n) if n > 0 && shown.len() > n => {
            shown.truncate(n);
            true
        }
        _ => false,
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        AnalysisSummary, ComplexityMetric, CrapScore, FunctionIdentity, FunctionVerdict,
        RiskDistribution, RiskLevel, ScoredFunction, SourceSpan,
    };

    fn make_verdict(file: &str, name: &str, score: f64, exceeds: bool) -> FunctionVerdict {
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: file.to_string(),
                    qualified_name: name.to_string(),
                    span: SourceSpan {
                        start_line: 1,
                        end_line: 5,
                        start_column: 0,
                        end_column: 0,
                    },
                },
                complexity: 5,
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: 50.0,
                crap: CrapScore {
                    value: score,
                    risk_level: if score > 30.0 {
                        RiskLevel::High
                    } else if score > 8.0 {
                        RiskLevel::Moderate
                    } else if score > 5.0 {
                        RiskLevel::Acceptable
                    } else {
                        RiskLevel::Low
                    },
                },
                contributors: vec![],
            },
            threshold: 25.0,
            exceeds,
            diagnostic: None,
        }
    }

    fn make_result(verdicts: Vec<FunctionVerdict>) -> AnalysisResult {
        let exceeding = verdicts.iter().filter(|v| v.exceeds).count();
        let total = verdicts.len();
        AnalysisResult {
            functions: verdicts,
            summary: AnalysisSummary {
                total_functions: total,
                total_files: 1,
                exceeding_threshold: exceeding,
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
            passed: exceeding == 0,
        }
    }

    // ── classification ──

    #[test]
    fn compute_identity_yields_all_modified_zero_delta() {
        let result = make_result(vec![
            make_verdict("a.rs", "alpha", 5.0, false),
            make_verdict("a.rs", "beta", 12.0, false),
            make_verdict("b.rs", "gamma", 47.0, true),
        ]);
        let delta = compute(result.clone(), result);
        assert_eq!(delta.changes.len(), 3);
        for change in &delta.changes {
            assert!(matches!(change, FunctionChange::Modified { .. }));
            assert_eq!(change.score_delta(), Some(0.0));
        }
    }

    #[test]
    fn compute_classifies_added_function() {
        let baseline = make_result(vec![]);
        let current = make_result(vec![make_verdict("a.rs", "new_fn", 10.0, false)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Added { .. }));
        assert_eq!(delta.changes[0].current_score(), Some(10.0));
        assert_eq!(delta.changes[0].baseline_score(), None);
        assert_eq!(delta.changes[0].score_delta(), None);
    }

    #[test]
    fn compute_classifies_removed_function() {
        let baseline = make_result(vec![make_verdict("a.rs", "old_fn", 8.0, false)]);
        let current = make_result(vec![]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Removed { .. }));
        assert_eq!(delta.changes[0].baseline_score(), Some(8.0));
        assert_eq!(delta.changes[0].current_score(), None);
    }

    #[test]
    fn compute_classifies_modified_function() {
        let baseline = make_result(vec![make_verdict("a.rs", "fn_a", 8.0, false)]);
        let current = make_result(vec![make_verdict("a.rs", "fn_a", 24.0, false)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Modified { .. }));
        assert_eq!(delta.changes[0].baseline_score(), Some(8.0));
        assert_eq!(delta.changes[0].current_score(), Some(24.0));
        assert_eq!(delta.changes[0].score_delta(), Some(16.0));
    }

    #[test]
    fn compute_same_name_different_files_are_separate() {
        let baseline = make_result(vec![make_verdict("a.rs", "log", 5.0, false)]);
        let current = make_result(vec![make_verdict("b.rs", "log", 5.0, false)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 2);
        let kinds: Vec<_> = delta.changes.iter().map(|c| c.kind()).collect();
        assert!(kinds.contains(&ChangeKind::Added));
        assert!(kinds.contains(&ChangeKind::Removed));
    }

    #[test]
    fn compute_same_file_rename_produces_add_remove() {
        let baseline = make_result(vec![make_verdict("a.rs", "v1", 5.0, false)]);
        let current = make_result(vec![make_verdict("a.rs", "v2", 5.0, false)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 2);
        let kinds: Vec<_> = delta.changes.iter().map(|c| c.kind()).collect();
        assert!(kinds.contains(&ChangeKind::Added));
        assert!(kinds.contains(&ChangeKind::Removed));
    }

    #[test]
    fn compute_ignores_span_when_matching() {
        // Same identity (file, name), different spans -> Modified, not Add+Remove
        let mut baseline_v = make_verdict("a.rs", "fn_a", 5.0, false);
        baseline_v.scored.identity.span = SourceSpan {
            start_line: 1,
            end_line: 5,
            start_column: 0,
            end_column: 0,
        };
        let mut current_v = make_verdict("a.rs", "fn_a", 5.0, false);
        current_v.scored.identity.span = SourceSpan {
            start_line: 100,
            end_line: 105,
            start_column: 0,
            end_column: 0,
        };
        let delta = compute(make_result(vec![baseline_v]), make_result(vec![current_v]));
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Modified { .. }));
    }

    #[test]
    fn removed_rows_are_emitted_in_identity_key_order() {
        // Removed-row determinism: pair_identities collects leftover
        // baseline entries from a HashMap; iterating the map directly
        // produces non-deterministic order. Sort by (file_path,
        // qualified_name) before emission so consumers that don't
        // apply a tie-breaking sort see a stable order.
        let baseline = make_result(vec![
            make_verdict("zeta.rs", "zeta_fn", 5.0, false),
            make_verdict("alpha.rs", "alpha_fn", 5.0, false),
            make_verdict("beta.rs", "beta_fn", 5.0, false),
        ]);
        // Empty current → all baseline entries are Removed.
        let current = make_result(vec![]);
        let delta = compute(baseline, current);

        // Should be sorted by (file_path, qualified_name) ascending —
        // alpha.rs < beta.rs < zeta.rs.
        assert_eq!(delta.changes.len(), 3);
        assert_eq!(delta.changes[0].file_path(), "alpha.rs");
        assert_eq!(delta.changes[1].file_path(), "beta.rs");
        assert_eq!(delta.changes[2].file_path(), "zeta.rs");
    }

    // ── summary counts ──

    #[test]
    fn summary_counts_added_removed_modified() {
        let changes = vec![
            FunctionChange::Added {
                current: make_verdict("a.rs", "new", 5.0, false),
            },
            FunctionChange::Removed {
                baseline: make_verdict("a.rs", "old", 5.0, false),
            },
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "fn_a", 5.0, false),
                current: make_verdict("a.rs", "fn_a", 8.0, false),
            },
        ];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.added, 1);
        assert_eq!(summary.removed, 1);
        assert_eq!(summary.modified, 1);
    }

    #[test]
    fn summary_regressions_are_modified_with_positive_delta() {
        let changes = vec![FunctionChange::Modified {
            baseline: make_verdict("a.rs", "fn_a", 5.0, false),
            current: make_verdict("a.rs", "fn_a", 10.0, false),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.regressions, 1);
        assert_eq!(summary.improvements, 0);
    }

    #[test]
    fn summary_improvements_are_modified_with_negative_delta() {
        let changes = vec![FunctionChange::Modified {
            baseline: make_verdict("a.rs", "fn_a", 47.0, true),
            current: make_verdict("a.rs", "fn_a", 12.0, false),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.regressions, 0);
        assert_eq!(summary.improvements, 1);
    }

    #[test]
    fn summary_zero_delta_neither_regression_nor_improvement() {
        let changes = vec![FunctionChange::Modified {
            baseline: make_verdict("a.rs", "fn_a", 5.0, false),
            current: make_verdict("a.rs", "fn_a", 5.0, false),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.regressions, 0);
        assert_eq!(summary.improvements, 0);
    }

    #[test]
    fn summary_new_violation_added_function_failing() {
        let changes = vec![FunctionChange::Added {
            current: make_verdict("a.rs", "new_bad", 31.0, true),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.new_violations, 1);
        assert!(!summary.passed);
    }

    #[test]
    fn summary_new_violation_modified_crossing_threshold() {
        let changes = vec![FunctionChange::Modified {
            baseline: make_verdict("a.rs", "fn_a", 8.0, false),
            current: make_verdict("a.rs", "fn_a", 47.0, true),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.new_violations, 1);
        assert_eq!(summary.regressions, 1);
    }

    #[test]
    fn summary_no_new_violation_when_modified_still_passing() {
        let changes = vec![FunctionChange::Modified {
            baseline: make_verdict("a.rs", "fn_a", 8.0, false),
            current: make_verdict("a.rs", "fn_a", 20.0, false),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.regressions, 1);
        assert_eq!(summary.new_violations, 0);
        assert!(summary.passed);
    }

    #[test]
    fn summary_pre_existing_violation_does_not_count_as_new() {
        let changes = vec![FunctionChange::Modified {
            baseline: make_verdict("a.rs", "fn_a", 47.0, true),
            current: make_verdict("a.rs", "fn_a", 60.0, true),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.regressions, 1);
        assert_eq!(summary.new_violations, 0);
        assert!(summary.passed);
    }

    #[test]
    fn summary_added_passing_function_not_a_new_violation() {
        let changes = vec![FunctionChange::Added {
            current: make_verdict("a.rs", "new_good", 5.0, false),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.added, 1);
        assert_eq!(summary.new_violations, 0);
    }

    #[test]
    fn summary_removed_function_never_counts_as_new_violation() {
        let changes = vec![FunctionChange::Removed {
            baseline: make_verdict("a.rs", "old_bad", 47.0, true),
        }];
        let summary = DeltaSummary::compute(&changes);
        assert_eq!(summary.removed, 1);
        assert_eq!(summary.new_violations, 0);
        assert!(summary.passed);
    }

    #[test]
    fn summary_passed_iff_new_violations_zero() {
        let zero = DeltaSummary::compute(&[]);
        assert!(zero.passed);

        let with_new = DeltaSummary::compute(&[FunctionChange::Added {
            current: make_verdict("a.rs", "bad", 31.0, true),
        }]);
        assert!(!with_new.passed);
    }

    // ── change accessors ──

    #[test]
    fn change_kind_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&ChangeKind::Added).unwrap(),
            "\"added\""
        );
        assert_eq!(
            serde_json::to_string(&ChangeKind::Modified).unwrap(),
            "\"modified\""
        );
        assert_eq!(
            serde_json::to_string(&ChangeKind::Removed).unwrap(),
            "\"removed\""
        );
    }

    #[test]
    fn change_kind_all_contains_every_variant() {
        assert_eq!(ChangeKind::ALL.len(), 3);
        assert!(ChangeKind::ALL.contains(&ChangeKind::Added));
        assert!(ChangeKind::ALL.contains(&ChangeKind::Removed));
        assert!(ChangeKind::ALL.contains(&ChangeKind::Modified));
    }

    #[test]
    fn change_kind_as_str_matches_serde() {
        for kind in ChangeKind::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            let stripped = json.trim_matches('"');
            assert_eq!(kind.as_str(), stripped);
        }
    }

    #[test]
    fn change_file_path_and_qualified_name_accessors() {
        let added = FunctionChange::Added {
            current: make_verdict("src/foo.rs", "module::fn_a", 5.0, false),
        };
        assert_eq!(added.file_path(), "src/foo.rs");
        assert_eq!(added.qualified_name(), "module::fn_a");

        let removed = FunctionChange::Removed {
            baseline: make_verdict("src/bar.rs", "module::fn_b", 5.0, false),
        };
        assert_eq!(removed.file_path(), "src/bar.rs");

        let modified = FunctionChange::Modified {
            baseline: make_verdict("src/baz.rs", "module::fn_c", 5.0, false),
            current: make_verdict("src/baz.rs", "module::fn_c", 10.0, false),
        };
        assert_eq!(modified.file_path(), "src/baz.rs");
    }

    // ── envelope ──

    #[test]
    fn analysis_delta_carries_baseline_current_and_changes() {
        let baseline = make_result(vec![make_verdict("a.rs", "fn_a", 5.0, false)]);
        let current = make_result(vec![make_verdict("a.rs", "fn_a", 5.0, false)]);
        let delta = compute(baseline.clone(), current.clone());
        assert_eq!(delta.baseline.functions.len(), baseline.functions.len());
        assert_eq!(delta.current.functions.len(), current.functions.len());
        assert_eq!(delta.changes.len(), 1);
    }

    #[test]
    fn empty_inputs_produce_empty_delta() {
        let delta = compute(make_result(vec![]), make_result(vec![]));
        assert!(delta.changes.is_empty());
        let summary = DeltaSummary::compute(&delta.changes);
        assert_eq!(summary.added, 0);
        assert_eq!(summary.removed, 0);
        assert_eq!(summary.modified, 0);
        assert!(summary.passed);
    }

    // ── DeltaView / apply ──

    fn delta_with_changes(changes: Vec<FunctionChange>) -> AnalysisDelta {
        AnalysisDelta {
            baseline: make_result(vec![]),
            current: make_result(vec![]),
            summary: DeltaSummary::compute(&changes),
            changes,
        }
    }

    #[test]
    fn apply_default_spec_returns_all_changes() {
        let delta = delta_with_changes(vec![
            FunctionChange::Added {
                current: make_verdict("a.rs", "x", 31.0, true),
            },
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "y", 5.0, false),
                current: make_verdict("a.rs", "y", 10.0, false),
            },
        ]);
        let view = apply(&delta, DeltaViewSpec::default());
        assert_eq!(view.shown.len(), 2);
        assert_eq!(view.eligible_count, 2);
        assert!(!view.truncated);
    }

    #[test]
    fn apply_default_sorts_by_signed_impact_descending() {
        // Signed impacts: small_mod=+1, big_mod=+20, big_added=+31
        let delta = delta_with_changes(vec![
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "small_mod", 5.0, false),
                current: make_verdict("a.rs", "small_mod", 6.0, false),
            },
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "big_mod", 5.0, false),
                current: make_verdict("a.rs", "big_mod", 25.0, false),
            },
            FunctionChange::Added {
                current: make_verdict("a.rs", "big_added", 31.0, true),
            },
        ]);
        let view = apply(&delta, DeltaViewSpec::default());
        assert_eq!(view.shown[0].qualified_name(), "big_added");
        assert_eq!(view.shown[1].qualified_name(), "big_mod");
        assert_eq!(view.shown[2].qualified_name(), "small_mod");
    }

    #[test]
    fn apply_default_sort_puts_regressions_above_improvements() {
        // Signed impacts: big_improvement=-25, small_regression=+5,
        // big_removed=-30 (Removed is treated as -baseline.crap),
        // big_added=+10. Ranking descending must be:
        //   small_regression (+5) > big_added (+10? no, +10 > +5)
        // Wait: +10 > +5, so big_added first, then small_regression,
        // then big_improvement (-25), then big_removed (-30).
        let delta = delta_with_changes(vec![
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "big_improvement", 30.0, true),
                current: make_verdict("a.rs", "big_improvement", 5.0, false),
            },
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "small_regression", 5.0, false),
                current: make_verdict("a.rs", "small_regression", 10.0, false),
            },
            FunctionChange::Removed {
                baseline: make_verdict("a.rs", "big_removed", 30.0, true),
            },
            FunctionChange::Added {
                current: make_verdict("a.rs", "big_added", 10.0, false),
            },
        ]);
        let view = apply(&delta, DeltaViewSpec::default());
        assert_eq!(view.shown[0].qualified_name(), "big_added"); // +10
        assert_eq!(view.shown[1].qualified_name(), "small_regression"); // +5
        assert_eq!(view.shown[2].qualified_name(), "big_improvement"); // -25
        assert_eq!(view.shown[3].qualified_name(), "big_removed"); // -30
    }

    #[test]
    fn apply_filter_change_kinds_added_only() {
        let delta = delta_with_changes(vec![
            FunctionChange::Added {
                current: make_verdict("a.rs", "added_one", 5.0, false),
            },
            FunctionChange::Removed {
                baseline: make_verdict("a.rs", "removed_one", 5.0, false),
            },
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "mod_one", 5.0, false),
                current: make_verdict("a.rs", "mod_one", 6.0, false),
            },
        ]);
        let mut kinds = BTreeSet::new();
        kinds.insert(ChangeKind::Added);
        let spec = DeltaViewSpec {
            filters: DeltaFilters {
                change_kinds: Some(kinds),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&delta, spec);
        assert_eq!(view.shown.len(), 1);
        assert_eq!(view.shown[0].kind(), ChangeKind::Added);
    }

    #[test]
    fn apply_filter_score_delta_min_excludes_below() {
        let delta = delta_with_changes(vec![
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "tiny", 5.0, false),
                current: make_verdict("a.rs", "tiny", 6.0, false),
            },
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "big", 5.0, false),
                current: make_verdict("a.rs", "big", 25.0, false),
            },
        ]);
        let spec = DeltaViewSpec {
            filters: DeltaFilters {
                min_score_delta: Some(10.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&delta, spec);
        assert_eq!(view.shown.len(), 1);
        assert_eq!(view.shown[0].qualified_name(), "big");
    }

    #[test]
    fn apply_filter_score_delta_passes_added_and_removed() {
        // Added/Removed have no score_delta — bound check shouldn't drop them
        let delta = delta_with_changes(vec![
            FunctionChange::Added {
                current: make_verdict("a.rs", "added_one", 5.0, false),
            },
            FunctionChange::Removed {
                baseline: make_verdict("a.rs", "removed_one", 5.0, false),
            },
        ]);
        let spec = DeltaViewSpec {
            filters: DeltaFilters {
                min_score_delta: Some(100.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let view = apply(&delta, spec);
        assert_eq!(view.shown.len(), 2);
    }

    #[test]
    fn apply_sort_current_crap_descending_removed_last() {
        let delta = delta_with_changes(vec![
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "modlow", 50.0, true),
                current: make_verdict("a.rs", "modlow", 5.0, false),
            },
            FunctionChange::Removed {
                baseline: make_verdict("a.rs", "removed_top", 999.0, true),
            },
            FunctionChange::Added {
                current: make_verdict("a.rs", "added_high", 47.0, true),
            },
        ]);
        let spec = DeltaViewSpec {
            sort: DeltaSortKey::CurrentCrap,
            ..Default::default()
        };
        let view = apply(&delta, spec);
        assert_eq!(view.shown[0].qualified_name(), "added_high"); // 47 (current)
        assert_eq!(view.shown[1].qualified_name(), "modlow"); // 5 (current)
        assert_eq!(view.shown[2].qualified_name(), "removed_top"); // None — last
    }

    #[test]
    fn apply_sort_path_alphabetical() {
        let delta = delta_with_changes(vec![
            FunctionChange::Modified {
                baseline: make_verdict("zzz.rs", "z", 5.0, false),
                current: make_verdict("zzz.rs", "z", 6.0, false),
            },
            FunctionChange::Modified {
                baseline: make_verdict("aaa.rs", "a", 5.0, false),
                current: make_verdict("aaa.rs", "a", 6.0, false),
            },
            FunctionChange::Modified {
                baseline: make_verdict("mmm.rs", "m", 5.0, false),
                current: make_verdict("mmm.rs", "m", 6.0, false),
            },
        ]);
        let spec = DeltaViewSpec {
            sort: DeltaSortKey::Path,
            ..Default::default()
        };
        let view = apply(&delta, spec);
        assert_eq!(view.shown[0].file_path(), "aaa.rs");
        assert_eq!(view.shown[1].file_path(), "mmm.rs");
        assert_eq!(view.shown[2].file_path(), "zzz.rs");
    }

    #[test]
    fn apply_truncate_marks_truncated_true() {
        let changes: Vec<FunctionChange> = (0..10)
            .map(|i| FunctionChange::Modified {
                baseline: make_verdict("a.rs", &format!("fn_{i}"), 5.0, false),
                current: make_verdict("a.rs", &format!("fn_{i}"), 5.0 + i as f64, false),
            })
            .collect();
        let delta = delta_with_changes(changes);
        let spec = DeltaViewSpec {
            limit: Some(3),
            ..Default::default()
        };
        let view = apply(&delta, spec);
        assert_eq!(view.shown.len(), 3);
        assert_eq!(view.eligible_count, 10);
        assert!(view.truncated);
    }

    #[test]
    fn apply_truncate_zero_means_no_limit() {
        let changes: Vec<FunctionChange> = (0..3)
            .map(|i| FunctionChange::Added {
                current: make_verdict("a.rs", &format!("fn_{i}"), 5.0, false),
            })
            .collect();
        let delta = delta_with_changes(changes);
        let spec = DeltaViewSpec {
            limit: Some(0),
            ..Default::default()
        };
        let view = apply(&delta, spec);
        assert_eq!(view.shown.len(), 3);
        assert!(!view.truncated);
    }

    #[test]
    fn apply_view_full_borrows_underlying_delta() {
        let delta = delta_with_changes(vec![]);
        let view = apply(&delta, DeltaViewSpec::default());
        assert!(std::ptr::eq(view.full, &delta));
    }
}

// ── Property tests ───────────────────────────────────────────────────

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::test_strategies::arb_analysis_result;
    use proptest::prelude::*;

    proptest! {
        /// `compute(r, r)` produces all-Modified, all-zero-delta, all-passing.
        /// The most fundamental invariant: a no-op delta is well-formed.
        #[test]
        fn prop_compute_identity_yields_all_modified_zero(result in arb_analysis_result()) {
            let n = result.functions.len();
            let delta = compute(result.clone(), result);
            prop_assert_eq!(delta.changes.len(), n);
            for change in &delta.changes {
                let is_modified = matches!(change, FunctionChange::Modified { .. });
                prop_assert!(is_modified);
                prop_assert_eq!(change.score_delta(), Some(0.0));
            }
            let summary = DeltaSummary::compute(&delta.changes);
            prop_assert_eq!(summary.added, 0);
            prop_assert_eq!(summary.removed, 0);
            prop_assert_eq!(summary.modified, n as u32);
            prop_assert_eq!(summary.regressions, 0);
            prop_assert_eq!(summary.improvements, 0);
            prop_assert_eq!(summary.new_violations, 0);
            prop_assert!(summary.passed);
        }

        /// Length of `changes` is matched + baseline-only + current-only;
        /// matched is bounded by min(|baseline|, |current|).
        #[test]
        fn prop_changes_count_bounded(
            baseline in arb_analysis_result(),
            current in arb_analysis_result(),
        ) {
            let baseline_len = baseline.functions.len();
            let current_len = current.functions.len();
            let delta = compute(baseline, current);
            let n = delta.changes.len();
            // Every change is one of the three; the union bound is
            // max + min + max = baseline + current. (Concretely:
            // matched can be 0; both-only appears as Add+Remove which
            // sums to baseline+current.)
            prop_assert!(n <= baseline_len + current_len);
            // Modified can't exceed either side.
            let modified_count = delta
                .changes
                .iter()
                .filter(|c| matches!(c, FunctionChange::Modified { .. }))
                .count();
            prop_assert!(modified_count <= baseline_len);
            prop_assert!(modified_count <= current_len);
        }

        /// new_violations is bounded by the count of Added rows that
        /// exceed plus Modified rows that crossed the threshold.
        #[test]
        fn prop_new_violations_well_bounded(
            baseline in arb_analysis_result(),
            current in arb_analysis_result(),
        ) {
            let delta = compute(baseline, current);
            let summary = DeltaSummary::compute(&delta.changes);
            prop_assert!(summary.new_violations <= summary.added + summary.modified);
            prop_assert_eq!(summary.passed, summary.new_violations == 0);
        }

        /// View shaping never adds rows. `view.shown.len() <= eligible_count
        /// <= delta.changes.len()`. Truncation flag is true iff
        /// `shown.len() < eligible_count`.
        #[test]
        fn prop_view_shown_subset_of_changes(
            baseline in arb_analysis_result(),
            current in arb_analysis_result(),
        ) {
            let delta = compute(baseline, current);
            let view = apply(&delta, DeltaViewSpec::default());
            prop_assert!(view.shown.len() <= view.eligible_count);
            prop_assert!(view.eligible_count <= delta.changes.len());
            prop_assert_eq!(view.shown.len() == view.eligible_count, !view.truncated);
        }

        /// Delta gate is unshapeable. `apply` does not mutate
        /// `full.summary.passed`.
        #[test]
        fn prop_apply_does_not_mutate_summary(
            baseline in arb_analysis_result(),
            current in arb_analysis_result(),
        ) {
            let delta = compute(baseline, current);
            let original_passed = delta.summary.passed;
            let view = apply(&delta, DeltaViewSpec::default());
            prop_assert_eq!(view.full.summary.passed, original_passed);
        }
    }
}
