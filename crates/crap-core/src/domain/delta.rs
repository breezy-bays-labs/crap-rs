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
use std::hash::Hash;

// ── Change kinds ─────────────────────────────────────────────────────

/// Classification of a single function across the baseline → current
/// transition.
///
/// Tag-only enum used in `DeltaSummary` and the JSON envelope. The
/// per-function payload lives on [`FunctionChange`]; this enum is for
/// shaping (filter / sort) and presentation. `#[non_exhaustive]`
/// leaves room for further variants without breaking downstream matches.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
    /// A function present in both analyses under a different identity —
    /// moved to another file, renamed, or moved between modules — whose
    /// body is otherwise unchanged. Paired out of the leftover
    /// Added/Removed sets by a 1:1 structural match, so a pure
    /// relocation reads as one change instead of an unrelated
    /// Removed + Added pair (and therefore never trips the delta gate
    /// on its own).
    Renamed,
}

impl ChangeKind {
    /// All known variants, in canonical order. Used by CLI parsers and
    /// serializers that enumerate the universe.
    pub const ALL: [ChangeKind; 4] = [
        ChangeKind::Added,
        ChangeKind::Removed,
        ChangeKind::Modified,
        ChangeKind::Renamed,
    ];

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
            ChangeKind::Renamed => "renamed",
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
///
/// [`Renamed`](FunctionChange::Renamed) carries both sides like
/// `Modified` — it is the same function under a new identity (file /
/// qualified name), so reporters render the old → new relocation and
/// the gate treats its score movement exactly like `Modified`.
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
    Renamed {
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
            FunctionChange::Renamed { .. } => ChangeKind::Renamed,
        }
    }

    pub fn current_score(&self) -> Option<f64> {
        match self {
            FunctionChange::Added { current } => Some(current.scored.crap.value),
            FunctionChange::Modified { current, .. } => Some(current.scored.crap.value),
            FunctionChange::Renamed { current, .. } => Some(current.scored.crap.value),
            FunctionChange::Removed { .. } => None,
        }
    }

    pub fn baseline_score(&self) -> Option<f64> {
        match self {
            FunctionChange::Removed { baseline } => Some(baseline.scored.crap.value),
            FunctionChange::Modified { baseline, .. } => Some(baseline.scored.crap.value),
            FunctionChange::Renamed { baseline, .. } => Some(baseline.scored.crap.value),
            FunctionChange::Added { .. } => None,
        }
    }

    /// `current - baseline` for `Modified` / `Renamed`; `None` for
    /// `Added` / `Removed`. A relocated function carries both sides, so
    /// a relocation that *also* changed the score exposes that movement
    /// here just like an in-place modification.
    pub fn score_delta(&self) -> Option<f64> {
        match self {
            FunctionChange::Modified { baseline, current }
            | FunctionChange::Renamed { baseline, current } => {
                Some(current.scored.crap.value - baseline.scored.crap.value)
            }
            _ => None,
        }
    }

    /// File path associated with this change. For Modified, baseline
    /// and current share the same path (matching keys on file_path);
    /// for Renamed the path changed, so this reports the *current*
    /// (post-relocation) location.
    pub fn file_path(&self) -> &str {
        match self {
            FunctionChange::Added { current } => &current.scored.identity.file_path,
            FunctionChange::Removed { baseline } => &baseline.scored.identity.file_path,
            FunctionChange::Modified { current, .. } => &current.scored.identity.file_path,
            FunctionChange::Renamed { current, .. } => &current.scored.identity.file_path,
        }
    }

    /// Qualified name associated with this change. For Renamed this is
    /// the *current* (post-relocation) name.
    pub fn qualified_name(&self) -> &str {
        match self {
            FunctionChange::Added { current } => &current.scored.identity.qualified_name,
            FunctionChange::Removed { baseline } => &baseline.scored.identity.qualified_name,
            FunctionChange::Modified { current, .. } => &current.scored.identity.qualified_name,
            FunctionChange::Renamed { current, .. } => &current.scored.identity.qualified_name,
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
    /// Relocated functions (moved file / renamed / moved module) paired
    /// as a single change. A distinct, non-overlapping bucket — a
    /// `Renamed` row is counted here and never in `added`, `removed`, or
    /// `modified`, so `added + removed + modified + renamed` accounts for
    /// every change row exactly once.
    pub renamed: u32,
    /// `Modified` / `Renamed` rows where `score_delta > 0` (current got
    /// worse). A pure relocation has zero score delta and so is neither
    /// a regression nor an improvement; a relocation that also worsened
    /// the score is counted here.
    pub regressions: u32,
    /// `Modified` / `Renamed` rows where `score_delta < 0` (current got
    /// better).
    pub improvements: u32,
    /// Threshold breaches *introduced* by this delta:
    /// - `Added` rows whose `current.exceeds == true`
    /// - `Modified` / `Renamed` rows where `baseline.exceeds == false`
    ///   AND `current.exceeds == true`
    ///
    /// Pre-existing violations (rows where `baseline.exceeds` was already
    /// true) do NOT contribute. This distinction matters for the delta
    /// gate — we want to fail PRs that *introduce* risk, not PRs that
    /// merely touch already-failing functions. A *pure* relocation has an
    /// identical score on both sides, so `baseline.exceeds ==
    /// current.exceeds` and it never contributes a new violation —
    /// migrations sail through the gate. A relocation that also crossed
    /// the threshold still counts (the relocation was not the only
    /// change).
    ///
    /// Pairing only ever *lowers* `new_violations` versus scoring the
    /// leftovers as Add+Remove (pinned by
    /// `proptests::prop_renamed_never_raises_new_violations`). That is the
    /// migration-friendly guarantee — enabling rename detection can never
    /// newly fail a PR. The flip side is the honest limitation: a *false*
    /// pairing also lowers the count, so a coincidental match CAN lower
    /// `new_violations` below the true figure. The 1:1 + name/signature
    /// guards make false pairings rare, but a genuinely-unrelated function
    /// whose signature exactly matches a removed one (and is its only
    /// signature-mate) is indistinguishable from a real rename at the
    /// verdict level and will pair. See
    /// `tests::compute_documented_limitation_coincidental_signature_pairs_as_renamed`.
    pub new_violations: u32,
    /// Would-be new violations that were NOT counted because the
    /// transition happened entirely inside the threshold *border band*
    /// (`|crap.value - threshold| < epsilon`), surfaced so a suppressed
    /// count is visible rather than silent. Zero unless `--threshold-epsilon`
    /// / `[delta] epsilon` is set above `0.0`.
    ///
    /// Honest limitation (do not "fix" by claiming noise-only safety):
    /// epsilon only ever *moves* a would-be new violation out of
    /// `new_violations` and into this bucket — it never adds one. So a
    /// genuinely-new over-threshold function that happens to land in the
    /// band IS hidden here by design. This is a threshold-border *jitter*
    /// knob, not a "suppresses noise only" guarantee.
    /// `new_violations + border_jitter_suppressed` is invariant under
    /// epsilon (pinned by `proptests::prop_border_band_conserves_new_violations`).
    pub border_jitter_suppressed: u32,
    /// `new_violations == 0`. Drives the optional `--delta-gate`.
    pub passed: bool,
}

impl DeltaSummary {
    /// Convenience for `compute_with_epsilon(changes, 0.0)` — the exact
    /// pre-epsilon behavior, kept for the many in-crate callers and tests
    /// that don't exercise the border band.
    ///
    /// PRODUCTION CODE MUST USE [`DeltaSummary::compute_with_epsilon`]:
    /// a bare `compute` silently runs at `epsilon = 0.0`, disabling
    /// border-band suppression.
    pub fn compute(changes: &[FunctionChange]) -> Self {
        Self::compute_with_epsilon(changes, 0.0)
    }

    /// Tally `changes`, suppressing would-be new violations whose
    /// transition stays inside the threshold border band of half-width
    /// `epsilon` (see [`change_is_new_violation`]). `epsilon == 0.0`
    /// reproduces [`compute`] byte-for-byte.
    pub fn compute_with_epsilon(changes: &[FunctionChange], epsilon: f64) -> Self {
        let mut summary = Self::default();
        for change in changes {
            tally(&mut summary, change, epsilon);
        }
        summary.passed = summary.new_violations == 0;
        summary
    }
}

/// True iff `verdict`'s CRAP score sits inside the threshold *border
/// band* — within `epsilon` (absolute, unitless CRAP points) of its
/// own per-function threshold.
///
/// `epsilon == 0.0` is always `false` (the strict `<` never admits a
/// zero-width band), which is what makes the default a byte-identical
/// no-op. A non-finite or negative `epsilon` must never reach here — it
/// is rejected at the config / CLI boundary — but if one did, `< NaN`
/// is always false and a negative band is empty, so the worst case is
/// "no suppression," never a panic or a widened gate.
fn within_band(verdict: &FunctionVerdict, epsilon: f64) -> bool {
    (verdict.scored.crap.value - verdict.threshold).abs() < epsilon
}

/// Does this change introduce a delta-gate *new violation*, after
/// threshold-border-band suppression of half-width `epsilon`?
///
/// - `Removed` never counts.
/// - `Added`: `current` breaches its threshold AND is not in the band.
/// - `Modified` / `Renamed`: `baseline` was clean and `current`
///   breaches, AND the transition is not fully inside the band (i.e.
///   NOT both sides within epsilon of threshold).
///
/// ASYMMETRY (documented behavior, not a bug): `Modified` / `Renamed`
/// require BOTH readings in the band — genuine oscillation across the
/// line. `Added` has only one reading and no prior state to "jitter"
/// from, so an `Added`-in-band is a *soft threshold bypass*, not jitter
/// suppression: brand-new code landing at 25.01 with threshold 25.0 and
/// epsilon 0.5 is forgiven. Stated here verbatim so no reader equates
/// "jitter" with "oscillation" for the `Added` case.
///
/// This is the single source of truth for the new-violation rule — the
/// summary tally (the count) and the markdown reporter's per-row
/// new-violations table both route through it, so the summary count can
/// never drift from the rendered table.
pub fn change_is_new_violation(change: &FunctionChange, epsilon: f64) -> bool {
    match change {
        FunctionChange::Added { current } => current.exceeds && !within_band(current, epsilon),
        FunctionChange::Modified { baseline, current }
        | FunctionChange::Renamed { baseline, current } => {
            !baseline.exceeds
                && current.exceeds
                && !(within_band(baseline, epsilon) && within_band(current, epsilon))
        }
        FunctionChange::Removed { .. } => false,
    }
}

fn tally(summary: &mut DeltaSummary, change: &FunctionChange, epsilon: f64) {
    // Per-variant kind tally + score movement (regressions / improvements).
    match change {
        FunctionChange::Added { .. } => summary.added += 1,
        FunctionChange::Removed { .. } => summary.removed += 1,
        FunctionChange::Modified { baseline, current } => {
            summary.modified += 1;
            tally_movement(summary, baseline, current);
        }
        FunctionChange::Renamed { baseline, current } => {
            summary.renamed += 1;
            // A relocation is the same function under a new identity, so
            // its score movement is accounted exactly like `Modified`. A
            // pure relocation has a zero delta and matching `exceeds`, so
            // it moves neither counter.
            tally_movement(summary, baseline, current);
        }
    }

    // New-violation / border-jitter accounting is uniform across every
    // kind, routed through the single `change_is_new_violation` predicate.
    // The else-if relies on `change_is_new_violation(_, eps)` being a
    // subset of `change_is_new_violation(_, 0.0)` — epsilon only ever
    // *removes* violations — so the two buckets partition the would-be
    // violations exactly: `new_violations + border_jitter_suppressed` is
    // invariant under epsilon (the conservation proptest).
    if change_is_new_violation(change, epsilon) {
        summary.new_violations += 1;
    } else if change_is_new_violation(change, 0.0) {
        summary.border_jitter_suppressed += 1;
    }
}

fn tally_movement(
    summary: &mut DeltaSummary,
    baseline: &FunctionVerdict,
    current: &FunctionVerdict,
) {
    // Use the same 0.005 cutoff the reporters apply when rendering
    // regression / improvement rows (it matches the `{:.2}` display
    // precision and absorbs float-subtraction noise on otherwise-equal
    // 2-decimal CRAP scores). Counting on a bare `> 0.0` would let a
    // sub-precision delta inflate `regressions` / `improvements` with a
    // row that never actually renders — keeping the summary counts and
    // the rendered tables in agreement.
    let delta = current.scored.crap.value - baseline.scored.crap.value;
    if delta >= 0.005 {
        summary.regressions += 1;
    } else if delta <= -0.005 {
        summary.improvements += 1;
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
    /// Effective threshold-border-band half-width used to compute
    /// `summary`. In-memory only (`#[serde(skip)]`): the wire envelope's
    /// AC-required surface is `summary.border_jitter_suppressed`. The
    /// markdown reporter re-derives a per-row new-violation verdict and
    /// reads this (via the `DeltaView`'s `full` borrow) so it applies the
    /// exact same band the summary used. `0.0` when no epsilon was set.
    #[serde(skip)]
    pub epsilon: f64,
}

// ── compute: pair → classify ────────────────────────────────────────

/// Identity tuple used for the exact-match pass across the baseline →
/// current transition. **Span (line range) is intentionally excluded** —
/// line numbers shift when surrounding code is edited, and we want a
/// minor edit to `foo` to register as `Modified`, not `Removed` +
/// `Added`. A function whose identity changed (renamed, or moved across
/// files / modules) misses this exact match and falls to the relocation
/// pass in [`pair_relocations`], which re-pairs it as `Renamed` when the
/// match is structurally unambiguous.
type IdentityKey<'a> = (&'a str, &'a str);

fn identity_key(identity: &FunctionIdentity) -> IdentityKey<'_> {
    (&identity.file_path, &identity.qualified_name)
}

/// Compare two analyses, classifying every function as Added, Removed,
/// Modified, or Renamed (relocated). Stable: `current`'s order is
/// preserved for matched + added rows, then renamed and baseline-only
/// (`Removed`) rows trail in sorted order.
///
/// Decomposed into a private helper `pair_identities` to keep the
/// public surface declarative and to localize the HashMap construction
/// for testing.
pub fn compute(baseline: AnalysisResult, current: AnalysisResult) -> AnalysisDelta {
    compute_with_epsilon(baseline, current, 0.0)
}

/// Like [`compute`] but suppresses would-be new violations whose
/// transition stays inside the threshold border band of half-width
/// `epsilon` (see [`change_is_new_violation`]). `epsilon == 0.0`
/// reproduces [`compute`] byte-for-byte.
///
/// PRODUCTION CODE MUST CALL THIS (the bare [`compute`] silently runs at
/// `epsilon = 0.0`). The effective `epsilon` is stored on the returned
/// [`AnalysisDelta`] so reporters that re-derive a per-row new-violation
/// verdict apply the exact same band the summary used.
pub fn compute_with_epsilon(
    baseline: AnalysisResult,
    current: AnalysisResult,
    epsilon: f64,
) -> AnalysisDelta {
    let changes = pair_identities(&baseline, &current);
    let summary = DeltaSummary::compute_with_epsilon(&changes, epsilon);
    AnalysisDelta {
        baseline,
        current,
        changes,
        summary,
        epsilon,
    }
}

fn pair_identities(baseline: &AnalysisResult, current: &AnalysisResult) -> Vec<FunctionChange> {
    // Pass 1 — exact `(file, qualified_name)` match. We index the
    // baseline, then sweep current: a hit is `Modified` (emitted in
    // current order); a miss is a current-only candidate for the
    // relocation pass. Whatever the sweep doesn't remove is baseline-only.
    let mut baseline_index: HashMap<IdentityKey<'_>, &FunctionVerdict> =
        HashMap::with_capacity(baseline.functions.len());
    for verdict in &baseline.functions {
        baseline_index.insert(identity_key(&verdict.scored.identity), verdict);
    }

    let mut modified: Vec<FunctionChange> = Vec::new();
    let mut current_only: Vec<&FunctionVerdict> = Vec::with_capacity(current.functions.len());
    for current_verdict in &current.functions {
        match baseline_index.remove(&identity_key(&current_verdict.scored.identity)) {
            Some(baseline_verdict) => modified.push(FunctionChange::Modified {
                baseline: baseline_verdict.clone(),
                current: current_verdict.clone(),
            }),
            None => current_only.push(current_verdict),
        }
    }
    let baseline_only: Vec<&FunctionVerdict> = baseline_index.into_values().collect();

    // Pass 2 — re-pair relocated functions out of the leftovers.
    let (renamed_pairs, baseline_only, current_only) =
        pair_relocations(baseline_only, current_only);

    // Pass 3 — assemble in canonical order: Modified (current order) →
    // Added (current order) → Renamed (sorted) → Removed (sorted). Raw
    // order is internal (reporters and the JSON envelope consume a
    // sorted `DeltaView`), but pinning it keeps direct `changes`
    // consumers and unit tests deterministic.
    let mut changes: Vec<FunctionChange> = Vec::with_capacity(
        modified.len() + current_only.len() + renamed_pairs.len() + baseline_only.len(),
    );
    changes.append(&mut modified);
    for current_verdict in current_only {
        changes.push(FunctionChange::Added {
            current: current_verdict.clone(),
        });
    }
    push_renamed_sorted(&mut changes, renamed_pairs);
    push_removed_sorted(&mut changes, baseline_only);
    changes
}

/// A baseline → current pairing of the same function under two
/// identities.
type VerdictPair<'a> = (&'a FunctionVerdict, &'a FunctionVerdict);

/// Append `Renamed` rows, sorted by the *current* (post-relocation)
/// identity key with a stable sort so output is byte-deterministic
/// across platforms.
fn push_renamed_sorted(changes: &mut Vec<FunctionChange>, mut pairs: Vec<VerdictPair<'_>>) {
    pairs.sort_by(|a, b| {
        identity_key(&a.1.scored.identity).cmp(&identity_key(&b.1.scored.identity))
    });
    for (baseline, current) in pairs {
        changes.push(FunctionChange::Renamed {
            baseline: baseline.clone(),
            current: current.clone(),
        });
    }
}

/// Append leftover baseline functions as `Removed`. `HashMap` iteration
/// order is unspecified, so we sort by identity key before emission —
/// otherwise consumers that iterate `delta.changes` directly (or apply a
/// sort that doesn't break ties on identity) observe run-to-run
/// flakiness. The identity-key sort is cheap, deterministic, and mirrors
/// the lexical ordering most operators expect.
fn push_removed_sorted(changes: &mut Vec<FunctionChange>, mut leftover: Vec<&FunctionVerdict>) {
    leftover
        .sort_by(|a, b| identity_key(&a.scored.identity).cmp(&identity_key(&b.scored.identity)));
    for baseline_verdict in leftover {
        changes.push(FunctionChange::Removed {
            baseline: baseline_verdict.clone(),
        });
    }
}

/// Structural signature of a function — invariant under a pure
/// relocation. Captures complexity + metric + the ordered
/// `(kind, increment, nesting)` fingerprint of its contributors.
/// Deliberately excludes coverage, CRAP score, and source positions
/// (file / line / column): those move when a function relocates or its
/// file's coverage shifts, but the structure does not. `ContributorKind`
/// and `ComplexityMetric` are `Eq` but not `Hash`, so they enter the key
/// via their pinned `as_wire_str()` — a `&'static str`, which is `Hash`.
type StructSig = (u32, &'static str, Vec<(&'static str, u32, u32)>);

fn struct_sig(verdict: &FunctionVerdict) -> StructSig {
    let scored = &verdict.scored;
    let contributors = scored
        .contributors
        .iter()
        .map(|c| (c.kind.as_wire_str(), c.increment, c.nesting_depth))
        .collect();
    (
        scored.complexity,
        scored.complexity_metric.as_wire_str(),
        contributors,
    )
}

/// Re-pair relocated functions out of the leftover Added/Removed sets.
///
/// Two tiers, each a 1:1-unambiguous match (an entry pairs only when
/// exactly one entry on each side shares its key — any ambiguous key
/// stays Added + Removed):
/// - Tier A keys on **both** the retained `qualified_name` AND the
///   structural signature — a cross-file move whose body is unchanged
///   (a top-level fn, or an `impl` method whose type is unchanged).
///   Requiring the signature too means a same-named-but-different-body
///   coincidence (two unrelated `run` / `new` functions) does NOT pair,
///   and a moved-*and-edited* function falls through to Add+Remove where
///   its edit is scrutinized — more faithful to "was relocation the only
///   change?".
/// - Tier B keys on the structural signature alone — a rename, or a
///   move that also changed the module-qualified name (body unchanged).
///
/// The 1:1 guard is both the false-positive defense (signature-colliding
/// functions never pair) and the cost ceiling (HashMap bucketing, no
/// O(n²) scan). It does **not** make the matcher sound: working from
/// verdicts (no source text), two genuinely-unrelated functions with an
/// identical signature that are the only leftover on each side are
/// indistinguishable from a real rename and WILL pair — see
/// `tests::compute_documented_limitation_coincidental_signature_pairs_as_renamed`.
/// Returns the `Renamed` pairs plus the still-unmatched baseline-only /
/// current-only leftovers, each in input order.
fn pair_relocations<'a>(
    baseline_only: Vec<&'a FunctionVerdict>,
    current_only: Vec<&'a FunctionVerdict>,
) -> (
    Vec<VerdictPair<'a>>,
    Vec<&'a FunctionVerdict>,
    Vec<&'a FunctionVerdict>,
) {
    // Tier A — retained qualified name AND matching structural signature.
    // The strongest signal (same name, same body, different file) and
    // applies at any complexity. Keying on name+signature rejects an
    // unrelated function that merely reused a common name.
    let (mut paired, baseline_only, current_only) = pair_by_key(baseline_only, current_only, |v| {
        (v.scored.identity.qualified_name.as_str(), struct_sig(v))
    });

    // Tier B — structural signature, restricted to structurally
    // distinctive functions. A complexity-1 body has no decision points,
    // so its signature is degenerate and shared by every trivial
    // function; pairing on it would mis-link two unrelated stubs that
    // merely happen to be the only leftover on each side. Trivial
    // leftovers pass straight through to Added / Removed.
    let (distinctive_baseline, trivial_baseline): (Vec<_>, Vec<_>) =
        baseline_only.into_iter().partition(|v| is_distinctive(v));
    let (distinctive_current, trivial_current): (Vec<_>, Vec<_>) =
        current_only.into_iter().partition(|v| is_distinctive(v));
    let (more, mut baseline_only, mut current_only) =
        pair_by_key(distinctive_baseline, distinctive_current, struct_sig);
    paired.extend(more);
    baseline_only.extend(trivial_baseline);
    current_only.extend(trivial_current);
    (paired, baseline_only, current_only)
}

/// Whether a function's structural signature is distinctive enough to be
/// a reliable relocation signal on its own. A complexity-1 function has
/// no decision points — its [`struct_sig`] is degenerate and matches
/// every other trivial function — so it is paired only by retained name
/// (Tier A), never by signature alone (Tier B).
fn is_distinctive(verdict: &FunctionVerdict) -> bool {
    verdict.scored.complexity > 1
}

/// Count how many entries carry each key.
fn count_by_key<'a, K, F>(items: &[&'a FunctionVerdict], key_of: &F) -> HashMap<K, u32>
where
    K: Eq + Hash,
    F: Fn(&'a FunctionVerdict) -> K,
{
    let mut counts: HashMap<K, u32> = HashMap::with_capacity(items.len());
    for verdict in items {
        *counts.entry(key_of(verdict)).or_insert(0) += 1;
    }
    counts
}

/// Pair baseline-only and current-only entries that share a key 1:1.
///
/// A key pairs only when exactly one baseline AND one current entry
/// carry it; any key with more than one entry on either side is
/// ambiguous and its entries pass through to the leftovers unpaired (in
/// input order). Generic over the key so the relocation tiers supply
/// `qualified_name` or the structural signature.
fn pair_by_key<'a, K, F>(
    baseline_only: Vec<&'a FunctionVerdict>,
    current_only: Vec<&'a FunctionVerdict>,
    key_of: F,
) -> (
    Vec<VerdictPair<'a>>,
    Vec<&'a FunctionVerdict>,
    Vec<&'a FunctionVerdict>,
)
where
    K: Eq + Hash,
    F: Fn(&'a FunctionVerdict) -> K,
{
    let baseline_counts = count_by_key(&baseline_only, &key_of);
    let current_counts = count_by_key(&current_only, &key_of);
    let pairable = |verdict: &'a FunctionVerdict| {
        let key = key_of(verdict);
        baseline_counts.get(&key) == Some(&1) && current_counts.get(&key) == Some(&1)
    };

    // Uniquely-keyed baseline entries are looked up by current entries;
    // everything else passes straight to the leftovers. `partition`
    // preserves input order.
    let (unique, leftover_baseline): (Vec<_>, Vec<_>) =
        baseline_only.into_iter().partition(|&v| pairable(v));
    let mut unique_baseline: HashMap<K, &'a FunctionVerdict> =
        unique.into_iter().map(|v| (key_of(v), v)).collect();

    let mut leftover_current: Vec<&'a FunctionVerdict> = Vec::new();
    let paired: Vec<VerdictPair<'a>> = current_only
        .into_iter()
        .filter_map(|v| pair_or_stash(&mut unique_baseline, &mut leftover_current, &key_of, v))
        .collect();
    // Every uniquely-keyed baseline entry has exactly one current match
    // by construction, so `unique_baseline` is now empty.
    debug_assert!(unique_baseline.is_empty());

    (paired, leftover_baseline, leftover_current)
}

/// Pull this current entry's 1:1 baseline partner, or stash it as a
/// leftover and yield `None`.
fn pair_or_stash<'a, K, F>(
    unique_baseline: &mut HashMap<K, &'a FunctionVerdict>,
    leftover_current: &mut Vec<&'a FunctionVerdict>,
    key_of: &F,
    current: &'a FunctionVerdict,
) -> Option<VerdictPair<'a>>
where
    K: Eq + Hash,
    F: Fn(&'a FunctionVerdict) -> K,
{
    match unique_baseline.remove(&key_of(current)) {
        Some(baseline) => Some((baseline, current)),
        None => {
            leftover_current.push(current);
            None
        }
    }
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
        FunctionChange::Modified { baseline, current }
        | FunctionChange::Renamed { baseline, current } => {
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
                branch_coverage_percent: None,
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

    /// Like [`make_verdict`] but with an explicit complexity, so a test
    /// can control the [`struct_sig`] used by the relocation pass's
    /// Tier B (structural-signature) matching.
    fn make_verdict_cx(
        file: &str,
        name: &str,
        score: f64,
        exceeds: bool,
        complexity: u32,
    ) -> FunctionVerdict {
        let mut verdict = make_verdict(file, name, score, exceeds);
        verdict.scored.complexity = complexity;
        verdict
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

    /// Cross-file move that keeps the qualified name → a single
    /// `Renamed`, paired by Tier A (retained name). Previously this
    /// surfaced as an unrelated Added + Removed pair.
    #[test]
    fn compute_same_name_different_files_pairs_as_renamed() {
        let baseline = make_result(vec![make_verdict("a.rs", "log", 5.0, false)]);
        let current = make_result(vec![make_verdict("b.rs", "log", 5.0, false)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Renamed { .. }));
        // The relocation reports the current (post-move) location.
        assert_eq!(delta.changes[0].file_path(), "b.rs");
        assert_eq!(delta.changes[0].qualified_name(), "log");
    }

    /// In-file rename (name changed, structure identical) → a single
    /// `Renamed`, paired by Tier B (structural signature). Previously
    /// this surfaced as an unrelated Added + Removed pair.
    #[test]
    fn compute_same_file_rename_pairs_as_renamed() {
        let baseline = make_result(vec![make_verdict("a.rs", "v1", 5.0, false)]);
        let current = make_result(vec![make_verdict("a.rs", "v2", 5.0, false)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Renamed { .. }));
        assert_eq!(delta.changes[0].qualified_name(), "v2");
    }

    /// Module move (both file and qualified name change) → a single
    /// `Renamed`, paired by Tier B on the structural signature.
    #[test]
    fn compute_module_move_pairs_as_renamed() {
        let baseline = make_result(vec![make_verdict("a.rs", "mod_a::foo", 5.0, false)]);
        let current = make_result(vec![make_verdict("b.rs", "mod_b::foo", 5.0, false)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Renamed { .. }));
        assert_eq!(delta.changes[0].file_path(), "b.rs");
        assert_eq!(delta.changes[0].qualified_name(), "mod_b::foo");
    }

    /// Tier B pairs only on a *matching* structural signature — two
    /// leftovers with different complexities are NOT a relocation, even
    /// though each side has exactly one entry.
    #[test]
    fn compute_relocation_requires_matching_signature() {
        let baseline = make_result(vec![make_verdict_cx("a.rs", "alpha", 5.0, false, 5)]);
        let current = make_result(vec![make_verdict_cx("a.rs", "beta", 5.0, false, 9)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 2);
        let kinds: Vec<_> = delta.changes.iter().map(|c| c.kind()).collect();
        assert!(kinds.contains(&ChangeKind::Added));
        assert!(kinds.contains(&ChangeKind::Removed));
        assert!(!kinds.contains(&ChangeKind::Renamed));
    }

    /// A relocation that *also* worsened the score is still a `Renamed`,
    /// and it exposes the score movement just like `Modified` — coverage
    /// dropped, so CRAP rose and the function crossed the threshold.
    /// This is the "the rename was not the only change" case: it DOES
    /// count as a new violation.
    #[test]
    fn compute_renamed_with_regression_counts_as_new_violation() {
        // baseline: passing (under threshold); current: same fn moved
        // and now over threshold.
        let baseline = make_result(vec![make_verdict("a.rs", "foo", 7.0, false)]);
        let current = make_result(vec![make_verdict("b.rs", "foo", 47.0, true)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Renamed { .. }));
        assert_eq!(delta.changes[0].score_delta(), Some(40.0));
        assert_eq!(delta.summary.renamed, 1);
        assert_eq!(delta.summary.regressions, 1);
        assert_eq!(delta.summary.new_violations, 1);
        assert!(!delta.summary.passed);
    }

    /// DOCUMENTED LIMITATION (not a guarantee). The matcher works from
    /// verdicts, not source text, so two genuinely-unrelated functions
    /// with an identical structural signature — one removed, one added,
    /// each the only signature-mate on its side — are indistinguishable
    /// from a real rename and WILL pair as `Renamed`. When both already
    /// exceed the threshold, the new function inherits the baseline's
    /// "already failing" status, so a genuinely new violation is NOT
    /// flagged. The 1:1 + name/signature guards make this rare, but it is
    /// irreducible without source-level matching. Captured here so the
    /// behavior is a known, tested property — the delta gate never
    /// *raises* new violations (migrations pass) but a coincidental match
    /// can lower the count; it does not "never hide" a violation.
    #[test]
    fn compute_documented_limitation_coincidental_signature_pairs_as_renamed() {
        // `alpha` (already failing) is removed; `beta` (genuinely new,
        // also failing) is added — different names, identical distinctive
        // signature, one each side. Tier B pairs them on signature alone.
        let baseline = make_result(vec![make_verdict_cx("a.rs", "alpha", 47.0, true, 5)]);
        let current = make_result(vec![make_verdict_cx("b.rs", "beta", 47.0, true, 5)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Renamed { .. }));
        // The accepted consequence: because the baseline side already
        // exceeded, the coincidental pairing hides what would otherwise be
        // a new violation. This documents the limitation; it is NOT a
        // claim that relocation can never mask a violation.
        assert_eq!(delta.summary.new_violations, 0);
        assert!(delta.summary.passed);
    }

    /// Ambiguity guard: two leftovers per side sharing a structural
    /// signature (a name-swap, or two functions relocated together)
    /// stay Added + Removed — the 1:1 rule declines to guess which maps
    /// to which.
    #[test]
    fn compute_ambiguous_signature_stays_add_remove() {
        let baseline = make_result(vec![
            make_verdict("a.rs", "foo", 5.0, false),
            make_verdict("a.rs", "bar", 5.0, false),
        ]);
        // Same file, swapped names, identical signatures, plus a move.
        let current = make_result(vec![
            make_verdict("a.rs", "baz", 5.0, false),
            make_verdict("a.rs", "qux", 5.0, false),
        ]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 4);
        assert_eq!(delta.summary.renamed, 0);
        assert_eq!(delta.summary.added, 2);
        assert_eq!(delta.summary.removed, 2);
    }

    /// Trivial zero-contributor stubs all share the complexity-1 empty
    /// signature, so a file of relocated stubs collides on every side
    /// and none are arbitrarily paired — they stay Added + Removed.
    #[test]
    fn compute_zero_contributor_stubs_do_not_pair() {
        let baseline = make_result(vec![
            make_verdict_cx("a.rs", "s1", 1.0, false, 1),
            make_verdict_cx("a.rs", "s2", 1.0, false, 1),
        ]);
        let current = make_result(vec![
            make_verdict_cx("b.rs", "s3", 1.0, false, 1),
            make_verdict_cx("b.rs", "s4", 1.0, false, 1),
        ]);
        let delta = compute(baseline, current);
        assert_eq!(delta.summary.renamed, 0);
        assert_eq!(delta.summary.added, 2);
        assert_eq!(delta.summary.removed, 2);
    }

    /// A single trivial (complexity-1) function removed and a single
    /// trivial function added — different names — are NOT paired by
    /// signature, even though each is the only leftover on its side. A
    /// degenerate empty signature is no relocation signal; this is the
    /// real-world `first()` → `third()` stub case a naive 1:1 match
    /// would mis-link.
    #[test]
    fn compute_single_trivial_functions_do_not_pair_by_signature() {
        let baseline = make_result(vec![make_verdict_cx("a.rs", "first", 1.0, false, 1)]);
        let current = make_result(vec![make_verdict_cx("b.rs", "third", 1.0, false, 1)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 2);
        assert_eq!(delta.summary.renamed, 0);
        assert_eq!(delta.summary.added, 1);
        assert_eq!(delta.summary.removed, 1);
    }

    /// A trivial (complexity-1) function moved to another file but
    /// keeping its name still pairs as `Renamed` — Tier A keys on the
    /// retained name, a confident signal at any complexity, so the
    /// distinctiveness gate (which only restricts Tier B) does not block
    /// it.
    #[test]
    fn compute_trivial_function_moved_keeping_name_pairs_via_tier_a() {
        let baseline = make_result(vec![make_verdict_cx("a.rs", "helper", 1.0, false, 1)]);
        let current = make_result(vec![make_verdict_cx("b.rs", "helper", 1.0, false, 1)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Renamed { .. }));
        assert_eq!(delta.summary.renamed, 1);
    }

    /// A pure relocation (identical score) never contributes a new
    /// violation — even when the function was already over threshold on
    /// both sides. This is the headline migration-friendly behavior.
    #[test]
    fn compute_pure_relocation_of_failing_fn_is_not_a_new_violation() {
        let baseline = make_result(vec![make_verdict("a.rs", "big", 47.0, true)]);
        let current = make_result(vec![make_verdict("b.rs", "big", 47.0, true)]);
        let delta = compute(baseline, current);
        assert_eq!(delta.changes.len(), 1);
        assert!(matches!(delta.changes[0], FunctionChange::Renamed { .. }));
        assert_eq!(delta.summary.renamed, 1);
        assert_eq!(delta.summary.new_violations, 0);
        assert!(delta.summary.passed);
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

    // ── threshold-border epsilon (#277) ──

    #[test]
    fn within_band_uses_strict_less_than_and_zero_is_empty() {
        // threshold is 25.0 in make_verdict.
        let on_line = make_verdict("a.rs", "f", 25.0, true);
        let just_inside = make_verdict("a.rs", "f", 25.4, true);
        let on_edge = make_verdict("a.rs", "f", 25.5, true); // distance == epsilon
        let just_outside = make_verdict("a.rs", "f", 25.6, true);

        assert!(within_band(&on_line, 0.5), "distance 0 < 0.5");
        assert!(within_band(&just_inside, 0.5), "distance 0.4 < 0.5");
        assert!(
            !within_band(&on_edge, 0.5),
            "distance 0.5 is NOT < 0.5 (strict)"
        );
        assert!(!within_band(&just_outside, 0.5), "distance 0.6 >= 0.5");
        // epsilon 0.0 admits nothing — not even a score exactly on the line.
        assert!(!within_band(&on_line, 0.0), "zero-width band is empty");
    }

    #[test]
    fn epsilon_zero_is_byte_identical_to_bare_compute() {
        // A representative mix: a crossing Modified (new violation), a
        // regressing-but-passing Modified, an Added violator, a Removed.
        let changes = vec![
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "cross", 24.99, false),
                current: make_verdict("a.rs", "cross", 25.01, true),
            },
            FunctionChange::Modified {
                baseline: make_verdict("a.rs", "still_ok", 8.0, false),
                current: make_verdict("a.rs", "still_ok", 20.0, false),
            },
            FunctionChange::Added {
                current: make_verdict("a.rs", "new_bad", 31.0, true),
            },
            FunctionChange::Removed {
                baseline: make_verdict("a.rs", "gone", 47.0, true),
            },
        ];
        let bare = DeltaSummary::compute(&changes);
        let eps0 = DeltaSummary::compute_with_epsilon(&changes, 0.0);
        assert_eq!(eps0.added, bare.added);
        assert_eq!(eps0.removed, bare.removed);
        assert_eq!(eps0.modified, bare.modified);
        assert_eq!(eps0.regressions, bare.regressions);
        assert_eq!(eps0.improvements, bare.improvements);
        assert_eq!(eps0.new_violations, bare.new_violations);
        assert_eq!(eps0.passed, bare.passed);
        // The new bucket is inert at epsilon 0.
        assert_eq!(eps0.border_jitter_suppressed, 0);
        assert_eq!(bare.border_jitter_suppressed, 0);
        // Sanity: the crossing Modified + the Added violator both counted.
        assert_eq!(bare.new_violations, 2);
    }

    #[test]
    fn border_band_suppresses_modified_oscillation() {
        // 24.99 → 25.01 across threshold 25.0, both within epsilon 0.5.
        let changes = vec![FunctionChange::Modified {
            baseline: make_verdict("a.rs", "jitter", 24.99, false),
            current: make_verdict("a.rs", "jitter", 25.01, true),
        }];
        let summary = DeltaSummary::compute_with_epsilon(&changes, 0.5);
        assert_eq!(summary.new_violations, 0);
        assert_eq!(summary.border_jitter_suppressed, 1);
        assert!(
            summary.passed,
            "a suppressed border-jitter PR passes the gate"
        );
    }

    #[test]
    fn border_band_modified_one_side_outside_still_counts() {
        // baseline far below the band (clean), current just over: only one
        // side is in the band, so this is a genuine crossing, not jitter.
        let changes = vec![FunctionChange::Modified {
            baseline: make_verdict("a.rs", "real", 5.0, false),
            current: make_verdict("a.rs", "real", 25.01, true),
        }];
        let summary = DeltaSummary::compute_with_epsilon(&changes, 0.5);
        assert_eq!(summary.new_violations, 1);
        assert_eq!(summary.border_jitter_suppressed, 0);
        assert!(!summary.passed);
    }

    #[test]
    fn border_band_suppresses_added_in_band() {
        // Added asymmetry (one-sided soft bypass): a brand-new function
        // landing inside the band is forgiven per the AC.
        let changes = vec![FunctionChange::Added {
            current: make_verdict("a.rs", "new_borderline", 25.01, true),
        }];
        let summary = DeltaSummary::compute_with_epsilon(&changes, 0.5);
        assert_eq!(summary.added, 1);
        assert_eq!(summary.new_violations, 0);
        assert_eq!(summary.border_jitter_suppressed, 1);
    }

    #[test]
    fn border_band_added_outside_band_still_counts() {
        let changes = vec![FunctionChange::Added {
            current: make_verdict("a.rs", "new_bad", 31.0, true),
        }];
        let summary = DeltaSummary::compute_with_epsilon(&changes, 0.5);
        assert_eq!(summary.new_violations, 1);
        assert_eq!(summary.border_jitter_suppressed, 0);
    }

    #[test]
    fn border_band_renamed_takes_the_same_dual_sided_check_as_modified() {
        // Relocate-and-regress straddling the line within the band → suppressed.
        let both_in_band = vec![FunctionChange::Renamed {
            baseline: make_verdict("a.rs", "old", 24.99, false),
            current: make_verdict("b.rs", "new", 25.01, true),
        }];
        let s = DeltaSummary::compute_with_epsilon(&both_in_band, 0.5);
        assert_eq!(s.renamed, 1);
        assert_eq!(s.new_violations, 0);
        assert_eq!(s.border_jitter_suppressed, 1);

        // One side far outside the band → genuine crossing, still counts.
        let one_side_out = vec![FunctionChange::Renamed {
            baseline: make_verdict("a.rs", "old", 5.0, false),
            current: make_verdict("b.rs", "new", 25.01, true),
        }];
        let s2 = DeltaSummary::compute_with_epsilon(&one_side_out, 0.5);
        assert_eq!(s2.renamed, 1);
        assert_eq!(s2.new_violations, 1);
        assert_eq!(s2.border_jitter_suppressed, 0);
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
        assert_eq!(
            serde_json::to_string(&ChangeKind::Renamed).unwrap(),
            "\"renamed\""
        );
    }

    #[test]
    fn change_kind_all_contains_every_variant() {
        assert_eq!(ChangeKind::ALL.len(), 4);
        assert!(ChangeKind::ALL.contains(&ChangeKind::Added));
        assert!(ChangeKind::ALL.contains(&ChangeKind::Removed));
        assert!(ChangeKind::ALL.contains(&ChangeKind::Modified));
        assert!(ChangeKind::ALL.contains(&ChangeKind::Renamed));
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
            epsilon: 0.0,
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
            // Modified and Renamed each consume one baseline and one
            // current entry, so neither can exceed either side.
            let modified_count = delta
                .changes
                .iter()
                .filter(|c| matches!(c, FunctionChange::Modified { .. }))
                .count();
            prop_assert!(modified_count <= baseline_len);
            prop_assert!(modified_count <= current_len);
            let renamed_count = delta
                .changes
                .iter()
                .filter(|c| matches!(c, FunctionChange::Renamed { .. }))
                .count();
            prop_assert!(renamed_count <= baseline_len);
            prop_assert!(renamed_count <= current_len);
        }

        /// new_violations is bounded by the count of Added rows that
        /// exceed plus Modified / Renamed rows that crossed the
        /// threshold.
        #[test]
        fn prop_new_violations_well_bounded(
            baseline in arb_analysis_result(),
            current in arb_analysis_result(),
        ) {
            let delta = compute(baseline, current);
            let summary = DeltaSummary::compute(&delta.changes);
            prop_assert!(
                summary.new_violations <= summary.added + summary.modified + summary.renamed
            );
            prop_assert_eq!(summary.passed, summary.new_violations == 0);
        }

        /// Pairing a relocation never *raises* `new_violations` versus
        /// the pre-feature Add+Remove behavior. A `Renamed` row uses the
        /// `Modified` rule (`!baseline.exceeds && current.exceeds`),
        /// which is always ≤ the `Added` rule (`current.exceeds`) the
        /// unpaired half would have used — so enabling rename detection
        /// can only hold or lower the count, never newly fail a PR the
        /// old behavior would have passed.
        #[test]
        fn prop_renamed_never_raises_new_violations(
            baseline in arb_analysis_result(),
            current in arb_analysis_result(),
        ) {
            let delta = compute(baseline, current);
            let without_pairing: u32 = delta
                .changes
                .iter()
                .map(|change| match change {
                    FunctionChange::Added { current } => u32::from(current.exceeds),
                    FunctionChange::Modified { baseline, current } => {
                        u32::from(!baseline.exceeds && current.exceeds)
                    }
                    // Counted as the bare Added its current half would
                    // have been without the relocation pass.
                    FunctionChange::Renamed { current, .. } => u32::from(current.exceeds),
                    FunctionChange::Removed { .. } => 0,
                })
                .sum();
            prop_assert!(delta.summary.new_violations <= without_pairing);
        }

        /// The threshold-border epsilon conserves would-be new violations:
        /// for any change set and any finite epsilon ≥ 0,
        /// `new_violations(eps) + border_jitter_suppressed(eps)` equals
        /// `new_violations(0)`. This is the honest claim — epsilon only
        /// ever *moves* a would-be violation into the suppressed bucket,
        /// never adds or drops one (the #274 "never raises ≠ never hides"
        /// shape, pinned). It follows that epsilon never *raises* the
        /// gate count, and that suppression is bounded by the eps-0 count.
        #[test]
        fn prop_border_band_conserves_new_violations(
            baseline in arb_analysis_result(),
            current in arb_analysis_result(),
            epsilon in 0.0f64..50.0,
        ) {
            let delta = compute(baseline, current);
            let at_zero = DeltaSummary::compute(&delta.changes);
            let with_eps = DeltaSummary::compute_with_epsilon(&delta.changes, epsilon);
            prop_assert_eq!(
                with_eps.new_violations + with_eps.border_jitter_suppressed,
                at_zero.new_violations
            );
            prop_assert!(with_eps.new_violations <= at_zero.new_violations);
            prop_assert!(with_eps.border_jitter_suppressed <= at_zero.new_violations);
            // The non-gate counters are independent of epsilon.
            prop_assert_eq!(with_eps.added, at_zero.added);
            prop_assert_eq!(with_eps.removed, at_zero.removed);
            prop_assert_eq!(with_eps.modified, at_zero.modified);
            prop_assert_eq!(with_eps.renamed, at_zero.renamed);
            prop_assert_eq!(with_eps.regressions, at_zero.regressions);
            prop_assert_eq!(with_eps.improvements, at_zero.improvements);
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
