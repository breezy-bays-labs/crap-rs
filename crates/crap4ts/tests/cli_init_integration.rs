//! Integration tests for `crap4ts init` (crap-rs#73).
//!
//! crap4ts inherits the `init` subcommand for free via `AdapterMeta`.
//! `init` writes the canonical config name — the unified `crap.toml`
//! shared by both adapters (crap-rs#345) — not a per-adapter file. These
//! tests live here (not in the crap4rs cucumber harness) because
//! `CARGO_BIN_EXE_<name>` is set per-package: from inside the crap4rs
//! harness `CARGO_BIN_EXE_crap4ts` would be undefined.
//!
//! Each test writes into a fresh tempdir so cwd-pollution between
//! parallel invocations stays impossible.

use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4ts");

#[test]
fn crap4ts_init_writes_canonical_crap_toml() {
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
        tmp.path().join("crap.toml").exists(),
        "init writes the canonical crap.toml",
    );
    // The legacy per-adapter names are discovery fallbacks only — `init`
    // never writes them.
    assert!(
        !tmp.path().join("crap4ts.toml").exists(),
        "legacy crap4ts.toml should NOT be written by init",
    );
    assert!(
        !tmp.path().join("crap4rs.toml").exists(),
        "the other adapter's legacy name should never appear",
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

    let content =
        std::fs::read_to_string(tmp.path().join("crap.toml")).expect("read generated crap.toml");
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
        "Rust-only exclude (benches/**) should not appear in crap.toml:\n{content}",
    );
}

#[test]
fn crap4ts_init_refuses_to_overwrite_without_force() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let cfg = tmp.path().join("crap.toml");
    std::fs::write(&cfg, "preset = \"lenient\"\n").expect("seed existing config");

    let output = Command::new(BINARY)
        .current_dir(tmp.path())
        .args(["init", "--non-interactive"])
        .output()
        .expect("invoke crap4ts binary");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("crap.toml already exists"),
        "expected collision error in stderr; got:\n{stderr}",
    );
    let preserved = std::fs::read_to_string(&cfg).expect("read config");
    assert_eq!(preserved, "preset = \"lenient\"\n");
}

#[test]
fn crap4ts_init_force_overwrites() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let cfg = tmp.path().join("crap.toml");
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
