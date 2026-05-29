//! Cross-adapter parity for crap-rs#345 — crap4ts discovers the unified
//! `crap.toml` and falls back to its legacy `crap4ts.toml` with the same
//! deprecation nudge as crap4rs.
//!
//! These mirror the crap4ts scenarios in
//! `crates/crap4rs/tests/features/config_discovery.feature`, which are
//! permanently unrunnable from the crap4rs cucumber harness because
//! `CARGO_BIN_EXE_crap4ts` is only set for same-package tests. Both
//! adapters share crap-core's `load_file_config`, so the discovery
//! behaviour is structurally identical — these tests guard that the
//! crap4ts binary actually wires the shared ordered name list
//! (`&["crap.toml", "crap4ts.toml"]`).
//!
//! The effective threshold is read from the `--summary` line, with
//! distinct non-default values (9 vs 22) proving which config drove the
//! run. crap4ts consumes Istanbul `coverage-final.json`.

use std::path::Path;
use std::process::{Command, Output};

const BINARY: &str = env!("CARGO_BIN_EXE_crap4ts");

/// Seed a minimal TS source tree + Istanbul coverage so a `--summary`
/// run completes. Coverage overlap is irrelevant — the summary threshold
/// comes from the discovered config, not from coverage.
fn seed_project(dir: &Path) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(
        src.join("app.ts"),
        "export function greet(name: string): string { return name; }\n",
    )
    .expect("write app.ts");

    let abs = src.join("app.ts").to_string_lossy().replace('\\', "/");
    let payload = format!(
        r#"{{ "{abs}": {{ "path": "{abs}", "s": {{ "0": 1 }},
          "statementMap": {{ "0": {{ "start": {{ "line": 1, "column": 0 }}, "end": {{ "line": 1, "column": 5 }} }} }} }} }}"#
    );
    std::fs::write(dir.join("coverage-final.json"), payload).expect("write coverage-final.json");
}

fn run_summary(dir: &Path) -> Output {
    Command::new(BINARY)
        .current_dir(dir)
        .args([
            "--summary",
            "--src",
            "src",
            "--coverage",
            "coverage-final.json",
            "--no-gitignore",
            "--no-fail",
        ])
        .output()
        .expect("failed to run crap4ts binary")
}

#[test]
fn crap4ts_discovers_canonical_crap_toml() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path());
    std::fs::write(dir.path().join("crap.toml"), "threshold = 22.0\n").unwrap();

    let out = run_summary(dir.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "run should succeed; stderr: {stderr}");
    assert!(
        !stderr.contains("deprecated") && !stderr.contains("safe to remove"),
        "canonical-only must emit no discovery notice; stderr: {stderr}"
    );
    assert!(stdout.contains("threshold (22)"), "stdout: {stdout}");
}

#[test]
fn crap4ts_falls_back_to_legacy_crap4ts_toml() {
    let dir = tempfile::tempdir().unwrap();
    seed_project(dir.path());
    std::fs::write(dir.path().join("crap4ts.toml"), "threshold = 9.0\n").unwrap();

    let out = run_summary(dir.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "run should succeed; stderr: {stderr}");
    // Same deprecation nudge shape as crap4rs, with crap4ts's legacy name.
    assert!(stderr.contains("crap4ts.toml"), "stderr: {stderr}");
    assert!(stderr.contains("deprecated"), "stderr: {stderr}");
    assert!(stderr.contains("crap.toml"), "stderr: {stderr}");
    assert!(
        stdout.contains("threshold (9)"),
        "legacy config must drive the run; stdout: {stdout}"
    );
}
