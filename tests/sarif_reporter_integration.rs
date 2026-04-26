//! Integration tests for `--format sarif` (issue #70).
//!
//! Drives the binary end-to-end. SARIF is a gate translation: the
//! contract is that results derive from the unshapeable
//! `view.full.functions` regardless of display flags (`--top`,
//! `--sort-by`, `--only-failing`). These tests pin that contract.

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

fn parse_json(output: &std::process::Output) -> serde_json::Value {
    let out = stdout_str(output);
    serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{out}"))
}

/// 6 functions: 3 trivial (covered, low CRAP), 3 branchy (uncovered, high CRAP).
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

/// All-passing fixture (3 trivial fns, fully covered) for the empty-results case.
const PASSING_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
";

const PASSING_LCOV: &str = "\
SF:lib.rs
DA:1,1
DA:2,1
DA:3,1
end_of_record
";

// ── Envelope shape ─────────────────────────────────────────────────

#[test]
fn sarif_envelope_has_v2_1_0_shape() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);

    let v = parse_json(&output);
    assert_eq!(
        v["$schema"], "https://json.schemastore.org/sarif-2.1.0.json",
        "schema URI"
    );
    assert_eq!(v["version"], "2.1.0", "SARIF version");
    assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "crap4rs");
    let version = v["runs"][0]["tool"]["driver"]["version"]
        .as_str()
        .expect("version must be a string");
    assert!(!version.is_empty(), "version must not be empty");
    assert_eq!(
        v["runs"][0]["tool"]["driver"]["rules"][0]["id"],
        "crap/threshold-exceeded"
    );
}

#[test]
fn sarif_results_match_exceeders_in_full_analysis() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);

    let v = parse_json(&output);
    let results = v["runs"][0]["results"].as_array().unwrap();
    // 3 branchy functions exceed; 3 trivial ones are below.
    assert_eq!(results.len(), 3, "expected 3 exceeders, got {results:?}");
    for r in results {
        assert!(r["ruleId"].is_string());
        assert!(r["level"].is_string());
        assert!(r["message"]["text"].is_string());
        assert!(r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"].is_string());
        assert!(r["locations"][0]["physicalLocation"]["region"]["startLine"].is_u64());
        assert!(r["locations"][0]["physicalLocation"]["region"]["endLine"].is_u64());
        assert!(r["partialFingerprints"]["functionIdentity"].is_string());
    }
}

#[test]
fn sarif_empty_results_when_nothing_exceeds() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), PASSING_SRC, PASSING_LCOV);
    let output = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);

    let v = parse_json(&output);
    let results = v["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 0, "expected empty results");
    // Rule must still be present so consumers can introspect.
    let rules = v["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["id"], "crap/threshold-exceeded");
}

// ── Gate keystone: SARIF iterates the FULL analysis ────────────────

#[test]
fn sarif_top_does_not_truncate() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let baseline = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);
    let with_top = run(
        dir.path(),
        &["--threshold", "8", "--format", "sarif", "--top", "1"],
    );
    assert_eq!(
        stdout_str(&baseline),
        stdout_str(&with_top),
        "--top must not affect SARIF output"
    );
}

#[test]
fn sarif_only_failing_does_not_shrink() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let baseline = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);
    let only_failing = run(
        dir.path(),
        &["--threshold", "8", "--format", "sarif", "--only-failing"],
    );
    assert_eq!(
        stdout_str(&baseline),
        stdout_str(&only_failing),
        "--only-failing must not affect SARIF output"
    );
}

#[test]
fn sarif_sort_by_does_not_reorder() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let baseline = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);
    let sorted = run(
        dir.path(),
        &[
            "--threshold",
            "8",
            "--format",
            "sarif",
            "--sort-by",
            "coverage",
        ],
    );
    assert_eq!(
        stdout_str(&baseline),
        stdout_str(&sorted),
        "--sort-by must not reorder SARIF results"
    );
}

// ── Exit code semantics ────────────────────────────────────────────

#[test]
fn sarif_exit_code_unchanged_by_format() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "exceeding functions exit 1 regardless of --format"
    );
    assert!(!stdout_str(&output).is_empty());
}

#[test]
fn sarif_no_fail_overrides_exit_but_still_lists_findings() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(
        dir.path(),
        &["--threshold", "8", "--format", "sarif", "--no-fail"],
    );
    assert_eq!(output.status.code(), Some(0), "--no-fail exits 0");
    let v = parse_json(&output);
    let results = v["runs"][0]["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        3,
        "--no-fail must not hide findings — gate is exit code, SARIF reports truth"
    );
}

// ── Location format ────────────────────────────────────────────────

#[test]
fn sarif_artifact_uri_is_repo_relative_no_scheme() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);
    let v = parse_json(&output);
    for r in v["runs"][0]["results"].as_array().unwrap() {
        let uri = r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        assert!(
            !uri.starts_with("file://"),
            "uri must not be a file:// URL: {uri}"
        );
        assert!(!uri.starts_with('/'), "uri must be repo-relative: {uri}");
        assert!(uri.ends_with(".rs"), "expected .rs path, got {uri}");
    }
}

// ── Determinism (fingerprint stability) ────────────────────────────

#[test]
fn sarif_byte_identical_across_runs() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let first = stdout_str(&run(dir.path(), &["--threshold", "8", "--format", "sarif"]));
    let second = stdout_str(&run(dir.path(), &["--threshold", "8", "--format", "sarif"]));
    assert_eq!(first, second, "SARIF must be byte-deterministic");
}

#[test]
fn sarif_partial_fingerprint_format() {
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);
    let output = run(dir.path(), &["--threshold", "8", "--format", "sarif"]);
    let v = parse_json(&output);
    for r in v["runs"][0]["results"].as_array().unwrap() {
        let fp = r["partialFingerprints"]["functionIdentity"]
            .as_str()
            .unwrap();
        let uri = r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        let expected_prefix = format!("{uri}:");
        assert!(
            fp.starts_with(&expected_prefix),
            "fingerprint {fp:?} should start with {expected_prefix:?}"
        );
    }
}
