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
    // Top-level keys in serde_json's pretty printer sit at indent 2.
    // Anchor the substring search to `\n  "<key>"` so future nested
    // fields with the same name (e.g. a per-verdict `threshold`)
    // can't shadow the top-level position (CodeRabbit CR-N5).
    let positions: Vec<usize> = keys
        .iter()
        .map(|k| {
            raw.find(&format!("\n  \"{k}\""))
                .unwrap_or_else(|| panic!("envelope missing top-level key {k}\nstdout:\n{raw}"))
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

// ── Shaped invocation DOES render a "View:" line in table output ──

#[test]
fn top_truncation_table_renders_view_line() {
    // The companion to the default-spec negative above: when `--top`
    // truncates the row set, `should_render_view_line(view) == true`, so
    // the reporter emits the "View:" subtitle below the Summary block.
    let dir = tempfile::tempdir().unwrap();
    let multi_fn_src =
        "pub fn alpha() -> i32 { 1 }\npub fn beta() -> i32 { 2 }\npub fn gamma() -> i32 { 3 }\n";
    let multi_fn_lcov = "SF:lib.rs\nDA:1,1\nDA:2,1\nDA:3,1\nend_of_record\n";
    setup_dir(dir.path(), multi_fn_src, multi_fn_lcov);

    let output = run(dir.path(), &["--top", "2"]);
    assert_success(&output);
    let out = stdout_str(&output);

    assert!(
        out.contains("View: showing 2 of 3 functions (top 2)"),
        "--top truncation must render the View line; got:\n{out}"
    );
    // The Summary line is still present, above the View line.
    assert!(
        out.contains("Summary:"),
        "Summary line must remain; got:\n{out}"
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
fn schema_version_is_two_in_v0_4_0() {
    // 0.4.0 bumped schema_version 1 → 2 via #107 (ComplexityContributor.column
    // 0-based → 1-based). The view block stayed additive (ADR D2); the bump
    // is for the column-convention shift, not the view block.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), SIMPLE_SRC, SIMPLE_LCOV);

    let output = run(dir.path(), &["--format", "json"]);
    assert_success(&output);
    let v = parse_json(&output);
    assert_eq!(
        v["schema_version"].as_u64(),
        Some(2),
        "0.4.0 bumped schema_version 1 → 2 for the contributor-column 1-based convention shift"
    );
}

// ── V1b: --only-failing routes through the View, summary stays unfiltered ──

/// 6-function fixture: 3 simple (CC=1, fully covered) and 3 branching
/// (uncovered). Threshold 5 puts 3 functions over the edge.
const ONLY_FAILING_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
pub fn failing_a(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_b(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
pub fn failing_c(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";

const ONLY_FAILING_LCOV: &str = "\
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
fn only_failing_summary_is_self_consistent() {
    // V1b regression: `--only-failing` must NOT mutate `result.functions`
    // or any field of `result.summary`. The full unfiltered analysis is
    // the unshapeable gate; only `view.shown` reflects the filter.
    //
    // CQO-C3: enumerate every summary field (total_functions,
    // exceeding_threshold, average_crap, median_crap, max_crap.value,
    // and all four distribution buckets) and assert each one matches
    // the baseline (no-filter) run.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), ONLY_FAILING_SRC, ONLY_FAILING_LCOV);

    // Threshold violations are expected → exit code 1, but stdout still
    // carries the JSON envelope. Only exit 2 (validation error) would be
    // a fatal failure here.
    let baseline = run(
        dir.path(),
        &["--threshold", "5", "--format", "json", "--no-gitignore"],
    );
    assert_ne!(
        baseline.status.code(),
        Some(2),
        "baseline run must not error: stderr:\n{}",
        stderr_str(&baseline)
    );
    let base = parse_json(&baseline);

    // Sanity: 6 total functions, 3 exceed at threshold 5.
    assert_eq!(
        base["result"]["summary"]["total_functions"], 6,
        "fixture sanity: expected 6 functions"
    );
    assert_eq!(
        base["result"]["summary"]["exceeding_threshold"], 3,
        "fixture sanity: expected 3 to exceed threshold 5"
    );

    let filtered = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--format",
            "json",
            "--no-gitignore",
            "--only-failing",
        ],
    );
    assert_ne!(
        filtered.status.code(),
        Some(2),
        "--only-failing run must not error: stderr:\n{}",
        stderr_str(&filtered)
    );
    let v = parse_json(&filtered);

    // Key: V1b leaves result.functions untouched. Under V1a this would
    // be 3 (the legacy retain). Under V1b it stays at 6.
    let funcs = v["result"]["functions"].as_array().expect("array");
    assert_eq!(
        funcs.len(),
        6,
        "result.functions must NOT be mutated by --only-failing under V1b"
    );

    // CQO-C3: every summary field equals the baseline.
    let s = &v["result"]["summary"];
    let bs = &base["result"]["summary"];
    assert_eq!(s["total_functions"], bs["total_functions"]);
    assert_eq!(s["total_files"], bs["total_files"]);
    assert_eq!(s["exceeding_threshold"], bs["exceeding_threshold"]);
    assert_eq!(s["average_crap"], bs["average_crap"]);
    assert_eq!(s["median_crap"], bs["median_crap"]);
    assert_eq!(s["max_crap"], bs["max_crap"]);
    assert_eq!(s["worst_function"], bs["worst_function"]);
    assert_eq!(s["distribution"]["low"], bs["distribution"]["low"]);
    assert_eq!(
        s["distribution"]["acceptable"],
        bs["distribution"]["acceptable"]
    );
    assert_eq!(
        s["distribution"]["moderate"],
        bs["distribution"]["moderate"]
    );
    assert_eq!(s["distribution"]["high"], bs["distribution"]["high"]);

    // The View carries the filtered subset.
    let shown = v["view"]["shown"].as_array().expect("view.shown array");
    assert_eq!(shown.len(), 3, "view.shown must reflect the filter");
    assert_eq!(
        v["view"]["shown_summary"]["total_functions"], 3,
        "view.shown_summary derives from the filtered subset"
    );
    assert_eq!(
        v["view"]["spec"]["filters"]["only_failing"], true,
        "ViewSpec must record the filter that was applied"
    );
}
