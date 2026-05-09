//! Integration tests for `--group-by file` (Bundle C, issue #64).
//!
//! Hand-asserted scenarios from `tests/features/group_by_file.feature`.
//! Mirrors the pattern in `tests/cli_minimal_view_integration.rs`.

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

/// Three files, six functions, mixed pass/fail.
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
fn default_invocation_has_null_grouped() {
    let tmp = tempfile::tempdir().expect("tempdir");
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
    assert_eq!(json["view"]["grouped"], serde_json::Value::Null);
    assert_eq!(json["view"]["spec"]["group_by"], serde_json::Value::Null);
}

#[test]
fn group_by_file_populates_view_grouped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--group-by",
            "file",
            "--format",
            "json",
        ],
    );
    let json = parse_json(&output);
    let grouped = json["view"]["grouped"]
        .as_object()
        .expect("view.grouped object expected under --group-by file");
    assert_eq!(grouped["key"], "file");
    let files = grouped["files"]
        .as_array()
        .expect("view.grouped.files array");
    // Single source file in this fixture (lib.rs).
    assert_eq!(files.len(), 1);
    let f0 = &files[0];
    for key in [
        "file_path",
        "function_count",
        "exceeding_count",
        "average_crap",
        "max_crap",
        "worst_function",
        "distribution",
        "average_coverage",
        "max_complexity",
    ] {
        assert!(
            f0.get(key).is_some(),
            "FileSummary missing key `{key}`: {f0}"
        );
    }
}

#[test]
fn group_by_file_keeps_shown_full_under_top() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--group-by",
            "file",
            "--top",
            "0",
            "--format",
            "json",
        ],
    );
    let json = parse_json(&output);
    let shown = json["view"]["shown"]
        .as_array()
        .expect("view.shown should remain populated under --group-by file");
    assert_eq!(
        shown.len(),
        6,
        "shown should carry the un-truncated eligible function set"
    );
    assert_eq!(json["view"]["truncated"].as_bool(), Some(false));
}

#[test]
fn group_by_file_top_truncates_files_not_functions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);
    // Fixture has only 1 source file; --top 1 doesn't actually truncate.
    // To exercise truncation we'd need multiple files — but at the integration
    // level we just verify the keys exist. Property of file-level truncation
    // is unit-tested in domain::view::tests::group_by_file_truncate_files.
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--group-by",
            "file",
            "--top",
            "1",
            "--format",
            "json",
        ],
    );
    let json = parse_json(&output);
    let grouped = json["view"]["grouped"].as_object().expect("grouped block");
    assert_eq!(grouped["files"].as_array().unwrap().len(), 1);
    // truncated key always present
    assert!(grouped.contains_key("truncated"));
    assert!(grouped.contains_key("eligible_count"));
}

#[test]
fn group_by_file_csv_schema_shifts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--group-by",
            "file",
            "--format",
            "csv",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().expect("at least one CSV line");
    assert_eq!(
        first_line,
        "file,function_count,exceeding_count,average_crap,max_crap,worst_function,distribution_low,distribution_acceptable,distribution_moderate,distribution_high",
        "CSV header should shift to per-file 10 columns"
    );
    // Per-function header MUST NOT appear
    assert!(
        !stdout.contains("complexity_metric"),
        "per-function header leaked into grouped CSV: {stdout}"
    );
}

#[test]
fn minimal_view_with_group_by_file_strips_shown_keeps_grouped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--group-by",
            "file",
            "--minimal-view",
            "--format",
            "json",
        ],
    );
    let json = parse_json(&output);
    let view = json["view"].as_object().expect("view object");
    assert!(
        !view.contains_key("shown"),
        "--minimal-view must strip view.shown even under --group-by file"
    );
    assert!(
        view.contains_key("grouped") && !view["grouped"].is_null(),
        "view.grouped must remain populated under --minimal-view"
    );
    let grouped = view["grouped"].as_object().unwrap();
    assert!(grouped["files"].is_array());
}

#[test]
fn group_by_file_does_not_change_exit_code_on_failure() {
    // Fixture has 3 high-CRAP functions exceeding threshold 5 → exit 1
    // without --no-fail, regardless of --group-by file.
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--group-by",
            "file",
            "--format",
            "json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "--group-by file must not change exit code (gate keystone)"
    );
    let json = parse_json(&output);
    assert_eq!(
        json["result"]["passed"].as_bool(),
        Some(false),
        "result.passed unchanged by grouping"
    );
}

#[test]
fn help_text_describes_group_by_semantic_shift() {
    let output = Command::new(BINARY)
        .arg("--help")
        .output()
        .expect("--help should run");
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("--group-by"),
        "--help must mention --group-by"
    );
    // Help text describes the file-level semantic shift for --top / --sort-by.
    assert!(
        help.contains("top N **files**") || help.contains("top N files"),
        "--help must call out that --top truncates files under --group-by: \n{help}"
    );
}
