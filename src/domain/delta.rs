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
use std::collections::HashMap;

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
    pub baseline: AnalysisResult,
    pub current: AnalysisResult,
    pub changes: Vec<FunctionChange>,
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
/// Decomposed into a private helper [`pair_identities`] to keep the
/// public surface declarative and to localize the HashMap construction
/// for testing.
pub fn compute(baseline: AnalysisResult, current: AnalysisResult) -> AnalysisDelta {
    let changes = pair_identities(&baseline, &current);
    AnalysisDelta {
        baseline,
        current,
        changes,
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
    let mut matched_count = 0usize;

    for current_verdict in &current.functions {
        let key = identity_key(&current_verdict.scored.identity);
        match baseline_index.remove(&key) {
            Some(baseline_verdict) => {
                matched_count += 1;
                changes.push(FunctionChange::Modified {
                    baseline: baseline_verdict.clone(),
                    current: current_verdict.clone(),
                });
            }
            None => changes.push(FunctionChange::Added {
                current: current_verdict.clone(),
            }),
        }
    }

    // Leftover baseline entries are Removed.
    debug_assert_eq!(
        baseline.functions.len() - matched_count,
        baseline_index.len(),
        "matched count + leftover should equal baseline size"
    );
    for (_, baseline_verdict) in baseline_index {
        changes.push(FunctionChange::Removed {
            baseline: baseline_verdict.clone(),
        });
    }

    changes
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
        };
        let mut current_v = make_verdict("a.rs", "fn_a", 5.0, false);
        current_v.scored.identity.span = SourceSpan {
            start_line: 100,
            end_line: 105,
        };
        let delta = compute(make_result(vec![baseline_v]), make_result(vec![current_v]));
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Modified { .. }));
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
    }
}
