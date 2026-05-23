//! W3.2 #190 — parity cross-validation harness.
//!
//! Shells the `crap4ts` Rust binary (NOT pnpm/node) against the
//! committed W3.1 crap4ts@1.x corpus, parses its `--format json`
//! envelope, and diffs every function against the committed oracle
//! `crap4ts-v1-reference.json`. The reference was captured **once**
//! during W3.1 and is consumed here byte-for-byte — this harness never
//! regenerates a baseline; pure Rust, no node toolchain in CI.
//!
//! See `parity_helpers` for the three-way classification (Match /
//! ThresholdDefaultChange / Crap37Improvement / ScoreRegression), the
//! hierarchical tolerance gate, and why the oracle carries no
//! contributor breakdown.
//!
//! The pure-`diff()` scenario tests below pin the
//! `parity_with_v1.feature` contract directly (the 5 scenarios stay
//! `@unwired` with their `# tracked:` comment until W3.3 wires the
//! cucumber harness; these tests assert the same acceptance now).

mod parity_helpers;

use parity_helpers::{Class, FnRecord, ParityReport, diff, parse_oracle, parse_v2};

const ORACLE_JSON: &str = include_str!("fixtures/crap4ts-v1-reference.json");

/// Run `crap4ts --format json` against the committed v1.x corpus and
/// return a classified parity report.
fn run_parity() -> ParityReport {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let src = format!("{manifest}/tests/fixtures/crap4ts-v1/src");
    let coverage = format!("{manifest}/tests/fixtures/crap4ts-v1/coverage-final.json");

    let out = assert_cmd::Command::cargo_bin("crap4ts")
        .expect("crap4ts binary discoverable")
        .arg("--src")
        .arg(&src)
        .arg("--coverage")
        .arg(&coverage)
        .arg("--format")
        .arg("json")
        .arg("--no-fail")
        .output()
        .expect("crap4ts executes");

    assert!(
        out.status.success(),
        "crap4ts exited non-zero under --no-fail: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let oracle = parse_oracle(ORACLE_JSON);
    let v2 = parse_v2(std::str::from_utf8(&out.stdout).expect("stdout is utf8"));
    diff(&oracle, &v2)
}

/// THE GATE. crap4ts@2 reproduces the v1.x oracle within the
/// hierarchical tolerance band: every oracle function discovered
/// (crap4ts@2 ⊇ v1.x), ≥ 95% exact cyclomatic complexity, zero
/// genuine score-regressions. Risk hard-match is enforced through the
/// classifier — a risk move explained by a crap4ts#37 improvement or a
/// threshold-default change is not a regression.
///
/// On failure the full structured per-function diff is printed so a
/// divergence is one read, not a manual re-run.
#[test]
fn crap4ts2_reproduces_v1_oracle_within_tolerance() {
    let report = run_parity();
    assert!(
        report.gate_passes(),
        "crap4ts@2 diverged from the v1.x oracle beyond tolerance:\n{}",
        report.render()
    );

    // Discovery contract: crap4ts@2 must find a superset of every
    // oracle function. Pin to the committed oracle's own size (not a
    // capture-variable corpus count) so this fails loudly if the
    // oracle is ever silently shrunk.
    assert!(
        report.v1_only.is_empty(),
        "crap4ts@2 failed to discover {} oracle function(s):\n{}",
        report.v1_only.len(),
        report.render()
    );
    assert_eq!(
        report.matched,
        parse_oracle(ORACLE_JSON).len(),
        "every oracle function must match exactly one crap4ts@2 function"
    );

    // Documented surviving state (see PR body): on the W3.1 corpus the
    // matched set is 100% exact-CC with zero genuine regressions; all
    // score/risk movement is the crap4ts#37 coverage-matcher
    // improvement. This is asserted as a band, not a brittle pin —
    // exact-CC rate must clear 95% and regressions must stay zero.
    assert!(
        report.exact_cc_rate() >= 0.95,
        "exact-CC rate {:.3} fell below 0.95:\n{}",
        report.exact_cc_rate(),
        report.render()
    );
    assert!(
        report.regressions().is_empty(),
        "{} unexplained score-regression(s):\n{}",
        report.regressions().len(),
        report.render()
    );
}

// ── parity_with_v1.feature scenario contracts (pure, deterministic) ──
//
// These exercise the classifier through the public `diff()` entry with
// synthetic records, pinning each BDD scenario's acceptance without the
// binary. W3.3 wires the cucumber step defs; the contract is verified
// here now.

// A flat positional builder keeps each scenario's expected record on
// one readable line; a struct literal per call would triple the line
// count of every scenario for no clarity gain in test-only code.
#[allow(clippy::too_many_arguments)]
fn rec(
    name: &str,
    sl: i64,
    cc: u32,
    cov: f64,
    crap: f64,
    risk: &str,
    exceeds: bool,
    contribs: &[&str],
) -> FnRecord {
    FnRecord {
        file: "src/x.ts".to_string(),
        name: name.to_string(),
        start_line: sl,
        cc,
        coverage: cov,
        crap,
        risk: risk.to_string(),
        exceeds,
        contributors: contribs.iter().map(|s| s.to_string()).collect(),
    }
}

/// Scenario: scores match within tolerance — exact CC + same risk
/// class classifies as `Match` and the gate passes.
#[test]
fn scenario_scores_match_within_tolerance() {
    let oracle = vec![rec("f", 10, 3, 100.0, 3.0, "low", false, &[])];
    let v2 = vec![rec("f", 10, 3, 100.0, 3.0, "low", false, &[])];
    let r = diff(&oracle, &v2);
    assert!(r.gate_passes());
    assert_eq!(r.divergences[0].class, Class::Match);
    assert!(r.v1_only.is_empty());
    assert_eq!(r.exact_cc_rate(), 1.0);
}

/// Scenario: divergence output shows per-function contributor
/// breakdown, not just a score diff. v1.x has CC 4; v2 has CC 3 with
/// `2× if-branch` — the report names the function, shows v2's
/// contributor breakdown, and the missing kind is inferable from it.
#[test]
fn scenario_divergence_shows_contributor_breakdown() {
    let oracle = vec![rec("dropTernary", 5, 4, 100.0, 4.0, "low", false, &[])];
    let v2 = vec![rec(
        "dropTernary",
        5,
        3,
        100.0,
        3.0,
        "low",
        false,
        &["if-branch", "if-branch"],
    )];
    let r = diff(&oracle, &v2);
    // CC differs with no benign explanation → regression, gate fails.
    assert_eq!(r.divergences[0].class, Class::ScoreRegression);
    assert!(!r.gate_passes());
    let out = r.render();
    assert!(out.contains("dropTernary"), "report names the function");
    assert!(
        out.contains("2× if-branch"),
        "report shows v2 contributor breakdown; got:\n{out}"
    );
    assert!(
        out.contains("file a follow-up under epic #173"),
        "report recommends a tracked follow-up; got:\n{out}"
    );
}

/// Scenario: risk classification labels match across versions (D8
/// invariance) — when nothing diverges, every risk label matches and
/// no risk-class divergence is emitted.
#[test]
fn scenario_risk_labels_match_when_no_divergence() {
    let oracle = vec![
        rec("a", 1, 2, 100.0, 2.0, "low", false, &[]),
        rec("b", 9, 9, 90.0, 9.01, "moderate", true, &[]),
    ];
    let v2 = vec![
        rec("a", 1, 2, 100.0, 2.0, "low", false, &[]),
        rec("b", 9, 9, 90.0, 9.01, "moderate", true, &[]),
    ];
    let r = diff(&oracle, &v2);
    assert!(r.gate_passes());
    assert!(r.divergences.iter().all(|d| d.class == Class::Match));
}

/// Scenario: threshold-default difference is documented, not a parity
/// failure. Same score; oracle gate says exceeding (threshold 12), v2
/// says passing (threshold 16) → `ThresholdDefaultChange`, gate passes.
#[test]
fn scenario_threshold_default_change_is_not_a_regression() {
    let oracle = vec![rec(
        "borderline",
        20,
        13,
        100.0,
        13.0,
        "moderate",
        true,
        &[],
    )];
    let v2 = vec![rec(
        "borderline",
        20,
        13,
        100.0,
        13.0,
        "moderate",
        false,
        &[],
    )];
    let r = diff(&oracle, &v2);
    assert_eq!(
        r.divergences[0].class,
        Class::ThresholdDefaultChange,
        "same score, only the gate verdict flipped → threshold-default-change"
    );
    assert!(
        r.gate_passes(),
        "threshold-default-change must NOT fail the gate"
    );
    assert!(r.render().contains("threshold-default-change"));
}

/// Scenario: a discovered score divergence triggers a tracked
/// follow-up. An unexplained crap jump (same CC + coverage, |Δcrap| >
/// 0.5) classifies as `ScoreRegression` and the report recommends a
/// follow-up under epic #173 with the function + v2 contributors.
#[test]
fn scenario_unexplained_divergence_recommends_followup() {
    let oracle = vec![rec("regressed", 30, 5, 80.0, 6.0, "acceptable", false, &[])];
    let v2 = vec![rec(
        "regressed",
        30,
        5,
        80.0,
        9.0,
        "moderate",
        false,
        &["if-branch", "ternary", "ternary"],
    )];
    let r = diff(&oracle, &v2);
    assert_eq!(r.divergences[0].class, Class::ScoreRegression);
    assert!(!r.gate_passes());
    let out = r.render();
    assert!(out.contains("epic #173"));
    assert!(out.contains("regressed"));
    assert!(
        out.contains("1× if-branch + 2× ternary"),
        "follow-up includes v2 contributor breakdown; got:\n{out}"
    );
}

/// crap4ts#37 improvement: v1.x reported 0% coverage (its
/// span-overlap-matcher bug); v2 reports real coverage on the same
/// complexity. Classifies as an improvement and PASSES — the risk
/// class moving because of it is not a regression (this is how the
/// README's "risk hard-match" and "improvement passes" rules
/// reconcile).
#[test]
fn crap37_improvement_passes_and_absorbs_risk_shift() {
    let oracle = vec![rec("buggyV1", 40, 3, 0.0, 12.0, "moderate", true, &[])];
    let v2 = vec![rec(
        "buggyV1",
        40,
        3,
        100.0,
        3.0,
        "low",
        false,
        &["if-branch", "if-branch"],
    )];
    let r = diff(&oracle, &v2);
    assert_eq!(r.divergences[0].class, Class::Crap37Improvement);
    assert!(
        r.gate_passes(),
        "a crap4ts#37 improvement (incl. its risk-class move) must pass"
    );
}

/// Direction-1 #252: v1.x's multi-statement-per-line conflation
/// overcounted coverage on a single-line arrow's body (the `cube` case)
/// — v2's MIN aggregation deflates to the correct value. Coverage
/// drops, CRAP rises, risk class moves up by one step.
#[test]
fn crap252_improvement_direction_one_coverage_deflation_passes() {
    // cube-shaped values: pre-fix v1 reported 50% (declaration's
    // module-load hit dominated the body's zero-hit), v2 now reports
    // 0%. CC = 1 (no contributors), CRAP 1.13 → 2.0, risk low → low.
    let oracle = vec![rec("cube", 2, 1, 50.0, 1.13, "low", false, &[])];
    let v2 = vec![rec("cube", 2, 1, 0.0, 2.0, "low", false, &[])];
    let r = diff(&oracle, &v2);
    assert_eq!(r.divergences[0].class, Class::Crap252Improvement);
    assert!(
        r.gate_passes(),
        "a crap-rs#252 deflation improvement must pass"
    );
}

/// Direction-2 #252: v1.x's conflation also INFLATED coverage when a
/// function's span contained lines with multiple duplicate uncovered
/// statements (phantom denominator weight). v2's MIN collapses these
/// to one record, raising coverage. CRAP barely moves (within
/// `CRAP_EPS`); risk class stays.
#[test]
fn crap252_improvement_direction_two_coverage_inflation_passes() {
    // createAutoDetectCoveragePort-shaped values: v1 75%, v2 78.5%, cc
    // = 1, CRAP 1.02 → 1.01. The coverage rise is small but exceeds
    // COV_EPS; CRAP is essentially flat.
    let oracle = vec![rec(
        "createAutoDetectCoveragePort",
        42,
        1,
        75.0,
        1.02,
        "low",
        false,
        &[],
    )];
    let v2 = vec![rec(
        "createAutoDetectCoveragePort",
        42,
        1,
        78.5,
        1.01,
        "low",
        false,
        &[],
    )];
    let r = diff(&oracle, &v2);
    assert_eq!(r.divergences[0].class, Class::Crap252Improvement);
    assert!(
        r.gate_passes(),
        "a crap-rs#252 inflation improvement must pass"
    );
}

/// Negative case: a coverage drop with CRAP rising MORE than
/// `CRAP_EPS` past the consistency bound is not absorbed — that is the
/// signature of a real regression (something v1 got right that v2
/// broke), not the structural #252 mechanism. CC mismatches also stay
/// in `ScoreRegression`: the classifier must not swallow walker drifts
/// under the #252 banner.
#[test]
fn crap252_improvement_does_not_swallow_cc_drift() {
    // Same CC pre/post, but the CRAP rise is consistent with the
    // coverage drop direction — this DOES pass under #252.
    let absorbed = diff(
        &[rec("f", 10, 2, 80.0, 2.32, "low", false, &[])],
        &[rec("f", 10, 2, 60.0, 2.51, "low", false, &["if-branch"])],
    );
    assert_eq!(absorbed.divergences[0].class, Class::Crap252Improvement);

    // CC drifts from v1 to v2: walker counted differently. Falls
    // through to ScoreRegression even though coverage moved in a
    // direction the structural rule otherwise allows.
    let cc_drift = diff(
        &[rec("f", 10, 2, 80.0, 2.32, "low", false, &[])],
        &[rec(
            "f",
            10,
            3,
            60.0,
            4.91,
            "low",
            false,
            &["if-branch", "logical-operator"],
        )],
    );
    assert_eq!(cc_drift.divergences[0].class, Class::ScoreRegression);
    assert!(!cc_drift.gate_passes());
}

/// Discovery contract: an oracle function crap4ts@2 fails to discover
/// is a hard gate failure (crap4ts@2 must be a superset of v1.x).
#[test]
fn missing_discovery_is_a_hard_gate_failure() {
    let oracle = vec![
        rec("found", 1, 1, 100.0, 1.0, "low", false, &[]),
        rec("vanished", 50, 2, 100.0, 2.0, "low", false, &[]),
    ];
    let v2 = vec![rec("found", 1, 1, 100.0, 1.0, "low", false, &[])];
    let r = diff(&oracle, &v2);
    assert!(!r.gate_passes());
    assert_eq!(r.v1_only.len(), 1);
    assert!(r.render().contains("vanished"));
    assert!(r.render().contains("DISCOVERY FAILURE"));
}

/// crap4ts@2 discovering functions the oracle lacks (its walker is
/// more thorough) is informational, never a regression.
#[test]
fn v2_only_functions_are_informational_not_a_failure() {
    let oracle = vec![rec("shared", 1, 1, 100.0, 1.0, "low", false, &[])];
    let v2 = vec![
        rec("shared", 1, 1, 100.0, 1.0, "low", false, &[]),
        rec("<arrow>", 7, 1, 100.0, 1.0, "low", false, &[]),
        rec("extraNested", 9, 2, 100.0, 2.0, "low", false, &[]),
    ];
    let r = diff(&oracle, &v2);
    assert!(r.gate_passes());
    assert_eq!(r.v2_only_count, 2);
}

/// Line-drift tolerance: a ±1 start-line shift (half-open span
/// boundary) still matches; a larger shift does not.
#[test]
fn start_line_drift_within_one_still_matches() {
    let oracle = vec![rec("shifted", 100, 2, 100.0, 2.0, "low", false, &[])];
    let within = vec![rec("shifted", 101, 2, 100.0, 2.0, "low", false, &[])];
    let beyond = vec![rec("shifted", 103, 2, 100.0, 2.0, "low", false, &[])];
    assert_eq!(diff(&oracle, &within).matched, 1);
    assert!(diff(&oracle, &within).gate_passes());
    assert_eq!(
        diff(&oracle, &beyond).matched,
        0,
        ">1 line drift is not a match"
    );
    assert!(!diff(&oracle, &beyond).gate_passes());
}
