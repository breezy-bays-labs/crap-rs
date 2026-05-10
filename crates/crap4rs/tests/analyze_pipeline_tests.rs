//! End-to-end pipeline tests for `crap_core::core::analyze`.
//!
//! Lives in crap4rs (not crap-core) because the orchestrator's S4-era
//! signature `analyze<P>(&AnalyzeOptions, &dyn ComplexityPort, &dyn
//! CoveragePort<Diagnostic = P>)` requires concrete adapters; the LCOV
//! parser (`LcovParser`) and `syn`-based complexity walker
//! (`SynComplexityAdapter`) live in this crate.
//!
//! Relocated from `crap_core::core::tests` (formerly
//! `crap4rs::core::tests`) during S4 (#136). The originals exercised the
//! same scenarios when `analyze` constructed its own adapters; the moves
//! are pure mechanical updates to thread the now-injected ports.

use std::fs;
use std::path::Path;

use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::adapters::coverage::LcovParser;
use crap4rs::core::{AnalyzeOptions, analyze};
use crap4rs::domain::threshold::{DEFAULT_THRESHOLD, ThresholdConfig, ThresholdOverride};
use crap4rs::domain::types::{ComplexityMetric, CoverageMetric};

/// Build the pair of adapters used by every test below. `LcovParser`
/// needs the source root for path-stripping; tests that use the helper
/// `setup_test_project(dir)` pass `dir.path().join("src")`.
fn adapters(src: &Path) -> (SynComplexityAdapter, LcovParser) {
    (
        SynComplexityAdapter::new(),
        LcovParser::new(src.to_path_buf()),
    )
}

fn setup_test_project(dir: &Path) {
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join("lib.rs"),
        r#"
pub fn simple() -> i32 {
    42
}

pub fn with_branch(x: i32) -> &'static str {
    if x > 0 {
        "positive"
    } else {
        "non-positive"
    }
}
"#,
    )
    .unwrap();

    fs::write(
        dir.join("lcov.info"),
        "SF:lib.rs\n\
         DA:2,1\n\
         DA:3,1\n\
         DA:6,1\n\
         DA:7,1\n\
         DA:8,1\n\
         DA:9,0\n\
         DA:10,0\n\
         end_of_record\n",
    )
    .unwrap();
}

#[test]
fn analyze_returns_results_for_simple_project() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: DEFAULT_THRESHOLD,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;

    assert_eq!(result.functions.len(), 2);
    assert_eq!(result.summary.total_functions, 2);
    assert_eq!(result.summary.total_files, 1);
}

#[test]
fn analyze_simple_fn_fully_covered_has_low_crap() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: DEFAULT_THRESHOLD,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    let simple = result
        .functions
        .iter()
        .find(|v| v.scored.identity.qualified_name == "simple")
        .expect("should find 'simple' function");

    assert_eq!(simple.scored.crap.value, 1.0);
    assert!(!simple.exceeds);
}

#[test]
fn analyze_branching_fn_partial_coverage_higher_crap() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: DEFAULT_THRESHOLD,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    let branching = result
        .functions
        .iter()
        .find(|v| v.scored.identity.qualified_name == "with_branch")
        .expect("should find 'with_branch' function");

    assert!(branching.scored.complexity > 1);
    assert!(branching.scored.crap.value > 1.0);
}

#[test]
fn analyze_pass_when_all_below_threshold() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: 100.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    assert!(result.passed);
    assert_eq!(result.summary.exceeding_threshold, 0);
}

#[test]
fn analyze_at_exact_threshold_does_not_exceed() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    // simple() has CC=1, 100% coverage → CRAP=1.0
    // Set threshold to exactly 1.0 — should NOT exceed
    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: 1.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    let simple = result
        .functions
        .iter()
        .find(|v| v.scored.identity.qualified_name == "simple")
        .expect("should find 'simple'");

    assert_eq!(simple.scored.crap.value, 1.0);
    assert!(!simple.exceeds, "CRAP at threshold should NOT exceed");
}

#[test]
fn analyze_fail_when_above_threshold() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: 0.5,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    assert!(!result.passed);
    assert!(result.summary.exceeding_threshold > 0);
}

#[test]
fn analyze_no_functions_extracted_errors() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "// just a comment\n").unwrap();
    fs::write(
        dir.path().join("lcov.info"),
        "SF:lib.rs\nDA:1,1\nend_of_record\n",
    )
    .unwrap();

    let (cx, cov) = adapters(&src_dir);
    let opts = AnalyzeOptions {
        src: src_dir,
        coverage: dir.path().join("lcov.info"),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let err = analyze(&opts, &cx, &cov).unwrap_err();
    assert!(err.to_string().contains("no functions extracted"));
}

#[test]
fn analyze_empty_src_dir_errors() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        dir.path().join("lcov.info"),
        "SF:lib.rs\nDA:1,1\nend_of_record\n",
    )
    .unwrap();

    let (cx, cov) = adapters(&src_dir);
    let opts = AnalyzeOptions {
        src: src_dir,
        coverage: dir.path().join("lcov.info"),
        ..AnalyzeOptions::default()
    };

    let err = analyze(&opts, &cx, &cov).unwrap_err();
    assert!(err.to_string().contains("no Rust source files"));
}

#[test]
fn analyze_missing_coverage_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "fn main() {}").unwrap();

    let (cx, cov) = adapters(&src_dir);
    let opts = AnalyzeOptions {
        src: src_dir,
        coverage: dir.path().join("nonexistent.info"),
        ..AnalyzeOptions::default()
    };

    let err = analyze(&opts, &cx, &cov).unwrap_err();
    assert!(err.to_string().contains("failed to read coverage file"));
}

#[test]
fn analyze_exclude_pattern_filters_files() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn kept() -> i32 { 1 }").unwrap();

    let tests_dir = src_dir.join("tests");
    fs::create_dir_all(&tests_dir).unwrap();
    fs::write(
        tests_dir.join("test_lib.rs"),
        "fn test_fn() { assert!(true); }",
    )
    .unwrap();

    fs::write(
        dir.path().join("lcov.info"),
        "SF:lib.rs\nDA:1,1\nend_of_record\n",
    )
    .unwrap();

    let (cx, cov) = adapters(&src_dir);
    let opts = AnalyzeOptions {
        src: src_dir,
        coverage: dir.path().join("lcov.info"),
        exclude: vec!["tests/**".to_string()],
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    for v in &result.functions {
        assert!(
            !v.scored.identity.file_path.contains("test"),
            "excluded file should not appear: {}",
            v.scored.identity.file_path
        );
    }
}

#[test]
fn analyze_with_cyclomatic_metric() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        metric: ComplexityMetric::Cyclomatic,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    for v in &result.functions {
        assert_eq!(v.scored.complexity_metric, ComplexityMetric::Cyclomatic);
    }
}

#[test]
fn summary_computed_correctly() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: DEFAULT_THRESHOLD,
            ..ThresholdConfig::default()
        },
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    let summary = &result.summary;

    assert_eq!(summary.total_functions, 2);
    assert_eq!(summary.total_files, 1);
    assert!(summary.average_crap > 0.0);
    assert!(summary.median_crap > 0.0);
    assert!(summary.max_crap.is_some());
    assert!(summary.worst_function.is_some());
}

#[test]
fn analyze_diff_ref_none_is_backward_compat() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        diff_ref: None,
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    assert_eq!(result.functions.len(), 2);
    assert_eq!(result.summary.total_functions, 2);
}

#[test]
fn analyze_returns_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: DEFAULT_THRESHOLD,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let output = analyze(&opts, &cx, &cov).unwrap();
    let diag = &output.diagnostics;

    assert_eq!(diag.files_found, 1);
    assert_eq!(diag.files_unparseable, 0);
    assert_eq!(diag.functions_extracted, 2);
    assert!(diag.parse_diagnostics.is_empty());
    assert_eq!(diag.functions_matched, 2);
    assert_eq!(diag.functions_no_coverage, 0);
}

#[test]
fn analyze_diagnostics_counts_no_coverage_functions() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn covered() -> i32 { 1 }").unwrap();
    fs::write(
        src_dir.join("other.rs"),
        "pub fn not_covered() -> i32 { 2 }",
    )
    .unwrap();

    fs::write(
        dir.path().join("lcov.info"),
        "SF:lib.rs\nDA:1,1\nend_of_record\n",
    )
    .unwrap();

    let (cx, cov) = adapters(&src_dir);
    let opts = AnalyzeOptions {
        src: src_dir,
        coverage: dir.path().join("lcov.info"),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let output = analyze(&opts, &cx, &cov).unwrap();
    let diag = &output.diagnostics;

    assert_eq!(diag.files_found, 2);
    assert_eq!(diag.functions_extracted, 2);
    assert_eq!(diag.functions_matched, 1);
    assert_eq!(diag.functions_no_coverage, 1);
}

#[test]
fn analyze_diagnostics_surfaces_parse_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("lib.rs"), "pub fn simple() -> i32 { 42 }").unwrap();

    fs::write(
        dir.path().join("lcov.info"),
        "SF:lib.rs\nDA:1,1\nDA:bad_line\nend_of_record\n",
    )
    .unwrap();

    let (cx, cov) = adapters(&src_dir);
    let opts = AnalyzeOptions {
        src: src_dir,
        coverage: dir.path().join("lcov.info"),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let output = analyze(&opts, &cx, &cov).unwrap();
    assert_eq!(output.diagnostics.parse_diagnostics.len(), 1);
}

#[test]
fn analyze_with_per_path_overrides() {
    let dir = tempfile::tempdir().unwrap();
    setup_test_project(dir.path());
    let src = dir.path().join("src");
    let (cx, cov) = adapters(&src);

    let opts = AnalyzeOptions {
        src,
        coverage: dir.path().join("lcov.info"),
        threshold_config: ThresholdConfig {
            global: 100.0,
            overrides: vec![ThresholdOverride {
                pattern: "lib.rs".to_string(),
                threshold: 0.5,
            }],
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    assert!(!result.passed);
    for v in &result.functions {
        assert_eq!(v.threshold, 0.5);
        assert!(v.exceeds);
    }
}

#[test]
fn analyze_options_default_coverage_metric_is_line() {
    let opts = AnalyzeOptions::default();
    assert_eq!(opts.coverage_metric, CoverageMetric::Line);
}

#[test]
fn analyze_passes_branch_data_through() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join("lib.rs"),
        r#"
pub fn with_branch(x: i32) -> &'static str {
    if x > 0 {
        "positive"
    } else {
        "non-positive"
    }
}
"#,
    )
    .unwrap();

    fs::write(
        dir.path().join("lcov.info"),
        "SF:lib.rs\n\
         DA:2,1\n\
         DA:3,1\n\
         DA:4,1\n\
         DA:5,0\n\
         DA:6,0\n\
         BRDA:2,0,0,1\n\
         BRDA:2,0,1,0\n\
         end_of_record\n",
    )
    .unwrap();

    let (cx, cov) = adapters(&src_dir);
    let opts = AnalyzeOptions {
        src: src_dir,
        coverage: dir.path().join("lcov.info"),
        respect_gitignore: false,
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;
    assert_eq!(result.functions.len(), 1);
    let verdict = &result.functions[0];
    assert!(verdict.scored.crap.value > 0.0);
}
