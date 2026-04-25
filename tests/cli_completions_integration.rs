//! Integration tests for `crap4rs completions <SHELL>` (issue #69).
//!
//! The subcommand prints a completion script for the chosen shell to
//! stdout and exits 0. No file I/O — callers redirect to wherever the
//! shell expects completions to live. `--coverage` is not required for
//! this subcommand.

use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

fn run_completions(shell: &str) -> std::process::Output {
    Command::new(BINARY)
        .args(["completions", shell])
        .output()
        .expect("failed to run crap4rs binary")
}

fn assert_script_for(shell: &str) {
    let out = run_completions(shell);
    assert!(
        out.status.success(),
        "expected exit 0 for shell {shell}; status={:?}, stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        !out.stdout.is_empty(),
        "expected non-empty stdout for shell {shell}",
    );
    let body = String::from_utf8_lossy(&out.stdout);
    assert!(
        body.contains("crap4rs"),
        "expected completion script to mention crap4rs binary name; got first 200 bytes: {}",
        &body.chars().take(200).collect::<String>(),
    );
}

#[test]
fn completions_bash() {
    assert_script_for("bash");
}

#[test]
fn completions_zsh() {
    assert_script_for("zsh");
}

#[test]
fn completions_fish() {
    assert_script_for("fish");
}

#[test]
fn completions_powershell() {
    assert_script_for("powershell");
}

#[test]
fn completions_elvish() {
    assert_script_for("elvish");
}

#[test]
fn completions_nushell() {
    assert_script_for("nushell");
}

#[test]
fn completions_unknown_shell_exits_2() {
    let out = Command::new(BINARY)
        .args(["completions", "tcsh"])
        .output()
        .expect("failed to run crap4rs binary");
    // clap rejects unknown ValueEnum variants with exit 2 ("invalid value")
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2 for unknown shell"
    );
}
