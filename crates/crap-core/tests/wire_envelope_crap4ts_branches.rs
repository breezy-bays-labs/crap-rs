//! Wire-envelope shape lock for the **branch-coverage-populated**
//! crap4ts `--format json` envelope (crap-rs#251).
//!
//! Sibling of `wire_envelope_crap4ts.rs`. Together they cover the two
//! wire shapes the JSON envelope can produce:
//!
//! | Canary                          | Fixture                                          | Wire shape locked |
//! |---------------------------------|--------------------------------------------------|-------------------|
//! | `wire_envelope_crap4ts`         | `istanbul-jest/coverage-final.json` (empty `b`)  | `branch_coverage_percent` field ABSENT (skip_serializing_if(None)) |
//! | `wire_envelope_crap4ts_branches`| `istanbul-jest/coverage-with-branches.json` (full `b` + `branchMap`) | `branch_coverage_percent` field PRESENT with `f64` value           |
//!
//! ## Why a second canary
//!
//! crap-rs#251 introduced `branch_coverage_percent: Option<f64>` on
//! `ScoredFunction` with `#[serde(skip_serializing_if = "Option::is_none")]`.
//! The default `wire_envelope_crap4ts` fixture has all-empty `branchMap`
//! entries, so its envelope's per-function rows continue to omit the
//! branch field — the canary stays byte-identical to its pre-#251 form,
//! which is the right behavior for "no branch data in play."
//!
//! That leaves the branch-populated shape uncovered by the snapshot
//! gate. This module fixes that asymmetry by exercising the same binary
//! against the branch-rich fixture committed in W2.3 (#186) — locking
//! the wire shape the JSON envelope produces when Istanbul branch
//! records WERE parsed and joined per-function.
//!
//! ## Volatile fields stripped
//!
//! Identical to `wire_envelope_crap4ts.rs`: `tool_version`, `timestamp`,
//! `baseline_tool_version`, `baseline_timestamp`. Per-tempdir paths
//! collapsed to `[TEMPDIR]` via defense-in-depth scrub.
//!
//! To update after a deliberate envelope change:
//!   `cargo insta review` → accept the new snapshot.

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const BRANCH_FIXTURE: &str =
    include_str!("../../crap4ts/tests/fixtures/istanbul-jest/coverage-with-branches.json");

/// Strip per-build / per-run fields so the snapshot only catches
/// shape drift, not version/timestamp churn. Identical to the
/// `wire_envelope_crap4ts.rs` strip.
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

/// Build a canonicalized tempdir with the branch-heavy TS source and
/// the matching Istanbul coverage JSON (with full `b` + `branchMap`
/// records). The `{SRC_ROOT}` template is substituted with the
/// canonical path, mirroring the same trick `wire_envelope_crap4ts.rs`
/// uses for the empty-branches fixture.
fn build_fixture() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Canonicalize: macOS's /tmp routes through /private/tmp; without
    // canonicalization the parser sees a different prefix than the
    // fixture path entries and the snapshot bakes absolute paths.
    let canonical = std::fs::canonicalize(tmp.path()).expect("canonicalize tempdir");

    std::fs::write(
        canonical.join("branch-heavy.ts"),
        include_str!("../../crap4ts/tests/fixtures/ts-fixtures/branch-heavy.ts"),
    )
    .expect("write branch-heavy.ts");

    // Normalize path separators before string-substituting into the
    // JSON template — same Windows-safety as `wire_envelope_crap4ts.rs`.
    let payload = BRANCH_FIXTURE.replace(
        "{SRC_ROOT}",
        &canonical.to_string_lossy().replace('\\', "/"),
    );
    std::fs::write(canonical.join("coverage-with-branches.json"), payload)
        .expect("write coverage-with-branches.json");

    (tmp, canonical)
}

/// Defense-in-depth tempdir scrub — same as `wire_envelope_crap4ts.rs`.
fn strip_tempdir_paths(value: &mut Value, tempdir: &str) {
    match value {
        Value::String(s) => *s = s.replace(tempdir, "[TEMPDIR]"),
        Value::Object(map) => {
            for v in map.values_mut() {
                strip_tempdir_paths(v, tempdir);
            }
        }
        Value::Array(items) => {
            for v in items {
                strip_tempdir_paths(v, tempdir);
            }
        }
        _ => {}
    }
}

#[test]
fn envelope() {
    let (_tmp, root) = build_fixture();
    let coverage = root.join("coverage-with-branches.json");
    let tempdir_str = root.to_string_lossy().replace('\\', "/");

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

    let mut envelope: Value =
        serde_json::from_slice(&output.stdout).expect("crap4ts --format json emits valid JSON");

    // Layer 1 drop migration (PR #292 — ergonomics-α):
    // The scorecard-smoke Layer 1 direct-binary probe used to assert
    // `.tool_version` populated; that probe was dropped when the smoke
    // job collapsed into the scorecard-smoke composite. The canary now
    // owns the assertion (pre-strip, since strip_volatile redacts it).
    assert!(
        envelope["tool_version"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "envelope.tool_version missing or empty (pre-strip)",
    );

    strip_volatile(&mut envelope);
    strip_tempdir_paths(&mut envelope, &tempdir_str);

    let pretty = serde_json::to_string_pretty(&envelope).expect("envelope re-serializes");
    insta::assert_snapshot!("envelope", pretty);
}
