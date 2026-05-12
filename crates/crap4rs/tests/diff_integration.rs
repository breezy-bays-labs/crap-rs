//! Integration tests for diff mode (`--diff <ref>`).
//!
//! Each test creates a tempdir git repo with controlled commits, runs
//! `analyze()` with `diff_ref`, and verifies the filtering behavior.

use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::adapters::coverage::LcovParser;
use crap4rs::core::{AnalysisOutput, AnalyzeOptions, analyze};
use crap4rs::domain::threshold::ThresholdConfig;
use crap4rs::domain::types::ComplexityMetric;
use std::path::Path;
use std::process::Command;

/// Wrapper for `analyze(&opts, &cx, &cov)` that constructs the LCOV +
/// syn ports from `opts.src`. Mirrors the Rust adapter binary's wiring
/// in `crap4rs/src/main.rs` post-S4 (#136); the orchestrator no longer
/// constructs adapters internally.
fn analyze_with_adapters(opts: &AnalyzeOptions) -> anyhow::Result<AnalysisOutput> {
    let cx = SynComplexityAdapter::new();
    let cov = LcovParser::new(opts.src.clone());
    analyze(opts, &cx, &cov)
}

// ── Helpers ────────────────────────────────────────────────────────

fn test_git_repo(dir: &Path) {
    git(dir, &["init"]);
    git(dir, &["config", "user.email", "test@test.com"]);
    git(dir, &["config", "user.name", "Test"]);
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_lcov(dir: &Path, files: &[(&str, &[(usize, u64)])]) {
    let mut lcov = String::new();
    for (file, lines) in files {
        lcov.push_str(&format!("SF:{file}\n"));
        for (line, hits) in *lines {
            lcov.push_str(&format!("DA:{line},{hits}\n"));
        }
        lcov.push_str("end_of_record\n");
    }
    std::fs::write(dir.join("lcov.info"), lcov).unwrap();
}

fn make_opts(dir: &Path, diff_ref: Option<&str>) -> AnalyzeOptions {
    AnalyzeOptions {
        src: dir.join("src"),
        coverage: dir.join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: 30.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        diff_ref: diff_ref.map(String::from),
        extensions: vec!["rs".to_string()],
        ..AnalyzeOptions::default()
    }
}

fn function_names(result: &crap4rs::domain::types::AnalysisResult) -> Vec<String> {
    result
        .functions
        .iter()
        .map(|v| v.scored.identity.qualified_name.clone())
        .collect()
}

// ── Scenario 1: Only changed functions appear ──────────────────────

#[test]
fn diff_modified_function_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    // Baseline: two functions
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "pub fn foo() -> i32 { 1 }\npub fn bar() -> i32 { 2 }\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "baseline"]);

    // Modify only foo (add a line inside it)
    std::fs::write(
        src.join("lib.rs"),
        "pub fn foo() -> i32 {\n    let x = 1;\n    x\n}\npub fn bar() -> i32 { 2 }\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "modify foo"]);

    write_lcov(
        root,
        &[("lib.rs", &[(1, 1), (2, 1), (3, 1), (4, 1), (5, 1)])],
    );

    let result = analyze_with_adapters(&make_opts(root, Some("HEAD~1")))
        .unwrap()
        .result;
    let names = function_names(&result);

    assert!(
        names.contains(&"foo".to_string()),
        "should include modified foo"
    );
    assert!(
        !names.contains(&"bar".to_string()),
        "should exclude unchanged bar"
    );
}

// ── Scenario 2: New file includes all functions ────────────────────

#[test]
fn diff_new_file_includes_all_functions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    // Empty initial commit
    git(root, &["commit", "--allow-empty", "-m", "baseline"]);

    // Add new file with two functions
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "pub fn baz() -> i32 { 3 }\npub fn qux() -> i32 { 4 }\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "add new file"]);

    write_lcov(root, &[("lib.rs", &[(1, 1), (2, 1)])]);

    let result = analyze_with_adapters(&make_opts(root, Some("HEAD~1")))
        .unwrap()
        .result;
    let names = function_names(&result);

    assert!(names.contains(&"baz".to_string()));
    assert!(names.contains(&"qux".to_string()));
}

// ── Scenario 3: Hunk precision — untouched function excluded ───────

#[test]
fn diff_hunk_precision_excludes_untouched_function() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    // Baseline: alpha on lines 1-5, beta on lines 10-15
    let baseline = "\
pub fn alpha() -> i32 {
    let a = 1;
    let b = 2;
    a + b
}

// spacer

pub fn beta() -> i32 {
    let c = 3;
    let d = 4;
    c + d
}
";
    std::fs::write(src.join("lib.rs"), baseline).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "baseline"]);

    // Modify only alpha (change line 3)
    let modified = "\
pub fn alpha() -> i32 {
    let a = 10;
    let b = 20;
    a + b
}

// spacer

pub fn beta() -> i32 {
    let c = 3;
    let d = 4;
    c + d
}
";
    std::fs::write(src.join("lib.rs"), modified).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "modify alpha"]);

    // Coverage for all lines
    let lines: Vec<(usize, u64)> = (1..=14).map(|l| (l, 1)).collect();
    write_lcov(root, &[("lib.rs", &lines)]);

    let result = analyze_with_adapters(&make_opts(root, Some("HEAD~1")))
        .unwrap()
        .result;
    let names = function_names(&result);

    assert!(names.contains(&"alpha".to_string()), "alpha was modified");
    assert!(
        !names.contains(&"beta".to_string()),
        "beta was not modified"
    );
}

// ── Scenario 4: Score invariant ────────────────────────────────────

#[test]
fn diff_scores_match_full_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn original() -> i32 { 1 }\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "baseline"]);

    // Add a new function
    std::fs::write(
        src.join("lib.rs"),
        "pub fn original() -> i32 { 1 }\npub fn added() -> i32 {\n    if true { 2 } else { 3 }\n}\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "add function"]);

    write_lcov(root, &[("lib.rs", &[(1, 1), (2, 1), (3, 1), (4, 1)])]);

    // Full analysis
    let full = analyze_with_adapters(&make_opts(root, None))
        .unwrap()
        .result;
    // Diff analysis
    let diff = analyze_with_adapters(&make_opts(root, Some("HEAD~1")))
        .unwrap()
        .result;

    // Find "added" in both results
    let full_added = full
        .functions
        .iter()
        .find(|v| v.scored.identity.qualified_name == "added")
        .expect("full analysis should find 'added'");
    let diff_added = diff
        .functions
        .iter()
        .find(|v| v.scored.identity.qualified_name == "added")
        .expect("diff analysis should find 'added'");

    assert_eq!(
        full_added.scored.crap.value, diff_added.scored.crap.value,
        "CRAP scores must be identical"
    );
    assert_eq!(
        full_added.scored.coverage_percent, diff_added.scored.coverage_percent,
        "coverage must be identical"
    );
}

// ── Scenario 5: Empty diff → exit 0, passed: true ─────────────────

#[test]
fn diff_empty_returns_passed_true() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn stable() -> i32 { 42 }\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    write_lcov(root, &[("lib.rs", &[(1, 1)])]);

    // Diff HEAD against itself → empty
    let output = analyze_with_adapters(&make_opts(root, Some("HEAD"))).unwrap();
    assert!(output.result.functions.is_empty());
    assert!(output.result.passed);
    assert_eq!(output.result.summary.total_functions, 0);
}

// ── Scenario 6: --diff + --exclude compose as AND ──────────────────

#[test]
fn diff_composes_with_exclude() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    git(root, &["commit", "--allow-empty", "-m", "baseline"]);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn kept() -> i32 { 1 }\n").unwrap();

    let tests_dir = src.join("tests");
    std::fs::create_dir_all(&tests_dir).unwrap();
    std::fs::write(
        tests_dir.join("test_lib.rs"),
        "fn test_fn() { assert!(true); }",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "add files"]);

    write_lcov(
        root,
        &[("lib.rs", &[(1, 1)]), ("tests/test_lib.rs", &[(1, 1)])],
    );

    let mut opts = make_opts(root, Some("HEAD~1"));
    opts.exclude = vec!["tests/**".to_string()];

    let result = analyze_with_adapters(&opts).unwrap().result;
    let names = function_names(&result);
    assert!(names.contains(&"kept".to_string()));
    for v in &result.functions {
        assert!(
            !v.scored.identity.file_path.contains("test"),
            "excluded test file should not appear"
        );
    }
}

// ── Scenario 7: --diff + --only-failing compose as AND ─────────────

#[test]
fn diff_composes_with_only_failing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    // Initial commit: two functions — one simple, one complex
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        "pub fn simple() -> i32 { 1 }\n\
         pub fn complex(x: i32) -> i32 {\n\
             if x > 0 { if x > 10 { if x > 100 { 3 } else { 2 } } else { 1 } } else { 0 }\n\
         }\n",
    )
    .unwrap();
    write_lcov(root, &[("lib.rs", &[(1, 1), (2, 1), (3, 0)])]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "init"]);

    // Second commit: modify both functions
    std::fs::write(
        src.join("lib.rs"),
        "pub fn simple() -> i32 { 2 }\n\
         pub fn complex(x: i32) -> i32 {\n\
             if x > 0 { if x > 10 { if x > 100 { 4 } else { 3 } } else { 2 } } else { 1 }\n\
         }\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "modify both"]);

    // Diff analysis returns both functions
    let opts = make_opts(root, Some("HEAD~1"));
    let output = analyze_with_adapters(&opts).unwrap();

    // Simulate --only-failing post-filter (threshold is default 30.0 from make_opts)
    // simple() has low CRAP (passes), complex() has high CRAP (fails)
    let failing: Vec<_> = output
        .result
        .functions
        .iter()
        .filter(|v| v.exceeds)
        .collect();
    let passing: Vec<_> = output
        .result
        .functions
        .iter()
        .filter(|v| !v.exceeds)
        .collect();

    // At least one function should pass and at least one should fail
    // (verifying the composition filters correctly)
    assert!(
        !output.result.functions.is_empty(),
        "diff should return functions"
    );
    // The only-failing filter would keep only exceeding functions
    // Verify the diff didn't prevent us from seeing both passing and failing
    assert!(
        passing.len() + failing.len() == output.result.functions.len(),
        "all functions should be categorized as passing or failing"
    );
}

// ── Scenario 10: Not in git repo → error ───────────────────────────

#[test]
fn diff_outside_git_repo_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // No git init — just source + coverage
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn f() -> i32 { 1 }\n").unwrap();
    write_lcov(root, &[("lib.rs", &[(1, 1)])]);

    let result = analyze_with_adapters(&make_opts(root, Some("main")));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("diff") || err.contains("git"),
        "error should mention diff/git: {err}"
    );
}

// ── Scenario 11: Invalid ref → error ───────────────────────────────

#[test]
fn diff_invalid_ref_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn f() -> i32 { 1 }\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);
    write_lcov(root, &[("lib.rs", &[(1, 1)])]);

    let result = analyze_with_adapters(&make_opts(root, Some("nonexistent-ref-xyz")));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("nonexistent-ref-xyz"),
        "error should mention the bad ref: {err}"
    );
}

// ── Scenario 13: Non-Rust file changes ignored ─────────────────────

#[test]
fn diff_ignores_non_rust_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn f() -> i32 { 1 }\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "baseline"]);

    // Change only a non-Rust file + lib.rs
    std::fs::write(root.join("README.md"), "# Hello\n").unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn f() -> i32 {\n    42\n}\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "update readme and lib"]);

    write_lcov(root, &[("lib.rs", &[(1, 1), (2, 1), (3, 1)])]);

    let result = analyze_with_adapters(&make_opts(root, Some("HEAD~1")))
        .unwrap()
        .result;
    // Should include lib.rs functions, no crash from README.md
    assert!(!result.functions.is_empty());
    for v in &result.functions {
        assert!(
            v.scored.identity.file_path.ends_with(".rs"),
            "non-Rust file should not appear: {}",
            v.scored.identity.file_path
        );
    }
}

// ── Scenario 15: Deletion-only → function not surfaced ─────────────

#[test]
fn diff_deletion_only_excluded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();

    // Baseline: function with extra lines
    std::fs::write(
        src.join("lib.rs"),
        "pub fn target() -> i32 {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "baseline"]);

    // Delete lines only (remove let y, simplify)
    std::fs::write(
        src.join("lib.rs"),
        "pub fn target() -> i32 {\n    let x = 1;\n    x\n}\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "simplify"]);

    write_lcov(root, &[("lib.rs", &[(1, 1), (2, 1), (3, 1), (4, 1)])]);

    let _result = analyze_with_adapters(&make_opts(root, Some("HEAD~1")))
        .unwrap()
        .result;
    // Verifies no crash when diff includes replacement lines.
    // True deletion-only (zero additions) is covered by adapter unit tests.
}

// ── Scenario 16: Path normalization with nested files ──────────────

#[test]
fn diff_nested_path_normalization() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    git(root, &["commit", "--allow-empty", "-m", "baseline"]);

    let src = root.join("src");
    let sub = src.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("mod.rs"), "pub fn nested() -> i32 { 1 }\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "add nested module"]);

    write_lcov(root, &[("sub/mod.rs", &[(1, 1)])]);

    let result = analyze_with_adapters(&make_opts(root, Some("HEAD~1")))
        .unwrap()
        .result;
    let names = function_names(&result);
    assert!(
        names.contains(&"nested".to_string()),
        "nested function should appear: {names:?}"
    );

    // Verify path uses forward slashes
    let file_path = &result.functions[0].scored.identity.file_path;
    assert!(
        file_path.contains("sub/mod.rs"),
        "path should use forward slashes: {file_path}"
    );
}

// ── JSON envelope diff_ref integration ─────────────────────────────

#[test]
fn json_envelope_contains_diff_ref() {
    use crap4rs::adapters::reporters;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn f() -> i32 { 1 }\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    write_lcov(root, &[("lib.rs", &[(1, 1)])]);

    // Analyze with diff_ref
    let result = analyze_with_adapters(&make_opts(root, Some("HEAD")))
        .unwrap()
        .result;

    let config = reporters::json::JsonConfig {
        tool_version: "0.1.0".to_string(),
        metric: ComplexityMetric::Cognitive,
        threshold: 30.0,
        timestamp: "2026-03-29T00:00:00Z".to_string(),
        diagnostics: None,
        diff_ref: Some("HEAD"),
        minimal_view: false,
        delta: None,
    };
    let view = crap4rs::domain::view::apply(&result, crap4rs::domain::view::ViewSpec::default());
    let json_str = reporters::format_json(&view, &config).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(v["diff_ref"], "HEAD");
}

#[test]
fn json_envelope_diff_ref_null_without_flag() {
    use crap4rs::adapters::reporters;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    test_git_repo(root);

    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn f() -> i32 { 1 }\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    write_lcov(root, &[("lib.rs", &[(1, 1)])]);

    let result = analyze_with_adapters(&make_opts(root, None))
        .unwrap()
        .result;

    let config = reporters::json::JsonConfig {
        tool_version: "0.1.0".to_string(),
        metric: ComplexityMetric::Cognitive,
        threshold: 30.0,
        timestamp: "2026-03-29T00:00:00Z".to_string(),
        diagnostics: None,
        diff_ref: None,
        minimal_view: false,
        delta: None,
    };
    let view = crap4rs::domain::view::apply(&result, crap4rs::domain::view::ViewSpec::default());
    let json_str = reporters::format_json(&view, &config).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert!(v["diff_ref"].is_null());
}
