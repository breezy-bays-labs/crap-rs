//! Integration tests for `--verbose` flag CLI wiring.
//!
//! Tests the full pipeline through the binary: that `--verbose` emits
//! `verbose:` lines to stderr, that `warn_if_issues()` always warns on
//! non-fatal issues, and that `--verbose --format json` includes the
//! `diagnostics` field in the JSON envelope.

use std::path::Path;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

// ── Helpers ────────────────────────────────────────────────────────

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

fn stderr_str(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout_str(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "binary exited with status {}: stderr:\n{}",
        output.status,
        stderr_str(output),
    );
}

fn parse_json(output: &std::process::Output) -> serde_json::Value {
    let out = stdout_str(output);
    serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{out}"))
}

const SIMPLE_SRC: &str = "pub fn simple() -> i32 { 1 }\n";
const SIMPLE_LCOV: &str = "SF:lib.rs\nDA:1,1\nend_of_record\n";

// ── Scenario 1: --verbose prints verbose: lines to stderr ──────────

#[test]
fn verbose_stderr_lines_present() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--verbose"]);
    assert_success(&output);
    let err = stderr_str(&output);

    assert!(
        err.contains("verbose: file discovery:"),
        "expected 'verbose: file discovery:' in stderr, got:\n{err}"
    );
    assert!(
        err.contains("verbose: complexity:"),
        "expected 'verbose: complexity:' in stderr, got:\n{err}"
    );
    assert!(
        err.contains("verbose: matching:"),
        "expected 'verbose: matching:' in stderr, got:\n{err}"
    );
}

// ── Scenario 2: without --verbose, no verbose: lines on stderr ─────

#[test]
fn no_verbose_no_verbose_lines() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &[]);
    assert_success(&output);
    let err = stderr_str(&output);

    assert!(
        !err.contains("verbose:"),
        "expected no 'verbose:' lines in stderr without --verbose, got:\n{err}"
    );
}

// ── Scenario 3: warn always; --verbose adds LCOV parse details ─────

#[test]
fn warn_without_verbose_on_lcov_parse_issues() {
    let dir = tempfile::tempdir().unwrap();
    let malformed_lcov = "SF:lib.rs\nDA:1,1\nDA:bad_line\nend_of_record\n";
    setup_dir(dir.path(), SIMPLE_SRC, malformed_lcov);

    let output = run(dir.path(), &[]);
    assert_success(&output);
    let err = stderr_str(&output);

    assert!(
        err.contains("warning:") && err.contains("coverage parse issue"),
        "expected warning about coverage parse issues without --verbose, got:\n{err}"
    );
    assert!(
        !err.contains("verbose:"),
        "expected no 'verbose:' lines without --verbose, got:\n{err}"
    );
}

#[test]
fn verbose_adds_lcov_parse_detail() {
    let dir = tempfile::tempdir().unwrap();
    let malformed_lcov = "SF:lib.rs\nDA:1,1\nDA:bad_line\nend_of_record\n";
    setup_dir(dir.path(), SIMPLE_SRC, malformed_lcov);

    let output = run(dir.path(), &["--verbose"]);
    assert_success(&output);
    let err = stderr_str(&output);

    assert!(
        err.contains("warning:"),
        "expected warning line with --verbose too, got:\n{err}"
    );
    assert!(
        err.contains("verbose: coverage parse diagnostics"),
        "expected 'verbose: coverage parse diagnostics' with --verbose, got:\n{err}"
    );
}

// ── Scenario 3b: warn always; unparseable source files ────────────

#[test]
fn warn_on_unparseable_source_files() {
    let dir = tempfile::tempdir().unwrap();

    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);
    // Add a second .rs file with syntax that syn cannot parse
    std::fs::write(dir.path().join("src/broken.rs"), "this is not rust {{{")
        .expect("write broken.rs");

    let output = run(dir.path(), &[]);
    assert_success(&output);
    let err = stderr_str(&output);

    assert!(
        err.contains("warning:") && err.contains("source file(s) could not be parsed"),
        "expected warning about unparseable source files, got:\n{err}"
    );
    assert!(
        !err.contains("verbose:"),
        "expected no 'verbose:' lines without --verbose, got:\n{err}"
    );
}

// ── Scenario 4: --verbose --format json includes diagnostics field ─

#[test]
fn verbose_json_includes_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--verbose", "--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);

    assert!(
        v.get("diagnostics").is_some() && !v["diagnostics"].is_null(),
        "expected non-null 'diagnostics' key in JSON with --verbose, got:\n{}",
        stdout_str(&output),
    );
}

// ── Scenario 5: --format json without --verbose omits diagnostics ──

#[test]
fn no_verbose_json_omits_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);

    assert!(
        v.get("diagnostics").is_none(),
        "expected no 'diagnostics' key in JSON without --verbose, got:\n{}",
        stdout_str(&output),
    );
}

// ── Scenario 5b: --verbose --format json with parse issues populates diagnostics ─

#[test]
fn verbose_json_parse_issues_in_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let malformed_lcov = "SF:lib.rs\nDA:1,1\nDA:bad_line\nend_of_record\n";
    setup_dir(dir.path(), SIMPLE_SRC, malformed_lcov);

    let output = run(dir.path(), &["--verbose", "--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);

    let parse_diags = v["diagnostics"]["parse_diagnostics"]
        .as_array()
        .expect("parse_diagnostics should be an array");
    assert!(
        !parse_diags.is_empty(),
        "expected at least one parse diagnostic in JSON, got:\n{}",
        stdout_str(&output),
    );
}

// ── Scenario 6: verbose function count matches source ──────────────

#[test]
fn verbose_counts_match_source() {
    let dir = tempfile::tempdir().unwrap();

    let two_fn_src = "pub fn alpha() -> i32 { 1 }\npub fn beta() -> i32 { 2 }\n";
    let two_fn_lcov = "SF:lib.rs\nDA:1,1\nDA:2,1\nend_of_record\n";
    setup_dir(dir.path(), two_fn_src, two_fn_lcov);

    let output = run(dir.path(), &["--verbose"]);
    assert_success(&output);
    let err = stderr_str(&output);

    assert!(
        err.contains("verbose: complexity: 2 functions extracted"),
        "expected 'verbose: complexity: 2 functions extracted' in stderr, got:\n{err}"
    );
    assert!(
        err.contains("verbose: file discovery: 1 files found, 0 unparseable"),
        "expected 'verbose: file discovery: 1 files found, 0 unparseable' in stderr, got:\n{err}"
    );
}

// ── Scenario 7: --verbose --quiet suppresses stdout, not stderr ────

#[test]
fn verbose_quiet_suppresses_stdout_not_stderr() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--verbose", "--quiet"]);
    assert_success(&output);
    let err = stderr_str(&output);
    let out = stdout_str(&output);

    assert!(
        err.contains("verbose: file discovery:"),
        "expected verbose: lines on stderr with --verbose --quiet, got:\n{err}"
    );
    assert!(
        out.is_empty(),
        "expected empty stdout with --quiet, got:\n{out}"
    );
}

// ── Scenario 8: functions without LCOV coverage reflected in count ─

#[test]
fn verbose_no_coverage_count() {
    let dir = tempfile::tempdir().unwrap();

    // Two functions in source, LCOV covers a different file entirely
    let two_fn_src = "pub fn alpha() -> i32 { 1 }\npub fn beta() -> i32 { 2 }\n";
    let unmatched_lcov = "SF:other.rs\nDA:1,1\nend_of_record\n";
    setup_dir(dir.path(), two_fn_src, unmatched_lcov);

    let output = run(dir.path(), &["--verbose"]);
    assert_success(&output);
    let err = stderr_str(&output);

    assert!(
        err.contains("verbose: matching: 0 matched with coverage, 2 without coverage data"),
        "expected '0 matched with coverage, 2 without coverage data' in stderr, got:\n{err}"
    );
}
