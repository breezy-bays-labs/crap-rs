//! Integration tests for `--view` saved view presets (Bundle D, issue #80).
//!
//! Hand-asserted scenarios from `tests/features/saved_view_presets.feature`.
//! Mirrors the pattern in `tests/group_by_file_integration.rs`.

use std::path::Path;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

fn setup_dir(dir: &Path, src_content: &str, lcov_content: &str, toml_content: Option<&str>) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(src.join("lib.rs"), src_content).expect("write lib.rs fixture");
    std::fs::write(dir.join("lcov.info"), lcov_content).expect("write lcov.info fixture");
    if let Some(toml) = toml_content {
        // Canonical config name (crap-rs#345) — writing the legacy
        // `crap4rs.toml` here would still be discovered via fallback but
        // would emit a deprecation warning on every run.
        std::fs::write(dir.join("crap.toml"), toml).expect("write crap.toml fixture");
    }
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

/// 6-function fixture mirroring `group_by_file_integration::FIXTURE_*`
/// for shape comparability: 3 simple/covered, 3 branchy/uncovered.
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

/// Two-preset config used by most resolution scenarios.
const FIXTURE_TOML: &str = "\
[views.ci]
top = 20
min_coverage = 0
max_coverage = 90
sort = \"coverage\"
only_failing = true
no_fail = false
group_by = \"file\"
minimal_view = true

[views.investigate]
sort = \"complexity\"
top = 10
";

// ── Resolution ─────────────────────────────────────────────────────

#[test]
fn view_ci_applies_every_preset_field() {
    // saved_view_presets.feature:35-43 (resolution).
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV, Some(FIXTURE_TOML));

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--view",
            "ci",
            "--format",
            "json",
        ],
    );
    assert!(
        output.status.success(),
        "expected exit 0 (preset applied + --no-fail), got {:?}\nstderr:\n{}",
        output.status,
        stderr_str(&output)
    );
    let json = parse_json(&output);
    assert_eq!(json["view"]["spec"]["limit"], 20);
    assert_eq!(
        json["view"]["spec"]["filters"]["coverage_range"]["min"],
        0.0
    );
    assert_eq!(
        json["view"]["spec"]["filters"]["coverage_range"]["max"],
        90.0
    );
    assert_eq!(json["view"]["spec"]["sort"], "coverage");
    assert_eq!(json["view"]["spec"]["filters"]["only_failing"], true);
    assert_eq!(json["view"]["spec"]["group_by"], "file");
    // minimal_view = true in preset → view.shown elided from JSON envelope.
    assert!(
        json["view"].get("shown").is_none(),
        "minimal_view preset must elide view.shown, got: {}",
        json["view"]
    );
}

// ── Override priority ──────────────────────────────────────────────

#[test]
fn cli_top_overrides_preset_top() {
    // saved_view_presets.feature:48-50 (override).
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV, Some(FIXTURE_TOML));

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--view",
            "ci",
            "--top",
            "5",
            "--format",
            "json",
        ],
    );
    let json = parse_json(&output);
    assert_eq!(
        json["view"]["spec"]["limit"], 5,
        "CLI --top 5 must override preset's top=20"
    );
    // Preset's other fields still applied.
    assert_eq!(json["view"]["spec"]["filters"]["only_failing"], true);
}

#[test]
fn cli_no_fail_or_merges_with_preset() {
    // saved_view_presets.feature:53-55. Preset has `no_fail = false`; CLI adds
    // `--no-fail`. Process must exit 0 even though violations exist.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV, Some(FIXTURE_TOML));

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--view",
            "ci",
            "--no-fail",
        ],
    );
    assert!(
        output.status.success(),
        "CLI --no-fail must override preset's no_fail=false; stderr:\n{}",
        stderr_str(&output)
    );
}

#[test]
fn investigate_preset_resolves_independently() {
    // saved_view_presets.feature:57-61.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV, Some(FIXTURE_TOML));

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--view",
            "investigate",
            "--format",
            "json",
        ],
    );
    let json = parse_json(&output);
    assert_eq!(json["view"]["spec"]["sort"], "complexity");
    assert_eq!(json["view"]["spec"]["limit"], 10);
    assert_eq!(
        json["view"]["spec"]["filters"]["only_failing"], false,
        "preset `investigate` does not assert only_failing"
    );
}

// ── Validation errors ──────────────────────────────────────────────

#[test]
fn unknown_preset_exits_2_with_available_list() {
    // saved_view_presets.feature:65-71.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV, Some(FIXTURE_TOML));

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--view",
            "nonsense",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit 2 for unknown preset; stderr:\n{}",
        stderr_str(&output)
    );
    let stderr = stderr_str(&output);
    assert!(stderr.contains("unknown view preset"), "stderr:\n{stderr}");
    assert!(stderr.contains("ci"), "must list `ci`: {stderr}");
    assert!(
        stderr.contains("investigate"),
        "must list `investigate`: {stderr}"
    );
}

#[test]
fn view_with_no_config_file_exits_2() {
    // saved_view_presets.feature:73-78. No config file in the directory.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV, None);

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--view",
            "ci",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit 2 when --view used without config; stderr:\n{}",
        stderr_str(&output)
    );
    let stderr = stderr_str(&output);
    assert!(stderr.contains("unknown view preset"));
    assert!(
        stderr.contains("crap.toml"),
        "should mention adapter config file: {stderr}"
    );
}

#[test]
fn invalid_preset_field_fails_at_config_load() {
    // saved_view_presets.feature:80-87. max_coverage = 105 is rejected at
    // parse time, before any CLI resolution.
    let dir = tempfile::tempdir().unwrap();
    let bad_toml = "[views.bad]\nmax_coverage = 105\n";
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV, Some(bad_toml));

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--view",
            "bad",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(stderr.contains("out of range"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("bad"),
        "error must name preset `bad`: {stderr}"
    );
}

// ── Gate keystone ──────────────────────────────────────────────────

#[test]
fn preset_does_not_change_exit_code_on_failing_analysis() {
    // saved_view_presets.feature:91-95. Preset `ci` has no_fail = false. The
    // unfiltered analysis exceeds threshold (3 failing functions); exit code
    // must be 1, and `result.passed` must be false.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV, Some(FIXTURE_TOML));

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--view",
            "ci",
            "--format",
            "json",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "preset must NOT override exit code; stderr:\n{}",
        stderr_str(&output)
    );
    let json = parse_json(&output);
    assert_eq!(json["result"]["passed"], false);
}

// ── Discoverability ────────────────────────────────────────────────

#[test]
fn help_advertises_view_flag() {
    // saved_view_presets.feature:99-101.
    let output = Command::new(BINARY)
        .args(["--help"])
        .output()
        .expect("run --help");
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("--view"), "--help must mention --view");
    assert!(
        stdout.contains("preset"),
        "--help must explain the preset concept: {stdout}"
    );
}
