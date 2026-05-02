//! Shape C `Diagnostic` types — the AST-derived remediation hint emitted
//! under `--format advice` (issue #76) and consumed by the
//! `/cut-the-crap` agent skill (issue #77).
//!
//! Hexagonal note: this module is pure domain — no `syn`, no LCOV, no I/O.
//! Helpers in V3 (`compute_diagnostic`, `extract_split_candidates`, …)
//! land alongside these types so reporters can stay format-only.

use serde::{Deserialize, Serialize};

use crate::domain::types::{ComplexityContributor, ContributorKind};

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
/// the threshold. Derived from the action set per S-6 (locked in shape):
///
/// - `LowCoverage` when the only action is `AddTestsForLines`
/// - `HighComplexity` when the only actions are split/simplify/accept
/// - `Both` when both kinds of action coexist
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RootCause {
    #[default]
    LowCoverage,
    HighComplexity,
    Both,
}

// ── Applicability ───────────────────────────────────────────────────

/// Confidence in a `SuggestedAction`, matching `rustc`'s `Applicability`
/// taxonomy so agents using rustc-shaped tooling can interpret crap4rs
/// suggestions without translation. T2 (locked): the default is
/// `Unspecified` because crap4rs does not verify the suggested change.
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

// ── SplitKind ───────────────────────────────────────────────────────

/// Which strategy produced a `ProposedSplit`. Priority order
/// `DeepestNesting > HighestBranchCount > LargestSubblock` is enforced
/// at dedup time (S-7). The default variant is `DeepestNesting` because
/// that is the highest-priority strategy and the most useful candidate
/// when only one is needed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SplitKind {
    #[default]
    DeepestNesting,
    LargestSubblock,
    HighestBranchCount,
}

// ── ProposedSplit ───────────────────────────────────────────────────

/// One AST-derived candidate for `extract_function`. The split is named
/// only by its line range (R6.3 — agents do prose, the CLI does
/// coordinates).
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
    /// `recommended: true` (S-7).
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
/// requested. Type-level `#[serde(default)]` lets older payloads (no
/// new fields) deserialize, so schema_version stays at 1 across the
/// v0.3.x experimental window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Diagnostic {
    pub coverage_gaps: Vec<LineRange>,
    pub complexity_drivers: Vec<ComplexityContributor>,
    pub suggested_actions: Vec<SuggestedAction>,
    pub root_cause: RootCause,
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
}
