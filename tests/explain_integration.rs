//! Integration tests for `--explain` CLI wiring.

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

fn assert_ran(output: &std::process::Output) {
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "binary exited with status {}: stderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
}

const NESTED_SRC: &str = "\
pub fn nested(x: bool, y: bool) -> i32 {
    if x {
        if y { 1 } else { 2 }
    } else {
        3
    }
}
";

const ZERO_COVERAGE_LCOV: &str = "\
SF:lib.rs
DA:1,0
DA:2,0
DA:3,0
DA:4,0
DA:5,0
DA:6,0
DA:7,0
end_of_record
";

#[test]
fn explain_adds_legend_for_nested_breakdown_output() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), NESTED_SRC, ZERO_COVERAGE_LCOV);

    let output = run(
        dir.path(),
        &["--threshold", "1", "--breakdown", "--explain"],
    );
    assert_ran(&output);
    let out = stdout_str(&output);

    assert!(
        out.contains("line 3: if-branch (+2 (nested))"),
        "stdout:\n{out}"
    );
    assert!(
        out.contains("Legend: +1 = base structural increment."),
        "stdout:\n{out}"
    );
}

#[test]
fn explain_without_breakdown_does_not_change_table_output() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), NESTED_SRC, ZERO_COVERAGE_LCOV);

    let output = run(dir.path(), &["--threshold", "1", "--explain"]);
    assert_ran(&output);
    let out = stdout_str(&output);

    assert!(!out.contains("Legend:"), "stdout:\n{out}");
    assert!(!out.contains("(nested)"), "stdout:\n{out}");
}

#[test]
fn explain_does_not_change_json_output_shape() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), NESTED_SRC, ZERO_COVERAGE_LCOV);

    let output = run(dir.path(), &["--format", "json", "--explain"]);
    assert_ran(&output);
    let v: serde_json::Value = serde_json::from_str(&stdout_str(&output)).unwrap();

    assert!(v.get("tool_version").is_some());
    assert!(v.get("result").is_some());
    assert!(v["result"].get("functions").is_some());
    assert!(v["result"].get("summary").is_some());
    assert!(v.get("diagnostics").is_none());
    assert!(v.get("legend").is_none());
    assert!(
        v["result"]["functions"][0]["scored"]
            .get("contributors")
            .is_some()
    );
}
