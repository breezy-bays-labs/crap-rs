//! Integration tests for `--format advice` (issue #76 V4).
//!
//! Each test transcribes one or more scenarios from
//! `tests/features/format_advice.feature`. The behavior contract there
//! is the spec; this file is the executable form (cucumber-rs not yet
//! adopted — see crap4rs#115 follow-up).

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

// Fixture: a high-complexity uncovered function (failing) + a simple
// covered function (passing). Lines align with the LCOV file so coverage
// gaps are deterministic.
const FIXTURE_SRC: &str = "\
pub fn simple_pass() -> i32 {
    42
}

pub fn branchy_fail(x: i32, y: i32) -> i32 {
    if x > 0 {
        if y > 0 {
            1
        } else {
            0
        }
    } else if x < 0 {
        if y > 0 {
            -1
        } else {
            -2
        }
    } else {
        0
    }
}
";

const FIXTURE_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
DA:5,0
DA:6,0
DA:7,0
DA:8,0
DA:9,0
DA:10,0
DA:11,0
DA:12,0
DA:13,0
DA:14,0
DA:15,0
DA:16,0
DA:17,0
DA:18,0
DA:19,0
DA:20,0
DA:21,0
end_of_record
";

// ── Envelope shape ──────────────────────────────────────────────────

#[test]
fn advice_emits_schema_version_one_and_view_shown_array() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let json = parse_json(&output);

    assert_eq!(json["schema_version"], 1);
    assert!(json["view"]["shown"].is_array());
}

#[test]
fn advice_exit_code_matches_json_when_exceeding() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let advice = run(
        tmp.path(),
        &["--threshold", "5", "--no-gitignore", "--format", "advice"],
    );
    let json_out = run(
        tmp.path(),
        &["--threshold", "5", "--no-gitignore", "--format", "json"],
    );
    assert_eq!(advice.status.code(), json_out.status.code());
    assert_eq!(advice.status.code(), Some(1));
}

// ── Diagnostic gating ───────────────────────────────────────────────

#[test]
fn under_threshold_verdicts_omit_diagnostic_key_in_serialized_json() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let json = parse_json(&output);
    let shown = json["view"]["shown"].as_array().expect("shown array");

    for entry in shown {
        let exceeds = entry["exceeds"].as_bool().unwrap_or(false);
        if !exceeds {
            assert!(
                entry.get("diagnostic").is_none(),
                "under-threshold verdict serialized a diagnostic key: {entry}"
            );
        }
    }
}

#[test]
fn over_threshold_verdicts_carry_all_four_diagnostic_fields() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let json = parse_json(&output);
    let shown = json["view"]["shown"].as_array().expect("shown array");
    let exceeding: Vec<_> = shown
        .iter()
        .filter(|e| e["exceeds"].as_bool().unwrap_or(false))
        .collect();
    assert!(
        !exceeding.is_empty(),
        "fixture must produce at least one exceeding verdict"
    );

    for verdict in exceeding {
        let diag = verdict
            .get("diagnostic")
            .unwrap_or_else(|| panic!("over-threshold verdict missing diagnostic: {verdict}"));
        for field in [
            "coverage_gaps",
            "complexity_drivers",
            "suggested_actions",
            "root_cause",
        ] {
            assert!(
                diag.get(field).is_some(),
                "diagnostic missing field {field}: {diag}"
            );
        }
    }
}

// ── SuggestedAction taxonomy ────────────────────────────────────────

#[test]
fn low_coverage_high_complexity_emits_both_actions_and_root_cause_both() {
    // The fixture's branchy_fail is uncovered AND complex, so it should
    // emit both AddTestsForLines and ExtractFunction; root_cause = both.
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let json = parse_json(&output);
    let shown = json["view"]["shown"].as_array().unwrap();
    let branchy = shown
        .iter()
        .find(|e| e["scored"]["identity"]["qualified_name"] == "branchy_fail")
        .expect("branchy_fail in shown");
    let diag = branchy.get("diagnostic").expect("diagnostic populated");

    assert_eq!(diag["root_cause"], "both");

    let kinds: Vec<&str> = diag["suggested_actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"add_tests_for_lines"),
        "expected add_tests_for_lines action, got {kinds:?}"
    );
    assert!(
        kinds.contains(&"extract_function"),
        "expected extract_function action, got {kinds:?}"
    );
}

// ── ProposedSplit shape ─────────────────────────────────────────────

#[test]
fn proposed_split_carries_five_wire_fields_and_exactly_one_recommended() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let json = parse_json(&output);
    let shown = json["view"]["shown"].as_array().unwrap();
    let branchy = shown
        .iter()
        .find(|e| e["scored"]["identity"]["qualified_name"] == "branchy_fail")
        .unwrap();

    let extract = branchy["diagnostic"]["suggested_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "extract_function")
        .expect("ExtractFunction emitted");
    let candidates = extract["candidates"].as_array().unwrap();
    assert!(
        !candidates.is_empty(),
        "candidates non-empty for high-complexity fn"
    );

    for cand in candidates {
        for field in [
            "line_range",
            "complexity_contribution",
            "branch_path",
            "kind",
            "recommended",
        ] {
            assert!(
                cand.get(field).is_some(),
                "ProposedSplit missing field {field}: {cand}"
            );
        }
        let kind = cand["kind"].as_str().unwrap();
        assert!(
            matches!(
                kind,
                "deepest_nesting" | "largest_subblock" | "highest_branch_count"
            ),
            "unexpected kind: {kind}"
        );
    }

    let recommended_count = candidates
        .iter()
        .filter(|c| c["recommended"].as_bool().unwrap_or(false))
        .count();
    assert_eq!(
        recommended_count, 1,
        "exactly one candidate must be recommended"
    );
}

// ── Composition with View flags ─────────────────────────────────────

#[test]
fn advice_composes_with_no_fail() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "--no-fail forces exit 0");

    let json = parse_json(&output);
    let shown = json["view"]["shown"].as_array().unwrap();
    let exceeding: Vec<_> = shown
        .iter()
        .filter(|e| e["exceeds"].as_bool().unwrap_or(false))
        .collect();
    assert!(
        !exceeding.is_empty(),
        "no-fail must not strip exceeding verdicts"
    );
    for v in exceeding {
        assert!(v.get("diagnostic").is_some());
    }
}

// ── Naming conflict (R6.6 / A1) ─────────────────────────────────────

#[test]
fn json_format_without_advice_has_no_diagnostic_keys() {
    let tmp = tempfile::tempdir().expect("create tempdir");
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
    let shown = json["view"]["shown"].as_array().unwrap();
    for entry in shown {
        assert!(
            entry.get("diagnostic").is_none(),
            "default --format json must not populate diagnostic; got {entry}"
        );
    }
}

// ── Stability / determinism ────────────────────────────────────────

#[test]
fn same_input_produces_byte_identical_advice_json() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let first = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let second = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    assert_eq!(first.stdout, second.stdout);
}

// ── Stderr summary (V5 / S-8) ───────────────────────────────────────

#[test]
fn advice_emits_stderr_summary_one_line_per_exceeding_function() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");

    let summary_lines: Vec<&str> = stderr.lines().filter(|l| l.starts_with("[crap=")).collect();
    assert!(
        !summary_lines.is_empty(),
        "expected at least one summary line, got stderr:\n{stderr}"
    );
    for line in &summary_lines {
        assert!(line.contains("[actions:"), "missing actions tag: {line}");
        assert!(line.contains("lib.rs"), "expected lib.rs in path: {line}");
        assert!(
            line.contains(" branchy_fail "),
            "expected qualified_name: {line}"
        );
    }
}

#[test]
fn json_format_emits_no_advice_summary_on_stderr() {
    let tmp = tempfile::tempdir().expect("create tempdir");
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
    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        !stderr.lines().any(|l| l.starts_with("[crap=")),
        "no advice summary lines should appear under --format json; stderr:\n{stderr}"
    );
}

#[test]
fn advice_stderr_summary_is_byte_identical_across_runs() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let first = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let second = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    assert_eq!(first.stderr, second.stderr);
}

// ── R6.3 — branch_path is AST-only, no prose ────────────────────────

#[test]
fn branch_path_is_kebab_case_kind_chain_no_prose() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    setup_dir(tmp.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        tmp.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--no-fail",
            "--format",
            "advice",
        ],
    );
    let json = parse_json(&output);
    let shown = json["view"]["shown"].as_array().unwrap();
    for entry in shown {
        let Some(diag) = entry.get("diagnostic") else {
            continue;
        };
        for action in diag["suggested_actions"].as_array().unwrap() {
            if action["kind"] != "extract_function" {
                continue;
            }
            for cand in action["candidates"].as_array().unwrap() {
                let path = cand["branch_path"].as_str().unwrap();
                assert!(
                    !path.contains(' '),
                    "branch_path contains whitespace (prose suspicion): {path:?}"
                );
                assert!(
                    !path.contains(','),
                    "branch_path contains comma (prose suspicion): {path:?}"
                );
            }
        }
    }
}
