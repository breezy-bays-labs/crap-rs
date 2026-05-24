//! Wire-envelope shape lock for the crap4rs `--format json` envelope.
//!
//! This module owns the **crap4rs** half of the dual wire-envelope
//! canary established in W1.3 (crap-rs#183). Its sibling
//! `wire_envelope_crap4ts.rs` owns the **crap4ts** half. From W1.3
//! onward, every W2+ PR body must declare either "no drift" or
//! "drift documented" for BOTH snapshots.
//!
//! This snapshot is the original canary from the S1–S5 monorepo
//! migration (crap4rs#132). Every PR must keep this snapshot byte-
//! identical OR document the drift in the PR body. Drift is the
//! deliberate breaking-change tripwire; a default-passing run means
//! the JSON envelope is unchanged.
//!
//! Why this lives in `crap-core` (which has no source code in S1):
//! the envelope shape is the language-agnostic contract that every
//! adapter (crap4rs, crap4ts, future siblings) must produce. Owning
//! the lock here documents that conceptually now, before code moves.
//!
//! Invocation crosses crate boundaries: `assert_cmd::Command::cargo_bin`
//! discovers `crap4rs` via Cargo's workspace test harness (the env
//! var `CARGO_BIN_EXE_crap4rs` is only set for tests inside the
//! crap4rs crate itself).
//!
//! Volatile fields stripped before snapshotting: `tool_version`,
//! `timestamp`, `baseline_tool_version`, `baseline_timestamp`. These
//! change per-build / per-run; the snapshot must catch envelope-shape
//! drift, not version-bump churn.
//!
//! To update after a deliberate envelope change:
//!   `cargo insta review` → accept the new snapshot.

use assert_cmd::Command;
use serde_json::Value;

/// Strip per-build / per-run fields so the snapshot only catches
/// shape drift, not version/timestamp churn. Operates recursively
/// to handle nested baseline contexts (`delta.baseline.*`) that may
/// appear once delta mode lands a baseline.
fn strip_volatile(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in [
                "tool_version",
                "timestamp",
                "baseline_tool_version",
                "baseline_timestamp",
            ] {
                if let Some(slot) = map.get_mut(key) {
                    *slot = Value::String(format!("[STRIPPED:{key}]"));
                }
            }
            for child in map.values_mut() {
                strip_volatile(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_volatile(item);
            }
        }
        _ => {}
    }
}

#[test]
fn envelope() {
    // Fixture: crap4rs's own LCOV against its own source. `--no-fail`
    // ensures the snapshot covers the threshold-exceeded branch
    // (passed=false). `--threshold 8` matches the post-#272 strict
    // cutoff and exposes the crap4rs-self.lcov over-threshold
    // functions — that's the realistic envelope shape.
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

    let mut envelope: Value =
        serde_json::from_slice(&output.stdout).expect("crap4rs --format json emits valid JSON");
    strip_volatile(&mut envelope);

    let pretty = serde_json::to_string_pretty(&envelope).expect("envelope re-serializes");
    insta::assert_snapshot!("envelope", pretty);
}
