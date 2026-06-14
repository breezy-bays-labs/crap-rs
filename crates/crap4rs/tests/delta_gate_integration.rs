//! Help-discoverability integration tests for `--baseline` /
//! `--delta-gate` (issue #81 VS6).
//!
//! The gate-semantics exit-code matrix that used to live here
//! (informational exit 0, `--delta-gate` exit 1, `--no-fail` override,
//! the clap `requires = "baseline"` rejection) is now wired as `@wired`
//! acceptance scenarios in `tests/features/delta.feature` +
//! `tests/delta_cucumber.rs` — the curated BDD pass makes cucumber the
//! single acceptance layer for these CLI-process contracts. These two
//! `--help` checks remain until the delta help-discoverability scenarios
//! (delta.feature § Help discoverability, still `@unwired`) are wired in
//! the next curated-pass slice, at which point this file is absorbed and
//! deleted.

use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

#[test]
fn help_text_documents_baseline_and_delta_gate() {
    let output = Command::new(BINARY)
        .arg("--help")
        .output()
        .expect("run --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--baseline"),
        "--help must mention --baseline: {stdout}"
    );
    assert!(
        stdout.contains("--delta-gate"),
        "--help must mention --delta-gate: {stdout}"
    );
}

#[test]
fn help_text_includes_delta_examples() {
    let output = Command::new(BINARY)
        .arg("--help")
        .output()
        .expect("run --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("COMPARING TWO ANALYSES"),
        "--help must surface the delta examples block: {stdout}"
    );
    assert!(
        stdout.contains("baseline.json"),
        "--help must show the basic capture/compare flow: {stdout}"
    );
}
