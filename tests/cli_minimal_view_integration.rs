//! Integration tests for `--minimal-view` (issue #79).
//!
//! `--minimal-view` is a payload-size escape hatch for very large
//! codebases. When set, the JSON envelope omits `view.shown` (the
//! denormalized per-row array) but keeps every other view metadata
//! key — `spec`, `eligible_count`, `truncated`, `shown_summary` —
//! so consumers retain full scope context. The flag is opt-in;
//! default behavior is unchanged.

use std::path::Path;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

fn setup_dir(dir: &Path, src_content: &str, lcov_content: &str) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(src.join("lib.rs"), src_content).expect("write lib.rs fixture");
    std::fs::write(dir.join("lcov.info"), lcov_content).expect("write lcov.info fixture");
}

fn run(dir: &Path, extra_args: &[&str]) -> std::process::Output {
    Command::new(BINARY)
        .current_dir(dir)
        .args(["--coverage", "lcov.info", "--src", "src"])
        .args(extra_args)
        .output()
        .expect("failed to run crap4rs binary")
}

fn parse_json(output: &std::process::Output) -> serde_json::Value {
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{out}"))
}

/// 6-function fixture: 3 simple/covered, 3 branchy/uncovered.
const FIXTURE_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
pub fn failing_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_c(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const FIXTURE_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
DA:4,0
DA:5,0
DA:6,0
end_of_record
";

#[test]
fn minimal_view_omits_shown_array() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--minimal-view",
            "--format",
            "json",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "--no-fail forces exit 0");

    let json = parse_json(&output);
    let view = json["view"]
        .as_object()
        .expect("view must be a JSON object even under --minimal-view");
    assert!(
        !view.contains_key("shown"),
        "--minimal-view must omit the `shown` array; keys: {:?}",
        view.keys().collect::<Vec<_>>()
    );
}

#[test]
fn minimal_view_preserves_scope_context() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--minimal-view",
            "--format",
            "json",
        ],
    );
    let json = parse_json(&output);
    let view = json["view"].as_object().expect("view object");

    for key in ["spec", "eligible_count", "truncated", "shown_summary"] {
        assert!(
            view.contains_key(key),
            "--minimal-view must preserve `{key}` so consumers keep scope context; \
             actual keys: {:?}",
            view.keys().collect::<Vec<_>>()
        );
    }

    // result.passed must remain truthful — gate is unshapeable.
    assert_eq!(
        json["result"]["passed"].as_bool(),
        Some(false),
        "result.passed must remain false (gate is unshapeable)"
    );
}

#[test]
fn default_invocation_includes_shown() {
    // Regression: without --minimal-view, the envelope still carries
    // `view.shown` exactly as before. This guards against accidental
    // tightening of the default JSON shape.
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "json",
        ],
    );
    let json = parse_json(&output);
    let shown = json["view"]["shown"]
        .as_array()
        .expect("default invocation must include `view.shown` as an array");
    assert!(
        !shown.is_empty(),
        "fixture has 6 functions; default `view.shown` should not be empty"
    );
}
