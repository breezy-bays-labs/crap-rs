//! Integration test: core::analyze() end-to-end.
//!
//! Tests the full pipeline: file discovery → complexity extraction →
//! coverage parsing → matching → scoring → verdict → summary.

use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::adapters::coverage::LcovParser;
use crap4rs::core::identity::IdentityBase;
use crap4rs::core::{AnalyzeOptions, analyze};
use crap4rs::domain::threshold::ThresholdConfig;
use crap4rs::domain::types::{ComplexityMetric, MissingCoveragePolicy};
use std::path::{Path, PathBuf};

/// Construct the LCOV/syn adapter pair rooted at `src`. Threaded
/// through the post-S4 (#136) `analyze` signature, which takes
/// `&dyn ComplexityPort` + `&dyn CoveragePort<Diagnostic = P>` from
/// the caller — the orchestrator no longer constructs adapters
/// internally.
fn adapters(src: &Path) -> (SynComplexityAdapter, LcovParser) {
    (
        SynComplexityAdapter::new(),
        LcovParser::new(src.to_path_buf()),
    )
}

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
    let tmp = tempfile::tempdir().unwrap();
    let lcov_path = tmp.path().join("lcov.info");
    std::fs::write(&lcov_path, &lcov).unwrap();

    let opts = AnalyzeOptions {
        identity_base: IdentityBase::SrcRelative(src.clone()),
        src: vec![src.clone()],
        coverage: lcov_path,
        threshold_config: ThresholdConfig {
            global: 30.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        extensions: vec!["rs".to_string()],
        ..AnalyzeOptions::default()
    };

    let (cx, cov) = adapters(&src);
    let result = analyze(&opts, &cx, &cov).unwrap().result;

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
    let tmp = tempfile::tempdir().unwrap();
    let lcov_path = tmp.path().join("lcov.info");
    std::fs::write(&lcov_path, &lcov).unwrap();

    let (cx, cov) = adapters(&src);
    let opts = AnalyzeOptions {
        identity_base: IdentityBase::SrcRelative(src.clone()),
        src: vec![src],
        coverage: lcov_path,
        threshold_config: ThresholdConfig {
            global: 30.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cyclomatic,
        exclude: Vec::new(),
        respect_gitignore: false,
        extensions: vec!["rs".to_string()],
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;

    // All functions should use cyclomatic metric
    for v in &result.functions {
        assert_eq!(v.scored.complexity_metric, ComplexityMetric::Cyclomatic);
    }

    // Should still find the same set of functions
    assert!(result.functions.len() > 10);
}

/// End-to-end snapshot of all three missing-coverage policies (AC #7):
/// a single function whose file is absent from coverage scores three
/// different ways. Complexity `c` is read from the result rather than
/// hardcoded, so the CRAP assertions stay robust to the walker's exact
/// cognitive count.
#[test]
fn missing_coverage_policy_changes_outputs_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    // Cognitive complexity > 1 so the 0%-vs-100% CRAP scores differ.
    std::fs::write(
        src.join("lib.rs"),
        "pub fn classify(n: i32) -> &'static str {\n\
         \x20   if n < 0 { \"neg\" } else if n == 0 { \"zero\" } else { \"pos\" }\n\
         }\n",
    )
    .unwrap();
    // LCOV covers an unrelated file, so `lib.rs` is absent from coverage
    // and the policy decides its fate.
    let lcov_path = tmp.path().join("lcov.info");
    std::fs::write(&lcov_path, "SF:unrelated.rs\nDA:1,1\nend_of_record\n").unwrap();

    let opts_for = |policy| AnalyzeOptions {
        identity_base: IdentityBase::SrcRelative(src.clone()),
        src: vec![src.clone()],
        coverage: lcov_path.clone(),
        threshold_config: ThresholdConfig {
            global: 1000.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        extensions: vec!["rs".to_string()],
        missing_coverage_policy: policy,
        ..AnalyzeOptions::default()
    };

    // Pessimistic: present, 0% coverage, CRAP = c² + c.
    let (cx, cov) = adapters(&src);
    let pess = analyze(&opts_for(MissingCoveragePolicy::Pessimistic), &cx, &cov)
        .unwrap()
        .result;
    assert_eq!(pess.functions.len(), 1, "pessimistic keeps the function");
    let f = &pess.functions[0].scored;
    let c = f.complexity as f64;
    assert!(c > 1.0, "fixture complexity must exceed 1, got {c}");
    assert_eq!(f.coverage_percent, 0.0);
    assert!(
        (f.crap.value - (c * c + c)).abs() < 1e-9,
        "pessimistic CRAP must be c²+c, got {} (c={c})",
        f.crap.value
    );

    // Optimistic: present, 100% coverage, CRAP = c.
    let (cx, cov) = adapters(&src);
    let opt = analyze(&opts_for(MissingCoveragePolicy::Optimistic), &cx, &cov)
        .unwrap()
        .result;
    assert_eq!(opt.functions.len(), 1, "optimistic keeps the function");
    let f = &opt.functions[0].scored;
    assert_eq!(f.coverage_percent, 100.0);
    assert!(
        (f.crap.value - c).abs() < 1e-9,
        "optimistic CRAP must equal complexity {c}, got {}",
        f.crap.value
    );

    // Skip: the function is omitted from the result entirely.
    let (cx, cov) = adapters(&src);
    let skip = analyze(&opts_for(MissingCoveragePolicy::Skip), &cx, &cov)
        .unwrap()
        .result;
    assert!(
        skip.functions.is_empty(),
        "skip omits functions whose file is absent from coverage"
    );
    assert!(skip.passed, "an empty skip result still passes the gate");
}

#[test]
fn self_referential_known_functions_present() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("src");

    let lcov = make_self_coverage(&src);
    let tmp = tempfile::tempdir().unwrap();
    let lcov_path = tmp.path().join("lcov.info");
    std::fs::write(&lcov_path, &lcov).unwrap();

    let (cx, cov) = adapters(&src);
    let opts = AnalyzeOptions {
        identity_base: IdentityBase::SrcRelative(src.clone()),
        src: vec![src],
        coverage: lcov_path,
        threshold_config: ThresholdConfig {
            global: 30.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        extensions: vec!["rs".to_string()],
        ..AnalyzeOptions::default()
    };

    let result = analyze(&opts, &cx, &cov).unwrap().result;

    let names: Vec<&str> = result
        .functions
        .iter()
        .map(|v| v.scored.identity.qualified_name.as_str())
        .collect();

    // Sentinel functions that must exist in crap4rs's adapter layers
    // (post-S4 monorepo split — `analyze`, domain helpers, and the
    // CLI dispatch all moved to crap-core, so the only landlord
    // functions remaining in crap4rs/src are the language-specific
    // adapter constructors and the LCOV parse machinery; main.rs is a
    // 20-line shell that doesn't expose function-name surface).
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
    let tmp_all = tempfile::tempdir().unwrap();
    let lcov_path = tmp_all.path().join("lcov.info");
    std::fs::write(&lcov_path, &lcov).unwrap();

    let (cx, cov) = adapters(&src);

    // Analyze all files
    let all_opts = AnalyzeOptions {
        identity_base: IdentityBase::SrcRelative(src.clone()),
        src: vec![src.clone()],
        coverage: lcov_path.clone(),
        threshold_config: ThresholdConfig {
            global: 100.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: Vec::new(),
        respect_gitignore: false,
        extensions: vec!["rs".to_string()],
        ..AnalyzeOptions::default()
    };
    let all_result = analyze(&all_opts, &cx, &cov).unwrap().result;

    // Analyze with adapter exclusion
    let filtered_opts = AnalyzeOptions {
        identity_base: IdentityBase::SrcRelative(src.clone()),
        src: vec![src],
        coverage: lcov_path,
        threshold_config: ThresholdConfig {
            global: 100.0,
            ..ThresholdConfig::default()
        },
        metric: ComplexityMetric::Cognitive,
        exclude: vec!["adapters/**".to_string()],
        respect_gitignore: false,
        extensions: vec!["rs".to_string()],
        ..AnalyzeOptions::default()
    };
    let filtered_result = analyze(&filtered_opts, &cx, &cov).unwrap().result;

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
