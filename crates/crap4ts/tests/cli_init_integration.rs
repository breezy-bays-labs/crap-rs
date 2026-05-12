//! Integration tests for `crap4ts init` (crap-rs#73).
//!
//! crap4ts inherits the `init` subcommand for free via `AdapterMeta`
//! (`config_file_name = "crap4ts.toml"`, `default_excludes = ["node_modules/**", …]`).
//! These tests live here (not in the crap4rs cucumber harness) because
//! `CARGO_BIN_EXE_<name>` is set per-package: from inside the crap4rs
//! harness `CARGO_BIN_EXE_crap4ts` would be undefined.
//!
//! Each test writes into a fresh tempdir so cwd-pollution between
//! parallel invocations stays impossible.

use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4ts");

#[test]
fn crap4ts_init_writes_crap4ts_toml_not_crap4rs_toml() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let output = Command::new(BINARY)
        .current_dir(tmp.path())
        .args(["init", "--non-interactive"])
        .output()
        .expect("invoke crap4ts binary");

    assert!(
        output.status.success(),
        "crap4ts init exited non-zero: status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        tmp.path().join("crap4ts.toml").exists(),
        "crap4ts.toml should exist after init",
    );
    assert!(
        !tmp.path().join("crap4rs.toml").exists(),
        "crap4rs.toml should NOT exist after `crap4ts init` (per-adapter file name)",
    );
}

#[test]
fn crap4ts_init_emits_ts_specific_excludes() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    Command::new(BINARY)
        .current_dir(tmp.path())
        .args(["init", "--non-interactive"])
        .output()
        .expect("invoke crap4ts binary");

    let content = std::fs::read_to_string(tmp.path().join("crap4ts.toml"))
        .expect("read generated crap4ts.toml");
    // The default excludes for TS come from AdapterMeta.default_excludes;
    // we assert the user-visible patterns the crap4ts main.rs declares.
    assert!(
        content.contains("node_modules/**"),
        "expected TS-flavored exclude (node_modules/**) in generated config:\n{content}",
    );
    assert!(
        content.contains("dist/**"),
        "expected TS-flavored exclude (dist/**) in generated config:\n{content}",
    );
    // Negative: Rust-flavored defaults from crap4rs should NOT leak in.
    assert!(
        !content.contains("benches/**"),
        "Rust-only exclude (benches/**) should not appear in crap4ts.toml:\n{content}",
    );
}

#[test]
fn crap4ts_init_refuses_to_overwrite_without_force() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let cfg = tmp.path().join("crap4ts.toml");
    std::fs::write(&cfg, "preset = \"lenient\"\n").expect("seed existing config");

    let output = Command::new(BINARY)
        .current_dir(tmp.path())
        .args(["init", "--non-interactive"])
        .output()
        .expect("invoke crap4ts binary");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("crap4ts.toml already exists"),
        "expected collision error in stderr; got:\n{stderr}",
    );
    let preserved = std::fs::read_to_string(&cfg).expect("read config");
    assert_eq!(preserved, "preset = \"lenient\"\n");
}

#[test]
fn crap4ts_init_force_overwrites() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let cfg = tmp.path().join("crap4ts.toml");
    std::fs::write(&cfg, "preset = \"lenient\"\n").expect("seed existing config");

    let output = Command::new(BINARY)
        .current_dir(tmp.path())
        .args(["init", "--non-interactive", "--force"])
        .output()
        .expect("invoke crap4ts binary");

    assert!(output.status.success());
    let content = std::fs::read_to_string(&cfg).expect("read config");
    assert!(content.contains("preset = \"default\""));
    assert!(!content.contains("preset = \"lenient\""));
}
