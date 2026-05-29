//! Cross-adapter `RiskLevel` consistency canary (crap-rs#317).
//!
//! Both `crap4rs` and `crap4ts` derive every function's `RiskLevel`
//! from one shared `crap_core::domain::crap::classify_risk` (score →
//! band) and
//! serialize it through one shared serde derive on
//! `crap_core::domain::types::RiskLevel`. Cross-adapter consistency
//! therefore holds *by construction today* — neither adapter has a
//! per-language risk-classification step. This canary mechanically
//! pins that property at the **wire level** so a future refactor
//! cannot quietly diverge it — e.g. by introducing a per-adapter
//! post-processing pass, renaming a serde variant on one path, or
//! reordering the round-then-classify pipeline. Mirrors the
//! "documentation rots; CI doesn't" discipline of
//! `scorecard_row_parity.rs` and the `wire_envelope_crap4{rs,ts}.rs`
//! envelope locks.
//!
//! ## What this canary pins
//!
//! * **Serialization vocabulary** — every `risk_level` string in
//!   either adapter's `--format json` envelope is one of the four
//!   canonical wire strings the shared `RiskLevel` enum emits
//!   (`low`/`acceptable`/`moderate`/`high`). The enum IS the
//!   "identical variant set across adapters" the issue asks for;
//!   each adapter's observed values are asserted to be a subset of
//!   that one canonical set, so neither adapter can mint a band the
//!   other can't.
//! * **Score → band oracle** — for every function in either
//!   envelope, `wire.risk_level ==
//!   classify_risk(wire.crap.value).as_wire_str()`. The adapter
//!   serializes `risk_level` and `crap.value` *independently* into
//!   the JSON, so re-deriving the band from the wire `value` and
//!   comparing it to the wire `risk_level` is a genuine round-trip
//!   check, not a tautology. Running it against the SAME shared
//!   `classify_risk` for both adapters is what makes it a
//!   *cross-adapter* consistency proof: one oracle, two adapters.
//! * **Round-then-classify ordering** — `classify_risk` classifies
//!   the rounded CRAP value; the oracle re-runs it on the wire
//!   `value` (which is
//!   already rounded), so a regression that classified the unrounded
//!   score on one path would surface as an oracle mismatch on a
//!   boundary function.
//!
//! ## Why this is NOT "mapped against per-adapter thresholds"
//!
//! Issue #317's prose says to map the High/Moderate/Acceptable/Low
//! buckets "against their respective per-adapter thresholds
//! (cognitive 15/25/40 ⇄ cyclomatic 8/16/30)". Reading the substrate
//! shows those two things are distinct and must not be conflated:
//!
//! * **`RiskLevel` bands are score-based and shared.** `classify_risk`
//!   keys off the raw CRAP *score* (≤8 Low, ≤15 Acceptable, ≤25
//!   Moderate, else
//!   High) and is metric-agnostic — identical for both adapters,
//!   independent of any `--threshold`.
//! * **The per-adapter calibrated numbers drive the `--threshold`
//!   GATE**, not the risk band. They decide `exceeds` /
//!   scorecard-row `status` (Green/Yellow/Red), which is a *separate*
//!   axis already locked by `scorecard_row_parity.rs` +
//!   `default_gate_threshold.rs`.
//!
//! So the honest invariant — the one ζ's Combined-view ranking
//! actually depends on (risk-level desc, then CRAP/threshold ratio
//! desc within band) — is "both adapters map score → band through
//! one shared, consistent enum". That is what this canary pins. It
//! deliberately does not re-assert the threshold-gate calibration;
//! that lives in the scorecard-row / default-gate canaries.
//!
//! ## Fixture corpus note (TS side is all-Low)
//!
//! The Rust self-LCOV fixture spans two bands at `--threshold 8`
//! (`low` + `acceptable`), so the oracle exercises a real band
//! boundary on the Rust side. The crap4ts istanbul fixture corpus is
//! entirely `low` (its functions are small, near-fully-covered). The
//! oracle check is the spine and is meaningful on all-`low` data —
//! it still re-derives and compares the band from the wire `value`
//! for every TS function, and the canonical-set/subset assertion +
//! the single shared `classify_risk` oracle carry the cross-adapter
//! guarantee
//! regardless of the TS spread. Constructing a synthetic high-CRAP
//! TS fixture would add brittleness (hand-authored istanbul
//! statement/branch maps) without strengthening the invariant this
//! canary locks.
//!
//! ## Mutants gate interaction
//!
//! The test fn is named `risk_level_envelope_parity` — the
//! `envelope` substring is covered by the existing `--skip envelope`
//! token in `.cargo/mutants.toml`, so a scoped
//! `cargo mutants --package crap-core` run (which does not build the
//! crap4rs / crap4ts bins) skips it instead of panicking on
//! `CARGO_BIN_EXE_*` unset in the unmutated baseline. See AGENTS.md
//! "Mutation testing" → "Why `additional_cargo_test_args` …".

use std::collections::BTreeSet;
use std::path::PathBuf;

use assert_cmd::Command;
use crap_core::domain::crap::classify_risk;
use crap_core::domain::types::RiskLevel;
use serde_json::Value;
use tempfile::TempDir;

const TS_FIXTURE_TEMPLATE: &str =
    include_str!("../../crap4ts/tests/fixtures/istanbul-jest/coverage-final.json");

/// The single canonical set of `risk_level` wire strings — the shared
/// `RiskLevel` enum enumerated through its own `as_wire_str`. Both
/// adapters' observed values must be a subset of this. Extending
/// `RiskLevel` requires extending this array (and `as_wire_str`, which
/// `as_wire_str_matches_serde.rs` cross-checks against serde).
const CANONICAL_VARIANTS: &[RiskLevel] = &[
    RiskLevel::Low,
    RiskLevel::Acceptable,
    RiskLevel::Moderate,
    RiskLevel::High,
];

/// Build a canonicalized tempdir holding the W1.1 jest fixture sources
/// plus a resolved `coverage-final.json`. Mirrors the helper in
/// `scorecard_row_parity.rs` / `wire_envelope_crap4ts.rs` so the canary
/// exercises a real Istanbul parse. The `TempDir` is returned so the
/// directory stays alive for the lifetime of the test.
fn build_ts_fixture() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    for (name, content) in [
        (
            "simple.ts",
            include_str!("../../crap4ts/tests/fixtures/ts-fixtures/simple.ts"),
        ),
        (
            "arrow.ts",
            include_str!("../../crap4ts/tests/fixtures/ts-fixtures/arrow.ts"),
        ),
        (
            "Button.tsx",
            include_str!("../../crap4ts/tests/fixtures/ts-fixtures/Button.tsx"),
        ),
        (
            "map.ts",
            include_str!("../../crap4ts/tests/fixtures/ts-fixtures/map.ts"),
        ),
        (
            "mixed.ts",
            include_str!("../../crap4ts/tests/fixtures/ts-fixtures/mixed.ts"),
        ),
    ] {
        std::fs::write(canonical.join(name), content).expect("write fixture");
    }

    let payload = TS_FIXTURE_TEMPLATE.replace(
        "{SRC_ROOT}",
        &canonical.to_string_lossy().replace('\\', "/"),
    );
    std::fs::write(canonical.join("coverage-final.json"), payload)
        .expect("write coverage-final.json");

    (tmp, canonical)
}

/// Invoke `crap4rs --format json` against its own self-LCOV fixture at
/// `--threshold 8` (the strict cutoff that surfaces the over-threshold
/// `acceptable`-band functions). Returns the parsed envelope.
fn run_crap4rs_json() -> Value {
    let output = Command::cargo_bin("crap4rs")
        .expect("crap4rs binary discoverable in workspace")
        .args([
            "--coverage",
            "../crap4rs/tests/fixtures/crap4rs-self.lcov",
            "--src",
            "../crap4rs/src",
            "--threshold",
            "8",
            "--format",
            "json",
            "--no-fail",
        ])
        .output()
        .expect("crap4rs binary executes");
    assert!(
        output.status.success(),
        "crap4rs exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_slice(&output.stdout).expect("crap4rs --format json emits valid JSON")
}

/// Invoke `crap4ts --format json` against the temp Istanbul fixture.
/// Returns the parsed envelope.
fn run_crap4ts_json() -> Value {
    let (_tmp, root) = build_ts_fixture();
    let coverage = root.join("coverage-final.json");
    let output = Command::cargo_bin("crap4ts")
        .expect("crap4ts binary discoverable in workspace")
        .arg("--coverage")
        .arg(&coverage)
        .arg("--src")
        .arg(&root)
        .args(["--threshold", "16", "--format", "json", "--no-fail"])
        .output()
        .expect("crap4ts binary executes");
    assert!(
        output.status.success(),
        "crap4ts exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_slice(&output.stdout).expect("crap4ts --format json emits valid JSON")
}

/// One `(crap.value, risk_level)` pair lifted from a wire envelope,
/// tagged with a human-readable location so a failure names the
/// offending function.
struct WireBand {
    location: String,
    value: f64,
    risk_level: String,
}

/// Recursively collect every `{ "value": <f64>, "risk_level": <str> }`
/// object reachable in the envelope. This catches `crap` objects on
/// `result.functions[].scored.crap`, the `result.summary.max_crap`
/// roll-up, and any future site that embeds a CRAP score — the canary
/// should not be tied to one path through the envelope. `path` is the
/// breadcrumb trail used in panic messages.
fn collect_bands(value: &Value, path: &str, out: &mut Vec<WireBand>) {
    match value {
        Value::Object(map) => {
            if let (Some(Value::Number(v)), Some(Value::String(rl))) =
                (map.get("value"), map.get("risk_level"))
            {
                if let Some(v) = v.as_f64() {
                    out.push(WireBand {
                        location: path.to_string(),
                        value: v,
                        risk_level: rl.clone(),
                    });
                }
            }
            for (k, child) in map {
                let child_path = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect_bands(child, &child_path, out);
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                collect_bands(item, &format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}

/// The canonical wire-string set the shared `RiskLevel` enum emits.
fn canonical_wire_set() -> BTreeSet<&'static str> {
    CANONICAL_VARIANTS
        .iter()
        .map(RiskLevel::as_wire_str)
        .collect()
}

/// Assert every band in `bands`:
///   1. uses a wire string drawn from the canonical `RiskLevel` set
///      (no adapter-minted variant), and
///   2. matches the band the shared oracle `classify_risk` derives
///      from the wire
///      `value` (cross-adapter score→band consistency).
fn assert_bands_consistent(bands: &[WireBand], adapter: &str) {
    assert!(
        !bands.is_empty(),
        "{adapter}: envelope carried no CRAP bands — fixture or envelope shape changed"
    );
    let canonical = canonical_wire_set();
    for band in bands {
        assert!(
            canonical.contains(band.risk_level.as_str()),
            "{adapter}: risk_level {:?} at {} is not a canonical RiskLevel wire string \
             (expected one of {canonical:?}) — an adapter minted a band the shared enum \
             does not define",
            band.risk_level,
            band.location,
        );
        let oracle = classify_risk(band.value).as_wire_str();
        assert_eq!(
            band.risk_level, oracle,
            "{adapter}: risk_level divergence at {} — wire value {} serialized band {:?} \
             but the shared crap_core::domain::crap::classify_risk oracle classifies it as {:?}. \
             A future per-adapter risk-classification step or a serde rename on one path \
             would surface here.",
            band.location, band.value, band.risk_level, oracle,
        );
    }
}

#[test]
fn risk_level_envelope_parity() {
    let rs = run_crap4rs_json();
    let ts = run_crap4ts_json();

    let mut rs_bands = Vec::new();
    collect_bands(&rs, "rust", &mut rs_bands);
    let mut ts_bands = Vec::new();
    collect_bands(&ts, "typescript", &mut ts_bands);

    // Per-adapter: canonical vocabulary + score→band oracle. The same
    // shared `classify_risk` oracle is applied to both, which is what
    // makes this a
    // cross-adapter consistency proof rather than two independent checks.
    assert_bands_consistent(&rs_bands, "crap4rs");
    assert_bands_consistent(&ts_bands, "crap4ts");

    // Sanity: the Rust self-LCOV fixture is expected to span more than
    // one band at --threshold 8 (it has uncovered functions that round
    // into `acceptable`). If this collapses to a single band, the
    // fixture changed and the oracle no longer exercises a real band
    // boundary — surface it loudly rather than silently weakening the
    // canary.
    let rs_observed: BTreeSet<&str> = rs_bands.iter().map(|b| b.risk_level.as_str()).collect();
    assert!(
        rs_observed.len() >= 2,
        "crap4rs self-LCOV fixture collapsed to a single risk band {rs_observed:?} at \
         --threshold 8 — the oracle no longer crosses a band boundary; refresh the fixture \
         expectation so the canary keeps exercising a transition"
    );

    // Both adapters draw from one shared enum, so neither observed set
    // can contain a variant outside the canonical set — already proven
    // per-band above, but assert the set relationship explicitly so the
    // "identical variant set across adapters" contract from #317 is
    // legible in one place.
    let canonical = canonical_wire_set();
    for (adapter, observed) in [
        ("crap4rs", &rs_observed),
        (
            "crap4ts",
            &ts_bands.iter().map(|b| b.risk_level.as_str()).collect(),
        ),
    ] {
        assert!(
            observed.is_subset(&canonical),
            "{adapter}: observed risk bands {observed:?} are not a subset of the canonical \
             RiskLevel set {canonical:?}"
        );
    }
}
