//! Pins every public domain enum's `as_wire_str()` output to its serde
//! JSON serialization (sans surrounding quotes). Drift means an
//! `as_wire_str` arm went stale relative to the `#[serde(rename_all = ...)]`
//! attribute, which would silently produce mismatched payloads. Adding
//! a new variant to any of these enums requires extending `as_wire_str`
//! and the corresponding `EXPECTED` table here.
//!
//! Coverage: all serializing string-scalar enums in `crap_core::domain`.
//! `ParseDiagnostic` (LCOV-shaped, lives in crap4rs as `LcovParseDiagnostic`)
//! and `FunctionChange` / `SuggestedAction` (struct-payload tagged enums)
//! are out of scope — they don't serialize as plain strings.

use crap_core::domain::delta::{ChangeKind, DeltaSortKey};
use crap_core::domain::diagnostic::{Applicability, RootCause, SplitKind};
use crap_core::domain::summary::CrapDeltaStatus;
use crap_core::domain::types::{
    ComplexityMetric, ContributorKind, CoverageMetric, MissingCoveragePolicy, RiskLevel,
};
use crap_core::domain::view::{GroupKey, SortKey};
use serde::Serialize;
use serde_json::Value;

fn assert_wire_str<E: Serialize + std::fmt::Debug>(variant: E, expected: &str) {
    let v = serde_json::to_value(&variant).unwrap_or_else(|e| {
        panic!("variant {variant:?} failed to serialize: {e}");
    });
    let serde_str = match v {
        Value::String(s) => s,
        other => panic!("variant {variant:?} did not serialize to a string scalar: {other:?}"),
    };
    assert_eq!(
        serde_str, expected,
        "as_wire_str drift on {variant:?}: serde={serde_str:?} as_wire_str={expected:?}"
    );
}

#[test]
fn contributor_kind_wire_str_matches_serde() {
    use ContributorKind::*;
    for v in [
        IfBranch,
        ForLoop,
        WhileLoop,
        DoWhileLoop,
        Catch,
        LogicalOperator,
        Match,
        MatchArm,
        Try,
        LetElse,
        Loop,
        Break,
        Continue,
        Unsafe,
        Switch,
        CaseBranch,
        Ternary,
        OptionalChain,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn complexity_metric_wire_str_matches_serde() {
    for v in [ComplexityMetric::Cognitive, ComplexityMetric::Cyclomatic] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn coverage_metric_wire_str_matches_serde() {
    for v in [CoverageMetric::Line, CoverageMetric::Branch] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn missing_coverage_policy_wire_str_matches_serde() {
    for v in [
        MissingCoveragePolicy::Pessimistic,
        MissingCoveragePolicy::Optimistic,
        MissingCoveragePolicy::Skip,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn risk_level_wire_str_matches_serde() {
    for v in [
        RiskLevel::Low,
        RiskLevel::Acceptable,
        RiskLevel::Moderate,
        RiskLevel::High,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn change_kind_wire_str_matches_serde() {
    for v in ChangeKind::ALL {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn delta_sort_key_wire_str_matches_serde() {
    for v in [
        DeltaSortKey::ScoreDelta,
        DeltaSortKey::CurrentCrap,
        DeltaSortKey::BaselineCrap,
        DeltaSortKey::Path,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn root_cause_wire_str_matches_serde() {
    for v in [
        RootCause::LowCoverage,
        RootCause::HighComplexity,
        RootCause::Both,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn applicability_wire_str_matches_serde() {
    for v in [
        Applicability::MachineApplicable,
        Applicability::MaybeIncorrect,
        Applicability::HasPlaceholders,
        Applicability::Unspecified,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn split_kind_wire_str_matches_serde() {
    for v in [
        SplitKind::DeepestNesting,
        SplitKind::LargestSubblock,
        SplitKind::HighestBranchCount,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn crap_delta_status_wire_str_matches_serde() {
    for v in [
        CrapDeltaStatus::Green,
        CrapDeltaStatus::Yellow,
        CrapDeltaStatus::Red,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}

#[test]
fn group_key_wire_str_matches_serde() {
    // Single-variant enum today (`#[non_exhaustive]` reserves namespace
    // for future `Risk` / `Module`). Add to the array as variants land.
    let v = GroupKey::File;
    assert_wire_str(v, v.as_wire_str());
}

#[test]
fn sort_key_wire_str_matches_serde() {
    for v in [
        SortKey::Crap,
        SortKey::Coverage,
        SortKey::Complexity,
        SortKey::Path,
    ] {
        assert_wire_str(v, v.as_wire_str());
    }
}
