//! Integration tests for `--no-fail` (issue #65).
//!
//! `--no-fail` is a CLI-only exit-code override: it forces the process to
//! exit `0` regardless of whether the underlying analysis passed. The
//! flag does NOT touch the View pipeline or the result block — JSON
//! consumers can still observe `result.passed == false` and react
//! accordingly. `--no-fail` composes with every shaping flag (filters,
//! sort, truncate) and with `--quiet` (silent success).

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

fn stdout_str(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn parse_json(output: &std::process::Output) -> serde_json::Value {
    let out = stdout_str(output);
    serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{out}"))
}

/// 6-function fixture: 3 simple/covered (passing), 3 branchy/uncovered
/// (failing). At threshold 5, the three uncovered branchy functions
/// exceed the threshold → `result.passed == false` → without `--no-fail`,
/// exit 1; with `--no-fail`, exit 0 (but `result.passed` stays false).
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

// ── Core --no-fail behavior ──────────────────────────────────────────

#[test]
fn no_fail_overrides_violation_to_exit_0() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    // Sanity: the same fixture without --no-fail should exit 1.
    let baseline = run(tmp.path(), &["--threshold", "5", "--no-gitignore"]);
    assert_eq!(
        baseline.status.code(),
        Some(1),
        "baseline (no --no-fail) must exit 1 on violations"
    );

    let output = run(
        tmp.path(),
        &["--threshold", "5", "--no-gitignore", "--no-fail"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-fail must exit 0 even when violations exist"
    );
}

#[test]
fn no_fail_is_no_op_when_clean() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    // Threshold 1000 — all functions pass; result.passed == true.
    let output = run(
        tmp.path(),
        &["--threshold", "1000", "--no-gitignore", "--no-fail"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-fail on a passing project still exits 0 (no-op)"
    );
}

#[test]
fn violations_without_no_fail_still_exit_1() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(tmp.path(), &["--threshold", "5", "--no-gitignore"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "without --no-fail, threshold violations must still exit 1"
    );
}

// ── --quiet composition ──────────────────────────────────────────────

#[test]
fn quiet_alone_preserves_exit_1_on_violations() {
    // Owns cli_ergonomics.feature:157 regression — `--quiet` alone must
    // NOT swallow the failing exit code; only `--no-fail` does that.
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &["--threshold", "5", "--no-gitignore", "--quiet"],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "--quiet alone must preserve CI exit-1 semantics"
    );
    assert!(
        stdout_str(&output).is_empty(),
        "--quiet must suppress stdout entirely"
    );
}

#[test]
fn quiet_no_fail_silent_success() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &["--threshold", "5", "--no-gitignore", "--quiet", "--no-fail"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "--quiet --no-fail composes to silent success"
    );
    assert!(
        stdout_str(&output).is_empty(),
        "--quiet must suppress stdout even when --no-fail is set"
    );
}

#[test]
fn quiet_with_format_json_suppresses_output() {
    // cli_ergonomics.feature:167 — `--quiet` also suppresses JSON output.
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--quiet",
            "--format",
            "json",
        ],
    );
    assert!(
        stdout_str(&output).is_empty(),
        "--quiet must suppress JSON stdout too"
    );
}

// ── result.passed truthfulness ───────────────────────────────────────

#[test]
fn result_passed_unchanged_under_no_fail() {
    // The keystone: the gate is unshapeable. JSON consumers must still
    // see `result.passed == false` so they can detect "would have failed"
    // even when the process exits 0 due to --no-fail.
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
    assert_eq!(output.status.code(), Some(0), "--no-fail forces exit 0");
    let json = parse_json(&output);
    assert_eq!(
        json["result"]["passed"].as_bool(),
        Some(false),
        "result.passed must remain false even when --no-fail forces exit 0"
    );
}

// ── Story B composition (cli_ergonomics.feature:284) ─────────────────

#[test]
fn story_b_composition_test() {
    // The investigator's full flag-set: filter + sort + truncate +
    // gate-override. Verifies that the entire W2 quartet composes
    // cleanly. View shows up to 5 violating rows in coverage-ascending
    // order; exit 0 thanks to --no-fail; result.passed stays false.
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--only-failing",
            "--sort-by",
            "coverage",
            "--top",
            "5",
            "--no-fail",
            "--format",
            "json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-fail must override exit code in the composed flag-set"
    );
    let json = parse_json(&output);
    assert_eq!(
        json["result"]["passed"].as_bool(),
        Some(false),
        "result.passed reflects the full analysis, unaffected by --no-fail"
    );

    let shown = json["view"]["shown"]
        .as_array()
        .expect("view.shown must be an array");
    assert!(
        shown.len() <= 5,
        "view.shown must contain at most 5 rows under --top 5"
    );
    assert!(
        !shown.is_empty(),
        "view.shown must contain the violating functions in this fixture"
    );

    // Every row must be a violator (--only-failing) AND ordered by
    // ascending coverage (--sort-by coverage). With this fixture all
    // violators have coverage_percent == 0.0, so the assertion is "all
    // violators present and all coverage values equal 0.0."
    for row in shown {
        assert_eq!(
            row["exceeds"].as_bool(),
            Some(true),
            "every shown row must exceed the threshold under --only-failing"
        );
    }
    let mut prev = f64::NEG_INFINITY;
    for row in shown {
        let cov = row["scored"]["coverage_percent"]
            .as_f64()
            .expect("coverage_percent must be a number");
        assert!(
            cov >= prev,
            "rows must be ordered by coverage ascending: prev={prev}, cur={cov}"
        );
        prev = cov;
    }
}
