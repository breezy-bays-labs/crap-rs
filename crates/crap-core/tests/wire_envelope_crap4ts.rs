//! Wire-envelope shape lock for the crap4ts `--format json` envelope.
//!
//! Sibling of `wire_envelope_crap4rs.rs`. Together they form the dual
//! wire-envelope canary established in W1.3 (crap-rs#183): every W2+
//! PR body must declare either "no drift" or "drift documented" for
//! BOTH snapshots.
//!
//! ## Why two modules
//!
//! crap-core's envelope types (`schema_version`, `metric`, `language`,
//! `result.functions[]`, `result.summary`, `diagnostics`, ...) are
//! shared across adapters. The serde derives enforce structural
//! parity; this snapshot canary enforces that the *materialized*
//! envelope for a representative TS run keeps a stable shape across
//! refactors. Separate per-adapter snapshots give cleaner failure
//! attribution than a single dual-fixture snapshot would.
//!
//! ## Wrong-by-design values that flipped at W2.5
//!
//! Through W1.3 the snapshot baked `metric: "cognitive"` — wrong for
//! crap4ts (no cognitive support; default should be cyclomatic).
//! W2.5 (crap-rs#188) flips this via `AdapterMeta::default_metric`
//! per locked decision #2. The snapshot was regenerated at W2.5 PR
//! time; `metric: "cyclomatic"` is now baked. `language: "rust"`
//! remains wrong-by-design — that flips later when the `Language`
//! enum lands as part of the deferred HTML-reporter Wave 5
//! re-launch.
//!
//! ## Volatile fields stripped
//!
//! `tool_version`, `timestamp`, `baseline_tool_version`,
//! `baseline_timestamp`. Same set as the crap4rs sibling.
//!
//! ## Fixture-path volatility
//!
//! The tempdir's canonical path leaks into the envelope's
//! `file_path` fields if `IstanbulCoverage::normalize_path` fails to
//! strip the `--src` prefix (e.g., the tempdir isn't canonicalized).
//! Per `istanbul_smoke::build_fixture`, macOS's `/tmp` → `/private/tmp`
//! symlink requires `std::fs::canonicalize` to keep the prefix-strip
//! consistent. The helper below does the same canonicalization
//! dance.
//!
//! To update after a deliberate envelope change:
//!   `cargo insta review` → accept the new snapshot.

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const FIXTURE_TEMPLATE: &str =
    include_str!("../../crap4ts/tests/fixtures/istanbul-jest/coverage-final.json");

/// Strip per-build / per-run fields so the snapshot only catches
/// shape drift, not version/timestamp churn. Mirrors the recursive
/// strip in `wire_envelope_crap4rs.rs`.
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

/// Build a canonicalized tempdir, write the W1.1 jest fixture sources
/// plus a resolved `coverage-final.json` into it, and return the path
/// pair the binary needs. `TempDir` is returned so the directory stays
/// alive for the lifetime of the test (drop cleans up).
fn build_fixture() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Canonicalize: macOS's /tmp routes through /private/tmp; without
    // canonicalization the parser sees a different prefix than the
    // fixture path entries and the snapshot bakes absolute paths.
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

    // Normalize path separators before string-substituting into the
    // JSON template — on Windows `canonical.to_string_lossy()` returns
    // backslashes, which would land in the JSON as unescaped `\p`-style
    // sequences and fail JSON parsing. Forward slashes are valid path
    // separators in JSON string values on every platform crap4ts
    // currently parses fixtures against. No-op on macOS/linux.
    let payload = FIXTURE_TEMPLATE.replace(
        "{SRC_ROOT}",
        &canonical.to_string_lossy().replace('\\', "/"),
    );
    std::fs::write(canonical.join("coverage-final.json"), payload)
        .expect("write coverage-final.json");

    (tmp, canonical)
}

/// Replace tempdir paths in the envelope with a stable placeholder so
/// the snapshot doesn't bake in per-run paths like
/// `/private/var/folders/.../coverage-final.json`. The crap4ts
/// `IstanbulCoverage::normalize_path` strips the `--src` prefix from
/// `file_path` entries (verified by `istanbul_smoke`), so `file_path`
/// fields should be relative (e.g., `simple.ts`) after parsing. This
/// pass is defense-in-depth: any string value that mentions the
/// tempdir gets neutralized.
fn strip_tempdir_paths(value: &mut Value, tempdir: &str) {
    match value {
        // `str::replace` is a no-op when the needle is absent, so the
        // common "no tempdir leak" path is a single allocation-free
        // scan; matches clippy's collapsible-match guidance.
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
    let coverage = root.join("coverage-final.json");
    // Match production path normalization: `crap_core::core` emits
    // forward-slash paths in the JSON envelope on every platform, so
    // the defense-in-depth `strip_tempdir_paths` needle must be the
    // forward-slash form too. No-op on macOS/linux where
    // `to_string_lossy()` already returns forward slashes.
    let tempdir_str = root.to_string_lossy().replace('\\', "/");

    // Pass `Path` references directly to `Command::arg` (Command::arg
    // takes `AsRef<OsStr>`, which `Path` impls): avoids forcing
    // UTF-8 validation through `to_str()` and handles platform-specific
    // OS string representations on Windows without lossy conversion.
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
    strip_volatile(&mut envelope);
    strip_tempdir_paths(&mut envelope, &tempdir_str);

    let pretty = serde_json::to_string_pretty(&envelope).expect("envelope re-serializes");
    insta::assert_snapshot!("envelope", pretty);
}
