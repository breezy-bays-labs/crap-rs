//! Integration tests for issue #331 — coverage matching must work when
//! `SF:` records use **workspace-relative** paths (the natural shape
//! `cargo llvm-cov` emits, e.g. `SF:crates/foo/src/bar.rs`), regardless
//! of whether `--src` is given as a bare-relative, `./`-relative, or
//! absolute path.
//!
//! Pre-fix, `LcovParser::normalize_path` was a pure lexical
//! `strip_prefix(root_path)`. The orchestrator hands it the
//! **canonicalized absolute** source root, but the walker keys functions
//! by `strip_prefix(options.src)` against the **raw** (often relative)
//! `--src`. A workspace-relative `SF:` line cannot lexically strip an
//! absolute root, so it kept the full path while the walker emitted the
//! src-relative basename — the two never matched and every function
//! reported `coverage_percent: 0`.
//!
//! These fixtures place a real source file on disk and emit
//! workspace-relative `SF:` lines, so the filesystem-validated
//! suffix-match fallback added in #331 is actually exercised.

use std::path::Path;
use std::process::Command;

const BINARY: &str = env!("CARGO_BIN_EXE_crap4rs");

const FIXTURE_SRC: &str = "\
pub fn passing_a() -> i32 { 1 }
pub fn passing_b() -> i32 { 2 }
pub fn passing_c() -> i32 { 3 }
";

/// Lay out a workspace-shaped tree under `dir`:
///   - `pkg/src/lib.rs` carrying `FIXTURE_SRC` (3 single-line fns).
///   - `lcov.info` with **workspace-relative** `SF:` paths
///     (`SF:pkg/src/lib.rs`) and full line hits.
///
/// The binary is invoked with `current_dir(dir)`, so `dir` plays the
/// role of the workspace root that workspace-relative paths resolve
/// against — exactly the `cargo llvm-cov` convention.
fn setup_dir(dir: &Path) {
    let src = dir.join("pkg").join("src");
    std::fs::create_dir_all(&src).expect("create pkg/src");
    std::fs::write(src.join("lib.rs"), FIXTURE_SRC).expect("write lib.rs fixture");

    // Workspace-relative SF: path — what `cargo llvm-cov --lcov` emits
    // when run from the workspace root without post-processing.
    let lcov = "SF:pkg/src/lib.rs\nDA:1,1\nDA:2,1\nDA:3,1\nend_of_record\n";
    std::fs::write(dir.join("lcov.info"), lcov).expect("write lcov.info fixture");
}

fn run_with_src(dir: &Path, src_arg: &str) -> std::process::Output {
    Command::new(BINARY)
        .current_dir(dir)
        .args([
            "--src",
            src_arg,
            "--coverage",
            "lcov.info",
            "--no-gitignore",
            "--threshold",
            "5",
            "--no-fail",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run crap4rs binary")
}

/// Parse the binary's JSON envelope and return per-function coverage.
fn coverage_percents(output: &std::process::Output) -> Vec<f64> {
    assert!(
        output.status.success(),
        "binary exited non-zero (stderr=\n{}\n stdout=\n{})",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\nraw stdout:\n{stdout}"));
    let functions = json["result"]["functions"]
        .as_array()
        .expect("envelope must carry a result.functions array");
    assert!(
        !functions.is_empty(),
        "expected at least one scored function; envelope = {json:#?}"
    );
    functions
        .iter()
        .map(|f| f["scored"]["coverage_percent"].as_f64().unwrap_or(-1.0))
        .collect()
}

#[test]
fn bare_relative_src_with_workspace_relative_coverage_matches() {
    let dir = tempfile::tempdir().expect("create tempdir");
    setup_dir(dir.path());

    // Bare-relative `--src` (the natural Cargo-convention shape) paired
    // with workspace-relative `SF:` paths. The three functions each
    // occupy a single fully-hit line, so all must score exactly 100.0%.
    let output = run_with_src(dir.path(), "pkg/src");

    assert_eq!(
        coverage_percents(&output),
        vec![100.0, 100.0, 100.0],
        "workspace-relative SF: paths must match the walker's src-relative \
         keys under a bare-relative --src (see #331)"
    );
}

#[test]
fn absolute_src_with_workspace_relative_coverage_matches() {
    let dir = tempfile::tempdir().expect("create tempdir");
    setup_dir(dir.path());

    // Absolute `--src` paired with workspace-relative `SF:` paths. This
    // row is only reachable through the filesystem suffix-match fallback
    // — there is no relative root to lexically strip an absolute SF
    // against, so a pure-lexical fix cannot satisfy it.
    let abs_src = dir.path().join("pkg").join("src");
    let output = run_with_src(dir.path(), &abs_src.to_string_lossy());

    assert_eq!(
        coverage_percents(&output),
        vec![100.0, 100.0, 100.0],
        "workspace-relative SF: paths must match under an absolute --src too (see #331)"
    );
}
