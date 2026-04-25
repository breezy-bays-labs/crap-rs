//! Integration tests for the V1a View walking-skeleton wiring through
//! the `crap4rs` binary.
//!
//! V1a is behavior-preserving: every default invocation produces the
//! same shape of output it did before, plus an additive `view` block
//! in the JSON envelope. Wave 1 / Wave 2 will surface CLI flags
//! (`--top`, `--min/max-coverage`, `--sort-by`, `--no-fail`) that this
//! file's scope deliberately does NOT exercise — those land with their
//! own integration test files.

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

// ── Default invocation: JSON envelope carries the additive view block ──

#[test]
fn default_invocation_json_envelope_includes_view_block() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);

    let view = v
        .get("view")
        .expect("envelope must include `view` block in V1a");
    assert!(view.get("spec").is_some(), "view.spec missing");
    assert!(
        view.get("eligible_count").is_some(),
        "view.eligible_count missing"
    );
    assert!(view.get("truncated").is_some(), "view.truncated missing");
    assert!(view.get("shown").is_some(), "view.shown missing");
    assert!(
        view.get("shown_summary").is_some(),
        "view.shown_summary missing"
    );
}

#[test]
fn default_invocation_view_block_echoes_default_spec() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);
    let spec = &v["view"]["spec"];

    assert_eq!(
        spec["filters"]["only_failing"], false,
        "default spec should not filter to failing"
    );
    assert!(
        spec["filters"]["coverage_range"].is_null(),
        "default spec should have no coverage_range"
    );
    assert_eq!(spec["sort"], "crap", "default sort should be 'crap'");
    assert!(spec["limit"].is_null(), "default limit should be null");
    assert_eq!(v["view"]["truncated"], false);
}

#[test]
fn view_full_is_elided_from_json() {
    // `view.full` is `#[serde(skip)]` — the envelope's `result` field
    // already serializes the full analysis, so emitting `view.full`
    // would double-emit. Consumers `jq '.view'` should not see `full`.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);
    assert!(
        v["view"].get("full").is_none(),
        "view.full must not appear in JSON output"
    );
}

#[test]
fn json_envelope_key_declaration_order() {
    // cli_ergonomics.feature:243-246 — the JSON envelope key declaration
    // order is `schema_version, tool_version, language, timestamp,
    // metric, threshold, diff_ref, result, view`. Asserted on the raw
    // stdout string, NOT the parsed serde_json::Value (which alphabetizes).
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--format", "json"]);
    assert_success(&output);
    let raw = stdout_str(&output);

    let keys = [
        "schema_version",
        "tool_version",
        "language",
        "timestamp",
        "metric",
        "threshold",
        "diff_ref",
        "result",
        "view",
    ];
    let positions: Vec<usize> = keys
        .iter()
        .map(|k| {
            raw.find(&format!("\"{k}\""))
                .unwrap_or_else(|| panic!("envelope missing key {k}\nstdout:\n{raw}"))
        })
        .collect();
    for (pair, w) in keys.windows(2).zip(positions.windows(2)) {
        assert!(
            w[0] < w[1],
            "envelope order: expected {} before {}, but got positions {} and {}",
            pair[0],
            pair[1],
            w[0],
            w[1],
        );
    }
}

// ── Default invocation does NOT render a "View:" line in table output ──

#[test]
fn default_invocation_table_does_not_render_view_line() {
    // V1a is behavior-preserving on the table path. Default spec means
    // `should_render_view_line(view) == false`, so the reporter must
    // not emit any "View:" header.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &[]);
    assert_success(&output);
    let out = stdout_str(&output);

    assert!(
        !out.contains("View:"),
        "default invocation must not render a 'View:' line in V1a; got:\n{out}"
    );
    // The legacy "Summary:" line is still present.
    assert!(
        out.contains("Summary:"),
        "default invocation must still print the Summary line; got:\n{out}"
    );
}

// ── Underlying analysis is unchanged by the View pipeline (gate is unshapeable) ──

#[test]
fn default_view_eligible_count_equals_total_functions() {
    let dir = tempfile::tempdir().unwrap();
    let two_fn_src = "pub fn alpha() -> i32 { 1 }\npub fn beta() -> i32 { 2 }\n";
    let two_fn_lcov = "SF:lib.rs\nDA:1,1\nDA:2,1\nend_of_record\n";
    setup_dir(dir.path(), two_fn_src, two_fn_lcov);

    let output = run(dir.path(), &["--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);

    let total = v["result"]["summary"]["total_functions"].as_u64().unwrap();
    let eligible = v["view"]["eligible_count"].as_u64().unwrap();
    assert_eq!(
        total, eligible,
        "default spec eligible_count must equal total_functions"
    );
    assert_eq!(v["view"]["truncated"], false);
}

#[test]
fn schema_version_remains_one_with_additive_view() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);
    assert_eq!(
        v["schema_version"].as_u64(),
        Some(1),
        "View block was added per ADR D2 additive rule; schema_version stays at 1"
    );
}
