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
//! Threshold cutoffs are routed metric-by-metric through
//! `ThresholdPreset::threshold`. Cognitive and cyclomatic columns
//! currently hold the same values (strict/default/lenient = 8/15/25
//! for both); the dual-column infrastructure is preserved so a future
//! per-metric divergence is a one-line constant change.
//!
//! | invocation                              | metric    | tier    | expect |
//! |-----------------------------------------|-----------|---------|--------|
//! | `crap4ts` (no flags)                    | cyclomatic| default | 15     |
//! | `crap4ts --strict`                      | cyclomatic| strict  | 8      |
//! | `crap4ts --lenient`                     | cyclomatic| lenient | 25     |
//! | `crap4rs` (no flags)                    | cognitive | default | 15     |
//! | `crap4rs --strict`                      | cognitive | strict  | 8      |
//! | `crap4rs --metric cyclomatic`           | cyclomatic| default | 15     |
//! | `crap4rs --metric cyclomatic --strict`  | cyclomatic| strict  | 8      |
//!
//! Tests assert the metric-keyed routing path even when the columns
//! agree, so if a future change splits the columns the regression
//! surfaces here first.

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
            // Explicit empty `--config` short-circuits walk-upward
            // discovery (crap-rs#339). Load-bearing here: this test asserts
            // the BUILT-IN default threshold, so it must not read the
            // repo-root `crap.toml`'s `preset` (crap-rs#346). See the fixture.
            "--config",
            "tests/fixtures/empty-config.toml",
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
fn default_gate_crap4ts_no_flag_is_cyclomatic_default() {
    assert_eq!(
        crap4ts_threshold(&[]),
        15.0,
        "crap4ts no-flag default must resolve to the cyclomatic \
         `default` cutoff via metric-keyed routing"
    );
}

#[test]
fn default_gate_crap4ts_strict_is_cyclomatic_strict() {
    assert_eq!(
        crap4ts_threshold(&["--strict"]),
        8.0,
        "crap4ts --strict must resolve to the cyclomatic `strict` cutoff"
    );
}

#[test]
fn default_gate_crap4ts_lenient_is_cyclomatic_lenient() {
    assert_eq!(
        crap4ts_threshold(&["--lenient"]),
        25.0,
        "crap4ts --lenient must resolve to the cyclomatic `lenient` cutoff"
    );
}

// ── crap4rs: cognitive-metric adapter (regression guards) ────────────

#[test]
fn default_gate_crap4rs_no_flag_is_cognitive_default() {
    assert_eq!(
        crap4rs_threshold(&[]),
        15.0,
        "crap4rs no-flag default must resolve to the cognitive \
         `default` cutoff via metric-keyed routing"
    );
}

#[test]
fn default_gate_crap4rs_strict_is_cognitive_strict() {
    assert_eq!(
        crap4rs_threshold(&["--strict"]),
        8.0,
        "crap4rs --strict (cognitive metric) must resolve to the \
         cognitive `strict` cutoff"
    );
}

#[test]
fn default_gate_crap4rs_metric_cyclomatic_no_flag_is_default() {
    // crap4rs *supports* cyclomatic too; --metric cyclomatic with no
    // threshold must route through the cyclomatic column.
    assert_eq!(
        crap4rs_threshold(&["--metric", "cyclomatic"]),
        15.0,
        "crap4rs --metric cyclomatic no-flag default must resolve to \
         the cyclomatic `default` cutoff"
    );
}

#[test]
fn default_gate_crap4rs_metric_cyclomatic_strict_is_strict() {
    assert_eq!(
        crap4rs_threshold(&["--metric", "cyclomatic", "--strict"]),
        8.0,
        "crap4rs --metric cyclomatic --strict must resolve to the \
         cyclomatic `strict` cutoff"
    );
}
