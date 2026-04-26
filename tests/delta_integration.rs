//! End-to-end integration tests for `--baseline` (issue #81 VS7).
//!
//! Capture a baseline JSON envelope, mutate the source / coverage,
//! re-run with `--baseline <path>`, and assert that each reporter
//! format surfaces the delta as designed:
//!
//! - JSON envelope: additive `delta` block with summary + shown rows
//! - Table: "Delta vs baseline:" section under the analysis table
//! - Markdown: `## CRAP Scorecard` section with regression / new-violation tables
//! - CSV: row-per-change schema (`change_kind` column), per-function schema gone
//!
//! Also covers two correctness invariants:
//! - line-range shift does not disrupt identity matching
//! - schema_version mismatch on baseline file fails fast with exit 2

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
        .args(["--coverage", "lcov.info", "--src", "src", "--no-gitignore"])
        .args(extra_args)
        .output()
        .expect("failed to run crap4rs binary")
}

fn stdout_str(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn capture_baseline(dir: &Path, threshold: &str) -> std::path::PathBuf {
    let baseline_path = dir.join("baseline.json");
    let output = Command::new(BINARY)
        .current_dir(dir)
        .args([
            "--coverage",
            "lcov.info",
            "--src",
            "src",
            "--no-gitignore",
            "--format",
            "json",
            "--threshold",
            threshold,
            "--no-fail",
        ])
        .output()
        .expect("failed to run crap4rs to capture baseline");
    std::fs::write(&baseline_path, &output.stdout).expect("write baseline.json");
    baseline_path
}

// ── Fixtures ────────────────────────────────────────────────────────

const BASELINE_SRC: &str = "\
pub fn first() -> i32 { 1 }
pub fn second(x: i32) -> i32 { if x > 0 { 1 } else { 2 } }
";

const BASELINE_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
end_of_record
";

/// Current source: `first` removed, `second` modified (line shift +
/// new branch), `third` added.
const CURRENT_SRC: &str = "\
// extra leading line to shift line numbers
pub fn second(x: i32) -> i32 {
    if x > 0 {
        if x > 5 { 1 } else { 2 }
    } else {
        3
    }
}
pub fn third() -> i32 { 42 }
";

const CURRENT_LCOV: &str = "\
SF:lib.rs
DA:2,0
DA:3,0
DA:4,0
DA:5,0
DA:6,0
DA:7,0
DA:8,0
DA:9,1
end_of_record
";

// ── Capture / compare happy path ────────────────────────────────────

#[test]
fn json_envelope_carries_delta_block_with_summary_and_shown() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), BASELINE_SRC, BASELINE_LCOV);
    let baseline = capture_baseline(tmp.path(), "5");

    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--baseline",
            baseline.to_str().unwrap(),
            "--format",
            "json",
            "--no-fail",
        ],
    );
    let body = stdout_str(&output);
    let v: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\nraw:\n{body}"));

    let delta = &v["delta"];
    assert!(delta.is_object(), "delta block missing");
    let summary = &delta["summary"];

    // first: removed; second: modified (line shift + new branch); third: added
    assert_eq!(summary["removed"].as_u64(), Some(1));
    assert_eq!(summary["added"].as_u64(), Some(1));
    assert_eq!(summary["modified"].as_u64(), Some(1));

    // delta.shown contains all three changes
    let shown = delta["shown"].as_array().expect("delta.shown is array");
    assert_eq!(shown.len(), 3, "expected 3 changes, got {}", shown.len());

    // baseline metadata propagated
    assert!(delta["baseline_tool_version"].is_string());
    assert!(delta["baseline_timestamp"].is_string());
}

#[test]
fn table_format_renders_delta_block_under_analysis() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), BASELINE_SRC, BASELINE_LCOV);
    let baseline = capture_baseline(tmp.path(), "1000");

    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "1000",
            "--baseline",
            baseline.to_str().unwrap(),
            "--color",
            "never",
        ],
    );
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("Delta vs baseline:"),
        "table missing delta header: {stdout}"
    );
    // Per-change content present
    assert!(
        stdout.contains("removed") && stdout.contains("added") && stdout.contains("modified"),
        "delta rows missing kind labels: {stdout}"
    );
}

#[test]
fn markdown_format_renders_scorecard_section() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), BASELINE_SRC, BASELINE_LCOV);
    let baseline = capture_baseline(tmp.path(), "1000");

    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--baseline",
            baseline.to_str().unwrap(),
            "--format",
            "markdown",
            "--no-fail",
        ],
    );
    let stdout = stdout_str(&output);
    assert!(stdout.contains("## CRAP Scorecard"));
    assert!(stdout.contains("- **Delta status:**"));
    assert!(stdout.contains("- **Changes:**"));
}

#[test]
fn csv_format_mode_switches_to_row_per_change() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), BASELINE_SRC, BASELINE_LCOV);
    let baseline = capture_baseline(tmp.path(), "1000");

    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--baseline",
            baseline.to_str().unwrap(),
            "--format",
            "csv",
            "--no-fail",
        ],
    );
    let stdout = stdout_str(&output);
    let header = stdout.lines().next().expect("csv header");
    assert!(
        header.starts_with("change_kind,"),
        "csv header should start with change_kind: {header}"
    );
    // Per-function header MUST NOT appear
    assert!(
        !stdout.contains("exceeds_threshold"),
        "per-function csv header leaked under --baseline: {stdout}"
    );
}

// ── Identity matching survives line shifts ──────────────────────────

#[test]
fn modified_function_matches_across_line_shift() {
    // BASELINE_SRC has `second` at line 2; CURRENT_SRC shifts it to
    // line 2 inside a multi-line body. Identity = (file, qualified_name)
    // means the match should still classify as Modified, not
    // Added + Removed.
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), BASELINE_SRC, BASELINE_LCOV);
    let baseline = capture_baseline(tmp.path(), "1000");

    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "1000",
            "--baseline",
            baseline.to_str().unwrap(),
            "--format",
            "json",
            "--no-fail",
        ],
    );
    let body = stdout_str(&output);
    let v: serde_json::Value = serde_json::from_str(&body).expect("valid JSON for line-shift test");

    let shown = v["delta"]["shown"]
        .as_array()
        .expect("delta.shown is array");
    let second_change = shown
        .iter()
        .find(|c| c["current"]["scored"]["identity"]["qualified_name"] == "second")
        .or_else(|| {
            shown
                .iter()
                .find(|c| c["baseline"]["scored"]["identity"]["qualified_name"] == "second")
        })
        .expect("second function present in changes");
    assert_eq!(
        second_change["kind"], "modified",
        "line shift must not promote Modified → Added+Removed: {second_change:?}"
    );
}

// ── Error paths ─────────────────────────────────────────────────────

#[test]
fn baseline_path_not_found_exits_2_with_actionable_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--baseline",
            "/tmp/definitely-does-not-exist-xyzzy.json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "missing baseline path must exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found") || stderr.contains("baseline"),
        "stderr should explain missing baseline: {stderr}"
    );
}

#[test]
fn baseline_unsupported_schema_version_exits_2() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    let bad_baseline = tmp.path().join("future.json");
    std::fs::write(
        &bad_baseline,
        r#"{
            "schema_version": 99,
            "result": {
                "functions": [],
                "summary": {
                    "total_functions": 0, "total_files": 0,
                    "exceeding_threshold": 0,
                    "average_crap": 0.0, "median_crap": 0.0,
                    "max_crap": null, "worst_function": null,
                    "distribution": {"low":0,"acceptable":0,"moderate":0,"high":0}
                },
                "passed": true
            }
        }"#,
    )
    .expect("write bad baseline");

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--baseline",
            bad_baseline.to_str().unwrap(),
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "schema_version mismatch must exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("schema_version") || stderr.contains("schema"),
        "stderr should explain version mismatch: {stderr}"
    );
}
