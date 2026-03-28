//! Wiring layer — composes adapters through ports, exposes `analyze()` API.
//!
//! Mirrors crap4ts's core/analyze.ts: reads source files, extracts complexity,
//! parses coverage, matches functions, scores, and produces results.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;

use crate::adapters::complexity::SynComplexityAdapter;
use crate::adapters::coverage::LcovParser;
use crate::domain::crap::compute_crap;
use crate::domain::matching::match_functions;
use crate::domain::summary::compute_summary;
use crate::domain::threshold::DEFAULT_THRESHOLD;
use crate::domain::types::{AnalysisResult, ComplexityMetric, FunctionVerdict, ScoredFunction};
use crate::ports::{ComplexityPort, CoveragePort};

/// Options for running a CRAP analysis.
#[derive(Debug)]
pub struct AnalyzeOptions {
    /// Root directory of Rust source files to analyze.
    pub src: PathBuf,
    /// Path to the LCOV coverage file.
    pub coverage: PathBuf,
    /// CRAP score threshold — functions above this are flagged.
    pub threshold: f64,
    /// Which complexity metric to use.
    pub metric: ComplexityMetric,
    /// Glob patterns to exclude from file discovery.
    pub exclude: Vec<String>,
    /// Whether to respect .gitignore files during file discovery.
    pub respect_gitignore: bool,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            src: PathBuf::from("src"),
            coverage: PathBuf::from("lcov.info"),
            threshold: DEFAULT_THRESHOLD,
            metric: ComplexityMetric::default(),
            exclude: Vec::new(),
            respect_gitignore: true,
        }
    }
}

/// Run CRAP analysis: discover files, extract complexity, parse coverage,
/// match, score, and produce results.
pub fn analyze(options: &AnalyzeOptions) -> Result<AnalysisResult> {
    // Canonicalize src path so LCOV absolute paths can be stripped correctly
    let src_canonical = options
        .src
        .canonicalize()
        .unwrap_or_else(|_| options.src.clone());

    // 1. Discover .rs source files
    let source_files =
        discover_rust_files(&options.src, &options.exclude, options.respect_gitignore)?;
    if source_files.is_empty() {
        bail!(
            "no Rust source files found in {}\n  \
             hint: check that --src points to a directory containing .rs files",
            options.src.display()
        );
    }

    // 2. Read and parse coverage data
    let coverage_data = std::fs::read_to_string(&options.coverage).with_context(|| {
        format!(
            "failed to read coverage file: {}",
            options.coverage.display()
        )
    })?;
    let parser = LcovParser::new(src_canonical);
    let parse_output = parser
        .parse(&coverage_data)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // 3. Extract complexity from each source file
    let complexity_adapter = SynComplexityAdapter::new();
    let mut all_complexities = Vec::new();

    for file_path in &source_files {
        let source = std::fs::read_to_string(file_path)
            .with_context(|| format!("failed to read source file: {}", file_path.display()))?;

        let relative = file_path
            .strip_prefix(&options.src)
            .unwrap_or(file_path)
            .to_string_lossy()
            .replace('\\', "/");

        match complexity_adapter.extract(&source, &relative, options.metric) {
            Ok(fns) => all_complexities.extend(fns),
            Err(e) => {
                // Non-fatal: skip unparseable files (e.g., proc-macro crates)
                eprintln!("warning: skipping {relative}: {e}");
            }
        }
    }

    // 4. Match complexity with coverage using line-range join
    let matched = match_functions(&all_complexities, &parse_output.coverage);

    // 5. Score each function, produce verdicts, and build result
    score_and_summarize(&matched, options.threshold)
}

/// Score matched functions against the threshold and produce the final result.
fn score_and_summarize(
    matched: &[(
        crate::domain::types::FunctionComplexity,
        crate::domain::types::FunctionCoverage,
    )],
    threshold: f64,
) -> Result<AnalysisResult> {
    let mut verdicts = Vec::with_capacity(matched.len());
    for (comp, cov) in matched {
        let crap = compute_crap(comp.complexity, cov.line_coverage.percent)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        verdicts.push(FunctionVerdict {
            scored: ScoredFunction {
                identity: comp.identity.clone(),
                complexity: comp.complexity,
                complexity_metric: comp.metric,
                coverage_percent: cov.line_coverage.percent,
                crap,
            },
            threshold,
            exceeds: crap.value > threshold,
        });
    }

    let summary = compute_summary(&verdicts);
    let passed = verdicts.iter().all(|v| !v.exceeds);

    Ok(AnalysisResult {
        functions: verdicts,
        summary,
        passed,
    })
}

/// Walk the source directory and collect all `.rs` files, respecting
/// .gitignore and user-provided exclude patterns.
fn discover_rust_files(
    src: &Path,
    exclude: &[String],
    respect_gitignore: bool,
) -> Result<Vec<PathBuf>> {
    let mut builder = WalkBuilder::new(src);
    builder.git_ignore(respect_gitignore);

    // Add exclude patterns as overrides
    if !exclude.is_empty() {
        let mut overrides = ignore::overrides::OverrideBuilder::new(src);
        for pattern in exclude {
            overrides
                .add(&format!("!{pattern}"))
                .with_context(|| format!("invalid exclude pattern: {pattern}"))?;
        }
        builder.overrides(overrides.build()?);
    }

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry?;
        if entry.file_type().is_some_and(|ft| ft.is_file())
            && entry.path().extension().is_some_and(|ext| ext == "rs")
        {
            files.push(entry.into_path());
        }
    }

    // Sort for deterministic output
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_project(dir: &Path) {
        // Create a minimal Rust source file
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

        // Create LCOV coverage data covering both functions
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

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            threshold: DEFAULT_THRESHOLD,
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
        };

        let result = analyze(&opts).unwrap();

        assert_eq!(result.functions.len(), 2);
        assert_eq!(result.summary.total_functions, 2);
        assert_eq!(result.summary.total_files, 1);
    }

    #[test]
    fn analyze_simple_fn_fully_covered_has_low_crap() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            threshold: DEFAULT_THRESHOLD,
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
        };

        let result = analyze(&opts).unwrap();
        let simple = result
            .functions
            .iter()
            .find(|v| v.scored.identity.qualified_name == "simple")
            .expect("should find 'simple' function");

        // simple() has complexity 1, full coverage → CRAP = 1.0
        assert_eq!(simple.scored.crap.value, 1.0);
        assert!(!simple.exceeds);
    }

    #[test]
    fn analyze_branching_fn_partial_coverage_higher_crap() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            threshold: DEFAULT_THRESHOLD,
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
        };

        let result = analyze(&opts).unwrap();
        let branching = result
            .functions
            .iter()
            .find(|v| v.scored.identity.qualified_name == "with_branch")
            .expect("should find 'with_branch' function");

        // with_branch has complexity > 1 and partial coverage → higher CRAP
        assert!(branching.scored.complexity > 1);
        assert!(branching.scored.crap.value > 1.0);
    }

    #[test]
    fn analyze_pass_when_all_below_threshold() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            threshold: 100.0, // Very high threshold
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
        };

        let result = analyze(&opts).unwrap();
        assert!(result.passed);
        assert_eq!(result.summary.exceeding_threshold, 0);
    }

    #[test]
    fn analyze_at_exact_threshold_does_not_exceed() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        // simple() has CC=1, 100% coverage → CRAP=1.0
        // Set threshold to exactly 1.0 — should NOT exceed
        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            threshold: 1.0,
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
        };

        let result = analyze(&opts).unwrap();
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

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            threshold: 0.5, // Very low threshold — everything exceeds
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
        };

        let result = analyze(&opts).unwrap();
        assert!(!result.passed);
        assert!(result.summary.exceeding_threshold > 0);
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

        let opts = AnalyzeOptions {
            src: src_dir,
            coverage: dir.path().join("lcov.info"),
            ..AnalyzeOptions::default()
        };

        let err = analyze(&opts).unwrap_err();
        assert!(err.to_string().contains("no Rust source files"));
    }

    #[test]
    fn analyze_missing_coverage_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("lib.rs"), "fn main() {}").unwrap();

        let opts = AnalyzeOptions {
            src: src_dir,
            coverage: dir.path().join("nonexistent.info"),
            ..AnalyzeOptions::default()
        };

        let err = analyze(&opts).unwrap_err();
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

        let opts = AnalyzeOptions {
            src: src_dir,
            coverage: dir.path().join("lcov.info"),
            exclude: vec!["tests/**".to_string()],
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap();
        // Should only find lib.rs, not tests/test_lib.rs
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

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            metric: ComplexityMetric::Cyclomatic,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap();
        // All functions should use cyclomatic metric
        for v in &result.functions {
            assert_eq!(v.scored.complexity_metric, ComplexityMetric::Cyclomatic);
        }
    }

    #[test]
    fn discover_rust_files_finds_nested() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("lib.rs"), "").unwrap();
        fs::write(src.join("sub").join("mod.rs"), "").unwrap();
        fs::write(src.join("readme.txt"), "").unwrap();

        let files = discover_rust_files(&src, &[], false).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.extension().unwrap() == "rs"));
    }

    #[test]
    fn discover_rust_files_sorted_deterministically() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("z.rs"), "").unwrap();
        fs::write(src.join("a.rs"), "").unwrap();
        fs::write(src.join("m.rs"), "").unwrap();

        let files = discover_rust_files(&src, &[], false).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.file_name().unwrap()).collect();
        assert_eq!(names, vec!["a.rs", "m.rs", "z.rs"]);
    }

    #[test]
    fn summary_computed_correctly() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            threshold: DEFAULT_THRESHOLD,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap();
        let summary = &result.summary;

        assert_eq!(summary.total_functions, 2);
        assert_eq!(summary.total_files, 1);
        assert!(summary.average_crap > 0.0);
        assert!(summary.median_crap > 0.0);
        assert!(summary.max_crap.is_some());
        assert!(summary.worst_function.is_some());
    }
}
