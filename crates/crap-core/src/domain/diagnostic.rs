//! Structured remediation hints (`Diagnostic`) attached to over-threshold
//! verdicts. Pure domain — no `syn`, no LCOV, no I/O.

use serde::{Deserialize, Serialize};

use crate::domain::types::{
    ComplexityContributor, ContributorKind, FunctionVerdict, LineCoverage, SourceSpan,
};

// ── Line range ──────────────────────────────────────────────────────

/// Inclusive 1-based line range. Mirrors `SourceSpan`'s end-inclusive
/// convention (per `.claude/rules/domain.md` §5) so coverage gaps and
/// proposed splits address the same line space as `ComplexityContributor`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

impl LineRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// True when `line` falls inside `[start, end]` (inclusive).
    pub fn contains(&self, line: usize) -> bool {
        self.start <= line && line <= self.end
    }
}

// ── Root cause ──────────────────────────────────────────────────────

/// Deterministic single-token classification of why a verdict exceeded
/// the threshold. `LowCoverage` when the only action is `AddTestsForLines`;
/// `HighComplexity` when the only actions are split/simplify/accept;
/// `Both` when both kinds of action coexist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RootCause {
    #[default]
    LowCoverage,
    HighComplexity,
    Both,
}

impl RootCause {
    /// Canonical wire string — equal to the serde JSON representation
    /// (sans quotes). See `crate::domain::types::ContributorKind::as_wire_str`
    /// for the rationale; equality with serde is pinned in
    /// `tests::wire_str_matches_serde`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::LowCoverage => "low_coverage",
            Self::HighComplexity => "high_complexity",
            Self::Both => "both",
        }
    }
}

// ── Applicability ───────────────────────────────────────────────────

/// Confidence in a `SuggestedAction`, matching `rustc`'s `Applicability`
/// taxonomy so agents using rustc-shaped tooling can interpret crap4rs
/// suggestions without translation. The default is `Unspecified` because
/// crap4rs does not verify the suggested change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Applicability {
    MachineApplicable,
    MaybeIncorrect,
    HasPlaceholders,
    #[default]
    Unspecified,
}

impl Applicability {
    /// Canonical wire string — see `RootCause::as_wire_str`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::MachineApplicable => "machine_applicable",
            Self::MaybeIncorrect => "maybe_incorrect",
            Self::HasPlaceholders => "has_placeholders",
            Self::Unspecified => "unspecified",
        }
    }
}

// ── SplitKind ───────────────────────────────────────────────────────

/// Which strategy produced a `ProposedSplit`. Priority order
/// `DeepestNesting > HighestBranchCount > LargestSubblock` is enforced
/// at dedup time. The default variant is `DeepestNesting` because that
/// is the highest-priority strategy and the most useful candidate when
/// only one is needed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SplitKind {
    #[default]
    DeepestNesting,
    LargestSubblock,
    HighestBranchCount,
}

impl SplitKind {
    /// Canonical wire string — see `RootCause::as_wire_str`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::DeepestNesting => "deepest_nesting",
            Self::LargestSubblock => "largest_subblock",
            Self::HighestBranchCount => "highest_branch_count",
        }
    }
}

// ── ProposedSplit ───────────────────────────────────────────────────

/// One AST-derived candidate for `extract_function`. The split is named
/// only by its line range — agents do prose, the CLI does coordinates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProposedSplit {
    pub line_range: LineRange,
    /// Sum of contributor increments inside `line_range` (cognitive or
    /// cyclomatic — the metric is recorded on `ScoredFunction`).
    pub complexity_contribution: u32,
    /// `/`-joined chain of `ContributorKind` Display strings, ascending
    /// by nesting up to the split's `start_line`. AST-only, no prose.
    pub branch_path: String,
    pub kind: SplitKind,
    /// Exactly one entry per non-empty candidate set carries
    /// `recommended: true`.
    pub recommended: bool,
}

// ── SuggestedAction ─────────────────────────────────────────────────

/// One remediation step. Tagged-enum serialization (`{"kind": "…", …}`)
/// keeps additive variants forward-compatible under `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SuggestedAction {
    AddTestsForLines {
        lines: Vec<LineRange>,
        applicability: Applicability,
    },
    ExtractFunction {
        candidates: Vec<ProposedSplit>,
        applicability: Applicability,
    },
    SimplifyBranching {
        drivers: Vec<ContributorKind>,
        applicability: Applicability,
    },
    AcceptInherentComplexity {
        applicability: Applicability,
    },
}

// ── Diagnostic ──────────────────────────────────────────────────────

/// Structured remediation hint attached to an over-threshold
/// `FunctionVerdict` when `--format advice` (or `--format sarif`) is
/// requested. Type-level `#[serde(default)]` lets older payloads
/// deserialize when fields are added under `#[non_exhaustive]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Diagnostic {
    pub coverage_gaps: Vec<LineRange>,
    pub complexity_drivers: Vec<ComplexityContributor>,
    pub suggested_actions: Vec<SuggestedAction>,
    pub root_cause: RootCause,
}

// ── Helpers: compute_diagnostic + sub-helpers ───────────────────────
//
// All helpers are pure: no I/O, no `syn`, no LCOV. They consume
// already-parsed `FunctionVerdict` + `LineCoverage` slices and emit
// AST-derived advice. Callers keep `compute_diagnostic` as the single
// entry point; the others are `pub(crate)` so tests can pin them.

/// Effective inclusive end line for a contributor. Older payloads may
/// carry `end_line == 0`; fall back to `line` so the construct is
/// treated as a single-line atomic.
fn effective_end_line(c: &ComplexityContributor) -> usize {
    if c.end_line >= c.line {
        c.end_line
    } else {
        c.line
    }
}

/// True when the construct spans multiple source lines.
fn is_compound(c: &ComplexityContributor) -> bool {
    effective_end_line(c) > c.line
}

/// Sum of contributor increments inside `range`. Each contributor is
/// counted iff its `line` falls inside `[range.start, range.end]`.
fn sum_increments_in(contributors: &[ComplexityContributor], range: LineRange) -> u32 {
    contributors
        .iter()
        .filter(|c| range.contains(c.line))
        .map(|c| c.increment)
        .sum()
}

/// Count of contributors whose `line` falls strictly inside `range` —
/// used by `pick_highest_branch_count` to score how many sub-decisions
/// a compound contributor encloses.
fn count_inner_contributors(
    contributors: &[ComplexityContributor],
    outer: &ComplexityContributor,
) -> usize {
    let outer_range = LineRange::new(outer.line, effective_end_line(outer));
    contributors
        .iter()
        .filter(|c| {
            // Skip the contributor itself.
            !(c.line == outer.line && c.kind == outer.kind && c.column == outer.column)
                && outer_range.contains(c.line)
        })
        .count()
}

/// True when a candidate range describes a meaningful extract target
/// inside `span` — must be multi-line, strictly within the function,
/// and carry at least one contributor.
fn is_viable_split(range: LineRange, span: &SourceSpan, contribution: u32) -> bool {
    range.start < range.end
        && range.start >= span.start_line
        && range.end <= span.end_line
        && !(range.start == span.start_line && range.end == span.end_line)
        && contribution >= 1
}

/// `/`-joined chain of `ContributorKind` Display strings for every
/// contributor that **strictly encloses** `start_line` (i.e., starts
/// before `start_line` and ends at or after it). Sorted by
/// `nesting_depth` ascending so the path reads outermost → innermost.
/// AST-derived only — never carries prose.
pub(crate) fn derive_branch_path(
    contributors: &[ComplexityContributor],
    start_line: usize,
) -> String {
    let mut enclosing: Vec<&ComplexityContributor> = contributors
        .iter()
        .filter(|c| c.line < start_line && start_line <= effective_end_line(c))
        .collect();
    enclosing.sort_by_key(|c| (c.nesting_depth, c.line));
    enclosing
        .iter()
        .map(|c| c.kind.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Coalesce uncovered (`hits == 0`) lines inside `span` into inclusive
/// ranges. Pre-condition: `line_coverage` may be unsorted; we sort by
/// line before coalescing so adapters don't have to.
pub(crate) fn derive_coverage_gaps(
    line_coverage: &[LineCoverage],
    span: &SourceSpan,
) -> Vec<LineRange> {
    let mut uncovered: Vec<usize> = line_coverage
        .iter()
        .filter(|lc| lc.hits == 0 && lc.line >= span.start_line && lc.line <= span.end_line)
        .map(|lc| lc.line)
        .collect();
    uncovered.sort_unstable();
    uncovered.dedup();

    let mut gaps: Vec<LineRange> = Vec::new();
    for line in uncovered {
        match gaps.last_mut() {
            Some(last) if last.end + 1 == line => last.end = line,
            _ => gaps.push(LineRange::new(line, line)),
        }
    }
    gaps
}

/// Pick the deepest-nested compound contributor as a split candidate.
/// Tiebreaker: lowest `line` wins so the result is deterministic.
pub(crate) fn pick_deepest_nesting(
    contributors: &[ComplexityContributor],
    span: &SourceSpan,
) -> Option<ProposedSplit> {
    let pick = contributors
        .iter()
        .filter(|c| is_compound(c) && c.nesting_depth > 0)
        .max_by_key(|c| (c.nesting_depth, std::cmp::Reverse(c.line)))?;
    let range = LineRange::new(pick.line, effective_end_line(pick));
    let contribution = sum_increments_in(contributors, range);
    if !is_viable_split(range, span, contribution) {
        return None;
    }
    Some(ProposedSplit {
        line_range: range,
        complexity_contribution: contribution,
        branch_path: derive_branch_path(contributors, range.start),
        kind: SplitKind::DeepestNesting,
        recommended: false,
    })
}

/// Pick the compound contributor covering the largest source span.
/// Tiebreaker: lowest `line` wins.
pub(crate) fn pick_largest_subblock(
    contributors: &[ComplexityContributor],
    span: &SourceSpan,
) -> Option<ProposedSplit> {
    let pick = contributors
        .iter()
        .filter(|c| is_compound(c))
        .max_by_key(|c| {
            let span_len = effective_end_line(c) - c.line;
            (span_len, std::cmp::Reverse(c.line))
        })?;
    let range = LineRange::new(pick.line, effective_end_line(pick));
    let contribution = sum_increments_in(contributors, range);
    if !is_viable_split(range, span, contribution) {
        return None;
    }
    Some(ProposedSplit {
        line_range: range,
        complexity_contribution: contribution,
        branch_path: derive_branch_path(contributors, range.start),
        kind: SplitKind::LargestSubblock,
        recommended: false,
    })
}

/// Pick the compound contributor that encloses the most other
/// contributors (densest decision cluster). Tiebreaker: lowest `line`.
pub(crate) fn pick_highest_branch_count(
    contributors: &[ComplexityContributor],
    span: &SourceSpan,
) -> Option<ProposedSplit> {
    let pick = contributors
        .iter()
        .filter(|c| is_compound(c))
        .max_by_key(|c| {
            let count = count_inner_contributors(contributors, c);
            (count, std::cmp::Reverse(c.line))
        })?;
    // Reject degenerate "no inner contributors" picks — that's a
    // single-construct function, not a branch cluster.
    if count_inner_contributors(contributors, pick) == 0 {
        return None;
    }
    let range = LineRange::new(pick.line, effective_end_line(pick));
    let contribution = sum_increments_in(contributors, range);
    if !is_viable_split(range, span, contribution) {
        return None;
    }
    Some(ProposedSplit {
        line_range: range,
        complexity_contribution: contribution,
        branch_path: derive_branch_path(contributors, range.start),
        kind: SplitKind::HighestBranchCount,
        recommended: false,
    })
}

/// Run all three selectors, dedup overlapping ranges by `SplitKind`
/// and mark exactly one survivor `recommended: true`.
pub(crate) fn extract_split_candidates(
    contributors: &[ComplexityContributor],
    span: &SourceSpan,
) -> Vec<ProposedSplit> {
    let raw: Vec<ProposedSplit> = [
        pick_deepest_nesting(contributors, span),
        pick_highest_branch_count(contributors, span),
        pick_largest_subblock(contributors, span),
    ]
    .into_iter()
    .flatten()
    .collect();
    dedup_splits(raw)
}

/// Priority weight for `SplitKind` (higher = wins under dedup).
fn split_kind_priority(kind: SplitKind) -> u8 {
    match kind {
        SplitKind::DeepestNesting => 3,
        SplitKind::HighestBranchCount => 2,
        SplitKind::LargestSubblock => 1,
    }
}

/// Dedup `splits` by `line_range`. When two candidates share the same
/// range, the highest-priority `kind` wins
/// (`DeepestNesting > HighestBranchCount > LargestSubblock`). Within
/// the same kind, the lowest `start_line` wins. Exactly one survivor
/// is marked `recommended: true` (highest priority overall).
pub(crate) fn dedup_splits(splits: Vec<ProposedSplit>) -> Vec<ProposedSplit> {
    let mut by_range: Vec<ProposedSplit> = Vec::new();
    for split in splits {
        match by_range
            .iter()
            .position(|existing| existing.line_range == split.line_range)
        {
            Some(idx) => {
                let new_priority = split_kind_priority(split.kind);
                let existing_priority = split_kind_priority(by_range[idx].kind);
                if new_priority > existing_priority {
                    by_range[idx] = split;
                }
            }
            None => by_range.push(split),
        }
    }

    by_range.sort_by_key(|s| {
        (
            std::cmp::Reverse(split_kind_priority(s.kind)),
            s.line_range.start,
        )
    });

    if let Some(first) = by_range.first_mut() {
        first.recommended = true;
    }
    by_range
}

/// Strict-majority dominant contributor kind: returns `Some(kind)` when
/// one kind accounts for **strictly more than 70%** of contributor
/// count. Returns `None` for a flat function (zero contributors).
///
/// Linear scan rather than `HashMap` keeps `ContributorKind`'s public
/// derives unchanged (no `Hash` requirement leaks into other crates).
fn dominant_contributor_kind(contributors: &[ComplexityContributor]) -> Option<ContributorKind> {
    if contributors.is_empty() {
        return None;
    }
    let total = contributors.len();
    let mut counts: Vec<(ContributorKind, usize)> = Vec::new();
    for c in contributors {
        match counts.iter_mut().find(|(k, _)| *k == c.kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((c.kind, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count * 100 > total * 70)
        .map(|(kind, _)| kind)
}

/// Build the `(suggested_actions, root_cause)` pair from gaps + splits
/// + contributors. Action gating:
///
/// 1. `gaps non-empty` → `AddTestsForLines`
/// 2. `splits non-empty` → `ExtractFunction`
/// 3. `splits empty AND >70% dominant kind` → `SimplifyBranching`
/// 4. `no other action emitted` → `AcceptInherentComplexity`
pub(crate) fn pick_actions(
    coverage_gaps: &[LineRange],
    splits: &[ProposedSplit],
    contributors: &[ComplexityContributor],
) -> (Vec<SuggestedAction>, RootCause) {
    let mut actions: Vec<SuggestedAction> = Vec::new();

    if !coverage_gaps.is_empty() {
        actions.push(SuggestedAction::AddTestsForLines {
            lines: coverage_gaps.to_vec(),
            applicability: Applicability::default(),
        });
    }

    if !splits.is_empty() {
        actions.push(SuggestedAction::ExtractFunction {
            candidates: splits.to_vec(),
            applicability: Applicability::default(),
        });
    } else if let Some(dominant) = dominant_contributor_kind(contributors) {
        actions.push(SuggestedAction::SimplifyBranching {
            drivers: vec![dominant],
            applicability: Applicability::default(),
        });
    }

    let has_complexity_action = actions
        .iter()
        .any(|a| !matches!(a, SuggestedAction::AddTestsForLines { .. }));

    if !has_complexity_action && coverage_gaps.is_empty() {
        actions.push(SuggestedAction::AcceptInherentComplexity {
            applicability: Applicability::default(),
        });
    }

    let has_add_tests = !coverage_gaps.is_empty();
    let has_complexity = actions
        .iter()
        .any(|a| !matches!(a, SuggestedAction::AddTestsForLines { .. }));

    let root_cause = match (has_add_tests, has_complexity) {
        (true, true) => RootCause::Both,
        (true, false) => RootCause::LowCoverage,
        (false, _) => RootCause::HighComplexity,
    };

    (actions, root_cause)
}

/// Build a structured remediation hint for `verdict`. Returns `None`
/// when the verdict is below threshold (no diagnostic for
/// passing functions). When the verdict exceeds, the diagnostic is
/// always populated with all four fields.
///
/// The `line_coverage` slice should hold every `DA:` entry from the
/// LCOV file scoped to the verdict's source file; callers in
/// `core::analyze` already have this in hand. Entries outside
/// `verdict.scored.identity.span` are filtered out by
/// `derive_coverage_gaps`.
pub fn compute_diagnostic(
    verdict: &FunctionVerdict,
    line_coverage: &[LineCoverage],
) -> Option<Diagnostic> {
    if !verdict.exceeds {
        return None;
    }

    let span = &verdict.scored.identity.span;
    let contributors = verdict.scored.contributors.as_slice();

    let coverage_gaps = derive_coverage_gaps(line_coverage, span);
    let splits = extract_split_candidates(contributors, span);
    let (suggested_actions, root_cause) = pick_actions(&coverage_gaps, &splits, contributors);

    Some(Diagnostic {
        coverage_gaps,
        complexity_drivers: contributors.to_vec(),
        suggested_actions,
        root_cause,
    })
}

// ── Tests: serde round-trip + default ───────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_range_contains_inclusive_endpoints() {
        let r = LineRange::new(10, 12);
        assert!(r.contains(10));
        assert!(r.contains(11));
        assert!(r.contains(12));
        assert!(!r.contains(9));
        assert!(!r.contains(13));
    }

    #[test]
    fn diagnostic_default_is_low_coverage_and_empty_vecs() {
        let d = Diagnostic::default();
        assert_eq!(d.root_cause, RootCause::LowCoverage);
        assert!(d.coverage_gaps.is_empty());
        assert!(d.complexity_drivers.is_empty());
        assert!(d.suggested_actions.is_empty());
    }

    #[test]
    fn diagnostic_deserializes_empty_object_to_default() {
        // Type-level `#[serde(default)]` means `{}` round-trips through
        // `Diagnostic::default()`. Pins the additive convention so future
        // payloads adding fields don't break older readers.
        let parsed: Diagnostic = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, Diagnostic::default());
    }

    #[test]
    fn root_cause_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&RootCause::LowCoverage).unwrap(),
            "\"low_coverage\""
        );
        assert_eq!(
            serde_json::to_string(&RootCause::HighComplexity).unwrap(),
            "\"high_complexity\""
        );
        assert_eq!(serde_json::to_string(&RootCause::Both).unwrap(), "\"both\"");
    }

    #[test]
    fn applicability_default_is_unspecified() {
        assert_eq!(Applicability::default(), Applicability::Unspecified);
        assert_eq!(
            serde_json::to_string(&Applicability::default()).unwrap(),
            "\"unspecified\""
        );
    }

    #[test]
    fn applicability_round_trips_all_variants() {
        for variant in [
            Applicability::MachineApplicable,
            Applicability::MaybeIncorrect,
            Applicability::HasPlaceholders,
            Applicability::Unspecified,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: Applicability = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn split_kind_default_is_deepest_nesting() {
        assert_eq!(SplitKind::default(), SplitKind::DeepestNesting);
    }

    #[test]
    fn split_kind_round_trips_all_variants() {
        for variant in [
            SplitKind::DeepestNesting,
            SplitKind::LargestSubblock,
            SplitKind::HighestBranchCount,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: SplitKind = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn proposed_split_round_trips() {
        let original = ProposedSplit {
            line_range: LineRange::new(20, 35),
            complexity_contribution: 7,
            branch_path: "if-branch/match".to_string(),
            kind: SplitKind::DeepestNesting,
            recommended: true,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ProposedSplit = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn suggested_action_serializes_with_kind_tag_add_tests() {
        let action = SuggestedAction::AddTestsForLines {
            lines: vec![LineRange::new(1, 5)],
            applicability: Applicability::Unspecified,
        };
        let value: serde_json::Value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "add_tests_for_lines");
        assert!(value["lines"].is_array());
        assert_eq!(value["applicability"], "unspecified");
    }

    #[test]
    fn suggested_action_serializes_with_kind_tag_extract_function() {
        let action = SuggestedAction::ExtractFunction {
            candidates: vec![ProposedSplit {
                line_range: LineRange::new(10, 20),
                complexity_contribution: 4,
                branch_path: "if-branch".to_string(),
                kind: SplitKind::HighestBranchCount,
                recommended: true,
            }],
            applicability: Applicability::Unspecified,
        };
        let value: serde_json::Value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "extract_function");
        assert!(value["candidates"].is_array());
    }

    #[test]
    fn suggested_action_serializes_with_kind_tag_simplify_branching() {
        let action = SuggestedAction::SimplifyBranching {
            drivers: vec![ContributorKind::Match],
            applicability: Applicability::Unspecified,
        };
        let value: serde_json::Value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "simplify_branching");
        assert_eq!(value["drivers"][0], "match");
    }

    #[test]
    fn suggested_action_serializes_with_kind_tag_accept_inherent() {
        let action = SuggestedAction::AcceptInherentComplexity {
            applicability: Applicability::Unspecified,
        };
        let value: serde_json::Value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "accept_inherent_complexity");
    }

    #[test]
    fn suggested_action_round_trips_through_json() {
        // Round-trip every variant to lock the tagged serde shape.
        let actions = vec![
            SuggestedAction::AddTestsForLines {
                lines: vec![LineRange::new(3, 7)],
                applicability: Applicability::MachineApplicable,
            },
            SuggestedAction::ExtractFunction {
                candidates: vec![],
                applicability: Applicability::MaybeIncorrect,
            },
            SuggestedAction::SimplifyBranching {
                drivers: vec![ContributorKind::IfBranch, ContributorKind::Match],
                applicability: Applicability::HasPlaceholders,
            },
            SuggestedAction::AcceptInherentComplexity {
                applicability: Applicability::Unspecified,
            },
        ];
        for original in actions {
            let json = serde_json::to_string(&original).unwrap();
            let parsed: SuggestedAction = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, original);
        }
    }

    #[test]
    fn diagnostic_round_trips_full_shape() {
        let original = Diagnostic {
            coverage_gaps: vec![LineRange::new(12, 14)],
            complexity_drivers: vec![ComplexityContributor {
                kind: ContributorKind::Match,
                line: 20,
                column: Some(4),
                increment: 2,
                end_line: 30,
                nesting_depth: 1,
            }],
            suggested_actions: vec![SuggestedAction::AcceptInherentComplexity {
                applicability: Applicability::Unspecified,
            }],
            root_cause: RootCause::HighComplexity,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Diagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    // ── Helper tests ───────────────────────────────────────────────

    use crate::domain::types::{
        ComplexityMetric, CrapScore, FunctionIdentity, FunctionVerdict, LineCoverage, RiskLevel,
        ScoredFunction, SourceSpan,
    };

    fn make_contributor(
        kind: ContributorKind,
        line: usize,
        end_line: usize,
        increment: u32,
        nesting_depth: u32,
    ) -> ComplexityContributor {
        ComplexityContributor {
            kind,
            line,
            column: None,
            increment,
            end_line,
            nesting_depth,
        }
    }

    fn span_for(start: usize, end: usize) -> SourceSpan {
        SourceSpan {
            start_line: start,
            end_line: end,
            start_column: 0,
            end_column: 0,
        }
    }

    fn make_verdict(
        contributors: Vec<ComplexityContributor>,
        span: SourceSpan,
        exceeds: bool,
    ) -> FunctionVerdict {
        FunctionVerdict {
            scored: ScoredFunction {
                identity: FunctionIdentity {
                    file_path: "src/lib.rs".to_string(),
                    qualified_name: "demo".to_string(),
                    span,
                },
                complexity: contributors.iter().map(|c| c.increment).sum::<u32>().max(1),
                complexity_metric: ComplexityMetric::Cognitive,
                coverage_percent: 100.0,
                crap: CrapScore {
                    value: 50.0,
                    risk_level: RiskLevel::High,
                },
                contributors,
            },
            threshold: 30.0,
            exceeds,
            diagnostic: None,
        }
    }

    // derive_branch_path

    #[test]
    fn derive_branch_path_empty_for_top_level_construct() {
        // Mirrors `single_if_fn`: a single if-branch at top level. Path is
        // empty because nothing encloses line 9.
        let contributors = vec![make_contributor(ContributorKind::IfBranch, 9, 13, 1, 0)];
        assert_eq!(derive_branch_path(&contributors, 9), "");
    }

    #[test]
    fn derive_branch_path_single_for_nested_if_inner_start() {
        // Outer if [17, 25] depth 0 + inner if [18, 22] depth 1.
        // start_line=18 picks the outer (encloses 18) but not the inner
        // (it starts AT 18, doesn't enclose).
        let contributors = vec![
            make_contributor(ContributorKind::IfBranch, 17, 25, 1, 0),
            make_contributor(ContributorKind::IfBranch, 18, 22, 2, 1),
        ];
        assert_eq!(derive_branch_path(&contributors, 18), "if-branch");
    }

    #[test]
    fn derive_branch_path_chains_outer_to_inner_by_nesting_depth() {
        // Mirrors `for_with_continue_fn`: ForLoop > IfBranch > Continue.
        // Path for line 93 (the Continue) ascends ForLoop → IfBranch.
        let contributors = vec![
            make_contributor(ContributorKind::ForLoop, 91, 96, 1, 0),
            make_contributor(ContributorKind::IfBranch, 92, 94, 2, 1),
            make_contributor(ContributorKind::Continue, 93, 93, 3, 2),
        ];
        assert_eq!(derive_branch_path(&contributors, 93), "for-loop/if-branch");
    }

    #[test]
    fn derive_branch_path_carries_no_prose_or_whitespace() {
        // R6.3: no human prose, only `/`-joined ContributorKind discriminants.
        let contributors = vec![
            make_contributor(ContributorKind::Match, 10, 30, 1, 0),
            make_contributor(ContributorKind::IfBranch, 15, 20, 2, 1),
        ];
        let path = derive_branch_path(&contributors, 17);
        for component in path.split('/') {
            assert!(
                !component.contains(' '),
                "branch_path component {component:?} contains whitespace"
            );
        }
    }

    // derive_coverage_gaps

    #[test]
    fn derive_coverage_gaps_returns_empty_for_full_coverage() {
        let cov = vec![
            LineCoverage { line: 5, hits: 1 },
            LineCoverage { line: 6, hits: 3 },
        ];
        let gaps = derive_coverage_gaps(&cov, &span_for(1, 10));
        assert!(gaps.is_empty());
    }

    #[test]
    fn derive_coverage_gaps_coalesces_contiguous_uncovered_lines() {
        let cov = vec![
            LineCoverage { line: 5, hits: 0 },
            LineCoverage { line: 6, hits: 0 },
            LineCoverage { line: 7, hits: 0 },
            LineCoverage { line: 9, hits: 0 },
        ];
        let gaps = derive_coverage_gaps(&cov, &span_for(1, 10));
        assert_eq!(gaps, vec![LineRange::new(5, 7), LineRange::new(9, 9)]);
    }

    #[test]
    fn derive_coverage_gaps_filters_lines_outside_span() {
        let cov = vec![
            LineCoverage { line: 1, hits: 0 },
            LineCoverage { line: 5, hits: 0 },
            LineCoverage { line: 99, hits: 0 },
        ];
        let gaps = derive_coverage_gaps(&cov, &span_for(4, 6));
        assert_eq!(gaps, vec![LineRange::new(5, 5)]);
    }

    #[test]
    fn derive_coverage_gaps_handles_unsorted_input() {
        let cov = vec![
            LineCoverage { line: 7, hits: 0 },
            LineCoverage { line: 5, hits: 0 },
            LineCoverage { line: 6, hits: 0 },
        ];
        let gaps = derive_coverage_gaps(&cov, &span_for(1, 10));
        assert_eq!(gaps, vec![LineRange::new(5, 7)]);
    }

    // pick_deepest_nesting

    #[test]
    fn pick_deepest_nesting_picks_inner_if_in_nested_function() {
        let contributors = vec![
            make_contributor(ContributorKind::IfBranch, 17, 25, 1, 0),
            make_contributor(ContributorKind::IfBranch, 18, 22, 2, 1),
        ];
        let split = pick_deepest_nesting(&contributors, &span_for(16, 26)).expect("viable");
        assert_eq!(split.line_range, LineRange::new(18, 22));
        assert_eq!(split.kind, SplitKind::DeepestNesting);
        assert_eq!(split.complexity_contribution, 2);
        assert_eq!(split.branch_path, "if-branch");
        assert!(!split.recommended); // marked by dedup, not the selector
    }

    #[test]
    fn pick_deepest_nesting_returns_none_for_flat_function() {
        // Single top-level if has no nested constructs — DeepestNesting
        // requires depth > 0.
        let contributors = vec![make_contributor(ContributorKind::IfBranch, 9, 13, 1, 0)];
        assert!(pick_deepest_nesting(&contributors, &span_for(8, 14)).is_none());
    }

    #[test]
    fn pick_deepest_nesting_skips_atomic_continue() {
        // for_with_continue_fn: Continue is depth 2 but single-line.
        // Selector falls back to the deepest *compound* contributor.
        let contributors = vec![
            make_contributor(ContributorKind::ForLoop, 91, 96, 1, 0),
            make_contributor(ContributorKind::IfBranch, 92, 94, 2, 1),
            make_contributor(ContributorKind::Continue, 93, 93, 3, 2),
        ];
        let split = pick_deepest_nesting(&contributors, &span_for(89, 98)).expect("viable");
        assert_eq!(split.line_range, LineRange::new(92, 94));
    }

    // pick_largest_subblock

    #[test]
    fn pick_largest_subblock_picks_outer_if_over_inner() {
        let contributors = vec![
            make_contributor(ContributorKind::IfBranch, 17, 25, 1, 0),
            make_contributor(ContributorKind::IfBranch, 18, 22, 2, 1),
        ];
        let split = pick_largest_subblock(&contributors, &span_for(16, 30)).expect("viable");
        assert_eq!(split.line_range, LineRange::new(17, 25));
        assert_eq!(split.kind, SplitKind::LargestSubblock);
    }

    #[test]
    fn pick_largest_subblock_returns_none_when_range_equals_full_function() {
        // The sole compound construct covers the whole function — no
        // viable extraction (would just be a self-rename).
        let contributors = vec![make_contributor(ContributorKind::IfBranch, 1, 10, 1, 0)];
        assert!(pick_largest_subblock(&contributors, &span_for(1, 10)).is_none());
    }

    // pick_highest_branch_count

    #[test]
    fn pick_highest_branch_count_picks_outer_with_inner_contributors() {
        // outer if encloses inner if + inner continue = 2 inner;
        // inner if encloses 1; continue encloses 0. Outer wins.
        let contributors = vec![
            make_contributor(ContributorKind::IfBranch, 91, 96, 1, 0),
            make_contributor(ContributorKind::IfBranch, 92, 94, 2, 1),
            make_contributor(ContributorKind::Continue, 93, 93, 3, 2),
        ];
        let split = pick_highest_branch_count(&contributors, &span_for(89, 98)).expect("viable");
        assert_eq!(split.line_range, LineRange::new(91, 96));
        assert_eq!(split.kind, SplitKind::HighestBranchCount);
    }

    #[test]
    fn pick_highest_branch_count_returns_none_when_no_nested_contributors() {
        // Single compound with nothing inside → degenerate, no candidate.
        let contributors = vec![make_contributor(ContributorKind::IfBranch, 9, 13, 1, 0)];
        assert!(pick_highest_branch_count(&contributors, &span_for(8, 14)).is_none());
    }

    // dedup_splits

    #[test]
    fn dedup_splits_keeps_highest_priority_for_duplicate_range() {
        // Same range, three kinds. DeepestNesting wins by priority.
        let range = LineRange::new(10, 20);
        let dn = ProposedSplit {
            line_range: range,
            complexity_contribution: 4,
            branch_path: "match".to_string(),
            kind: SplitKind::DeepestNesting,
            recommended: false,
        };
        let hbc = ProposedSplit {
            kind: SplitKind::HighestBranchCount,
            ..dn.clone()
        };
        let lsb = ProposedSplit {
            kind: SplitKind::LargestSubblock,
            ..dn.clone()
        };
        let result = dedup_splits(vec![lsb, dn.clone(), hbc]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, SplitKind::DeepestNesting);
        assert!(result[0].recommended);
    }

    #[test]
    fn dedup_splits_keeps_highest_branch_count_over_largest_subblock() {
        let range = LineRange::new(10, 20);
        let hbc = ProposedSplit {
            line_range: range,
            complexity_contribution: 4,
            branch_path: String::new(),
            kind: SplitKind::HighestBranchCount,
            recommended: false,
        };
        let lsb = ProposedSplit {
            kind: SplitKind::LargestSubblock,
            ..hbc.clone()
        };
        let result = dedup_splits(vec![lsb, hbc]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, SplitKind::HighestBranchCount);
        assert!(result[0].recommended);
    }

    #[test]
    fn dedup_splits_marks_exactly_one_recommended_when_distinct_ranges() {
        let s1 = ProposedSplit {
            line_range: LineRange::new(10, 20),
            complexity_contribution: 2,
            branch_path: String::new(),
            kind: SplitKind::DeepestNesting,
            recommended: false,
        };
        let s2 = ProposedSplit {
            line_range: LineRange::new(30, 40),
            kind: SplitKind::HighestBranchCount,
            ..s1.clone()
        };
        let s3 = ProposedSplit {
            line_range: LineRange::new(50, 60),
            kind: SplitKind::LargestSubblock,
            ..s1.clone()
        };
        let result = dedup_splits(vec![s3, s2, s1]);
        assert_eq!(result.len(), 3);
        let recommended_count = result.iter().filter(|s| s.recommended).count();
        assert_eq!(recommended_count, 1);
        // Highest priority survivor = DeepestNesting; sort puts it first.
        assert!(result[0].recommended);
        assert_eq!(result[0].kind, SplitKind::DeepestNesting);
    }

    #[test]
    fn dedup_splits_empty_input_yields_empty() {
        assert!(dedup_splits(vec![]).is_empty());
    }

    // pick_actions — covers the root_cause Examples table

    #[test]
    fn pick_actions_low_coverage_only() {
        let gaps = vec![LineRange::new(5, 7)];
        let (actions, root) = pick_actions(&gaps, &[], &[]);
        assert_eq!(root, RootCause::LowCoverage);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            SuggestedAction::AddTestsForLines { .. }
        ));
    }

    #[test]
    fn pick_actions_high_complexity_only_via_extract_function() {
        let split = ProposedSplit {
            line_range: LineRange::new(10, 20),
            complexity_contribution: 3,
            branch_path: String::new(),
            kind: SplitKind::DeepestNesting,
            recommended: true,
        };
        let (actions, root) = pick_actions(&[], &[split], &[]);
        assert_eq!(root, RootCause::HighComplexity);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            SuggestedAction::ExtractFunction { .. }
        ));
    }

    #[test]
    fn pick_actions_both_emits_both_actions() {
        let gaps = vec![LineRange::new(5, 7)];
        let split = ProposedSplit {
            line_range: LineRange::new(10, 20),
            complexity_contribution: 3,
            branch_path: String::new(),
            kind: SplitKind::DeepestNesting,
            recommended: true,
        };
        let (actions, root) = pick_actions(&gaps, &[split], &[]);
        assert_eq!(root, RootCause::Both);
        assert_eq!(actions.len(), 2);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SuggestedAction::AddTestsForLines { .. }))
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SuggestedAction::ExtractFunction { .. }))
        );
    }

    #[test]
    fn pick_actions_no_splits_no_gaps_falls_back_to_accept_inherent() {
        let (actions, root) = pick_actions(&[], &[], &[]);
        assert_eq!(root, RootCause::HighComplexity);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            SuggestedAction::AcceptInherentComplexity { .. }
        ));
    }

    #[test]
    fn pick_actions_dominant_kind_emits_simplify_branching_when_no_splits() {
        // Four IfBranch contributors out of five (80%) — dominant.
        let mut contribs = vec![
            make_contributor(ContributorKind::IfBranch, 1, 1, 1, 0),
            make_contributor(ContributorKind::IfBranch, 2, 2, 1, 0),
            make_contributor(ContributorKind::IfBranch, 3, 3, 1, 0),
            make_contributor(ContributorKind::IfBranch, 4, 4, 1, 0),
        ];
        contribs.push(make_contributor(ContributorKind::Match, 5, 5, 1, 0));
        let (actions, root) = pick_actions(&[], &[], &contribs);
        assert_eq!(root, RootCause::HighComplexity);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SuggestedAction::SimplifyBranching { .. }))
        );
    }

    #[test]
    fn pick_actions_simplify_branching_with_low_coverage_yields_both() {
        let gaps = vec![LineRange::new(2, 3)];
        let contribs = vec![
            make_contributor(ContributorKind::IfBranch, 1, 1, 1, 0),
            make_contributor(ContributorKind::IfBranch, 2, 2, 1, 0),
            make_contributor(ContributorKind::IfBranch, 3, 3, 1, 0),
            make_contributor(ContributorKind::IfBranch, 4, 4, 1, 0),
        ];
        let (actions, root) = pick_actions(&gaps, &[], &contribs);
        assert_eq!(root, RootCause::Both);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SuggestedAction::AddTestsForLines { .. }))
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, SuggestedAction::SimplifyBranching { .. }))
        );
    }

    #[test]
    fn pick_actions_extract_function_takes_precedence_over_simplify() {
        // Even though IfBranch is 100% dominant, splits are non-empty so
        // we emit ExtractFunction (concrete) and skip SimplifyBranching.
        let contribs = vec![
            make_contributor(ContributorKind::IfBranch, 1, 5, 1, 0),
            make_contributor(ContributorKind::IfBranch, 2, 4, 2, 1),
        ];
        let split = ProposedSplit {
            line_range: LineRange::new(2, 4),
            complexity_contribution: 2,
            branch_path: "if-branch".to_string(),
            kind: SplitKind::DeepestNesting,
            recommended: true,
        };
        let (actions, _) = pick_actions(&[], &[split], &contribs);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, SuggestedAction::SimplifyBranching { .. }))
        );
    }

    // compute_diagnostic — orchestrator

    #[test]
    fn compute_diagnostic_returns_none_for_passing_verdict() {
        let verdict = make_verdict(vec![], span_for(1, 10), false);
        assert!(compute_diagnostic(&verdict, &[]).is_none());
    }

    #[test]
    fn compute_diagnostic_returns_some_for_exceeding_verdict() {
        let verdict = make_verdict(
            vec![
                make_contributor(ContributorKind::IfBranch, 17, 25, 1, 0),
                make_contributor(ContributorKind::IfBranch, 18, 22, 2, 1),
            ],
            span_for(16, 26),
            true,
        );
        let diag = compute_diagnostic(&verdict, &[]).expect("diagnostic populated");
        assert!(diag.coverage_gaps.is_empty());
        assert_eq!(diag.complexity_drivers.len(), 2);
        assert!(!diag.suggested_actions.is_empty());
    }

    #[test]
    fn compute_diagnostic_low_coverage_only_emits_add_tests() {
        // Flat function (no contributors) with uncovered lines — only
        // path is AddTestsForLines.
        let verdict = make_verdict(vec![], span_for(1, 10), true);
        let cov = vec![LineCoverage { line: 5, hits: 0 }];
        let diag = compute_diagnostic(&verdict, &cov).expect("populated");
        assert_eq!(diag.root_cause, RootCause::LowCoverage);
        assert_eq!(diag.coverage_gaps, vec![LineRange::new(5, 5)]);
        assert_eq!(diag.suggested_actions.len(), 1);
        assert!(matches!(
            diag.suggested_actions[0],
            SuggestedAction::AddTestsForLines { .. }
        ));
    }

    #[test]
    fn compute_diagnostic_full_coverage_no_splits_falls_back_to_accept_inherent() {
        let verdict = make_verdict(vec![], span_for(1, 10), true);
        let diag = compute_diagnostic(&verdict, &[]).expect("populated");
        assert_eq!(diag.root_cause, RootCause::HighComplexity);
        assert_eq!(diag.suggested_actions.len(), 1);
        assert!(matches!(
            diag.suggested_actions[0],
            SuggestedAction::AcceptInherentComplexity { .. }
        ));
    }

    #[test]
    fn compute_diagnostic_extract_function_carries_recommended_marker() {
        let verdict = make_verdict(
            vec![
                make_contributor(ContributorKind::IfBranch, 17, 25, 1, 0),
                make_contributor(ContributorKind::IfBranch, 18, 22, 2, 1),
            ],
            span_for(16, 30),
            true,
        );
        let diag = compute_diagnostic(&verdict, &[]).expect("populated");
        let ef = diag
            .suggested_actions
            .iter()
            .find_map(|a| match a {
                SuggestedAction::ExtractFunction { candidates, .. } => Some(candidates),
                _ => None,
            })
            .expect("ExtractFunction emitted");
        assert!(!ef.is_empty());
        let recommended_count = ef.iter().filter(|s| s.recommended).count();
        assert_eq!(recommended_count, 1);
    }
}

// ── Property tests ──────────────────────────────────────────────────

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_split_kind() -> impl Strategy<Value = SplitKind> {
        prop_oneof![
            Just(SplitKind::DeepestNesting),
            Just(SplitKind::LargestSubblock),
            Just(SplitKind::HighestBranchCount),
        ]
    }

    fn arb_proposed_split() -> impl Strategy<Value = ProposedSplit> {
        (1usize..200, 1usize..200, 0u32..50, arb_split_kind()).prop_map(
            |(start, len, contribution, kind)| ProposedSplit {
                line_range: LineRange::new(start, start + len),
                complexity_contribution: contribution,
                branch_path: String::new(),
                kind,
                recommended: false,
            },
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Non-empty `dedup_splits` output always has exactly one
        /// recommended entry.
        #[test]
        fn dedup_splits_has_exactly_one_recommended_when_non_empty(
            splits in proptest::collection::vec(arb_proposed_split(), 1..10)
        ) {
            let result = dedup_splits(splits);
            if !result.is_empty() {
                let count = result.iter().filter(|s| s.recommended).count();
                prop_assert_eq!(count, 1);
            }
        }

        /// `dedup_splits` is idempotent — running it again on its own
        /// output produces the same result.
        #[test]
        fn dedup_splits_is_idempotent(
            splits in proptest::collection::vec(arb_proposed_split(), 0..10)
        ) {
            let once = dedup_splits(splits);
            let twice = dedup_splits(once.clone());
            prop_assert_eq!(once, twice);
        }

        /// `derive_branch_path` produces no whitespace and no commas —
        /// only `/` as separator (AST-derived chains only).
        #[test]
        fn branch_path_carries_no_whitespace_or_commas(
            depths in proptest::collection::vec(0u32..6, 0..10)
        ) {
            // Synthesize a fake nested chain: each contributor encloses
            // line 100 with strictly lower start lines.
            let contributors: Vec<ComplexityContributor> = depths
                .iter()
                .enumerate()
                .map(|(i, depth)| {
                    let start = 50 + i;
                    ComplexityContributor {
                        kind: ContributorKind::IfBranch,
                        line: start,
                        column: None,
                        increment: 1,
                        end_line: 200,
                        nesting_depth: *depth,
                    }
                })
                .collect();
            let path = derive_branch_path(&contributors, 100);
            prop_assert!(!path.contains(' '));
            prop_assert!(!path.contains(','));
        }

        /// `dedup_splits` sorts by priority (highest first), so
        /// `result[0].kind` always has the maximum priority.
        #[test]
        fn dedup_splits_sorts_by_priority_descending(
            splits in proptest::collection::vec(arb_proposed_split(), 1..10)
        ) {
            let result = dedup_splits(splits);
            for window in result.windows(2) {
                prop_assert!(
                    split_kind_priority(window[0].kind)
                        >= split_kind_priority(window[1].kind)
                );
            }
        }
    }
}
