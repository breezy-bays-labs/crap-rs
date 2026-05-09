//! Integration tests for `--sort-by` (issue #68).
//!
//! Wires CLI flag through `cli::view_args::build_view_spec` into
//! `domain::view::ViewSpec::sort`. The four `SortKey` variants
//! (`Crap`, `Coverage`, `Complexity`, `Path`) cover the dimensions an
//! investigator wants to reorder along; clap rejects malformed values
//! at parse time, so no `validate_view_args` arm is needed.

use std::path::Path;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

fn setup_dir(dir: &Path, src_content: &str, lcov_content: &str) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    std::fs::write(src.join("lib.rs"), src_content).expect("write lib.rs fixture");
    std::fs::write(dir.join("lcov.info"), lcov_content).expect("write lcov.info fixture");
}

fn setup_multi_file(dir: &Path, files: &[(&str, &str)], lcov_content: &str) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    for (name, content) in files {
        std::fs::write(src.join(name), content).expect("write fixture file");
    }
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

/// 6-function fixture mirrors `cli_top_integration`: 3 simple/covered, 3
/// branchy/uncovered. The functions span a range of CRAP, coverage, and
/// complexity values so each `--sort-by` dimension produces a distinct
/// ordering — the assertion shape is "compare adjacent rows," not
/// "match exact CRAP scores."
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

fn shown_coverage_seq(v: &serde_json::Value) -> Vec<f64> {
    v["view"]["shown"]
        .as_array()
        .expect("view.shown array")
        .iter()
        .map(|row| {
            row["scored"]["coverage_percent"]
                .as_f64()
                .expect("coverage")
        })
        .collect()
}

fn shown_complexity_seq(v: &serde_json::Value) -> Vec<u64> {
    v["view"]["shown"]
        .as_array()
        .expect("view.shown array")
        .iter()
        .map(|row| {
            row["scored"]["complexity"]
                .as_u64()
                .expect("complexity u64")
        })
        .collect()
}

fn shown_crap_seq(v: &serde_json::Value) -> Vec<f64> {
    v["view"]["shown"]
        .as_array()
        .expect("view.shown array")
        .iter()
        .map(|row| {
            row["scored"]["crap"]["value"]
                .as_f64()
                .expect("crap.value f64")
        })
        .collect()
}

// ── Happy path: each sort key produces the expected ordering ──────────

#[test]
fn sort_by_crap_descending_explicit() {
    // cli_ergonomics.feature:116-119. `--sort-by crap` is the explicit
    // form of the default; result must match the default ordering and
    // the JSON envelope's `view.spec.sort` echoes "crap".
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
            "--sort-by",
            "crap",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    assert_eq!(v["view"]["spec"]["sort"], "crap");

    let crap = shown_crap_seq(&v);
    assert_eq!(crap.len(), 6);
    for w in crap.windows(2) {
        assert!(
            w[0] >= w[1],
            "expected CRAP descending; saw {} before {}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn sort_by_coverage_ascending() {
    // cli_ergonomics.feature:121-124. Coverage ascending — uncovered
    // (0%) rows surface first. The fixture's bimodal coverage (0.0 vs
    // 100.0) makes the assertion straightforward: the first 3 rows are
    // 0% and the last 3 are 100%.
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
            "--sort-by",
            "coverage",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    assert_eq!(v["view"]["spec"]["sort"], "coverage");

    let cov = shown_coverage_seq(&v);
    assert_eq!(cov.len(), 6);
    for w in cov.windows(2) {
        assert!(
            w[0] <= w[1],
            "expected coverage ascending; saw {} before {}",
            w[0],
            w[1]
        );
    }
    assert_eq!(&cov[..3], &[0.0, 0.0, 0.0]);
    assert_eq!(&cov[3..], &[100.0, 100.0, 100.0]);
}

#[test]
fn sort_by_complexity_descending() {
    // cli_ergonomics.feature:126-129. Complexity descending — the
    // branchy functions (CC ~3) sort before the simple (CC=1) ones.
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
            "--sort-by",
            "complexity",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    assert_eq!(v["view"]["spec"]["sort"], "complexity");

    let cx = shown_complexity_seq(&v);
    assert_eq!(cx.len(), 6);
    for w in cx.windows(2) {
        assert!(
            w[0] >= w[1],
            "expected complexity descending; saw {} before {}",
            w[0],
            w[1]
        );
    }
    // First three rows must be the branchy functions (CC > 1).
    assert!(
        cx[0] > 1 && cx[1] > 1 && cx[2] > 1,
        "branchy functions must lead under complexity-desc; got {cx:?}"
    );
    // Last three rows must be the simple functions (CC == 1).
    assert_eq!(&cx[3..], &[1, 1, 1]);
}

#[test]
fn sort_by_path_alphabetical_then_crap_within_file() {
    // cli_ergonomics.feature:131-137 + view.feature:124-128.
    // Two-file fixture: src/a.rs and src/b.rs, each containing one
    // simple (covered, low CRAP) and one branchy (uncovered, high CRAP)
    // function. Expected order under `--sort-by path`:
    //   a.rs::failing_one  (high CRAP within file)
    //   a.rs::passing_one  (low CRAP within file)
    //   b.rs::failing_two  (high CRAP within file)
    //   b.rs::passing_two  (low CRAP within file)
    // i.e. files alphabetical, CRAP descending within each file.
    let a_src = "\
pub fn passing_one() -> i32 { 1 }
pub fn failing_one(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";
    let b_src = "\
pub fn passing_two() -> i32 { 1 }
pub fn failing_two(x: i32) -> i32 { if x > 0 { if x > 5 { 1 } else { 2 } } else { 3 } }
";
    let lcov = "\
SF:a.rs
DA:1,1
DA:2,0
end_of_record
SF:b.rs
DA:1,1
DA:2,0
end_of_record
";
    let dir = tempfile::tempdir().unwrap();
    setup_multi_file(dir.path(), &[("a.rs", a_src), ("b.rs", b_src)], lcov);

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--format",
            "json",
            "--no-gitignore",
            "--sort-by",
            "path",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    assert_eq!(v["view"]["spec"]["sort"], "path");

    let shown = v["view"]["shown"].as_array().expect("view.shown array");
    assert_eq!(shown.len(), 4, "two files × two functions = 4 rows");

    // Build a (file_path, qualified_name, crap) triple per row.
    let rows: Vec<(String, String, f64)> = shown
        .iter()
        .map(|row| {
            let id = &row["scored"]["identity"];
            let path = id["file_path"].as_str().expect("file_path").to_string();
            let name = id["qualified_name"]
                .as_str()
                .expect("qualified_name")
                .to_string();
            let crap = row["scored"]["crap"]["value"]
                .as_f64()
                .expect("crap.value f64");
            (path, name, crap)
        })
        .collect();

    // File-paths must be alphabetical across the row list.
    let paths: Vec<&str> = rows.iter().map(|(p, _, _)| p.as_str()).collect();
    let mut sorted_paths = paths.clone();
    sorted_paths.sort();
    assert_eq!(
        paths, sorted_paths,
        "rows must be alphabetical by file_path; got {paths:?}"
    );

    // Within each file, CRAP must be descending.
    let mut current_path: Option<&str> = None;
    let mut last_crap = f64::INFINITY;
    for (p, _, c) in &rows {
        match current_path {
            Some(prev) if prev == p => {
                assert!(
                    last_crap >= *c,
                    "within file {p}, CRAP must be descending; saw {last_crap} before {c}"
                );
            }
            _ => {
                current_path = Some(p);
            }
        }
        last_crap = *c;
    }

    // Spot-check the leading rows: a.rs's branchy fn (high CRAP) before
    // a.rs's simple fn, then b.rs follows.
    assert_eq!(rows[0].0, "a.rs");
    assert_eq!(rows[0].1, "failing_one");
    assert_eq!(rows[1].0, "a.rs");
    assert_eq!(rows[1].1, "passing_one");
    assert_eq!(rows[2].0, "b.rs");
    assert_eq!(rows[2].1, "failing_two");
    assert_eq!(rows[3].0, "b.rs");
    assert_eq!(rows[3].1, "passing_two");
}

// ── Validation: clap rejects unknown values at parse time ────────────

#[test]
fn sort_by_invalid_value_exits_2() {
    // cli_ergonomics.feature exit-code matrix. clap's `ValueEnum` rejects
    // unknown variants automatically — no custom `validate_view_args` arm
    // is needed. The error must be attributed to `--sort-by`.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(dir.path(), &["--no-gitignore", "--sort-by", "foo"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("invalid value 'foo' for '--sort-by"),
        "expected clap value error attributed to --sort-by; got:\n{stderr}"
    );
}

// ── Display invariant: sort-only does NOT render the View banner ─────

#[test]
fn sort_only_does_not_render_view_banner() {
    // cli_ergonomics.feature:265-267. Sorting reorders without reducing
    // rows, so `should_render_view_line(view) == false` and the table
    // reporter must not emit a "View:" header.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    let output = run(
        dir.path(),
        &[
            "--threshold",
            "5",
            "--no-gitignore",
            "--sort-by",
            "coverage",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let out = stdout_str(&output);
    assert!(
        !out.contains("View:"),
        "--sort-by alone must not render a 'View:' line; got:\n{out}"
    );
    assert!(
        out.contains("Summary:"),
        "Summary line still expected; got:\n{out}"
    );
}

// ── JSON envelope: spec.sort serializes lowercase per ValueEnum ──────

#[test]
fn sort_by_serializes_in_json_envelope_lowercase() {
    // cli_ergonomics.feature:206 + the SortKey enum's
    // `#[serde(rename_all = "lowercase")]`. Exercise all four variants
    // and confirm each serializes as the expected lowercase string.
    let dir = tempfile::tempdir().unwrap();
    setup_dir(dir.path(), FIXTURE_SRC, FIXTURE_LCOV);

    for key in ["crap", "coverage", "complexity", "path"] {
        let output = run(
            dir.path(),
            &[
                "--threshold",
                "5",
                "--format",
                "json",
                "--no-gitignore",
                "--sort-by",
                key,
            ],
        );
        assert_ne!(
            output.status.code(),
            Some(2),
            "--sort-by {key} should not error: stderr:\n{}",
            stderr_str(&output)
        );
        let v = parse_json(&output);
        assert_eq!(
            v["view"]["spec"]["sort"], key,
            "spec.sort must echo the requested key as lowercase string"
        );
    }
}

// ── Composition with --top: filter then sort then truncate ───────────

#[test]
fn sort_by_coverage_composes_with_top() {
    // cli_ergonomics.feature:139-142. `--sort-by coverage --top 2`
    // surfaces the two lowest-coverage rows. Filter step is a no-op
    // (no coverage range), sort is coverage-ascending, truncate keeps
    // 2. The fixture's bimodal coverage means both surfaced rows have
    // coverage_percent == 0.0.
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
            "--sort-by",
            "coverage",
            "--top",
            "2",
        ],
    );
    assert_ne!(
        output.status.code(),
        Some(2),
        "validation should pass: stderr:\n{}",
        stderr_str(&output)
    );
    let v = parse_json(&output);

    assert_eq!(v["view"]["spec"]["sort"], "coverage");
    assert_eq!(v["view"]["spec"]["limit"].as_u64(), Some(2));
    assert_eq!(v["view"]["truncated"], true);
    assert_eq!(v["view"]["eligible_count"], 6);

    let cov = shown_coverage_seq(&v);
    assert_eq!(cov.len(), 2, "--top 2 must truncate to 2 rows");
    assert_eq!(
        cov,
        vec![0.0, 0.0],
        "the two lowest-coverage rows must be the 0% (uncovered) ones"
    );
}
