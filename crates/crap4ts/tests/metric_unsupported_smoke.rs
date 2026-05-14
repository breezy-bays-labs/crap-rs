//! Integration smoke tests for `CrapError::MetricNotSupported` rendering.
//!
//! W2.5 (crap-rs#188) lands `CrapError::MetricNotSupported { metric }`
//! plus the per-adapter `AdapterMeta::default_metric` mechanism. These
//! tests assert the four user-visible contracts from
//! `tests/features/metric_unsupported.feature`:
//!
//! 1. `crap4ts --metric cognitive` exits status 2 and emits the exact
//!    user-facing string from scenario 1 (byte-for-byte).
//! 2. `crap4ts --metric cyclomatic` (explicit) succeeds — the walker
//!    accepts cyclomatic; the only unsupported metric is cognitive.
//! 3. `crap4ts` (no `--metric` flag) succeeds — exercises the new
//!    `AdapterMeta::default_metric = Cyclomatic` plumbing for crap4ts.
//!    Without this, the no-flag invocation would fall through to
//!    `ComplexityMetric::default() == Cognitive` and trip the walker
//!    guard.
//! 4. `crap4rs` (no `--metric` flag) still succeeds with the cognitive
//!    default — regression check for the shared `merge_effective_inputs`
//!    signature change.
//!
//! The fixture-loading helper duplicates the W1.3
//! `end_to_end_smoke::build_jest_fixture` substitution pattern (per
//! ADR-d: per-file 30-LOC tempdir helper). Each test file owns its
//! own helper rather than sharing — copy-2 is still under the threshold
//! where a `crap4ts-fixtures` test-support crate would pay off.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

const FIXTURE_TEMPLATE: &str = include_str!("fixtures/istanbul-jest/coverage-final.json");

/// Build a canonicalized tempdir, copy the W1.1 jest fixture TS files
/// into it, and write a `coverage-final.json` with `{SRC_ROOT}`
/// substituted. Mirrors `end_to_end_smoke::build_jest_fixture` per the
/// W1.3 substitution convention (canonicalize because macOS routes
/// `/tmp` through `/private/tmp`; forward-slash normalize because
/// Windows backslashes would break JSON parsing). Returns the `TempDir`
/// guard (drop = cleanup) plus the canonical fixture root.
fn build_jest_fixture() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    for (name, content) in [
        ("simple.ts", include_str!("fixtures/ts-fixtures/simple.ts")),
        ("arrow.ts", include_str!("fixtures/ts-fixtures/arrow.ts")),
        (
            "Button.tsx",
            include_str!("fixtures/ts-fixtures/Button.tsx"),
        ),
        ("map.ts", include_str!("fixtures/ts-fixtures/map.ts")),
        ("mixed.ts", include_str!("fixtures/ts-fixtures/mixed.ts")),
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

/// Spawn a binary by name with the supplied args. Used for both
/// `crap4ts` and `crap4rs` invocations across the four tests below.
fn run_bin(name: &str, src: &Path, coverage: &Path, extra: &[&str]) -> std::process::Output {
    Command::cargo_bin(name)
        .unwrap_or_else(|_| panic!("{name} binary discoverable in workspace"))
        .arg("--coverage")
        .arg(coverage)
        .arg("--src")
        .arg(src)
        .args(extra)
        .output()
        .unwrap_or_else(|e| panic!("{name} executes: {e}"))
}

// ── 1. `--metric cognitive` produces the exact-string scenario-1 error ──

#[test]
fn cognitive_flag_exits_with_metric_not_supported_error() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");
    let out = run_bin(
        "crap4ts",
        &root,
        &coverage,
        &["--metric", "cognitive", "--no-fail"],
    );

    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit code 2 (CrapError); got status={:?}\nstdout={}\nstderr={}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    // The exact-string contract from
    // `tests/features/metric_unsupported.feature` scenario 1. Asserted
    // via `contains` because (a) the binary prints `\n` after the
    // message and (b) the wider stderr may include the per-file
    // `warning: skipping` traffic from the parse-failure-continues
    // pipeline. The user-facing line itself is byte-identical to the
    // feature-file expected output.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let expected = "crap4ts: complexity metric `cognitive` is not yet supported. Use `--metric cyclomatic` (the default for crap4ts) or track support at https://github.com/breezy-bays-labs/crap-rs.";
    assert!(
        stderr.contains(expected),
        "stderr missing exact MetricNotSupported message.\nExpected to contain:\n{expected}\nActual stderr:\n{stderr}"
    );
    // Explicitly assert the `error:` prefix is NOT present — the
    // metric-unsupported renderer bypasses the generic prefix per
    // breadboard W-5 (CPO sharpening: direct guidance, no `run --help`
    // indirection, no generic prefix).
    assert!(
        !stderr.contains("error: crap4ts:"),
        "rendered error should not have generic `error:` prefix for MetricNotSupported; stderr=\n{stderr}"
    );
}

// ── 2. `--metric cyclomatic` (explicit) succeeds ──────────────────────

#[test]
fn cyclomatic_flag_explicit_succeeds() {
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");
    let out = run_bin(
        "crap4ts",
        &root,
        &coverage,
        &["--metric", "cyclomatic", "--threshold", "16", "--no-fail"],
    );

    assert!(
        out.status.success(),
        "crap4ts --metric cyclomatic exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

// ── 3. `crap4ts` no-flag invocation defaults to cyclomatic + succeeds ─

#[test]
fn no_flag_invocation_defaults_to_cyclomatic_via_adapter_meta() {
    // This is the load-bearing test for `AdapterMeta::default_metric`.
    // Without the W2.5 plumbing, `crap4ts` with no `--metric` flag
    // would fall through to `ComplexityMetric::default() == Cognitive`,
    // trip the walker's MetricNotSupported guard, and exit 2.
    let (_tmp, root) = build_jest_fixture();
    let coverage = root.join("coverage-final.json");
    let out = run_bin(
        "crap4ts",
        &root,
        &coverage,
        &["--threshold", "16", "--no-fail"],
    );

    assert!(
        out.status.success(),
        "crap4ts (no flag) should default to cyclomatic via AdapterMeta::default_metric; got non-zero exit\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    // Sanity check: stderr must not contain a MetricNotSupported
    // signal — that would mean the no-flag invocation defaulted to
    // cognitive (the pre-W2.5 fallthrough).
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("is not yet supported"),
        "no-flag crap4ts emitted MetricNotSupported phrase (default fell through to cognitive?); stderr=\n{stderr}"
    );
}

// ── 4. `crap4rs` no-flag default-cognitive regression check ───────────

#[test]
fn crap4rs_no_flag_default_cognitive_still_works() {
    // Regression check on the shared `merge_effective_inputs` change.
    // crap4rs's `AdapterMeta::default_metric` is `Cognitive`; the
    // walker supports both metrics; no-flag invocation should succeed
    // and produce a complete scorecard with cognitive scoring.
    //
    // We re-use crap4rs's existing self-LCOV fixture (the same one
    // `wire_envelope_crap4rs.rs` uses) and analyze crap4rs's own
    // source. `--no-fail` because the self-LCOV has threshold
    // violations baked in — we only care that the binary runs
    // green end-to-end, not that it passes the gate.
    let out = Command::cargo_bin("crap4rs")
        .expect("crap4rs binary discoverable in workspace")
        .args([
            "--coverage",
            "../crap4rs/tests/fixtures/crap4rs-self.lcov",
            "--src",
            "../crap4rs/src",
            "--threshold",
            "25",
            "--no-fail",
        ])
        .output()
        .expect("crap4rs binary executes");

    assert!(
        out.status.success(),
        "crap4rs (no flag) should default to cognitive via AdapterMeta::default_metric; got non-zero exit\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("is not yet supported"),
        "no-flag crap4rs emitted MetricNotSupported (regression in shared merge_effective_inputs?); stderr=\n{stderr}"
    );
}
