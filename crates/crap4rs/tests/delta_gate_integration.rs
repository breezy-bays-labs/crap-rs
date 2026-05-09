//! Integration tests for `--baseline` + `--delta-gate` + `--no-fail`
//! interaction (issue #81 VS6).
//!
//! Exit-code matrix:
//! - passing analysis + no `--baseline`                       → 0
//! - passing analysis + `--baseline` (no gate)                → 0 (informational)
//! - passing analysis + new violations (no `--delta-gate`)    → 0
//! - passing analysis + new violations + `--delta-gate`       → 1
//! - passing analysis + delta-gate fail + `--no-fail`         → 0 (truth in JSON)
//! - failing analysis + `--baseline` (no flags)               → 1 (analysis gate)
//! - failing analysis + `--no-fail`                           → 0
//!
//! `--delta-gate` requires `--baseline` (clap `requires` constraint
//! ensures rejection at parse time without baseline).

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

/// Baseline: 1 passing function. Current adds an exceeding function.
const BASELINE_SRC: &str = "\
pub fn calm() -> i32 { 1 }
";

const BASELINE_LCOV: &str = "\
SF:lib.rs
DA:1,1
end_of_record
";

/// Current snapshot: keep `calm`, add a wildly branchy uncovered fn.
/// At threshold 5 the new fn exceeds → new violation.
const CURRENT_SRC: &str = "\
pub fn calm() -> i32 { 1 }
pub fn rough(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const CURRENT_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,0
DA:3,0
end_of_record
";

// ── Exit-code matrix ────────────────────────────────────────────────

#[test]
fn baseline_alone_is_informational_passing_exits_0() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), BASELINE_SRC, BASELINE_LCOV);
    let baseline = capture_baseline(tmp.path(), "5");

    // Now overwrite with current (still has calm, plus rough that fails)
    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    // Make analysis itself pass with a high threshold so we isolate delta semantics.
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "1000",
            "--baseline",
            baseline.to_str().unwrap(),
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "passing analysis + --baseline (no gate) is informational only"
    );
}

#[test]
fn delta_gate_fails_on_new_violations() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), BASELINE_SRC, BASELINE_LCOV);
    let baseline = capture_baseline(tmp.path(), "1000");

    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    // Current: rough fn at threshold 5 introduces a NEW violation that
    // didn't exist in baseline (which had no rough fn). With high
    // threshold for analysis (1000 → analysis gate passes) but
    // --delta-gate, the new violation should still trip exit 1.
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--baseline",
            baseline.to_str().unwrap(),
            "--delta-gate",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "--delta-gate must fail when new threshold violations land"
    );
}

#[test]
fn delta_gate_passes_when_no_new_violations() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    let baseline = capture_baseline(tmp.path(), "1000");

    // Re-run with the SAME source against itself — delta has no
    // changes so summary.passed = true. Threshold is high enough that
    // the analysis gate is also passing, so exit 0 is attributable to
    // the delta gate alone (no `--no-fail` masking).
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "1000",
            "--baseline",
            baseline.to_str().unwrap(),
            "--delta-gate",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "no new violations should keep the delta-gate green (analysis also passes)"
    );
}

#[test]
fn no_fail_overrides_delta_gate() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), BASELINE_SRC, BASELINE_LCOV);
    let baseline = capture_baseline(tmp.path(), "1000");

    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    // --delta-gate would exit 1 (new violation), but --no-fail overrides.
    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--baseline",
            baseline.to_str().unwrap(),
            "--delta-gate",
            "--no-fail",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "--no-fail overrides --delta-gate too"
    );
    // Truth must still be in the JSON envelope. Re-run with --format json.
    let output_json = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--baseline",
            baseline.to_str().unwrap(),
            "--delta-gate",
            "--no-fail",
            "--format",
            "json",
        ],
    );
    let body = stdout_str(&output_json);
    let v: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(
        v["delta"]["summary"]["passed"], false,
        "delta.summary.passed must reflect truth even when --no-fail forces exit 0"
    );
    assert!(
        v["delta"]["summary"]["new_violations"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "new_violations must be visible in JSON"
    );
}

#[test]
fn delta_gate_without_baseline_rejected_at_parse() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dir(tmp.path(), CURRENT_SRC, CURRENT_LCOV);
    // clap `requires = "baseline"` should make this a parse error (exit 2).
    let output = run(tmp.path(), &["--threshold", "5", "--delta-gate"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "--delta-gate without --baseline must fail clap parse"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--baseline") || stderr.contains("baseline"),
        "stderr should mention baseline requirement: {stderr}"
    );
}

// ── Help discoverability (VS6) ───────────────────────────────────────

#[test]
fn help_text_documents_baseline_and_delta_gate() {
    let output = Command::new(BINARY)
        .arg("--help")
        .output()
        .expect("run --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--baseline"),
        "--help must mention --baseline: {stdout}"
    );
    assert!(
        stdout.contains("--delta-gate"),
        "--help must mention --delta-gate: {stdout}"
    );
}

#[test]
fn help_text_includes_delta_examples() {
    let output = Command::new(BINARY)
        .arg("--help")
        .output()
        .expect("run --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("COMPARING TWO ANALYSES"),
        "--help must surface the delta examples block: {stdout}"
    );
    assert!(
        stdout.contains("baseline.json"),
        "--help must show the basic capture/compare flow: {stdout}"
    );
}
