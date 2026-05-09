//! Integration tests for `--min-coverage` / `--max-coverage` (issue #63).
//!
//! Wires CLI flags through `cli::view_args::validate_view_args` and
//! `cli::view_args::build_view_spec` into `domain::view::Filters::coverage_range`.
//! `CoverageRange::new` (V1a) is the single source of truth for validity;
//! the CLI translates each domain error variant to flag-attributed prose.

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

/// 6-function fixture mirroring `view_integration::ONLY_FAILING_*`.
/// Lines 1–3 fully covered; lines 4–6 uncovered.
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

// ── Happy path: --min-coverage filters and the JSON shape carries the range ──

#[test]
fn min_coverage_filters_uncovered_functions() {
    // cli_ergonomics.feature:78-81. With --min-coverage 1, the 3 fully-uncovered
    // (cov=0.0) functions must drop out of `view.shown`. The JSON envelope
    // declares the resolved range as `{ "min": 1.0, "max": 100.0 }`.
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
            "--min-coverage",
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

    let shown = v["view"]["shown"].as_array().expect("view.shown array");
    for entry in shown {
        let cov = entry["scored"]["coverage_percent"].as_f64().unwrap();
        assert!(
            cov >= 1.0,
            "every shown function must have coverage >= 1.0; got {cov}"
        );
    }

    let range = &v["view"]["spec"]["filters"]["coverage_range"];
    assert!(!range.is_null(), "coverage_range must be present");
    assert_eq!(range["min"].as_f64(), Some(1.0));
    assert_eq!(range["max"].as_f64(), Some(100.0));
}

#[test]
fn max_coverage_zero_surfaces_only_untested_functions() {
    // cli_ergonomics.feature:83-86. With --max-coverage 0, only fully-uncovered
    // (cov=0.0) functions remain. JSON resolved range is `{ "min": 0, "max": 0 }`.
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
            "0",
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
    for entry in shown {
        let cov = entry["scored"]["coverage_percent"].as_f64().unwrap();
        assert_eq!(cov, 0.0, "--max-coverage 0 must keep only cov=0 rows");
    }

    let range = &v["view"]["spec"]["filters"]["coverage_range"];
    assert_eq!(range["min"].as_f64(), Some(0.0));
    assert_eq!(range["max"].as_f64(), Some(0.0));
}

#[test]
fn combining_min_and_max_targets_partial_coverage() {
    // cli_ergonomics.feature:92-95. Both bounds explicit.
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
            "--min-coverage",
            "1",
            "--max-coverage",
            "90",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    let range = &v["view"]["spec"]["filters"]["coverage_range"];
    assert_eq!(range["min"].as_f64(), Some(1.0));
    assert_eq!(range["max"].as_f64(), Some(90.0));
}

#[test]
fn no_coverage_flags_leaves_range_null() {
    // cli_ergonomics.feature:204-205 — view.filters.coverage_range is null
    // on default invocation (no bound passed).
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &["--threshold", "5", "--format", "json", "--no-gitignore"],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "default invocation should not error: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    assert!(
        v["view"]["spec"]["filters"]["coverage_range"].is_null(),
        "default invocation must leave coverage_range null"
    );
}

// ── Validation errors → exit 2 with flag-attributed prose ──

#[test]
fn min_out_of_range_negative_exits_2() {
    // cli_ergonomics.feature:104.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(dir.path(), &["--no-gitignore", "--min-coverage", "-5"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("--min-coverage must be in [0, 100]"),
        "expected `--min-coverage must be in [0, 100]` in stderr; got:\n{stderr}"
    );
}

#[test]
fn max_out_of_range_above_100_exits_2() {
    // cli_ergonomics.feature:105.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(dir.path(), &["--no-gitignore", "--max-coverage", "105"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("--max-coverage must be in [0, 100]"),
        "expected `--max-coverage must be in [0, 100]` in stderr; got:\n{stderr}"
    );
}

#[test]
fn min_exceeds_max_exits_2() {
    // cli_ergonomics.feature:106.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &[
            "--no-gitignore",
            "--min-coverage",
            "90",
            "--max-coverage",
            "30",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("--min-coverage must not exceed --max-coverage"),
        "expected `--min-coverage must not exceed --max-coverage` in stderr; got:\n{stderr}"
    );
}

// ── Filter hiding violations does NOT change the gate (exit code) ──

#[test]
fn filter_hiding_violations_still_exits_1() {
    // cli_ergonomics.feature:108-111 — keystone: gate is unshapeable.
    // --min-coverage 99 hides every failing row from the view, yet
    // result.passed (the gate) reflects the unfiltered analysis.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &["--threshold", "5", "--no-gitignore", "--min-coverage", "99"],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "filter hiding violations must NOT flip the gate to exit 0; stderr:\n{}",
        stderr_str(&output)
    );
}

// ── --only-failing composes with --min-coverage as AND (W2 composition test) ──

#[test]
fn only_failing_composes_with_min_coverage_as_and() {
    // cli_ergonomics.feature:195-197. Composes V1b's --only-failing with
    // V2.2's --min-coverage. Every shown row must satisfy BOTH predicates.
    // Fixture: 3 passing (CRAP < 5, fully covered) and 3 failing (uncovered).
    // --only-failing --min-coverage 50 should produce zero shown rows
    // (failing rows have cov=0; passing rows are excluded by only_failing).
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
            "--only-failing",
            "--min-coverage",
            "50",
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
    for entry in shown {
        let exceeds = entry["exceeds"].as_bool().unwrap();
        let cov = entry["scored"]["coverage_percent"].as_f64().unwrap();
        assert!(exceeds, "every shown row must exceed (only_failing)");
        assert!(
            cov >= 50.0,
            "every shown row must have cov >= 50; got {cov}"
        );
    }
    // Both filter spec fields are reflected.
    assert_eq!(v["view"]["spec"]["filters"]["only_failing"], true);
    let range = &v["view"]["spec"]["filters"]["coverage_range"];
    assert_eq!(range["min"].as_f64(), Some(50.0));
    assert_eq!(range["max"].as_f64(), Some(100.0));

    // Underlying gate still reflects threshold violations on the full set.
    assert_eq!(v["result"]["summary"]["exceeding_threshold"], 3);
}
