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
    // Use a path under the existing tempdir — portable across
    // platforms (Windows lacks `/tmp`) and race-free (the tempdir is
    // unique to this test).
    let missing = tmp.path().join("does-not-exist.json");
    let output = run(
        tmp.path(),
        &["--threshold", "5", "--baseline", missing.to_str().unwrap()],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "missing baseline path must exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "stderr should specifically explain missing baseline: {stderr}"
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
    // Tight match on the exact error message from
    // `BaselineError::UnsupportedSchemaVersion` so a generic parse
    // error (e.g. malformed JSON happening to mention "schema") can't
    // silently satisfy this test.
    assert!(
        stderr.contains("unsupported baseline schema_version"),
        "stderr should specifically signal version mismatch (not a parse error): {stderr}"
    );
}

// ── Relocation (Renamed) ────────────────────────────────────────────

/// A branchy, fully-uncovered function — CRAP `c² + c`, comfortably over
/// a threshold of 5 on both sides of the delta. Reused byte-identically
/// across files so the relocation pass pairs it as a single change.
const RELOCATED_FN: &str = "\
pub fn process(x: i32) -> i32 {
    if x > 0 {
        if x > 5 { 1 } else { 2 }
    } else {
        3
    }
}
";

/// LCOV marking every line of [`RELOCATED_FN`] uncovered for the given
/// source file, so the function scores identically (and over threshold)
/// wherever it lives.
fn relocated_lcov(file: &str) -> String {
    format!("SF:{file}\nDA:1,0\nDA:2,0\nDA:3,0\nDA:4,0\nDA:5,0\nDA:6,0\nDA:7,0\nend_of_record\n")
}

fn write_single_fn_file(dir: &Path, file_name: &str, src: &str, lcov: &str) {
    let src_dir = dir.join("src");
    std::fs::create_dir_all(&src_dir).expect("create src dir");
    std::fs::write(src_dir.join(file_name), src).expect("write src file");
    std::fs::write(dir.join("lcov.info"), lcov).expect("write lcov.info");
}

/// End-to-end: a function moved to a different file (body byte-identical,
/// over threshold on both sides) is classified as a single `Renamed` —
/// not an unrelated Added + Removed pair — and contributes NO new
/// violation, so the delta gate stays green. This is the headline
/// migration-friendly behavior: relocating an already-complex function
/// does not trip the delta. (The whole-project analysis still flags it —
/// it is genuinely over threshold — but that is the separate analysis
/// gate, not the delta gate.)
#[test]
fn relocated_function_is_renamed_and_adds_no_new_violation() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // Baseline: `process` lives in old_mod.rs, over threshold.
    write_single_fn_file(
        tmp.path(),
        "old_mod.rs",
        RELOCATED_FN,
        &relocated_lcov("old_mod.rs"),
    );
    let baseline = capture_baseline(tmp.path(), "5");

    // Current: identical `process` relocated to new_mod.rs; old file gone.
    std::fs::remove_file(tmp.path().join("src").join("old_mod.rs")).expect("remove old file");
    write_single_fn_file(
        tmp.path(),
        "new_mod.rs",
        RELOCATED_FN,
        &relocated_lcov("new_mod.rs"),
    );

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

    let summary = &v["delta"]["summary"];
    // One relocation, classified as Renamed — never Added + Removed.
    assert_eq!(
        summary["renamed"].as_u64(),
        Some(1),
        "expected exactly one renamed: {summary}"
    );
    assert_eq!(summary["added"].as_u64(), Some(0), "no added: {summary}");
    assert_eq!(
        summary["removed"].as_u64(),
        Some(0),
        "no removed: {summary}"
    );
    // The headline: a pure relocation of an already-failing function
    // introduces no new violation, so the delta gate stays green.
    assert_eq!(
        summary["new_violations"].as_u64(),
        Some(0),
        "relocation must not be a new violation: {summary}"
    );
    assert_eq!(summary["passed"], true, "delta gate stays green: {summary}");

    // The single shown row is a renamed row carrying BOTH sides: the
    // current (post-move) and baseline (pre-move) locations — the
    // old → new audit trail.
    let shown = v["delta"]["shown"]
        .as_array()
        .expect("delta.shown is array");
    let renamed = shown
        .iter()
        .find(|c| c["kind"] == "renamed")
        .unwrap_or_else(|| panic!("a renamed change is present: {shown:?}"));
    assert_eq!(
        renamed["current"]["scored"]["identity"]["file_path"], "new_mod.rs",
        "renamed reports the current location: {renamed}"
    );
    assert_eq!(
        renamed["baseline"]["scored"]["identity"]["file_path"], "old_mod.rs",
        "renamed carries the baseline (pre-move) location: {renamed}"
    );
}

// ── Threshold-border epsilon (#277) ─────────────────────────────────

/// A complexity-4 function. Fully covered it scores CRAP `4.0`; fully
/// uncovered it scores `4² + 4 = 20.0`. Reused byte-identically between
/// baseline and current so the only thing that moves is coverage —
/// driving the CRAP score across a chosen threshold.
const CLASSIFY_FN: &str = "\
pub fn classify(x: i32) -> i32 {
    if x > 0 {
        if x > 5 { 1 } else { 2 }
    } else {
        3
    }
}
";

/// LCOV for [`CLASSIFY_FN`] with every line marked covered (`hits = 1`)
/// or uncovered (`hits = 0`), so the same source scores `4.0` or `20.0`.
fn classify_lcov(file: &str, hits: u32) -> String {
    let mut s = format!("SF:{file}\n");
    for line in 1..=7 {
        s.push_str(&format!("DA:{line},{hits}\n"));
    }
    s.push_str("end_of_record\n");
    s
}

/// End-to-end: a function whose CRAP oscillates across the threshold
/// (4.0 → 20.0 at threshold 12, both within ±10) is treated as
/// threshold-border jitter — it does NOT register as a new violation, so
/// the delta gate (`delta.summary.passed`) stays green and the suppressed
/// count surfaces it. We assert the delta summary (not the process exit
/// code): a real crossing means the current run IS over threshold, so the
/// independent whole-project analysis gate fails regardless — exactly why
/// the #274 relocation test also reads the summary under `--no-fail`.
#[test]
fn border_jitter_crossing_within_epsilon_is_suppressed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_single_fn_file(
        tmp.path(),
        "lib.rs",
        CLASSIFY_FN,
        &classify_lcov("lib.rs", 1),
    );
    let baseline = capture_baseline(tmp.path(), "12");

    // Current: same source, now fully uncovered → CRAP jumps 4.0 → 20.0,
    // crossing threshold 12. With epsilon 10 both readings (4 and 20) sit
    // within ±10 of 12, so the crossing is suppressed.
    write_single_fn_file(
        tmp.path(),
        "lib.rs",
        CLASSIFY_FN,
        &classify_lcov("lib.rs", 0),
    );
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "12",
            "--threshold-epsilon",
            "10",
            "--baseline",
            baseline.to_str().unwrap(),
            "--delta-gate",
            "--format",
            "json",
            "--no-fail",
        ],
    );
    let body = stdout_str(&output);
    let v: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\nraw:\n{body}"));
    let summary = &v["delta"]["summary"];
    assert_eq!(
        summary["new_violations"].as_u64(),
        Some(0),
        "border-jitter crossing must not count as a new violation: {summary}"
    );
    assert_eq!(
        summary["border_jitter_suppressed"].as_u64(),
        Some(1),
        "the suppressed crossing must be surfaced: {summary}"
    );
    assert_eq!(
        summary["passed"], true,
        "delta gate stays green when the only crossing is border jitter: {summary}"
    );
}

/// Negative control: the identical 4.0 → 20.0 crossing with no epsilon
/// (default 0.0) is a genuine new violation — the delta gate goes red.
#[test]
fn threshold_crossing_outside_epsilon_still_counts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_single_fn_file(
        tmp.path(),
        "lib.rs",
        CLASSIFY_FN,
        &classify_lcov("lib.rs", 1),
    );
    let baseline = capture_baseline(tmp.path(), "12");

    write_single_fn_file(
        tmp.path(),
        "lib.rs",
        CLASSIFY_FN,
        &classify_lcov("lib.rs", 0),
    );
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "12",
            "--baseline",
            baseline.to_str().unwrap(),
            "--delta-gate",
            "--format",
            "json",
            "--no-fail",
        ],
    );
    let body = stdout_str(&output);
    let v: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\nraw:\n{body}"));
    let summary = &v["delta"]["summary"];
    assert_eq!(
        summary["new_violations"].as_u64(),
        Some(1),
        "a real crossing with no epsilon is a new violation: {summary}"
    );
    assert_eq!(
        summary["border_jitter_suppressed"].as_u64(),
        Some(0),
        "nothing is suppressed at epsilon 0: {summary}"
    );
    assert_eq!(
        summary["passed"], false,
        "delta gate is red on a genuine new violation: {summary}"
    );
}
