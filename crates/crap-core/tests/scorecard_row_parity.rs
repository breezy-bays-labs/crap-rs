//! Cross-adapter scorecard-row parity canary.
//!
//! Both `crap4rs` and `crap4ts` route `--format scorecard-row` through
//! crap-core's shared `cli::format_as_scorecard_row` →
//! `domain::summary::project_crap_delta_row` →
//! `reporters::format_scorecard_row` pipeline (see
//! `crates/crap-core/src/cli/mod.rs` `FormatArg::ScorecardRow` arm).
//! The Row JSON shape is therefore structurally guaranteed to be
//! identical across both adapters. This test mechanically enforces
//! that guarantee so a future refactor cannot quietly diverge the
//! shapes — e.g. by adding a per-adapter post-processing step or by
//! introducing language-specific Row fields. Mirrors the
//! "documentation rots; CI doesn't" pattern from
//! `scripts/bdd-tracked-lint.py` and `scripts/mutants-skip-lint.py`.
//!
//! Scope of the parity contract enforced here:
//!
//! * **Key set parity** — the JSON object emitted by each adapter
//!   carries the same top-level keys, both in the Green (no
//!   violations) branch and in the Red (above-threshold) branch.
//!   The Red branch is significant because it adds
//!   `failure_detail_md` to the Green key set; verifying both
//!   branches catches a divergence on either path.
//! * **Value-shape parity** — `type`/`id`/`label`/`anchor` are
//!   stable string literals, `status` is one of the documented
//!   enum members, `threshold` is an integer, `delta_count` is an
//!   integer. The locked scorecard-row schema expects these shapes;
//!   a quiet shift (e.g. `"status": "yellow"` lowercase) would slip
//!   past key-set parity alone.
//!
//! Out of scope:
//!
//! * **Value content parity.** The two adapters analyze different
//!   source trees (Rust vs TypeScript), so `delta_count` /
//!   `delta_text` / `failure_detail_md` values are expected to
//!   differ. The test only asserts shape.
//! * **Multi-language Row variants.** If the schema ever gains a
//!   `language` field (per #114 Discovery: "the Row contract may
//!   need a `language` field"), update this test to include it in
//!   the expected key set and assert each adapter emits its own
//!   language value.
//!
//! Wire-envelope canary discipline applies but the snapshot is
//! NOT taken here — the wire envelope (`--format json`) is locked
//! by `wire_envelope_crap4{rs,ts}.rs`; this module locks the
//! scorecard-row shape only.
//!
//! ## Mutants gate interaction
//!
//! Test fn is named `envelope` so the `--skip envelope` substring
//! token in `.cargo/mutants.toml` covers it under `--package
//! crap-core`. That scoped mutants run does not build the crap4rs
//! or crap4ts bins; without the `--skip` the unmutated baseline
//! would panic on `CARGO_BIN_EXE_*` unset and the gate would go
//! silently dead. See AGENTS.md "Mutation testing" → "Why
//! `additional_cargo_test_args` …" for the full rationale.

use std::collections::BTreeSet;
use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const TS_FIXTURE_TEMPLATE: &str =
    include_str!("../../crap4ts/tests/fixtures/istanbul-jest/coverage-final.json");

/// Keys emitted in the Green branch (no functions over threshold).
const GREEN_KEYS: &[&str] = &[
    "anchor",
    "delta_count",
    "delta_text",
    "id",
    "label",
    "status",
    "threshold",
    "type",
];

/// Keys emitted in the Red/Yellow branch. Adds `failure_detail_md`
/// to the Green key set.
const RED_KEYS: &[&str] = &[
    "anchor",
    "delta_count",
    "delta_text",
    "failure_detail_md",
    "id",
    "label",
    "status",
    "threshold",
    "type",
];

/// Build a TypeScript fixture mirroring `wire_envelope_crap4ts`'s
/// helper, so the parity test exercises a real Istanbul parse. Returns
/// the tempdir (held alive for the lifetime of the test) and the
/// canonical fixture root path.
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

/// Invoke `crap4rs --format scorecard-row` against its own self-LCOV
/// fixture at the given threshold; return the parsed Row JSON.
fn run_crap4rs_row(threshold: &str) -> Value {
    let output = Command::cargo_bin("crap4rs")
        .expect("crap4rs binary discoverable in workspace")
        .args([
            "--coverage",
            "../crap4rs/tests/fixtures/crap4rs-self.lcov",
            "--src",
            "../crap4rs/src",
            "--threshold",
            threshold,
            "--format",
            "scorecard-row",
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
    serde_json::from_slice(&output.stdout).expect("crap4rs --format scorecard-row emits valid JSON")
}

/// Invoke `crap4ts --format scorecard-row` against the temp Istanbul
/// fixture at the given threshold; return the parsed Row JSON.
fn run_crap4ts_row(threshold: &str) -> Value {
    let (_tmp, root) = build_ts_fixture();
    let coverage = root.join("coverage-final.json");
    let output = Command::cargo_bin("crap4ts")
        .expect("crap4ts binary discoverable in workspace")
        .arg("--coverage")
        .arg(&coverage)
        .arg("--src")
        .arg(&root)
        .args([
            "--threshold",
            threshold,
            "--format",
            "scorecard-row",
            "--no-fail",
        ])
        .output()
        .expect("crap4ts binary executes");
    assert!(
        output.status.success(),
        "crap4ts exited non-zero: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_slice(&output.stdout).expect("crap4ts --format scorecard-row emits valid JSON")
}

/// Sorted top-level key set of a JSON object.
fn key_set(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("Row JSON is a top-level object")
        .keys()
        .cloned()
        .collect()
}

/// Stable-string-or-integer shape assertions both adapters' Rows
/// must satisfy. Anything that varies by analyzed source (delta
/// counts, failure detail text) is intentionally not asserted —
/// see module docstring.
fn assert_value_shape(row: &Value, adapter: &str) {
    let obj = row.as_object().unwrap_or_else(|| {
        panic!("{adapter}: Row JSON must be an object, got {row}");
    });

    assert_eq!(
        obj.get("type").and_then(Value::as_str),
        Some("CrapDelta"),
        "{adapter}: type must be \"CrapDelta\""
    );
    assert_eq!(
        obj.get("id").and_then(Value::as_str),
        Some("crap_delta"),
        "{adapter}: id must be \"crap_delta\""
    );
    assert_eq!(
        obj.get("label").and_then(Value::as_str),
        Some("CRAP Δ"),
        "{adapter}: label must be \"CRAP Δ\""
    );
    assert_eq!(
        obj.get("anchor").and_then(Value::as_str),
        Some("crap-delta"),
        "{adapter}: anchor must be \"crap-delta\""
    );

    let status = obj
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "{adapter}: status must be a string, got {:?}",
                obj.get("status")
            );
        });
    assert!(
        ["Green", "Yellow", "Red"].contains(&status),
        "{adapter}: status must be Green|Yellow|Red, got {status}"
    );

    assert!(
        obj.get("threshold").and_then(Value::as_u64).is_some(),
        "{adapter}: threshold must be a non-negative integer, got {:?}",
        obj.get("threshold")
    );
    // `delta_count` is signed in the domain (`i32` on
    // `CrapDeltaRowData`): `current_count - baseline_count`, which can
    // be negative when violations decrease vs the baseline. Validate
    // as a signed integer.
    assert!(
        obj.get("delta_count").and_then(Value::as_i64).is_some(),
        "{adapter}: delta_count must be a signed integer, got {:?}",
        obj.get("delta_count")
    );
    assert!(
        obj.get("delta_text").and_then(Value::as_str).is_some(),
        "{adapter}: delta_text must be a string, got {:?}",
        obj.get("delta_text")
    );
}

#[test]
fn envelope() {
    // Green branch: high threshold puts both adapters in
    // "no violations" territory. Asserts the minimal key set.
    let green_threshold = "1000";
    let rs_green = run_crap4rs_row(green_threshold);
    let ts_green = run_crap4ts_row(green_threshold);

    let expected_green: BTreeSet<String> = GREEN_KEYS.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        key_set(&rs_green),
        expected_green,
        "crap4rs Green Row key set drifted from contract: {rs_green}"
    );
    assert_eq!(
        key_set(&ts_green),
        expected_green,
        "crap4ts Green Row key set drifted from contract: {ts_green}"
    );
    assert_eq!(
        key_set(&rs_green),
        key_set(&ts_green),
        "crap4rs Green Row keys diverged from crap4ts:\n\
         crap4rs: {rs_green}\n\
         crap4ts: {ts_green}"
    );
    assert_eq!(
        rs_green.get("status").and_then(Value::as_str),
        Some("Green"),
        "crap4rs at threshold {green_threshold} must report Green status"
    );
    assert_eq!(
        ts_green.get("status").and_then(Value::as_str),
        Some("Green"),
        "crap4ts at threshold {green_threshold} must report Green status"
    );
    assert_value_shape(&rs_green, "crap4rs (green)");
    assert_value_shape(&ts_green, "crap4ts (green)");

    // Red branch: low threshold forces violations on both adapters.
    // Asserts the key set expands with `failure_detail_md` for both
    // and stays parity-locked. (`crap4rs-self.lcov` has functions
    // well above 1; the TS fixture's `cube` arrow at 0% coverage hits
    // CRAP=2.0 — both adapters cross threshold=1.)
    let red_threshold = "1";
    let rs_red = run_crap4rs_row(red_threshold);
    let ts_red = run_crap4ts_row(red_threshold);

    let expected_red: BTreeSet<String> = RED_KEYS.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        key_set(&rs_red),
        expected_red,
        "crap4rs Red Row key set drifted from contract: {rs_red}"
    );
    assert_eq!(
        key_set(&ts_red),
        expected_red,
        "crap4ts Red Row key set drifted from contract: {ts_red}"
    );
    assert_eq!(
        key_set(&rs_red),
        key_set(&ts_red),
        "crap4rs Red Row keys diverged from crap4ts:\n\
         crap4rs: {rs_red}\n\
         crap4ts: {ts_red}"
    );
    assert!(
        rs_red
            .get("failure_detail_md")
            .and_then(Value::as_str)
            .is_some_and(|s| s.contains("over CRAP threshold")),
        "crap4rs Red failure_detail_md must reference threshold overrun: {rs_red}"
    );
    assert!(
        ts_red
            .get("failure_detail_md")
            .and_then(Value::as_str)
            .is_some_and(|s| s.contains("over CRAP threshold")),
        "crap4ts Red failure_detail_md must reference threshold overrun: {ts_red}"
    );
    assert_value_shape(&rs_red, "crap4rs (red)");
    assert_value_shape(&ts_red, "crap4ts (red)");
}
