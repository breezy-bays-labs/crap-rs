//! Integration tests for crap-rs#345 — unified `crap.toml` config name
//! with legacy `crap4rs.toml` dual-discovery.
//!
//! These exercise the end-to-end CLI wiring that the four
//! `discover_config` unit cases (`crates/crap-core/src/adapters/config.rs`)
//! cannot reach: the deprecation / shadow notices are emitted by the CLI
//! layer (`load_file_config`) on stderr, so they only surface through a
//! real subprocess run. Together they cover the wired half of
//! `tests/features/config_discovery.feature` (the crap4rs scenarios); the
//! crap4ts scenarios are mirrored in `crates/crap4ts/tests/`.
//!
//! The effective threshold is observed via the `--summary` line
//! (`PASS: N functions | M above threshold (T) | …`), which prints the
//! post-merge threshold — distinct, non-default values (9 vs 22) prove
//! *which* config file actually drove the run.

use std::path::Path;
use std::process::{Command, Output};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

/// Minimal source tree + LCOV so the run completes (a `--summary` run
/// requires `--coverage`). The exact coverage match is irrelevant — the
/// threshold in the summary line comes from the discovered config, not
/// from coverage overlap.
fn seed_project(dir: &Path) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(src.join("lib.rs"), "pub fn a() -> i32 { 1 }\n").expect("write lib.rs");
    std::fs::write(
        dir.join("lcov.info"),
        "SF:src/lib.rs\nDA:1,1\nend_of_record\n",
    )
    .expect("write lcov.info");
}

/// Run `crap4rs --summary` in `dir` with discovery active (no `--config`).
fn run_summary(dir: &Path) -> Output {
    Command::new(BINARY)
        .current_dir(dir)
        .args([
            "--summary",
            "--src",
            "src",
            "--coverage",
            "lcov.info",
            "--no-gitignore",
            "--no-fail",
        ])
        .output()
        .expect("failed to run crap4rs binary")
}

#[test]
fn legacy_crap4rs_toml_discovered_emits_deprecation_notice() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path());
    // Only the legacy name present — discovery must fall back to it.
    std::fs::write(dir.path().join("crap4rs.toml"), "threshold = 9.0\n").unwrap();

    let out = run_summary(dir.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "run should succeed; stderr: {stderr}");
    // The deprecation nudge names the legacy file, the word "deprecated",
    // and the canonical name to rename to.
    assert!(stderr.contains("crap4rs.toml"), "stderr: {stderr}");
    assert!(stderr.contains("deprecated"), "stderr: {stderr}");
    assert!(stderr.contains("crap.toml"), "stderr: {stderr}");
    // The legacy config actually drove the run (threshold 9, not a default).
    assert!(
        stdout.contains("threshold (9)"),
        "summary must reflect the legacy config's threshold; stdout: {stdout}"
    );
}

#[test]
fn canonical_crap_toml_wins_and_reports_legacy_shadowed() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path());
    // Both present — canonical wins, legacy is shadowed.
    std::fs::write(dir.path().join("crap.toml"), "threshold = 22.0\n").unwrap();
    std::fs::write(dir.path().join("crap4rs.toml"), "threshold = 9.0\n").unwrap();

    let out = run_summary(dir.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "run should succeed; stderr: {stderr}");
    // The shadow notice names the ignored legacy file and says it is safe
    // to remove — no "deprecated" nudge (the canonical is in use).
    assert!(stderr.contains("crap4rs.toml"), "stderr: {stderr}");
    assert!(stderr.contains("safe to remove"), "stderr: {stderr}");
    assert!(
        !stderr.contains("deprecated"),
        "canonical-in-use must not emit a deprecation notice; stderr: {stderr}"
    );
    // The canonical config drove the run (threshold 22), not the legacy 9.
    assert!(
        stdout.contains("threshold (22)"),
        "canonical must win precedence; stdout: {stdout}"
    );
}

#[test]
fn canonical_crap_toml_emits_no_notice() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path());
    std::fs::write(dir.path().join("crap.toml"), "threshold = 22.0\n").unwrap();

    let out = run_summary(dir.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "run should succeed; stderr: {stderr}");
    // Canonical-only discovery is the happy path — no deprecation, no
    // shadow notice on stderr.
    assert!(
        !stderr.contains("deprecated") && !stderr.contains("safe to remove"),
        "canonical-only must emit no discovery notice; stderr: {stderr}"
    );
    assert!(stdout.contains("threshold (22)"), "stdout: {stdout}");
}

#[test]
fn no_config_file_uses_builtin_defaults_silently() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path());
    // No crap.toml and no legacy file present.

    let out = run_summary(dir.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "run should succeed; stderr: {stderr}");
    // No config discovered → no discovery notice at all.
    assert!(
        !stderr.contains("deprecated") && !stderr.contains("safe to remove"),
        "no-config must emit no discovery notice; stderr: {stderr}"
    );
    // The built-in default threshold (15) drives the run.
    assert!(
        stdout.contains("threshold (15)"),
        "no-config must fall back to the built-in default; stdout: {stdout}"
    );
}

#[test]
fn malformed_canonical_crap_toml_errors_without_legacy_fallthrough() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path());
    // A present-but-malformed canonical file must WIN discovery and
    // surface its parse error — it must NOT silently fall through to the
    // co-present (valid) legacy file. Pins the "first existing file wins"
    // contract: a typo in crap.toml can never be masked by a stale legacy.
    std::fs::write(dir.path().join("crap.toml"), "threshold = not_a_number\n").unwrap();
    std::fs::write(dir.path().join("crap4rs.toml"), "threshold = 9.0\n").unwrap();

    let out = run_summary(dir.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        !out.status.success(),
        "a malformed canonical config must fail the run, not fall through"
    );
    assert!(
        stderr.contains("crap.toml") && stderr.contains("parse"),
        "the error must name the malformed canonical file; stderr: {stderr}"
    );
    // The legacy file's threshold (9) must NOT have taken effect.
    assert!(
        !stdout.contains("threshold (9)"),
        "must not fall through to the legacy config; stdout: {stdout}"
    );
}
