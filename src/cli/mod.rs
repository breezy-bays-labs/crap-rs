//! CLI entry point — thin shell over the library crate.
//!
//! Parses args with clap, validates inputs, delegates to `core::analyze()`.
//! No business logic lives here.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use anyhow::{Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell as ClapShell;

use crap4rs::adapters::baseline::{self, BaselineSnapshot};
use crap4rs::adapters::config::{self, FileConfig};
use crap4rs::adapters::reporters;
use crap4rs::adapters::reporters::json::DeltaContext;
use crap4rs::core::AnalyzeOptions;
use crap4rs::domain::delta::{self, AnalysisDelta};
use crap4rs::domain::threshold::{
    DEFAULT_THRESHOLD, LENIENT_THRESHOLD, STRICT_THRESHOLD, ThresholdConfig, is_valid_threshold,
};
use crap4rs::domain::types::{AnalysisDiagnostics, ComplexityMetric};
use crap4rs::domain::view::{self, GroupKey, SortKey};

mod view_args;

// ── ValueEnum wrappers (keep domain types clap-free) ────────────────

/// Complexity metric for CRAP score computation.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum MetricArg {
    /// Nesting depth + structural complexity (default for Rust)
    Cognitive,
    /// Decision-point count, classic CRAP metric
    Cyclomatic,
}

impl From<MetricArg> for ComplexityMetric {
    fn from(arg: MetricArg) -> Self {
        match arg {
            MetricArg::Cognitive => ComplexityMetric::Cognitive,
            MetricArg::Cyclomatic => ComplexityMetric::Cyclomatic,
        }
    }
}

/// Output format for the CRAP report.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FormatArg {
    /// Human-readable table with ANSI colors
    Table,
    /// Nested JSON envelope (pipe to jq for filtering)
    Json,
    /// GitHub-flavored Markdown — paste into PR comments or issues
    Markdown,
    /// RFC 4180 CSV — one row per function, no summary
    Csv,
}

/// Sort key for the displayed view (issue #68).
///
/// CLI-side wrapper that keeps `clap::ValueEnum` out of the domain.
/// `From<SortKeyArg> for SortKey` is the boundary; `build_view_spec`
/// translates at the edge so `domain::view::SortKey` stays clap-free.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SortKeyArg {
    /// CRAP score descending (default — investigator's first cut)
    Crap,
    /// Coverage percent ascending (lowest coverage first)
    Coverage,
    /// Complexity descending (most complex first)
    Complexity,
    /// Alphabetical by file_path, then CRAP descending within file
    Path,
}

impl From<SortKeyArg> for SortKey {
    fn from(arg: SortKeyArg) -> Self {
        match arg {
            SortKeyArg::Crap => SortKey::Crap,
            SortKeyArg::Coverage => SortKey::Coverage,
            SortKeyArg::Complexity => SortKey::Complexity,
            SortKeyArg::Path => SortKey::Path,
        }
    }
}

/// Reverse mapping for saved view presets (issue #80) — preset stores
/// domain `SortKey`, but `FilterArgs.sort_by` is the clap-side wrapper.
///
/// `SortKey` is `#[non_exhaustive]` cross-crate, so the wildcard arm is
/// required by the compiler. New domain variants must land with a paired
/// CLI variant in the same PR — the panic surfaces a missed update
/// during integration testing rather than masking it.
impl From<SortKey> for SortKeyArg {
    fn from(key: SortKey) -> Self {
        match key {
            SortKey::Crap => SortKeyArg::Crap,
            SortKey::Coverage => SortKeyArg::Coverage,
            SortKey::Complexity => SortKeyArg::Complexity,
            SortKey::Path => SortKeyArg::Path,
            other => unreachable!(
                "domain::view::SortKey::{other:?} has no CLI mapping; add a SortKeyArg variant"
            ),
        }
    }
}

/// Group key for the displayed view (issue #64).
///
/// Today only `file` is supported. The wrapper keeps `clap::ValueEnum`
/// out of the domain; `From<GroupByArg> for GroupKey` is the boundary.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GroupByArg {
    /// Aggregate by source file path
    File,
}

impl From<GroupByArg> for GroupKey {
    fn from(arg: GroupByArg) -> Self {
        match arg {
            GroupByArg::File => GroupKey::File,
        }
    }
}

/// Reverse mapping for saved view presets (issue #80). See `From<SortKey>`
/// above for the wildcard-arm rationale.
impl From<GroupKey> for GroupByArg {
    fn from(key: GroupKey) -> Self {
        match key {
            GroupKey::File => GroupByArg::File,
            other => unreachable!(
                "domain::view::GroupKey::{other:?} has no CLI mapping; add a GroupByArg variant"
            ),
        }
    }
}

/// When to colorize output.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum ColorArg {
    /// Colorize when writing to a terminal
    #[default]
    Auto,
    /// Always colorize output
    Always,
    /// Never colorize output
    Never,
}

// ── Arg groups ──────────────────────────────────────────────────────

/// Shell name for completion script generation (#69).
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellArg {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
    Nushell,
}

/// Top-level subcommands. Optional — when absent, crap4rs runs the
/// default analysis path that requires `--coverage`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Generate a shell completion script to stdout.
    Completions {
        #[arg(value_enum)]
        shell: ShellArg,
    },
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Input")]
pub struct InputArgs {
    /// Path to LCOV coverage file (from `cargo llvm-cov --lcov`).
    /// Required for analysis; not required for `crap4rs completions`.
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub coverage: Option<PathBuf>,

    /// Root directory of Rust source files to analyze [default: src]
    #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
    pub src: Option<PathBuf>,

    /// Complexity metric to use [default: cognitive]
    #[arg(long, value_enum)]
    pub metric: Option<MetricArg>,

    /// Path to config file (default: auto-discover crap4rs.toml)
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub config: Option<PathBuf>,

    /// Resolve and apply a saved view preset from `crap4rs.toml`.
    ///
    /// The preset's fields (`top`, `min_coverage`, `max_coverage`, `sort`,
    /// `only_failing`, `no_fail`, `group_by`, `minimal_view`) are folded
    /// into the parsed CLI before the report is shaped. CLI flags
    /// override the preset's `Option<T>` fields. Bare-bool flags
    /// OR-merge with the preset (an explicit `--no-fail` adds to a
    /// preset's value but cannot turn off `no_fail = true`).
    #[arg(long, value_name = "NAME")]
    pub view: Option<String>,

    /// Path to a previously-emitted crap4rs JSON envelope, used as the
    /// baseline for delta analysis.
    ///
    /// Crap4rs runs the current analysis as usual, then compares against
    /// the baseline's `result` block to produce a `delta` block in the
    /// output (see `--format json`, `--format markdown` for rendering).
    /// Generate the baseline file by piping a previous run:
    /// `crap4rs --coverage lcov.info --format json > baseline.json`.
    ///
    /// **Delta is informational by default.** Pass `--delta-gate` to
    /// make the delta contribute to the exit code (fails on new
    /// threshold violations introduced by this PR).
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub baseline: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Output")]
pub struct OutputArgs {
    /// Output format
    #[arg(short, long, value_enum, default_value_t = FormatArg::Table)]
    pub format: FormatArg,

    /// CRAP score threshold — functions above this fail the check [default: 25]
    // allow_hyphen_values: lets clap parse `--threshold -5` as a value
    // (not a flag), so our validate_inputs can give an actionable error.
    #[arg(long, allow_hyphen_values = true, group = "threshold_select")]
    pub threshold: Option<f64>,

    /// Use strict threshold (15) — for high-quality or safety-critical code
    #[arg(long, group = "threshold_select")]
    pub strict: bool,

    /// Use lenient threshold (40) — for legacy or transitional code
    #[arg(long, group = "threshold_select")]
    pub lenient: bool,

    /// Always exit 0, even when threshold violations exist.
    ///
    /// Overrides only the exit-code translation; the underlying analysis
    /// is untouched and `result.passed` in JSON output still reflects
    /// the truthful pass/fail state, so consumers can detect "would
    /// have failed" even when the process exits 0. Composes with
    /// `--quiet` for silent success in CI.
    #[arg(long)]
    pub no_fail: bool,

    /// Omit the denormalized `view.shown` row array from JSON output.
    ///
    /// Payload-size escape hatch for very large codebases. The
    /// envelope's `result` block (the gate) is unaffected; `view.spec`,
    /// `view.eligible_count`, `view.truncated`, and `view.shown_summary`
    /// remain so consumers retain full scope context. Only meaningful
    /// with `--format json`.
    #[arg(long)]
    pub minimal_view: bool,
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Filtering")]
pub struct FilterArgs {
    /// Glob patterns to exclude from analysis (repeatable)
    ///
    /// Build artifacts (target/) are excluded automatically via .gitignore.
    /// Test files are NOT excluded by default — use `--exclude "tests/**"`
    /// if you want to skip them.
    #[arg(long, action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,

    /// Do not respect .gitignore files
    ///
    /// By default, paths in .gitignore are skipped (e.g., target/).
    /// Pass this flag to analyze all files regardless of .gitignore.
    #[arg(long)]
    pub no_gitignore: bool,

    /// Git ref to diff against — only analyze functions in changed files/hunks
    ///
    /// Scopes analysis to functions in files that changed since the given ref.
    /// Useful for CI PR gating: `crap4rs --coverage lcov.info --diff main`
    #[arg(long, value_name = "REF")]
    pub diff: Option<String>,

    /// Only show functions that exceed the threshold
    ///
    /// Display-only filter: the underlying analysis (the gate) and its
    /// summary remain over the full unfiltered set, so the exit code and
    /// every aggregate (`average_crap`, `median_crap`, `distribution`,
    /// etc.) reflect the whole codebase. Only the row list and
    /// `view.shown_summary` are reduced.
    #[arg(long)]
    pub only_failing: bool,

    /// Lower bound (inclusive) on coverage_percent for the displayed view.
    ///
    /// `allow_hyphen_values`: lets clap parse `--min-coverage -5` as a
    /// value (not an unknown flag) so `validate_view_args` can report
    /// the right error.
    #[arg(long, allow_hyphen_values = true, value_name = "PCT")]
    pub min_coverage: Option<f64>,

    /// Upper bound (inclusive) on coverage_percent for the displayed view.
    #[arg(long, allow_hyphen_values = true, value_name = "PCT")]
    pub max_coverage: Option<f64>,

    /// Sort key for the displayed view (default: crap descending).
    ///
    /// `crap` (default) — CRAP score descending; `coverage` — coverage
    /// percent ascending (lowest first); `complexity` — complexity
    /// descending; `path` — alphabetical by file, then CRAP descending
    /// within file. Sorting reorders without reducing rows, so the gate
    /// (exit code) is unaffected. Unknown values are rejected by clap
    /// at parse time with an `invalid value` error attributed to
    /// `--sort-by`, so no custom validation is needed here.
    #[arg(long, value_enum, value_name = "KEY")]
    pub sort_by: Option<SortKeyArg>,

    /// Truncate the displayed view to the top N highest-CRAP rows.
    ///
    /// `--top 0` means "no limit" — equivalent to omitting the flag.
    /// The full unfiltered analysis still drives the gate (exit code),
    /// so truncating violations out of the view does not change the outcome.
    ///
    /// `allow_hyphen_values`: lets clap parse `--top -3` as a value (not an
    /// unknown flag) so the resulting error message is attributed to `--top`.
    #[arg(long, allow_hyphen_values = true, value_name = "N")]
    pub top: Option<u32>,

    /// Aggregate the displayed view by a key. Today: `file` only.
    ///
    /// When set, the report shifts to per-file rows. `--top N` truncates
    /// to the top N **files** (not functions); `--sort-by` keys at the
    /// file level (`crap` → average CRAP descending; `coverage` →
    /// average coverage ascending; `complexity` → max complexity
    /// descending; `path` → alphabetical). The full per-function row
    /// list still appears in JSON `view.shown` for drill-down. The
    /// gate (exit code) is unaffected.
    #[arg(long, value_enum, value_name = "KEY")]
    pub group_by: Option<GroupByArg>,
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Display")]
pub struct DisplayArgs {
    /// When to use terminal colors
    #[arg(long, value_enum, default_value_t = ColorArg::Auto)]
    pub color: ColorArg,

    /// Show parse diagnostics and matching statistics
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress report output, only set exit code
    #[arg(short, long)]
    pub quiet: bool,

    /// Show complexity contributors for functions exceeding threshold.
    ///
    /// JSON output always includes contributors regardless of this flag.
    #[arg(long)]
    pub breakdown: bool,

    /// Explain nested breakdown increments in table output.
    ///
    /// Only affects table output, and only when `--breakdown` is enabled.
    #[arg(long)]
    pub explain: bool,
}

// ── Top-level CLI ───────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(
    version,
    long_version = env!("CRAP4RS_LONG_VERSION"),
    author,
    about = "CRAP score analyzer for Rust",
    long_about = "CRAP (Change Risk Anti-Patterns) score analyzer for Rust codebases.\n\n\
                  Combines complexity analysis (via syn) with line coverage data \
                  (LCOV from cargo-llvm-cov) to identify functions that are both \
                  complex and under-tested.\n\n\
                  Default metric is cognitive complexity (not cyclomatic), which \
                  better captures Rust idioms like match arms and nested control flow.",
    after_help = "\
EXAMPLES:
  crap4rs --coverage lcov.info
  crap4rs --coverage lcov.info --threshold 15 --metric cyclomatic
  crap4rs --coverage lcov.info --format json | jq '.functions[] | select(.exceeds)'
  crap4rs --coverage lcov.info --only-failing
  crap4rs --coverage lcov.info --exclude \"tests/**\" --exclude \"benches/**\"

INVESTIGATION PATTERNS:
  # First-run scan: keep the report short
  crap4rs --coverage lcov.info --top 20

  # Worst partially-covered functions, sorted by coverage ascending,
  # never fail the build — useful when investigating an untested codebase
  crap4rs --coverage lcov.info --min-coverage 1 --max-coverage 90 --sort-by coverage --top 10 --no-fail

  # Saved view preset: bake a flag set under [views.ci] in crap4rs.toml,
  # then invoke it by name. CLI flags override preset values.
  crap4rs --coverage lcov.info --view ci"
)]
pub struct Cli {
    #[command(flatten)]
    pub input: InputArgs,

    #[command(flatten)]
    pub output: OutputArgs,

    #[command(flatten)]
    pub filter: FilterArgs,

    #[command(flatten)]
    pub display: DisplayArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

// ── Entry point ─────────────────────────────────────────────────────

pub fn run() -> ExitCode {
    match run_inner() {
        Ok(true) => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run_inner() -> Result<bool> {
    let mut cli = Cli::parse();

    if let Some(Command::Completions { shell }) = cli.command {
        emit_completions(shell);
        return Ok(true);
    }

    validate_display_flags(&cli)?;

    apply_color(cli.display.color);

    // Load config file (explicit path or auto-discovered)
    let file_config = load_file_config(&cli)?;

    // Resolve `--view <NAME>` (issue #80) before validate_view_args runs
    // so preset fields participate in the same validation pass as CLI
    // flags. `apply_preset_to_cli` mutates `cli` in place: CLI explicit
    // values win on `Option<T>` fields, bools OR-merge.
    view_args::resolve_view_preset(&mut cli, file_config.as_ref())?;
    view_args::validate_view_args(&cli)?;

    // Merge: CLI explicit > config file > hardcoded defaults
    let effective_src = cli
        .input
        .src
        .clone()
        .or_else(|| file_config.as_ref().and_then(|c| c.src.clone()))
        .unwrap_or_else(|| PathBuf::from("src"));
    let effective_metric: ComplexityMetric = cli
        .input
        .metric
        .map(Into::into)
        .or_else(|| file_config.as_ref().and_then(|c| c.metric))
        .unwrap_or_default();
    let (threshold_config, effective_threshold) = merge_threshold(&cli, &file_config);
    let effective_exclude = merge_exclude(&cli, &file_config);

    // `--coverage` is required on the analysis path; subcommands like
    // `completions` skip this branch. Clap can't express "required
    // unless subcommand X" in derive, so we enforce it here.
    let Some(coverage_path) = cli.input.coverage.as_ref() else {
        bail!(
            "--coverage <FILE> is required (run `crap4rs --help` for usage, or `crap4rs completions <SHELL>` for shell completion scripts)"
        );
    };

    // Validate effective values (after merging)
    validate_inputs(coverage_path, &effective_src, effective_threshold)?;
    preflight_checks(coverage_path, &effective_src)?;

    // Validate --diff ref if provided
    if let Some(ref diff_ref) = cli.filter.diff {
        validate_diff_ref(diff_ref)?;
        preflight_git_worktree(&effective_src)?;
    }

    let options = AnalyzeOptions {
        src: effective_src,
        coverage: coverage_path.clone(),
        threshold_config,
        metric: effective_metric,
        exclude: effective_exclude,
        respect_gitignore: !cli.filter.no_gitignore,
        diff_ref: cli.filter.diff.clone(),
        ..AnalyzeOptions::default()
    };

    let analysis = crap4rs::core::analyze(&options)?;
    let result = analysis.result;
    let passed = result.passed;

    // Always warn about non-fatal issues (details require --verbose)
    warn_if_issues(&analysis.diagnostics);

    // Print full diagnostics to stderr when --verbose
    if cli.display.verbose {
        print_diagnostics(&analysis.diagnostics);
    }

    // Resolve --baseline (issue #81): load a previously-emitted JSON
    // envelope and compute the AnalysisDelta. None when --baseline is
    // absent — the JSON envelope omits the `delta` block entirely so
    // existing consumers see byte-identical output.
    let delta_state: Option<DeltaState> = load_delta_state(&cli, &result)?;

    // Build the spec, then shape the result through the View pipeline.
    // V1b: `--only-failing` flows through `Filters::only_failing` here.
    // W2 fills in `--top`, `--min/max-coverage`, `--sort-by`. The
    // underlying `result` is never mutated — the gate is unshapeable.
    let spec = view_args::build_view_spec(&cli);
    let view = view::apply(&result, spec);

    if !cli.display.quiet {
        let output = match cli.output.format {
            FormatArg::Table => reporters::format_table_with_explain(
                &view,
                effective_threshold,
                cli.display.breakdown,
                cli.display.explain,
            ),
            FormatArg::Json => {
                let delta_ctx = delta_state.as_ref().map(|s| DeltaContext {
                    delta: &s.delta,
                    baseline_tool_version: &s.snapshot.tool_version,
                    baseline_timestamp: &s.snapshot.timestamp,
                    baseline_diagnostics: s.snapshot.diagnostics.as_ref(),
                });
                let config = reporters::json::JsonConfig {
                    tool_version: env!("CARGO_PKG_VERSION").to_string(),
                    metric: effective_metric,
                    threshold: effective_threshold,
                    timestamp: now_unix_epoch(),
                    diagnostics: cli.display.verbose.then_some(&analysis.diagnostics),
                    diff_ref: cli.filter.diff.as_deref(),
                    minimal_view: cli.output.minimal_view,
                    delta: delta_ctx,
                };
                reporters::format_json(&view, &config)?
            }
            FormatArg::Markdown => reporters::format_markdown(
                &view,
                effective_threshold,
                cli.display.breakdown,
                cli.display.explain,
            ),
            FormatArg::Csv => reporters::format_csv(&view, effective_metric),
        };
        print!("{output}");
    }

    // Exit code derives from `view.full.passed` — i.e., the underlying
    // analysis. The View shapes the display, never the gate. `--no-fail`
    // overrides only the gate-to-exit-code translation; `result.passed`
    // in JSON output still reflects the truthful pass/fail state.
    //
    // Delta is informational by default (issue #81 §gate semantics);
    // VS6 will add the opt-in `--delta-gate` flag here.
    let _ = &delta_state; // VS6 will read this for delta-gate
    Ok(passed || cli.output.no_fail)
}

// ── Delta orchestration ─────────────────────────────────────────────

/// In-flight delta state — owned baseline metadata + computed delta.
/// `cli/mod.rs` keeps this for the lifetime of `run_inner` so reporters
/// can borrow through it. Constructed once per invocation when
/// `--baseline` is set; absent otherwise.
struct DeltaState {
    snapshot: BaselineSnapshot,
    delta: AnalysisDelta,
}

fn load_delta_state(
    cli: &Cli,
    current: &crap4rs::domain::types::AnalysisResult,
) -> Result<Option<DeltaState>> {
    let Some(path) = cli.input.baseline.as_ref() else {
        return Ok(None);
    };
    let snapshot = baseline::load(path).map_err(|e| anyhow::anyhow!("{e}"))?;
    // delta::compute consumes both — we own snapshot.result, clone the
    // current analysis so the surrounding pipeline keeps its handle.
    let delta = delta::compute(snapshot.result.clone(), current.clone());
    Ok(Some(DeltaState { snapshot, delta }))
}

fn validate_display_flags(cli: &Cli) -> Result<()> {
    if cli.display.explain
        && matches!(cli.output.format, FormatArg::Table)
        && !cli.display.breakdown
    {
        bail!("--explain requires --breakdown for table output");
    }
    Ok(())
}

// ── Config loading & merging ───────────────────────────────────────

fn load_file_config(cli: &Cli) -> Result<Option<FileConfig>> {
    if let Some(path) = &cli.input.config {
        Ok(Some(config::load_config(path)?))
    } else {
        match config::discover_config()? {
            Some(path) => Ok(Some(config::load_config(&path)?)),
            None => Ok(None),
        }
    }
}

/// Merge CLI threshold with config file. Returns (ThresholdConfig, effective_display_threshold).
///
/// Resolution order (first match wins):
/// 1. `--threshold N`   — explicit CLI value
/// 2. `--strict`        → STRICT_THRESHOLD
/// 3. `--lenient`       → LENIENT_THRESHOLD
/// 4. config `preset`   → preset.threshold()
/// 5. config `threshold`
/// 6. DEFAULT_THRESHOLD
fn merge_threshold(cli: &Cli, file_config: &Option<FileConfig>) -> (ThresholdConfig, f64) {
    let global = cli
        .output
        .threshold
        .or_else(|| cli.output.strict.then_some(STRICT_THRESHOLD))
        .or_else(|| cli.output.lenient.then_some(LENIENT_THRESHOLD))
        .or_else(|| {
            file_config
                .as_ref()
                .and_then(|c| c.preset)
                .map(|p| p.threshold())
        })
        .or_else(|| file_config.as_ref().and_then(|c| c.threshold))
        .unwrap_or(DEFAULT_THRESHOLD);

    let overrides = file_config
        .as_ref()
        .map(|fc| fc.overrides.clone())
        .unwrap_or_default();

    let config = ThresholdConfig { global, overrides };
    (config, global)
}

fn merge_exclude(cli: &Cli, file_config: &Option<FileConfig>) -> Vec<String> {
    let mut exclude = cli.filter.exclude.clone();
    if let Some(fc) = file_config
        && let Some(fc_exclude) = &fc.exclude
    {
        let seen: std::collections::HashSet<String> = exclude.iter().cloned().collect();
        for pattern in fc_exclude {
            if !seen.contains(pattern) {
                exclude.push(pattern.clone());
            }
        }
    }
    exclude
}

// ── Validation ──────────────────────────────────────────────────────

fn validate_inputs(
    coverage: &std::path::Path,
    src: &std::path::Path,
    threshold: f64,
) -> Result<()> {
    match std::fs::metadata(coverage) {
        Ok(m) if m.is_file() => {}
        Ok(_) => bail!(
            "coverage path is not a file: {}\n  \
             hint: pass --coverage pointing to an LCOV file, not a directory",
            coverage.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "coverage file not found: {}\n  \
             hint: run `cargo llvm-cov --lcov --output-path lcov.info` first",
            coverage.display()
        ),
        Err(e) => bail!(
            "cannot access coverage file: {}: {e}\n  \
             hint: check file permissions",
            coverage.display()
        ),
    }
    match std::fs::metadata(src) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => bail!(
            "source path is not a directory: {}\n  \
             hint: pass --src <DIR> pointing to your Rust source root",
            src.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "source directory not found: {}\n  \
             hint: pass --src <DIR> pointing to your Rust source root",
            src.display()
        ),
        Err(e) => bail!(
            "cannot access source directory: {}: {e}\n  \
             hint: check directory permissions",
            src.display()
        ),
    }
    if !is_valid_threshold(threshold) {
        bail!(
            "threshold must be a finite positive number, got: {}",
            threshold
        );
    }
    Ok(())
}

// ── Diff validation ────────────────────────────────────────────────

fn validate_diff_ref(diff_ref: &str) -> Result<()> {
    if diff_ref.is_empty() {
        bail!("invalid diff ref: ref must not be empty");
    }
    if diff_ref.starts_with('-') {
        bail!(
            "invalid diff ref: {diff_ref}\n  \
             hint: ref must not start with a dash"
        );
    }
    Ok(())
}

fn preflight_git_worktree(src: &Path) -> Result<()> {
    let output = std::process::Command::new("git")
        .current_dir(src)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();

    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            bail!(
                "not inside a git work tree\n  \
                 hint: --diff requires a git repository\n  \
                 git: {stderr}",
            );
        }
        Err(e) => bail!(
            "not inside a git work tree\n  \
             hint: --diff requires git to be installed\n  \
             error: {e}",
        ),
    }
}

// ── Pre-flight checks ──────────────────────────────────────────────

fn preflight_checks(coverage: &std::path::Path, src: &std::path::Path) -> Result<()> {
    check_coverage_has_data(coverage)?;
    check_src_has_rust_files(src)?;
    Ok(())
}

fn check_coverage_has_data(path: &std::path::Path) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut in_sf_block = false;

    for line in reader.lines() {
        let line = line?;
        if line.starts_with("SF:") {
            in_sf_block = true;
            continue;
        }
        if in_sf_block
            && let Some(rest) = line.strip_prefix("DA:")
            && let Some((line_no, hits)) = rest.split_once(',')
            && line_no.parse::<usize>().is_ok()
            && hits.split(',').next().unwrap_or("").parse::<u64>().is_ok()
        {
            return Ok(());
        }
    }
    bail!(
        "no coverage data found in {}\n  \
         hint: ensure tests ran with coverage enabled (`cargo llvm-cov --lcov`)",
        path.display()
    );
}

fn check_src_has_rust_files(path: &std::path::Path) -> Result<()> {
    fn has_rs_files(dir: &std::path::Path) -> std::io::Result<bool> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            if ft.is_file() && entry.path().extension().is_some_and(|ext| ext == "rs") {
                return Ok(true);
            }
            if ft.is_dir() && has_rs_files(&entry.path())? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    if !has_rs_files(path)? {
        bail!(
            "no Rust source files found in {}\n  \
             hint: check that --src points to a directory containing .rs files",
            path.display()
        );
    }
    Ok(())
}

// ── Timestamp ──────────────────────────────────────────────────────

fn now_unix_epoch() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{secs}")
}

// ── Verbose diagnostics ────────────────────────────────────────────

fn majority_zero_coverage(files_analyzed: usize, files_zero_coverage: usize) -> bool {
    files_analyzed > 0 && files_zero_coverage * 2 > files_analyzed
}

fn warn_if_issues(diag: &AnalysisDiagnostics) {
    if !diag.parse_diagnostics.is_empty() {
        eprintln!(
            "warning: {} LCOV parse issue(s) encountered (use --verbose for details)",
            diag.parse_diagnostics.len()
        );
    }
    if diag.files_unparseable > 0 {
        eprintln!(
            "warning: {} source file(s) could not be parsed (use --verbose for details)",
            diag.files_unparseable
        );
    }
    if majority_zero_coverage(diag.files_analyzed, diag.files_zero_coverage) {
        eprintln!(
            "warning: in {}/{} analyzed files, all analyzed functions have 0% line coverage",
            diag.files_zero_coverage, diag.files_analyzed
        );
        eprintln!(
            "  hint: `cargo llvm-cov --lib` does not cover integration-only code (handlers, Tauri entry, BDD tests)"
        );
        eprintln!(
            "  hint: use --exclude to skip uncoverable paths (e.g., --exclude \"services/api/src/**\")"
        );
    }
}

fn print_diagnostics(diag: &AnalysisDiagnostics) {
    eprintln!(
        "verbose: file discovery: {} files found, {} unparseable",
        diag.files_found, diag.files_unparseable
    );
    eprintln!(
        "verbose: complexity: {} functions extracted",
        diag.functions_extracted
    );
    eprintln!(
        "verbose: matching: {} matched with coverage, {} without coverage data",
        diag.functions_matched, diag.functions_no_coverage
    );
    eprintln!(
        "verbose: coverage: {} files analyzed, {} where all analyzed functions have 0% line coverage",
        diag.files_analyzed, diag.files_zero_coverage
    );
    if !diag.parse_diagnostics.is_empty() {
        eprintln!(
            "verbose: LCOV parse diagnostics ({}):",
            diag.parse_diagnostics.len()
        );
        for d in &diag.parse_diagnostics {
            eprintln!("  {d}");
        }
    }
}

// ── Shell completions ───────────────────────────────────────────────

/// Print a shell completion script to stdout for the given shell.
/// `clap_complete::generate` covers POSIX shells + PowerShell + Elvish;
/// nushell uses the separate `clap_complete_nushell` crate.
fn emit_completions(shell: ShellArg) {
    let mut cmd = Cli::command();
    let bin = "crap4rs";
    let stdout = &mut std::io::stdout();
    match shell {
        ShellArg::Bash => clap_complete::generate(ClapShell::Bash, &mut cmd, bin, stdout),
        ShellArg::Zsh => clap_complete::generate(ClapShell::Zsh, &mut cmd, bin, stdout),
        ShellArg::Fish => clap_complete::generate(ClapShell::Fish, &mut cmd, bin, stdout),
        ShellArg::Powershell => {
            clap_complete::generate(ClapShell::PowerShell, &mut cmd, bin, stdout)
        }
        ShellArg::Elvish => clap_complete::generate(ClapShell::Elvish, &mut cmd, bin, stdout),
        ShellArg::Nushell => {
            clap_complete::generate(clap_complete_nushell::Nushell, &mut cmd, bin, stdout)
        }
    }
}

// ── Color wiring ────────────────────────────────────────────────────

fn apply_color(choice: ColorArg) {
    match choice {
        ColorArg::Auto => colored::control::unset_override(),
        ColorArg::Always => colored::control::set_override(true),
        ColorArg::Never => colored::control::set_override(false),
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut full = vec!["crap4rs"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full)
    }

    #[test]
    fn no_args_parses_with_coverage_none() {
        // `--coverage` is enforced at runtime via run_inner (so that
        // the `completions` subcommand can skip it), not at clap parse
        // time. Bare `crap4rs` therefore parses successfully here but
        // would `bail!` once dispatched.
        let cli = parse(&[]).unwrap();
        assert!(cli.input.coverage.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn minimal_valid_args() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert_eq!(cli.input.coverage.as_deref(), Some(Path::new("lcov.info")));
        assert_eq!(cli.input.src, None);
    }

    #[test]
    fn completions_subcommand_does_not_require_coverage() {
        let cli = parse(&["completions", "bash"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Completions {
                shell: ShellArg::Bash
            })
        ));
        assert!(cli.input.coverage.is_none());
    }

    #[test]
    fn default_metric_is_none() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(cli.input.metric.is_none());
    }

    #[test]
    fn default_format_is_table() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(matches!(cli.output.format, FormatArg::Table));
    }

    #[test]
    fn default_threshold_is_none() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(cli.output.threshold.is_none());
    }

    #[test]
    fn default_color_is_auto() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(matches!(cli.display.color, ColorArg::Auto));
    }

    #[test]
    fn metric_cyclomatic() {
        let cli = parse(&["--coverage", "lcov.info", "--metric", "cyclomatic"]).unwrap();
        assert!(matches!(cli.input.metric, Some(MetricArg::Cyclomatic)));
    }

    #[test]
    fn format_json() {
        let cli = parse(&["--coverage", "lcov.info", "--format", "json"]).unwrap();
        assert!(matches!(cli.output.format, FormatArg::Json));
    }

    #[test]
    fn custom_threshold() {
        let cli = parse(&["--coverage", "lcov.info", "--threshold", "15.5"]).unwrap();
        assert_eq!(cli.output.threshold, Some(15.5));
    }

    #[test]
    fn custom_src() {
        let cli = parse(&["--coverage", "lcov.info", "--src", "crates/"]).unwrap();
        assert_eq!(cli.input.src, Some(PathBuf::from("crates/")));
    }

    #[test]
    fn exclude_repeatable() {
        let cli = parse(&[
            "--coverage",
            "lcov.info",
            "--exclude",
            "tests/**",
            "--exclude",
            "benches/**",
        ])
        .unwrap();
        assert_eq!(cli.filter.exclude, vec!["tests/**", "benches/**"]);
    }

    #[test]
    fn no_gitignore_flag() {
        let cli = parse(&["--coverage", "lcov.info", "--no-gitignore"]).unwrap();
        assert!(cli.filter.no_gitignore);
    }

    #[test]
    fn only_failing_flag() {
        let cli = parse(&["--coverage", "lcov.info", "--only-failing"]).unwrap();
        assert!(cli.filter.only_failing);
    }

    #[test]
    fn group_by_file_parses() {
        let cli = parse(&["--coverage", "lcov.info", "--group-by", "file"]).unwrap();
        assert!(matches!(cli.filter.group_by, Some(GroupByArg::File)));
    }

    #[test]
    fn group_by_absence_is_none() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(cli.filter.group_by.is_none());
    }

    #[test]
    fn group_by_invalid_value_rejected() {
        let err = parse(&["--coverage", "lcov.info", "--group-by", "module"]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("invalid value"), "expected clap error: {msg}");
        assert!(
            msg.contains("--group-by") || msg.contains("module"),
            "error should attribute to --group-by: {msg}"
        );
    }

    #[test]
    fn group_by_arg_to_domain_file() {
        let domain: GroupKey = GroupByArg::File.into();
        assert_eq!(domain, GroupKey::File);
    }

    #[test]
    fn verbose_flag() {
        let cli = parse(&["--coverage", "lcov.info", "-v"]).unwrap();
        assert!(cli.display.verbose);
    }

    #[test]
    fn quiet_flag() {
        let cli = parse(&["--coverage", "lcov.info", "-q"]).unwrap();
        assert!(cli.display.quiet);
    }

    #[test]
    fn color_always() {
        let cli = parse(&["--coverage", "lcov.info", "--color", "always"]).unwrap();
        assert!(matches!(cli.display.color, ColorArg::Always));
    }

    #[test]
    fn color_never() {
        let cli = parse(&["--coverage", "lcov.info", "--color", "never"]).unwrap();
        assert!(matches!(cli.display.color, ColorArg::Never));
    }

    #[test]
    fn invalid_metric_rejected() {
        let err = parse(&["--coverage", "lcov.info", "--metric", "halstead"]).unwrap_err();
        assert!(err.to_string().contains("invalid value"));
    }

    #[test]
    fn invalid_format_rejected() {
        let err = parse(&["--coverage", "lcov.info", "--format", "xml"]).unwrap_err();
        assert!(err.to_string().contains("invalid value"));
    }

    #[test]
    fn metric_arg_to_domain_cognitive() {
        let domain: ComplexityMetric = MetricArg::Cognitive.into();
        assert_eq!(domain, ComplexityMetric::Cognitive);
    }

    #[test]
    fn metric_arg_to_domain_cyclomatic() {
        let domain: ComplexityMetric = MetricArg::Cyclomatic.into();
        assert_eq!(domain, ComplexityMetric::Cyclomatic);
    }

    #[test]
    fn validate_missing_coverage_file() {
        let err = validate_inputs(
            Path::new("nonexistent.info"),
            Path::new("src"),
            DEFAULT_THRESHOLD,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("coverage file not found"));
        assert!(msg.contains("cargo llvm-cov"));
    }

    #[test]
    fn validate_missing_src_dir() {
        let err = validate_inputs(
            Path::new("Cargo.toml"),
            Path::new("nonexistent_dir"),
            DEFAULT_THRESHOLD,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("source directory not found"));
    }

    #[test]
    fn validate_negative_threshold() {
        let err = validate_inputs(Path::new("Cargo.toml"), Path::new("src"), -5.0).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_zero_threshold() {
        let err = validate_inputs(Path::new("Cargo.toml"), Path::new("src"), 0.0).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_infinity_threshold() {
        let err =
            validate_inputs(Path::new("Cargo.toml"), Path::new("src"), f64::INFINITY).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_src_is_file_not_dir() {
        let err = validate_inputs(
            Path::new("Cargo.toml"),
            Path::new("Cargo.toml"),
            DEFAULT_THRESHOLD,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("source path is not a directory"));
    }

    #[test]
    fn validate_coverage_is_dir_not_file() {
        let err =
            validate_inputs(Path::new("src"), Path::new("src"), DEFAULT_THRESHOLD).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("coverage path is not a file"));
    }

    #[test]
    fn format_short_flag() {
        let cli = parse(&["--coverage", "lcov.info", "-f", "json"]).unwrap();
        assert!(matches!(cli.output.format, FormatArg::Json));
    }

    #[test]
    fn config_flag_accepts_path() {
        let cli = parse(&["--coverage", "lcov.info", "--config", "my-config.toml"]).unwrap();
        assert_eq!(cli.input.config, Some(PathBuf::from("my-config.toml")));
    }

    #[test]
    fn config_flag_defaults_to_none() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert_eq!(cli.input.config, None);
    }

    #[test]
    fn view_flag_accepts_name() {
        let cli = parse(&["--coverage", "lcov.info", "--view", "ci"]).unwrap();
        assert_eq!(cli.input.view, Some("ci".to_string()));
    }

    #[test]
    fn view_flag_defaults_to_none() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert_eq!(cli.input.view, None);
    }

    #[test]
    fn merge_threshold_cli_overrides_config() {
        let cli = parse(&["--coverage", "lcov.info", "--threshold", "15.0"]).unwrap();
        let file_config = Some(FileConfig {
            threshold: Some(10.0),
            ..FileConfig::default()
        });
        let (config, display) = merge_threshold(&cli, &file_config);
        assert_eq!(config.global, 15.0);
        assert_eq!(display, 15.0);
    }

    #[test]
    fn merge_threshold_uses_config_when_cli_default() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let file_config = Some(FileConfig {
            threshold: Some(12.0),
            ..FileConfig::default()
        });
        let (config, display) = merge_threshold(&cli, &file_config);
        assert_eq!(config.global, 12.0);
        assert_eq!(display, 12.0);
    }

    #[test]
    fn merge_threshold_preserves_overrides() {
        use crap4rs::domain::threshold::ThresholdOverride;
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let file_config = Some(FileConfig {
            threshold: Some(10.0),
            overrides: vec![ThresholdOverride {
                pattern: "domain/**".to_string(),
                threshold: 5.0,
            }],
            ..FileConfig::default()
        });
        let (config, _) = merge_threshold(&cli, &file_config);
        assert_eq!(config.overrides.len(), 1);
        assert_eq!(config.overrides[0].pattern, "domain/**");
    }

    #[test]
    fn merge_threshold_no_config() {
        let cli = parse(&["--coverage", "lcov.info", "--threshold", "20.0"]).unwrap();
        let (config, display) = merge_threshold(&cli, &None);
        assert_eq!(config.global, 20.0);
        assert!(config.overrides.is_empty());
        assert_eq!(display, 20.0);
    }

    #[test]
    fn merge_threshold_explicit_default_overrides_config() {
        // User explicitly passes --threshold 8.0 (same as DEFAULT_THRESHOLD).
        // This MUST override the config file's threshold of 12.0.
        let cli = parse(&["--coverage", "lcov.info", "--threshold", "8.0"]).unwrap();
        let file_config = Some(FileConfig {
            threshold: Some(12.0),
            ..FileConfig::default()
        });
        let (config, display) = merge_threshold(&cli, &file_config);
        assert_eq!(
            config.global, 8.0,
            "explicit CLI default must override config"
        );
        assert_eq!(display, 8.0);
    }

    #[test]
    fn merge_threshold_no_cli_no_config_uses_hardcoded_default() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let (config, display) = merge_threshold(&cli, &None);
        assert_eq!(config.global, DEFAULT_THRESHOLD);
        assert_eq!(display, DEFAULT_THRESHOLD);
    }

    #[test]
    fn merge_exclude_combines_cli_and_config() {
        let cli = parse(&["--coverage", "lcov.info", "--exclude", "tests/**"]).unwrap();
        let file_config = Some(FileConfig {
            exclude: Some(vec!["benches/**".to_string()]),
            ..FileConfig::default()
        });
        let exclude = merge_exclude(&cli, &file_config);
        assert_eq!(exclude, vec!["tests/**", "benches/**"]);
    }

    #[test]
    fn merge_exclude_deduplicates() {
        let cli = parse(&["--coverage", "lcov.info", "--exclude", "tests/**"]).unwrap();
        let file_config = Some(FileConfig {
            exclude: Some(vec!["tests/**".to_string()]),
            ..FileConfig::default()
        });
        let exclude = merge_exclude(&cli, &file_config);
        assert_eq!(exclude, vec!["tests/**"]);
    }

    // ── --diff flag tests ───────────────────────────────────────────

    #[test]
    fn diff_flag_accepts_ref() {
        let cli = parse(&["--coverage", "lcov.info", "--diff", "main"]).unwrap();
        assert_eq!(cli.filter.diff, Some("main".to_string()));
    }

    #[test]
    fn diff_flag_defaults_to_none() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert_eq!(cli.filter.diff, None);
    }

    #[test]
    fn diff_flag_accepts_commit_sha() {
        let cli = parse(&["--coverage", "lcov.info", "--diff", "abc123"]).unwrap();
        assert_eq!(cli.filter.diff, Some("abc123".to_string()));
    }

    #[test]
    fn diff_flag_accepts_head_tilde() {
        let cli = parse(&["--coverage", "lcov.info", "--diff", "HEAD~1"]).unwrap();
        assert_eq!(cli.filter.diff, Some("HEAD~1".to_string()));
    }

    #[test]
    fn validate_diff_ref_rejects_empty_string() {
        let err = validate_diff_ref("").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("must not be empty"));
    }

    #[test]
    fn validate_diff_ref_rejects_dash_prefix() {
        let err = validate_diff_ref("--malicious").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("invalid diff ref"));
        assert!(msg.contains("must not start with a dash"));
    }

    #[test]
    fn validate_diff_ref_accepts_normal_ref() {
        assert!(validate_diff_ref("main").is_ok());
        assert!(validate_diff_ref("HEAD~1").is_ok());
        assert!(validate_diff_ref("abc123").is_ok());
    }

    #[test]
    fn preflight_git_worktree_passes_in_git_repo() {
        // Initialize a fresh git repo in a temp dir so the test is self-contained
        // and works under tools (e.g. cargo-mutants) that copy the source tree
        // without `.git`.
        let tmp = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(tmp.path())
            .status()
            .expect("git init");
        assert!(status.success(), "git init failed");
        assert!(preflight_git_worktree(tmp.path()).is_ok());
    }

    #[test]
    fn breakdown_flag_parsed() {
        let cli = parse(&["--coverage", "lcov.info", "--breakdown"]).unwrap();
        assert!(cli.display.breakdown);
    }

    #[test]
    fn breakdown_flag_default_false() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(!cli.display.breakdown);
    }

    #[test]
    fn explain_flag_parsed() {
        let cli = parse(&["--coverage", "lcov.info", "--explain"]).unwrap();
        assert!(cli.display.explain);
    }

    #[test]
    fn explain_flag_default_false() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(!cli.display.explain);
    }

    #[test]
    fn explain_requires_breakdown_for_table_output() {
        let cli = parse(&["--coverage", "lcov.info", "--explain"]).unwrap();
        let err = validate_display_flags(&cli).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--breakdown"));
        assert!(msg.contains("--explain"));
    }

    #[test]
    fn explain_allowed_for_json_output() {
        let cli = parse(&["--coverage", "lcov.info", "--format", "json", "--explain"]).unwrap();
        assert!(validate_display_flags(&cli).is_ok());
    }

    #[test]
    fn color_overrides_set_global_state() {
        // Combined into one test to avoid nondeterministic interleaving —
        // colored::control uses a process-global flag that parallel tests
        // can race on.
        apply_color(ColorArg::Never);
        assert!(!colored::control::SHOULD_COLORIZE.should_colorize());

        apply_color(ColorArg::Always);
        assert!(colored::control::SHOULD_COLORIZE.should_colorize());

        apply_color(ColorArg::Auto);
    }

    // ── Pre-flight check tests ─────────────────────────────────────────

    #[test]
    fn preflight_empty_coverage_file() {
        let dir = tempfile::tempdir().unwrap();
        let cov = dir.path().join("empty.info");
        std::fs::write(&cov, "").unwrap();

        let err = check_coverage_has_data(&cov).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no coverage data found"));
        assert!(msg.contains("cargo llvm-cov"));
    }

    #[test]
    fn preflight_coverage_no_da_lines() {
        let dir = tempfile::tempdir().unwrap();
        let cov = dir.path().join("no_da.info");
        std::fs::write(&cov, "SF:src/main.rs\nend_of_record\n").unwrap();

        let err = check_coverage_has_data(&cov).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no coverage data found"));
    }

    #[test]
    fn preflight_coverage_with_da_lines_passes() {
        let dir = tempfile::tempdir().unwrap();
        let cov = dir.path().join("good.info");
        std::fs::write(&cov, "SF:src/main.rs\nDA:1,5\nend_of_record\n").unwrap();

        assert!(check_coverage_has_data(&cov).is_ok());
    }

    #[test]
    fn preflight_coverage_da_outside_sf_block_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cov = dir.path().join("orphan_da.info");
        std::fs::write(&cov, "DA:1,5\nend_of_record\n").unwrap();

        let err = check_coverage_has_data(&cov).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no coverage data found"));
    }

    #[test]
    fn preflight_coverage_malformed_da_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cov = dir.path().join("bad_da.info");
        std::fs::write(&cov, "SF:src/main.rs\nDA:not_a_number\nend_of_record\n").unwrap();

        let err = check_coverage_has_data(&cov).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no coverage data found"));
    }

    #[test]
    fn preflight_src_dir_no_rust_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello").unwrap();

        let err = check_src_has_rust_files(dir.path()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no Rust source files found"));
    }

    #[test]
    fn preflight_src_dir_empty() {
        let dir = tempfile::tempdir().unwrap();

        let err = check_src_has_rust_files(dir.path()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no Rust source files found"));
    }

    #[test]
    fn preflight_src_dir_with_rs_files_passes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        assert!(check_src_has_rust_files(dir.path()).is_ok());
    }

    #[test]
    fn preflight_src_dir_nested_rs_files_passes() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("sub");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("lib.rs"), "pub fn foo() {}").unwrap();

        assert!(check_src_has_rust_files(dir.path()).is_ok());
    }

    // ── --strict / --lenient flag tests ───────────────────────────────

    #[test]
    fn strict_flag_parses() {
        let cli = parse(&["--coverage", "lcov.info", "--strict"]).unwrap();
        assert!(cli.output.strict);
    }

    #[test]
    fn lenient_flag_parses() {
        let cli = parse(&["--coverage", "lcov.info", "--lenient"]).unwrap();
        assert!(cli.output.lenient);
    }

    #[test]
    fn strict_and_threshold_mutually_exclusive() {
        parse(&["--coverage", "lcov.info", "--strict", "--threshold", "20"]).unwrap_err();
    }

    #[test]
    fn strict_and_lenient_mutually_exclusive() {
        parse(&["--coverage", "lcov.info", "--strict", "--lenient"]).unwrap_err();
    }

    #[test]
    fn merge_threshold_strict_flag() {
        use crap4rs::domain::threshold::STRICT_THRESHOLD;
        let cli = parse(&["--coverage", "lcov.info", "--strict"]).unwrap();
        let (config, display) = merge_threshold(&cli, &None);
        assert_eq!(config.global, STRICT_THRESHOLD);
        assert_eq!(display, STRICT_THRESHOLD);
    }

    #[test]
    fn merge_threshold_lenient_flag() {
        use crap4rs::domain::threshold::LENIENT_THRESHOLD;
        let cli = parse(&["--coverage", "lcov.info", "--lenient"]).unwrap();
        let (config, display) = merge_threshold(&cli, &None);
        assert_eq!(config.global, LENIENT_THRESHOLD);
        assert_eq!(display, LENIENT_THRESHOLD);
    }

    #[test]
    fn merge_threshold_toml_preset_used_when_no_cli_flag() {
        use crap4rs::domain::threshold::{STRICT_THRESHOLD, ThresholdPreset};
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let file_config = Some(FileConfig {
            preset: Some(ThresholdPreset::Strict),
            ..FileConfig::default()
        });
        let (config, _) = merge_threshold(&cli, &file_config);
        assert_eq!(config.global, STRICT_THRESHOLD);
    }

    #[test]
    fn merge_threshold_cli_threshold_overrides_toml_preset() {
        use crap4rs::domain::threshold::ThresholdPreset;
        let cli = parse(&["--coverage", "lcov.info", "--threshold", "50.0"]).unwrap();
        let file_config = Some(FileConfig {
            preset: Some(ThresholdPreset::Strict),
            ..FileConfig::default()
        });
        let (config, _) = merge_threshold(&cli, &file_config);
        assert_eq!(config.global, 50.0);
    }

    // ── majority_zero_coverage predicate tests ─────────────────────────

    #[test]
    fn zero_coverage_warn_triggers_above_50_percent() {
        assert!(majority_zero_coverage(10, 6));
        assert!(majority_zero_coverage(1, 1));
        assert!(majority_zero_coverage(3, 2));
    }

    #[test]
    fn zero_coverage_warn_does_not_trigger_at_exactly_50_percent() {
        assert!(!majority_zero_coverage(10, 5));
        assert!(!majority_zero_coverage(2, 1));
    }

    #[test]
    fn zero_coverage_warn_does_not_trigger_when_no_files() {
        assert!(!majority_zero_coverage(0, 0));
    }
}
