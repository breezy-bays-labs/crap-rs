//! Integration test: core::analyze() end-to-end.
//!
//! Tests the full pipeline: file discovery → complexity extraction →
//! coverage parsing → matching → scoring → verdict → summary.

use crap4rs::core::{AnalyzeOptions, analyze};
use crap4rs::domain::types::ComplexityMetric;
use std::path::PathBuf;

/// Self-referential test: crap4rs analyzes its own source code.
///
/// Uses a hand-crafted LCOV fixture to avoid requiring cargo-llvm-cov
/// at test time. The fixture covers known functions in the domain layer.
#[test]
fn self_referential_analysis() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");

    // Create a minimal LCOV that covers a few known domain functions
    let lcov = make_self_coverage(&src);
    let lcov_path = std::env::temp_dir().join("crap4rs-self-test.lcov");
    std::fs::write(&lcov_path, &lcov).unwrap();

    let opts = AnalyzeOptions {
        src: src.clone(),
        coverage: lcov_path,
        threshold: 30.0,
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
    };

    let result = analyze(&opts).unwrap();

    // Must find functions (we know crap4rs has many)
    assert!(
        result.functions.len() > 10,
        "Expected >10 functions, got {}",
        result.functions.len()
    );

    // Must span multiple files
    assert!(
        result.summary.total_files > 3,
        "Expected >3 files, got {}",
        result.summary.total_files
    );

    // Summary must be consistent
    assert_eq!(result.summary.total_functions, result.functions.len());
    assert!(result.summary.average_crap > 0.0);
    assert!(result.summary.max_crap.is_some());
    assert!(result.summary.worst_function.is_some());

    // Risk distribution must add up
    let dist = &result.summary.distribution;
    assert_eq!(
        dist.low + dist.acceptable + dist.moderate + dist.high,
        result.summary.total_functions
    );

    // pass/fail must be consistent with exceeding count
    if result.summary.exceeding_threshold > 0 {
        assert!(!result.passed);
    } else {
        assert!(result.passed);
    }
}

#[test]
fn self_referential_with_cyclomatic() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");

    let lcov = make_self_coverage(&src);
    let lcov_path = std::env::temp_dir().join("crap4rs-self-cyclomatic.lcov");
    std::fs::write(&lcov_path, &lcov).unwrap();

    let opts = AnalyzeOptions {
        src,
        coverage: lcov_path,
        threshold: 30.0,
        metric: ComplexityMetric::Cyclomatic,
        exclude: Vec::new(),
        respect_gitignore: false,
    };

    let result = analyze(&opts).unwrap();

    // All functions should use cyclomatic metric
    for v in &result.functions {
        assert_eq!(v.scored.complexity_metric, ComplexityMetric::Cyclomatic);
    }

    // Should still find the same set of functions
    assert!(result.functions.len() > 10);
}

#[test]
fn self_referential_known_functions_present() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");

    let lcov = make_self_coverage(&src);
    let lcov_path = std::env::temp_dir().join("crap4rs-self-known.lcov");
    std::fs::write(&lcov_path, &lcov).unwrap();

    let opts = AnalyzeOptions {
        src,
        coverage: lcov_path,
        threshold: 30.0,
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
    };

    let result = analyze(&opts).unwrap();

    let names: Vec<&str> = result
        .functions
        .iter()
        .map(|v| v.scored.identity.qualified_name.as_str())
        .collect();

    // These functions must exist in crap4rs source
    assert!(names.contains(&"compute_crap"), "missing compute_crap");
    assert!(
        names.contains(&"match_functions"),
        "missing match_functions"
    );
    assert!(
        names.contains(&"compute_summary"),
        "missing compute_summary"
    );
    assert!(names.contains(&"analyze"), "missing analyze");
    assert!(
        names.contains(&"SynComplexityAdapter::new"),
        "missing SynComplexityAdapter::new"
    );
    assert!(
        names.contains(&"LcovParser::new"),
        "missing LcovParser::new"
    );
}

#[test]
fn exclude_pattern_filters_correctly() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");

    let lcov = make_self_coverage(&src);
    let lcov_path = std::env::temp_dir().join("crap4rs-self-exclude.lcov");
    std::fs::write(&lcov_path, &lcov).unwrap();

    // Analyze all files
    let all_opts = AnalyzeOptions {
        src: src.clone(),
        coverage: lcov_path.clone(),
        threshold: 100.0,
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
    };
    let all_result = analyze(&all_opts).unwrap();

    // Analyze with adapter exclusion
    let filtered_opts = AnalyzeOptions {
        src,
        coverage: lcov_path,
        threshold: 100.0,
        metric: ComplexityMetric::Cognitive,
        exclude: vec!["adapters/**".to_string()],
        respect_gitignore: false,
    };
    let filtered_result = analyze(&filtered_opts).unwrap();

    // Filtered should have fewer functions
    assert!(
        filtered_result.functions.len() < all_result.functions.len(),
        "Exclusion should reduce function count: {} vs {}",
        filtered_result.functions.len(),
        all_result.functions.len()
    );

    // No adapter functions in filtered result
    for v in &filtered_result.functions {
        assert!(
            !v.scored.identity.file_path.starts_with("adapters/"),
            "excluded adapter file appeared: {}",
            v.scored.identity.file_path
        );
    }
}

/// Generate minimal LCOV covering all .rs files in the src directory.
/// Every line gets DA:N,1 (hit once) — simple but sufficient for testing.
fn make_self_coverage(src: &std::path::Path) -> String {
    let mut lcov = String::new();
    collect_rs_files(src, src, &mut lcov);
    lcov
}

fn collect_rs_files(root: &std::path::Path, dir: &std::path::Path, lcov: &mut String) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(root, &path, lcov);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let source = std::fs::read_to_string(&path).unwrap();
            let line_count = source.lines().count();

            lcov.push_str(&format!("SF:{relative}\n"));
            for line in 1..=line_count {
                lcov.push_str(&format!("DA:{line},1\n"));
            }
            lcov.push_str("end_of_record\n");
        }
    }
}
