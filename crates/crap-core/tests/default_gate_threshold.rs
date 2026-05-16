//! End-to-end gate `threshold` resolution canary, exercised through
//! the real binaries with NO explicit `--threshold`.
//!
//! ## Why this file exists separately from the wire snapshots
//!
//! `wire_envelope_crap4ts.rs` invokes the binary with an *explicit*
//! `--threshold 16` and locks the result with `insta`. An explicit
//! flag can never exercise — let alone catch a regression in — the
//! *default* threshold resolution, and an `insta` snapshot of an
//! explicit-flag run would simply re-bless whatever the default
//! produced. This file is deliberately:
//!
//!   * **no explicit threshold flag** — exercises the
//!     no-CLI/no-config default-resolution path (and the `--strict` /
//!     `--lenient` preset paths);
//!   * **no `insta`** — a plain `assert_eq!` on the parsed envelope's
//!     top-level `threshold`, so the assertion *is* the contract, not
//!     a regenerable snapshot.
//!
//! It checks the produced artifact through the binary, independently
//! of any in-crate unit test of the resolution function — a wrong
//! cutoff calibrated for one metric but applied to another is exactly
//! the kind of defect a same-axis snapshot misses.
//!
//! ## What it locks (the calibration table, observed end-to-end)
//!
//! Threshold cutoffs are metric-keyed: for the same code, cyclomatic
//! and cognitive scores differ in magnitude, so each tier maps to a
//! different number per metric (strict/default/lenient = cyclomatic
//! 8/16/30, cognitive 15/25/40):
//!
//! | invocation                              | metric    | tier    | expect |
//! |-----------------------------------------|-----------|---------|--------|
//! | `crap4ts` (no flags)                    | cyclomatic| default | 16     |
//! | `crap4ts --strict`                      | cyclomatic| strict  | 8      |
//! | `crap4ts --lenient`                     | cyclomatic| lenient | 30     |
//! | `crap4rs` (no flags)                    | cognitive | default | 25     |
//! | `crap4rs --strict`                      | cognitive | strict  | 15     |
//! | `crap4rs --metric cyclomatic`           | cyclomatic| default | 16     |
//! | `crap4rs --metric cyclomatic --strict`  | cyclomatic| strict  | 8      |
//!
//! The crap4rs cognitive rows are regression guards: the
//! `merge_threshold` signature change must not move the Rust adapter's
//! long-standing cognitive defaults.

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const FIXTURE_TEMPLATE: &str =
    include_str!("../../crap4ts/tests/fixtures/istanbul-jest/coverage-final.json");

/// Build the same canonicalized jest tempdir fixture
/// `wire_envelope_crap4ts.rs` uses (W1.1 sources + resolved
/// `coverage-final.json`). Kept structurally identical so the only
/// behavioral difference between the two tests is the *absence* of
/// `--threshold` here.
fn build_crap4ts_fixture() -> (TempDir, PathBuf) {
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

    let payload = FIXTURE_TEMPLATE.replace(
        "{SRC_ROOT}",
        &canonical.to_string_lossy().replace('\\', "/"),
    );
    std::fs::write(canonical.join("coverage-final.json"), payload)
        .expect("write coverage-final.json");

    (tmp, canonical)
}

/// Extract the envelope's top-level `threshold` as `f64`.
fn top_level_threshold(stdout: &[u8], who: &str) -> f64 {
    let envelope: Value = serde_json::from_slice(stdout)
        .unwrap_or_else(|e| panic!("{who} --format json emits valid JSON: {e}"));
    envelope
        .get("threshold")
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("{who} envelope has a numeric top-level `threshold`"))
}

/// Run `crap4ts` against the jest tempdir fixture with `extra_args`
/// (NO explicit threshold) and return the envelope's top-level
/// `threshold`. `--no-fail` keeps exit 0 so a parseable envelope is
/// always emitted regardless of gate outcome.
fn crap4ts_threshold(extra_args: &[&str]) -> f64 {
    let (_tmp, root) = build_crap4ts_fixture();
    let coverage = root.join("coverage-final.json");
    let output = Command::cargo_bin("crap4ts")
        .expect("crap4ts binary discoverable in workspace")
        .arg("--coverage")
        .arg(&coverage)
        .arg("--src")
        .arg(&root)
        .args(extra_args)
        .args(["--format", "json", "--no-fail"])
        .output()
        .expect("crap4ts binary executes");
    assert!(
        output.status.success(),
        "crap4ts exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    top_level_threshold(&output.stdout, "crap4ts")
}

/// Run `crap4rs` against its own source + LCOV with `extra_args` (NO
/// explicit threshold) and return the envelope's top-level `threshold`.
fn crap4rs_threshold(extra_args: &[&str]) -> f64 {
    let output = Command::cargo_bin("crap4rs")
        .expect("crap4rs binary discoverable in workspace")
        .args([
            "--coverage",
            "../crap4rs/tests/fixtures/crap4rs-self.lcov",
            "--src",
            "../crap4rs/src",
        ])
        .args(extra_args)
        .args(["--format", "json", "--no-fail"])
        .output()
        .expect("crap4rs binary executes");
    assert!(
        output.status.success(),
        "crap4rs exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    top_level_threshold(&output.stdout, "crap4rs")
}

// ── crap4ts: cyclomatic-metric adapter ───────────────────────────────

#[test]
fn default_gate_crap4ts_no_flag_is_cyclomatic_16() {
    assert_eq!(
        crap4ts_threshold(&[]),
        16.0,
        "crap4ts no-flag default must be the cyclomatic `default` cutoff \
         (16), not the cognitive 25 — a single shared default applied a \
         cognitive cutoff to cyclomatic scores"
    );
}

#[test]
fn default_gate_crap4ts_strict_is_cyclomatic_8() {
    assert_eq!(
        crap4ts_threshold(&["--strict"]),
        8.0,
        "crap4ts --strict must be the cyclomatic `strict` cutoff (8), \
         not the cognitive 15"
    );
}

#[test]
fn default_gate_crap4ts_lenient_is_cyclomatic_30() {
    assert_eq!(
        crap4ts_threshold(&["--lenient"]),
        30.0,
        "crap4ts --lenient must be the cyclomatic `lenient` cutoff (30), \
         not the cognitive 40"
    );
}

// ── crap4rs: cognitive-metric adapter (regression guards) ────────────

#[test]
fn default_gate_crap4rs_no_flag_stays_cognitive_25() {
    assert_eq!(
        crap4rs_threshold(&[]),
        25.0,
        "crap4rs no-flag default must stay the cognitive `default` \
         cutoff (25) — the resolution refactor must not regress the \
         Rust adapter"
    );
}

#[test]
fn default_gate_crap4rs_strict_stays_cognitive_15() {
    assert_eq!(
        crap4rs_threshold(&["--strict"]),
        15.0,
        "crap4rs --strict (cognitive metric) must stay 15"
    );
}

#[test]
fn default_gate_crap4rs_metric_cyclomatic_no_flag_is_16() {
    // The deeper instance of the same defect class: crap4rs *supports*
    // cyclomatic too, and `--metric cyclomatic` with no threshold must
    // resolve to the cyclomatic `default` (16), not the cognitive 25.
    assert_eq!(
        crap4rs_threshold(&["--metric", "cyclomatic"]),
        16.0,
        "crap4rs --metric cyclomatic no-flag default must be the \
         cyclomatic cutoff (16), not the cognitive 25"
    );
}

#[test]
fn default_gate_crap4rs_metric_cyclomatic_strict_is_8() {
    assert_eq!(
        crap4rs_threshold(&["--metric", "cyclomatic", "--strict"]),
        8.0,
        "crap4rs --metric cyclomatic --strict must be the cyclomatic \
         strict cutoff (8), not the cognitive 15"
    );
}
