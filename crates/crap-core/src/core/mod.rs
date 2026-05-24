//! Wiring layer — composes adapters through ports, exposes `analyze()` API.
//!
//! Language-agnostic orchestrator: discovers source files, dispatches per-
//! function complexity extraction through `ComplexityPort`, parses coverage
//! through `CoveragePort<Diagnostic = P>`, matches functions, scores, and
//! produces results. Per-adapter language coupling stays in the caller's
//! crate (`crap4rs::adapters::complexity`, `crap4rs::adapters::coverage`).
//!
//! `analyze` is generic over `<P: ParseDiagnostic>` so the same
//! orchestrator powers both the LCOV adapter (crap4rs) and future
//! siblings (crap4ts's Istanbul adapter), per ADR D9
//! (mixed-dispatch-strategy).

pub mod walker;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use self::walker::discover_source_files;

use crate::adapters::diff::GitDiffAdapter;
use crate::domain::crap::compute_crap;
use crate::domain::diagnostic::compute_diagnostic;
use crate::domain::matching::{match_functions, overlaps_any};
use crate::domain::summary::compute_summary;
use crate::domain::threshold::ThresholdConfig;
use crate::domain::types::{
    AnalysisDiagnostics, AnalysisResult, ComplexityMetric, CoverageMetric, FileChangeKind,
    FunctionComplexity, FunctionVerdict, LineCoverage, ScoredFunction,
};
use crate::ports::{ComplexityPort, CoveragePort, DiffPort, ParseDiagnostic, ParseOutput};

/// Options for running a CRAP analysis.
#[derive(Debug)]
pub struct AnalyzeOptions {
    /// Root directory of source files to analyze.
    pub src: PathBuf,
    /// Path to the coverage file (adapter-specific format: LCOV for
    /// crap4rs, Istanbul JSON for crap4ts, etc.).
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
    /// File extensions the walker should pick up. Adapter-specific
    /// (`["rs"]` for crap4rs; `["ts","tsx","js","jsx","mjs","cjs"]`
    /// for crap4ts). `Default::default()` is `Vec::new()` since #161 —
    /// the adapter-language coupling lives in each binary's
    /// `AdapterMeta::extensions`, threaded into `AnalyzeOptions` at
    /// the CLI boundary. Library tests that construct `AnalyzeOptions`
    /// directly must set this explicitly;
    /// `core::ensure_source_files_found` surfaces a parser-neutral
    /// diagnostic when no files match.
    pub extensions: Vec<String>,
    /// When `true`, populate `FunctionVerdict.diagnostic` for every
    /// over-threshold verdict via `domain::diagnostic::compute_diagnostic`.
    /// CLI sets this for `--format advice` and `--format sarif`. The
    /// computation is pure-domain and bounded — runs only on exceeding
    /// verdicts, so the cost scales with violations, not total functions.
    pub compute_diagnostics: bool,
}

/// Full output from an analysis run, including both the scored results
/// and process diagnostics (surfaced by `--verbose`).
///
/// Generic over `P: ParseDiagnostic` so `AnalysisDiagnostics<P>` can carry
/// the adapter-specific parse-diagnostic shape (LCOV's
/// `LcovParseDiagnostic`, future Istanbul adapter's diagnostic, etc.) per
/// ADR D9.
#[derive(Debug)]
pub struct AnalysisOutput<P: ParseDiagnostic> {
    pub result: AnalysisResult,
    pub diagnostics: AnalysisDiagnostics<P>,
}

struct DiscoveredSources {
    source_files: Vec<PathBuf>,
    files_found: usize,
}

struct ExtractedComplexities {
    all_complexities: Vec<FunctionComplexity>,
    files_unparseable: usize,
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
            // Empty by default — adapter-language coupling stays in
            // each binary's `AdapterMeta::extensions`. Library tests
            // that construct `AnalyzeOptions` directly must set this
            // explicitly; `ensure_source_files_found` surfaces a
            // parser-neutral diagnostic when nothing matches (#161).
            extensions: Vec::new(),
            compute_diagnostics: false,
        }
    }
}

/// Run CRAP analysis: discover files, extract complexity, parse coverage,
/// match, score, and produce results with diagnostics.
///
/// Thin facade over a private `AnalysisContext`. Phase methods on the
/// context compose the pipeline; diff-mode short-circuits are surfaced
/// via `Option<AnalysisOutput<P>>` so the top-level orchestration stays
/// flat.
///
/// Mixed dispatch (ADR D9): port adapters arrive as trait objects so the
/// monomorphization tax stays bounded; `<P>` is the single generic
/// parameter that flows from `CoveragePort::Diagnostic` through to
/// `AnalysisDiagnostics<P>` in the result.
pub fn analyze<P: ParseDiagnostic>(
    options: &AnalyzeOptions,
    complexity: &dyn ComplexityPort,
    coverage: &dyn CoveragePort<Diagnostic = P>,
) -> Result<AnalysisOutput<P>> {
    AnalysisContext::new(options, complexity, coverage).run()
}

/// Mutable per-run context: bundles `&AnalyzeOptions` with the canonicalized
/// source root and the injected port adapters so phase methods don't repeat
/// that setup.
///
/// The pipeline phases are `&self`-methods that read options and produce
/// fresh values; nothing is mutated on the context itself. Diff-mode
/// short-circuits return `Option<AnalysisOutput<P>>` — `Some(early)`
/// signals "no work left, return this," `None` signals "continue."
struct AnalysisContext<'a, P: ParseDiagnostic> {
    options: &'a AnalyzeOptions,
    complexity: &'a dyn ComplexityPort,
    coverage: &'a dyn CoveragePort<Diagnostic = P>,
    src_canonical: PathBuf,
}

impl<'a, P: ParseDiagnostic> AnalysisContext<'a, P> {
    fn new(
        options: &'a AnalyzeOptions,
        complexity: &'a dyn ComplexityPort,
        coverage: &'a dyn CoveragePort<Diagnostic = P>,
    ) -> Self {
        Self {
            options,
            complexity,
            coverage,
            src_canonical: canonicalize_src(&options.src),
        }
    }

    fn run(&self) -> Result<AnalysisOutput<P>> {
        let mut discovered = self.discover_sources()?;
        let diff_data = self.load_diff_data(&discovered.source_files)?;

        if let Some(early) = self.short_circuit_on_files(&mut discovered, diff_data.as_ref()) {
            return Ok(early);
        }

        let mut parse_output = self.parse_coverage()?;
        let ExtractedComplexities {
            mut all_complexities,
            files_unparseable,
        } = self.extract_complexities(&discovered.source_files)?;
        let functions_extracted = all_complexities.len();

        if let Some(early) = self.short_circuit_on_complexities(
            &mut all_complexities,
            diff_data.as_ref(),
            &mut parse_output,
            &discovered,
            files_unparseable,
            functions_extracted,
        ) {
            return Ok(early);
        }

        ensure_functions_extracted(&all_complexities, &self.options.src)?;

        let matched = match_functions(
            &all_complexities,
            &parse_output.coverage,
            parse_output.branches.as_ref(),
        );

        let functions_no_coverage = matched
            .iter()
            .filter(|(comp, _)| !parse_output.coverage.contains_key(&comp.identity.file_path))
            .count();
        let functions_matched = matched.len() - functions_no_coverage;

        let resolver = ThresholdResolver::new(&self.options.threshold_config)?;
        let mut result = score_and_summarize(&matched, &resolver)?;
        populate_diagnostics(
            &mut result.functions,
            &parse_output.coverage,
            self.options.compute_diagnostics,
        );

        let (files_analyzed, files_zero_coverage) = compute_file_coverage_stats(&result);

        let diagnostics = AnalysisDiagnostics {
            parse_diagnostics: parse_output.diagnostics,
            files_found: discovered.files_found,
            files_unparseable,
            functions_extracted,
            functions_matched,
            functions_no_coverage,
            files_analyzed,
            files_zero_coverage,
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

    fn discover_sources(&self) -> Result<DiscoveredSources> {
        let extensions: Vec<&str> = self.options.extensions.iter().map(String::as_str).collect();
        let source_files = discover_source_files(
            &self.options.src,
            &self.options.exclude,
            self.options.respect_gitignore,
            &extensions,
        )?;
        let files_found = source_files.len();
        ensure_source_files_found(&source_files, &self.options.src, &extensions)?;

        Ok(DiscoveredSources {
            source_files,
            files_found,
        })
    }

    fn load_diff_data(
        &self,
        source_files: &[PathBuf],
    ) -> Result<Option<std::collections::HashMap<String, FileChangeKind>>> {
        self.options
            .diff_ref
            .as_deref()
            .map(|diff_ref| {
                compute_diff_regions(
                    diff_ref,
                    &self.src_canonical,
                    &self.options.src,
                    source_files,
                )
            })
            .transpose()
    }

    fn parse_coverage(&self) -> Result<ParseOutput<P>> {
        // `CoveragePort::parse` takes `&Path` post-#179 — each adapter
        // owns its slurp-vs-stream decision internally (LCOV slurps;
        // Istanbul slurps; `validate` streams in the LCOV case). The
        // orchestrator no longer pre-reads, eliminating the double-read
        // trap that motivated `feedback_trait_io_input_path_over_str`.
        //
        // The pre-#179 implementation slurped here and wrapped any
        // open/read failure with `"failed to read coverage file: <path>"`
        // via `with_context`. The path-aware context is still load-
        // bearing (test contract + user-facing error UX), so we
        // re-add it at the orchestrator boundary — only when the
        // adapter surfaces an I/O failure (the path-doesn't-exist
        // case), letting per-adapter parse errors (malformed JSON,
        // etc.) flow through their own messages.
        //
        // Uses `anyhow::Error::new(e).context(...)` (not
        // `anyhow!("{e}")`) so the underlying `CrapError` / `io::Error`
        // chain is preserved for `err.source()` / `err.root_cause()`
        // walks. `err.to_string()` still returns the top context
        // message (`"failed to read coverage file: <path>"`), keeping
        // the analyze-pipeline test contract green.
        self.coverage
            .parse(&self.options.coverage)
            .map_err(|e| match e {
                crate::domain::types::CrapError::Io(_) => anyhow::Error::new(e).context(format!(
                    "failed to read coverage file: {}",
                    self.options.coverage.display()
                )),
                other => anyhow::Error::new(other),
            })
    }

    fn extract_complexities(&self, source_files: &[PathBuf]) -> Result<ExtractedComplexities> {
        let mut all_complexities = Vec::new();
        let mut files_unparseable = 0usize;

        for file_path in source_files {
            let source = std::fs::read_to_string(file_path)
                .with_context(|| format!("failed to read source file: {}", file_path.display()))?;
            let relative = src_relative_path(file_path, &self.options.src);

            match self
                .complexity
                .extract(&source, &relative, self.options.metric)
            {
                Ok(fns) => all_complexities.extend(fns),
                // `MetricNotSupported` is a configuration mismatch
                // (caller asked for a metric the adapter doesn't
                // implement), not a per-file parse failure — bail
                // immediately so the CLI's renderer surfaces the
                // adapter-named hint instead of N stuttering
                // `warning: skipping` lines followed by a
                // misleading "no functions extracted" error.
                Err(e @ crate::domain::types::CrapError::MetricNotSupported { .. }) => {
                    return Err(e.into());
                }
                Err(e) => {
                    files_unparseable += 1;
                    eprintln!("warning: skipping {relative}: {e}");
                }
            }
        }

        Ok(ExtractedComplexities {
            all_complexities,
            files_unparseable,
        })
    }

    /// Apply diff-mode file filtering and signal early-exit if nothing remains.
    /// Returns `None` when there is no diff ref OR the filter still leaves
    /// files to analyze; returns `Some(empty_output)` when the diff cleared
    /// every source file.
    fn short_circuit_on_files(
        &self,
        discovered: &mut DiscoveredSources,
        diff_data: Option<&std::collections::HashMap<String, FileChangeKind>>,
    ) -> Option<AnalysisOutput<P>> {
        let diff_result = diff_data?;
        retain_changed_source_files(&mut discovered.source_files, &self.options.src, diff_result);
        if !discovered.source_files.is_empty() {
            return None;
        }
        Some(empty_output_with_diagnostics(diagnostics_for_empty_result(
            vec![],
            discovered.files_found,
            0,
            0,
        )))
    }

    /// Apply diff-mode function filtering and signal early-exit if nothing
    /// remains. Same `Option<AnalysisOutput<P>>` signal as
    /// [`Self::short_circuit_on_files`]; takes `parse_output` mutably so the
    /// empty-result diagnostics can move out of `parse_output.diagnostics`
    /// without cloning when this branch fires.
    fn short_circuit_on_complexities(
        &self,
        complexities: &mut Vec<FunctionComplexity>,
        diff_data: Option<&std::collections::HashMap<String, FileChangeKind>>,
        parse_output: &mut ParseOutput<P>,
        discovered: &DiscoveredSources,
        files_unparseable: usize,
        functions_extracted: usize,
    ) -> Option<AnalysisOutput<P>> {
        let diff_result = diff_data?;
        retain_changed_functions(complexities, diff_result);
        if !complexities.is_empty() {
            return None;
        }
        Some(empty_output_with_diagnostics(diagnostics_for_empty_result(
            std::mem::take(&mut parse_output.diagnostics),
            discovered.files_found,
            files_unparseable,
            functions_extracted,
        )))
    }
}

/// Canonicalize the effective source root.
///
/// Falls back to the raw path on failure (e.g., the directory was
/// validated to exist by `validate_runtime_inputs` but vanished in a
/// TOCTOU window before this call). The fallback is observable via
/// stderr — silent regression would re-introduce a path-strip
/// mismatch for adapters that depend on the canonical root.
///
/// `pub(crate)` so `cli::prepare_pipeline` can late-bind coverage
/// adapter construction against the same path that
/// `AnalysisContext::new` uses internally — single source of truth
/// for canonicalization semantics.
pub(crate) fn canonicalize_src(src: &Path) -> PathBuf {
    src.canonicalize().unwrap_or_else(|e| {
        eprintln!(
            "warning: failed to canonicalize {}: {e}; coverage path-strip may misalign",
            src.display()
        );
        src.to_path_buf()
    })
}

fn ensure_source_files_found(
    source_files: &[PathBuf],
    src: &Path,
    extensions: &[&str],
) -> Result<()> {
    if source_files.is_empty() {
        // Render a comma-separated list of dotted extensions for the
        // hint. Single source of truth — `cli/mod.rs` previously had a
        // duplicate pre-flight walk that emitted identical wording; that
        // duplicate was retired once both paths agreed (issue #163).
        let pretty = match extensions {
            [] => "supported".to_string(),
            [only] => format!(".{only}"),
            [first, rest @ .., last] => {
                let mut out = format!(".{first}");
                for e in rest {
                    out.push_str(", .");
                    out.push_str(e);
                }
                out.push_str(", or .");
                out.push_str(last);
                out
            }
        };
        bail!(
            "no source files found in {}\n  \
             hint: check that --src points to a directory containing {} files",
            src.display(),
            pretty,
        );
    }
    Ok(())
}

fn retain_changed_source_files(
    source_files: &mut Vec<PathBuf>,
    src_root: &Path,
    diff_result: &std::collections::HashMap<String, FileChangeKind>,
) {
    source_files.retain(|path| {
        let rel = src_relative_path(path, src_root);
        diff_result.contains_key(&rel)
    });
}

fn retain_changed_functions(
    all_complexities: &mut Vec<FunctionComplexity>,
    diff_result: &std::collections::HashMap<String, FileChangeKind>,
) {
    all_complexities.retain(|comp| match diff_result.get(&comp.identity.file_path) {
        Some(FileChangeKind::NewFile) => true,
        Some(FileChangeKind::Modified(ranges)) => overlaps_any(&comp.identity.span, ranges),
        None => false,
    });
}

fn ensure_functions_extracted(all_complexities: &[FunctionComplexity], src: &Path) -> Result<()> {
    if all_complexities.is_empty() {
        bail!(
            "no functions extracted from source files in {}\n  \
             hint: check that source files contain valid function definitions for the selected adapter",
            src.display()
        );
    }
    Ok(())
}

fn diagnostics_for_empty_result<P: ParseDiagnostic>(
    parse_diagnostics: Vec<P>,
    files_found: usize,
    files_unparseable: usize,
    functions_extracted: usize,
) -> AnalysisDiagnostics<P> {
    AnalysisDiagnostics {
        parse_diagnostics,
        files_found,
        files_unparseable,
        functions_extracted,
        functions_matched: 0,
        functions_no_coverage: 0,
        files_analyzed: 0,
        files_zero_coverage: 0,
    }
}

fn empty_output_with_diagnostics<P: ParseDiagnostic>(
    diagnostics: AnalysisDiagnostics<P>,
) -> AnalysisOutput<P> {
    AnalysisOutput {
        result: empty_passing_result(),
        diagnostics,
    }
}

fn compute_file_coverage_stats(result: &AnalysisResult) -> (usize, usize) {
    let mut file_is_zero_coverage: std::collections::HashMap<&str, bool> =
        std::collections::HashMap::new();
    for verdict in &result.functions {
        let entry = file_is_zero_coverage
            .entry(verdict.scored.identity.file_path.as_str())
            .or_insert(true);
        if verdict.scored.coverage_percent > 0.0 {
            *entry = false;
        }
    }

    let total = file_is_zero_coverage.len();
    let zero = file_is_zero_coverage
        .values()
        .filter(|&&is_zero| is_zero)
        .count();
    (total, zero)
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
                // Surface the branch coverage already computed by
                // `domain::matching::compute_branch_coverage`. `None`
                // when the parser produced no branches for this file
                // (LCOV runs, jest without branch instrumentation) or
                // when none of the file's branches fell inside this
                // function's span — both cases are collapsed at the
                // matcher.
                branch_coverage_percent: cov.branch_coverage.as_ref().map(|bc| bc.percent),
                crap,
                contributors: comp.contributors.clone(),
            },
            threshold,
            exceeds: crap.value > threshold,
            diagnostic: None,
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

/// Populate `verdict.diagnostic` for every over-threshold verdict.
/// `compute_diagnostic` returns `None` for passing verdicts, so the
/// existing `skip_serializing_if = "Option::is_none"` keeps the JSON
/// envelope clean for them.
fn populate_diagnostics(
    verdicts: &mut [FunctionVerdict],
    coverage: &std::collections::HashMap<String, Vec<LineCoverage>>,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    for verdict in verdicts.iter_mut() {
        if !verdict.exceeds {
            continue;
        }
        let lines = coverage
            .get(&verdict.scored.identity.file_path)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        verdict.diagnostic = compute_diagnostic(verdict, lines).map(Box::new);
    }
}

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

// The walker lives at `crap_core::core::walker::discover_source_files`;
// the import at the top of this file binds the name into local scope
// for the call sites in `discover_sources`. The `.rs` filter is
// parameter-driven via `AnalyzeOptions::extensions`.

#[cfg(test)]
mod tests {
    //! Language-agnostic core tests. The end-to-end pipeline tests that
    //! exercise `analyze` over real LCOV + Rust source live in
    //! `crap4rs/tests/analyze_pipeline_tests.rs` — `analyze` takes
    //! `&dyn ComplexityPort` + `&dyn CoveragePort`, and the LCOV/syn
    //! adapters that satisfy those traits live in the crap4rs crate.
    use super::*;
    use crate::domain::threshold::ThresholdOverride;

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
            end_line: 5,
            nesting_depth: 0,
        };
        let comp = crate::domain::types::FunctionComplexity {
            identity: crate::domain::types::FunctionIdentity {
                file_path: "src/lib.rs".to_string(),
                qualified_name: "test_fn".to_string(),
                span: SourceSpan {
                    start_line: 1,
                    end_line: 10,
                    start_column: 0,
                    end_column: 0,
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
                start_column: 0,
                end_column: 0,
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

    #[test]
    fn empty_passing_result_has_zero_functions() {
        let result = empty_passing_result();
        assert!(result.functions.is_empty());
        assert!(result.passed);
        assert_eq!(result.summary.total_functions, 0);
    }
}
