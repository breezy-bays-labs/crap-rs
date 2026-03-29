//! Wiring layer — composes adapters through ports, exposes `analyze()` API.
//!
//! Mirrors crap4ts's core/analyze.ts: reads source files, extracts complexity,
//! parses coverage, matches functions, scores, and produces results.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;

use crate::adapters::complexity::SynComplexityAdapter;
use crate::adapters::coverage::LcovParser;
use crate::adapters::diff::GitDiffAdapter;
use crate::domain::crap::compute_crap;
use crate::domain::matching::{match_functions, overlaps_any};
use crate::domain::summary::compute_summary;
use crate::domain::threshold::ThresholdConfig;
use crate::domain::types::{
    AnalysisDiagnostics, AnalysisResult, ComplexityMetric, CoverageMetric, FileChangeKind,
    FunctionVerdict, ScoredFunction,
};
use crate::ports::{ComplexityPort, CoveragePort, DiffPort};

/// Options for running a CRAP analysis.
#[derive(Debug)]
pub struct AnalyzeOptions {
    /// Root directory of Rust source files to analyze.
    pub src: PathBuf,
    /// Path to the LCOV coverage file.
    pub coverage: PathBuf,
    /// Threshold configuration with optional per-path overrides.
    pub threshold_config: ThresholdConfig,
    /// Which complexity metric to use.
    pub metric: ComplexityMetric,
    /// Which coverage metric to use for analysis.
    pub coverage_metric: CoverageMetric,
    /// Glob patterns to exclude from file discovery.
    pub exclude: Vec<String>,
    /// Whether to respect .gitignore files during file discovery.
    pub respect_gitignore: bool,
    /// Git ref to diff against. When set, only changed functions are analyzed.
    pub diff_ref: Option<String>,
}

/// Full output from an analysis run, including both the scored results
/// and process diagnostics (surfaced by `--verbose`).
#[derive(Debug)]
pub struct AnalysisOutput {
    pub result: AnalysisResult,
    pub diagnostics: AnalysisDiagnostics,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            src: PathBuf::from("src"),
            coverage: PathBuf::from("lcov.info"),
            threshold_config: ThresholdConfig::default(),
            metric: ComplexityMetric::default(),
            coverage_metric: CoverageMetric::default(),
            exclude: Vec::new(),
            respect_gitignore: true,
            diff_ref: None,
        }
    }
}

/// Run CRAP analysis: discover files, extract complexity, parse coverage,
/// match, score, and produce results with diagnostics.
pub fn analyze(options: &AnalyzeOptions) -> Result<AnalysisOutput> {
    // Canonicalize src path so LCOV absolute paths can be stripped correctly
    let src_canonical = options
        .src
        .canonicalize()
        .unwrap_or_else(|_| options.src.clone());

    // 1. Discover .rs source files
    let mut source_files =
        discover_rust_files(&options.src, &options.exclude, options.respect_gitignore)?;
    let files_found = source_files.len();
    if source_files.is_empty() {
        bail!(
            "no Rust source files found in {}\n  \
             hint: check that --src points to a directory containing .rs files",
            options.src.display()
        );
    }

    // Phase 1: file intersection — filter to only changed files when --diff is set
    let diff_data = if let Some(ref diff_ref) = options.diff_ref {
        let diff_result =
            compute_diff_regions(diff_ref, &src_canonical, &options.src, &source_files)?;

        source_files.retain(|p| {
            let rel = src_relative_path(p, &options.src);
            diff_result.contains_key(&*rel)
        });
        if source_files.is_empty() {
            return Ok(AnalysisOutput {
                result: empty_passing_result(),
                diagnostics: AnalysisDiagnostics {
                    parse_diagnostics: vec![],
                    files_found,
                    files_unparseable: 0,
                    functions_extracted: 0,
                    functions_matched: 0,
                    functions_no_coverage: 0,
                },
            });
        }
        Some(diff_result)
    } else {
        None
    };

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
    let parse_diagnostics = parse_output.diagnostics;

    // 3. Extract complexity from each source file
    let complexity_adapter = SynComplexityAdapter::new();
    let mut all_complexities = Vec::new();
    let mut files_unparseable = 0usize;

    for file_path in &source_files {
        let source = std::fs::read_to_string(file_path)
            .with_context(|| format!("failed to read source file: {}", file_path.display()))?;

        let relative = src_relative_path(file_path, &options.src);

        match complexity_adapter.extract(&source, &relative, options.metric) {
            Ok(fns) => all_complexities.extend(fns),
            Err(e) => {
                // Non-fatal: skip unparseable files (e.g., proc-macro crates)
                files_unparseable += 1;
                eprintln!("warning: skipping {relative}: {e}");
            }
        }
    }

    let functions_extracted = all_complexities.len();

    // Phase 2: hunk overlap — filter to only functions overlapping changed hunks
    if let Some(ref diff_result) = diff_data {
        all_complexities.retain(|comp| match diff_result.get(&comp.identity.file_path) {
            Some(FileChangeKind::NewFile) => true,
            Some(FileChangeKind::Modified(ranges)) => overlaps_any(&comp.identity.span, ranges),
            None => false,
        });
        if all_complexities.is_empty() {
            return Ok(AnalysisOutput {
                result: empty_passing_result(),
                diagnostics: AnalysisDiagnostics {
                    parse_diagnostics,
                    files_found,
                    files_unparseable,
                    functions_extracted,
                    functions_matched: 0,
                    functions_no_coverage: 0,
                },
            });
        }
    }

    if all_complexities.is_empty() {
        bail!(
            "no functions extracted from source files in {}\n  \
             hint: check that source files contain valid Rust function definitions",
            options.src.display()
        );
    }

    // 4. Match complexity with coverage using line-range join
    let matched = match_functions(
        &all_complexities,
        &parse_output.coverage,
        parse_output.branches.as_ref(),
    );

    // Count functions with no LCOV data (file not in coverage map)
    let functions_no_coverage = matched
        .iter()
        .filter(|(comp, _)| !parse_output.coverage.contains_key(&comp.identity.file_path))
        .count();
    let functions_matched = matched.len() - functions_no_coverage;

    // 5. Compile threshold overrides for glob matching
    let resolver = ThresholdResolver::new(&options.threshold_config)?;

    // 6. Score each function, produce verdicts, and build result
    let result = score_and_summarize(&matched, &resolver)?;

    let diagnostics = AnalysisDiagnostics {
        parse_diagnostics,
        files_found,
        files_unparseable,
        functions_extracted,
        functions_matched,
        functions_no_coverage,
    };

    debug_assert_eq!(
        diagnostics.functions_matched + diagnostics.functions_no_coverage,
        result.functions.len(),
        "diagnostics counts must partition scored functions"
    );

    Ok(AnalysisOutput {
        result,
        diagnostics,
    })
}

/// Resolves per-function thresholds using glob-based overrides.
///
/// Compiled once per analysis run. Patterns are evaluated in declaration
/// order with last-match-wins semantics.
struct ThresholdResolver {
    global: f64,
    overrides: Vec<(globset::GlobMatcher, f64)>,
}

impl ThresholdResolver {
    fn new(config: &ThresholdConfig) -> Result<Self> {
        let overrides = config
            .overrides
            .iter()
            .map(|o| {
                let glob = globset::Glob::new(&o.pattern)
                    .with_context(|| format!("invalid glob pattern: {}", o.pattern))?
                    .compile_matcher();
                Ok((glob, o.threshold))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            global: config.global,
            overrides,
        })
    }

    /// Resolve the threshold for a given file path.
    /// Last matching override wins; falls back to global.
    fn resolve(&self, file_path: &str) -> f64 {
        let mut threshold = self.global;
        for (matcher, override_threshold) in &self.overrides {
            if matcher.is_match(file_path) {
                threshold = *override_threshold;
            }
        }
        threshold
    }
}

/// Score matched functions against the threshold config and produce the final result.
fn score_and_summarize(
    matched: &[(
        crate::domain::types::FunctionComplexity,
        crate::domain::types::FunctionCoverage,
    )],
    resolver: &ThresholdResolver,
) -> Result<AnalysisResult> {
    let mut verdicts = Vec::with_capacity(matched.len());
    for (comp, cov) in matched {
        let crap = compute_crap(comp.complexity, cov.line_coverage.percent)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let threshold = resolver.resolve(&comp.identity.file_path);

        verdicts.push(FunctionVerdict {
            scored: ScoredFunction {
                identity: comp.identity.clone(),
                complexity: comp.complexity,
                complexity_metric: comp.metric,
                coverage_percent: cov.line_coverage.percent,
                crap,
                contributors: comp.contributors.clone(),
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

/// Find the git repository root by running `git rev-parse --show-toplevel`.
/// Strip `src_root` prefix from a path, returning a forward-slash-normalised string.
/// Panics if the path is not under `src_root` (a bug in the file walker).
fn src_relative_path(path: &Path, src_root: &Path) -> String {
    path.strip_prefix(src_root)
        .expect("discovered file should be under the source root")
        .to_string_lossy()
        .replace('\\', "/")
}

/// Compute diff regions for the given ref, reconciling repo-root-relative paths
/// from `git diff` with src-relative paths used by the complexity adapter.
fn compute_diff_regions(
    diff_ref: &str,
    src_canonical: &Path,
    src_original: &Path,
    source_files: &[PathBuf],
) -> Result<std::collections::HashMap<String, FileChangeKind>> {
    let diff_adapter = GitDiffAdapter::new();

    // Git diff outputs paths relative to repo root, but the complexity
    // adapter uses paths relative to options.src. We bridge via src_prefix.
    let repo_root = git_toplevel(src_canonical)?;
    let src_prefix = src_canonical
        .strip_prefix(&repo_root)
        .with_context(|| {
            format!(
                "--src directory {} is not inside the git repository at {}\n  \
                 hint: --diff requires --src to be within the git work tree",
                src_canonical.display(),
                repo_root.display(),
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");

    // Paths passed to `git diff -- <paths>` must be repo-relative.
    // source_files use the original (possibly symlinked) src path.
    let repo_relative_paths: Vec<String> = source_files
        .iter()
        .map(|p| {
            let src_rel = src_relative_path(p, src_original);
            if src_prefix.is_empty() {
                src_rel
            } else {
                format!("{src_prefix}/{src_rel}")
            }
        })
        .collect();

    let raw_diff = diff_adapter
        .changed_regions(diff_ref, &repo_root, &repo_relative_paths)
        .map_err(|e| anyhow::anyhow!(e))?;

    // Strip src_prefix from diff result keys to get src-relative paths
    let prefix_with_slash = if src_prefix.is_empty() {
        String::new()
    } else {
        format!("{src_prefix}/")
    };
    Ok(raw_diff
        .into_iter()
        .filter_map(|(path, kind)| {
            if prefix_with_slash.is_empty() {
                Some((path, kind))
            } else {
                path.strip_prefix(&prefix_with_slash)
                    .map(|stripped| (stripped.to_string(), kind))
            }
        })
        .collect())
}

fn git_toplevel(from_dir: &Path) -> Result<PathBuf> {
    let output = std::process::Command::new("git")
        .current_dir(from_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;

    if output.status.success() {
        let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
        PathBuf::from(&toplevel)
            .canonicalize()
            .with_context(|| format!("failed to canonicalize git toplevel: {toplevel}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("not inside a git work tree: {}", stderr.trim());
    }
}

/// Produce an empty, passing result for when diff filtering yields no functions.
fn empty_passing_result() -> AnalysisResult {
    AnalysisResult {
        functions: vec![],
        summary: compute_summary(&[]),
        passed: true,
    }
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
    use crate::domain::threshold::{DEFAULT_THRESHOLD, ThresholdOverride};
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
    fn score_and_summarize_threads_contributors() {
        use crate::domain::types::{
            ComplexityContributor, ContributorKind, FunctionCoverage, SourceSpan,
        };

        let contributor = ComplexityContributor {
            kind: ContributorKind::IfBranch,
            line: 5,
            column: Some(4),
            increment: 1,
        };
        let comp = crate::domain::types::FunctionComplexity {
            identity: crate::domain::types::FunctionIdentity {
                file_path: "src/lib.rs".to_string(),
                qualified_name: "test_fn".to_string(),
                span: SourceSpan {
                    start_line: 1,
                    end_line: 10,
                },
            },
            complexity: 2,
            metric: crate::domain::types::ComplexityMetric::Cognitive,
            contributors: vec![contributor.clone()],
        };
        let cov = FunctionCoverage {
            file_path: "src/lib.rs".to_string(),
            span: SourceSpan {
                start_line: 1,
                end_line: 10,
            },
            line_coverage: crate::domain::types::CoverageRatio {
                covered: 10,
                total: 10,
                percent: 100.0,
            },
            branch_coverage: None,
        };

        let config = crate::domain::threshold::ThresholdConfig::default();
        let resolver = ThresholdResolver::new(&config).unwrap();
        let result = score_and_summarize(&[(comp, cov)], &resolver).unwrap();

        assert_eq!(result.functions.len(), 1);
        let verdict = &result.functions[0];
        assert_eq!(verdict.scored.contributors.len(), 1);
        assert_eq!(verdict.scored.contributors[0], contributor);
    }

    #[test]
    fn analyze_returns_results_for_simple_project() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
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

        let result = analyze(&opts).unwrap().result;

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
            threshold_config: ThresholdConfig {
                global: DEFAULT_THRESHOLD,
                ..ThresholdConfig::default()
            },
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
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
            threshold_config: ThresholdConfig {
                global: DEFAULT_THRESHOLD,
                ..ThresholdConfig::default()
            },
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
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
            threshold_config: ThresholdConfig {
                global: 100.0,
                ..ThresholdConfig::default()
            }, // Very high threshold
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
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
            threshold_config: ThresholdConfig {
                global: 1.0,
                ..ThresholdConfig::default()
            },
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
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
            threshold_config: ThresholdConfig {
                global: 0.5,
                ..ThresholdConfig::default()
            }, // Very low threshold — everything exceeds
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
        assert!(!result.passed);
        assert!(result.summary.exceeding_threshold > 0);
    }

    #[test]
    fn analyze_no_functions_extracted_errors() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        // .rs file with no function definitions
        fs::write(src_dir.join("lib.rs"), "// just a comment\n").unwrap();
        fs::write(
            dir.path().join("lcov.info"),
            "SF:lib.rs\nDA:1,1\nend_of_record\n",
        )
        .unwrap();

        let opts = AnalyzeOptions {
            src: src_dir,
            coverage: dir.path().join("lcov.info"),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let err = analyze(&opts).unwrap_err();
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

        let result = analyze(&opts).unwrap().result;
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

        let result = analyze(&opts).unwrap().result;
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
            threshold_config: ThresholdConfig {
                global: DEFAULT_THRESHOLD,
                ..ThresholdConfig::default()
            },
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
        let summary = &result.summary;

        assert_eq!(summary.total_functions, 2);
        assert_eq!(summary.total_files, 1);
        assert!(summary.average_crap > 0.0);
        assert!(summary.median_crap > 0.0);
        assert!(summary.max_crap.is_some());
        assert!(summary.worst_function.is_some());
    }

    // ── ThresholdResolver tests ───────────────────────────────────────

    #[test]
    fn resolver_global_only() {
        let config = ThresholdConfig {
            global: 10.0,
            overrides: vec![],
        };
        let resolver = ThresholdResolver::new(&config).unwrap();
        assert_eq!(resolver.resolve("domain/crap.rs"), 10.0);
        assert_eq!(resolver.resolve("adapters/coverage/mod.rs"), 10.0);
    }

    #[test]
    fn resolver_override_matches() {
        let config = ThresholdConfig {
            global: 8.0,
            overrides: vec![ThresholdOverride {
                pattern: "domain/**".to_string(),
                threshold: 5.0,
            }],
        };
        let resolver = ThresholdResolver::new(&config).unwrap();
        assert_eq!(resolver.resolve("domain/crap.rs"), 5.0);
        assert_eq!(resolver.resolve("adapters/coverage/mod.rs"), 8.0);
    }

    #[test]
    fn resolver_last_match_wins() {
        let config = ThresholdConfig {
            global: 8.0,
            overrides: vec![
                ThresholdOverride {
                    pattern: "**/*.rs".to_string(),
                    threshold: 10.0,
                },
                ThresholdOverride {
                    pattern: "domain/**".to_string(),
                    threshold: 5.0,
                },
            ],
        };
        let resolver = ThresholdResolver::new(&config).unwrap();
        // domain/crap.rs matches both — last wins (5.0)
        assert_eq!(resolver.resolve("domain/crap.rs"), 5.0);
        // adapters/mod.rs matches only first (10.0)
        assert_eq!(resolver.resolve("adapters/mod.rs"), 10.0);
    }

    #[test]
    fn resolver_no_match_falls_back_to_global() {
        let config = ThresholdConfig {
            global: 8.0,
            overrides: vec![ThresholdOverride {
                pattern: "domain/**".to_string(),
                threshold: 5.0,
            }],
        };
        let resolver = ThresholdResolver::new(&config).unwrap();
        assert_eq!(resolver.resolve("cli/mod.rs"), 8.0);
    }

    #[test]
    fn resolver_invalid_glob_rejected() {
        let config = ThresholdConfig {
            global: 8.0,
            overrides: vec![ThresholdOverride {
                pattern: "[invalid".to_string(),
                threshold: 5.0,
            }],
        };
        assert!(ThresholdResolver::new(&config).is_err());
    }

    // ── Diff mode unit tests ───────────────────────────────────────

    #[test]
    fn analyze_diff_ref_none_is_backward_compat() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            diff_ref: None,
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
        assert_eq!(result.functions.len(), 2);
        assert_eq!(result.summary.total_functions, 2);
    }

    #[test]
    fn empty_passing_result_has_zero_functions() {
        let result = empty_passing_result();
        assert!(result.functions.is_empty());
        assert!(result.passed);
        assert_eq!(result.summary.total_functions, 0);
    }

    #[test]
    fn analyze_returns_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
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

        let output = analyze(&opts).unwrap();
        let diag = &output.diagnostics;

        assert_eq!(diag.files_found, 1);
        assert_eq!(diag.files_unparseable, 0);
        assert_eq!(diag.functions_extracted, 2);
        assert!(diag.parse_diagnostics.is_empty());
        // Both functions are in lib.rs which has LCOV data
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

        // LCOV only has coverage for lib.rs, not other.rs
        fs::write(
            dir.path().join("lcov.info"),
            "SF:lib.rs\nDA:1,1\nend_of_record\n",
        )
        .unwrap();

        let opts = AnalyzeOptions {
            src: src_dir,
            coverage: dir.path().join("lcov.info"),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let output = analyze(&opts).unwrap();
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

        // LCOV with a malformed DA line
        fs::write(
            dir.path().join("lcov.info"),
            "SF:lib.rs\nDA:1,1\nDA:bad_line\nend_of_record\n",
        )
        .unwrap();

        let opts = AnalyzeOptions {
            src: src_dir,
            coverage: dir.path().join("lcov.info"),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let output = analyze(&opts).unwrap();
        assert_eq!(output.diagnostics.parse_diagnostics.len(), 1);
    }

    #[test]
    fn analyze_with_per_path_overrides() {
        let dir = tempfile::tempdir().unwrap();
        setup_test_project(dir.path());

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            threshold_config: ThresholdConfig {
                global: 100.0, // Very high — everything passes by default
                overrides: vec![ThresholdOverride {
                    pattern: "lib.rs".to_string(),
                    threshold: 0.5, // Very low — everything in lib.rs fails
                }],
            },
            metric: ComplexityMetric::Cognitive,
            exclude: Vec::new(),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
        // All functions are in lib.rs, which has the 0.5 override
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

        // LCOV with both DA and BRDA records
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

        let opts = AnalyzeOptions {
            src: dir.path().join("src"),
            coverage: dir.path().join("lcov.info"),
            respect_gitignore: false,
            ..AnalyzeOptions::default()
        };

        let result = analyze(&opts).unwrap().result;
        assert_eq!(result.functions.len(), 1);

        // CRAP score should still use line coverage (dark infra)
        let verdict = &result.functions[0];
        assert!(verdict.scored.crap.value > 0.0);
    }
}
