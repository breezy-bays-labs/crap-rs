//! Integration tests for `--top N` (issue #62).
//!
//! Wires CLI flag through `cli::view_args::build_view_spec` into
//! `domain::view::ViewSpec::limit`. `Some(0)` and `None` are treated
//! identically as "no limit" by `domain::view::truncate_to`; the CLI
//! canonicalises `--top 0` to `None` at the boundary so JSON readers
//! see the effective behaviour rather than the user's literal input.

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

fn stderr_str(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn parse_json(output: &std::process::Output) -> serde_json::Value {
    let out = stdout_str(output);
    serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{out}"))
}

/// 6-function fixture: 3 simple/covered, 3 branchy/uncovered.
/// Mirrors `cli_coverage_range_integration::FIXTURE_*` so composition
/// scenarios produce comparable shapes.
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

// ── Happy path: --top truncates and JSON shape carries the limit ──

#[test]
fn top_n_truncates_to_n_rows() {
    // cli_ergonomics.feature:35-40. With 6 eligible functions and --top 3,
    // view.shown collapses to 3 rows, view.truncated is true, and
    // view.eligible_count reports the pre-truncation count.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--format",
            "json",
            "--no-gitignore",
            "--top",
            "3",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    let shown = v["view"]["shown"].as_array().expect("view.shown array");
    assert_eq!(shown.len(), 3, "shown must collapse to top 3");
    assert_eq!(v["view"]["truncated"], true);
    assert_eq!(v["view"]["eligible_count"], 6);
    assert_eq!(v["view"]["spec"]["limit"].as_u64(), Some(3));
}

#[test]
fn top_zero_is_no_limit() {
    // --top 0 means "no limit" — view.shown equals eligible_count and
    // view.truncated is false. JSON view.spec.limit serialises as null
    // because the CLI canonicalises 0 → None.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--format",
            "json",
            "--no-gitignore",
            "--top",
            "0",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "--top 0 must not error: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    let shown = v["view"]["shown"].as_array().expect("view.shown array");
    let eligible = v["view"]["eligible_count"].as_u64().unwrap() as usize;
    assert_eq!(shown.len(), eligible, "--top 0 must not truncate");
    assert_eq!(v["view"]["truncated"], false);
    assert!(
        v["view"]["spec"]["limit"].is_null(),
        "--top 0 canonicalises to spec.limit = null; got {:?}",
        v["view"]["spec"]["limit"]
    );
}

#[test]
fn top_greater_than_eligible_does_not_truncate() {
    // cli_ergonomics.feature:42-45. Limit far above the eligible count
    // is effectively no-op and must NOT mark view.truncated.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--format",
            "json",
            "--no-gitignore",
            "--top",
            "1000000",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    let shown = v["view"]["shown"].as_array().expect("view.shown array");
    let eligible = v["view"]["eligible_count"].as_u64().unwrap() as usize;
    assert_eq!(shown.len(), eligible);
    assert_eq!(v["view"]["truncated"], false);
}

#[test]
fn no_top_flag_leaves_limit_null() {
    // Default invocation echoes spec.limit as null (no truncation).
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &["--threshold", "5", "--format", "json", "--no-gitignore"],
    );
    assert_ne!(output.status.code(), Some(2));
    let v = parse_json(&output);

    assert!(
        v["view"]["spec"]["limit"].is_null(),
        "default invocation must leave spec.limit null; got {:?}",
        v["view"]["spec"]["limit"]
    );
    assert_eq!(v["view"]["truncated"], false);
}

// ── Validation errors → exit 2 with clap's flag-attributed prose ──

#[test]
fn top_negative_exits_2() {
    // cli_ergonomics.feature:72. --top -3 must exit 2 with a clap-attributed
    // value error, not "unexpected argument" — that's what
    // allow_hyphen_values = true protects.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(dir.path(), &["--no-gitignore", "--top", "-3"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("invalid value '-3' for '--top"),
        "expected clap value error attributed to --top; got:\n{stderr}"
    );
}

#[test]
fn top_non_integer_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(dir.path(), &["--no-gitignore", "--top", "3.5"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("invalid value '3.5' for '--top"),
        "expected clap value error attributed to --top; got:\n{stderr}"
    );
}

#[test]
fn top_alpha_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(dir.path(), &["--no-gitignore", "--top", "abc"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("invalid value 'abc' for '--top"),
        "expected clap value error attributed to --top; got:\n{stderr}"
    );
}

// ── Keystone: truncating violations does NOT change the gate ──

#[test]
fn top_truncating_violations_still_exits_1() {
    // cli_ergonomics.feature:328 — gate is unshapeable. The fixture has 3
    // failing functions exceeding threshold 5; --top 1 hides 2 of them
    // from the displayed view, but result.passed (the gate) reflects the
    // full unfiltered analysis.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--format",
            "json",
            "--no-gitignore",
            "--top",
            "1",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "truncating violations must NOT flip the gate to exit 0; stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);
    // Sanity: only one row shown, but the underlying gate counts all 3.
    assert_eq!(v["view"]["shown"].as_array().unwrap().len(), 1);
    assert_eq!(v["view"]["truncated"], true);
    assert_eq!(v["result"]["summary"]["exceeding_threshold"], 3);
}

// ── Composition with --min/--max-coverage (W2 cross-flag check) ──

#[test]
fn top_composes_with_coverage_range() {
    // cli_ergonomics.feature:224-227. Filter first (coverage range), then
    // truncate. The fixture has 3 covered (cov=100) and 3 uncovered (cov=0)
    // functions. `--max-coverage 90` excludes the 3 fully-covered rows;
    // 3 remain eligible, so `--top 1` truncates from 3 to 1.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--format",
            "json",
            "--no-gitignore",
            "--max-coverage",
            "90",
            "--top",
            "1",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    assert_eq!(
        v["view"]["eligible_count"], 3,
        "filter must keep 3 uncovered rows before truncate"
    );
    assert_eq!(
        v["view"]["shown"].as_array().unwrap().len(),
        1,
        "--top 1 must truncate the 3 eligible to 1"
    );
    assert_eq!(v["view"]["truncated"], true);
    assert_eq!(v["view"]["spec"]["limit"].as_u64(), Some(1));
}
