//! CLI entry point — thin shell over the library crate.
//!
//! Parses args with clap, validates inputs, delegates to `core::analyze()`.
//! No business logic lives here.
//!
//! `cli::run<P>` is generic over the coverage adapter's parse-diagnostic
//! type so the same dispatch shell drives every adapter binary. The
//! per-binary main.rs supplies the complexity + coverage ports as `&dyn`
//! trait objects (ADR D9) plus an `AdapterMeta` carrying the binary's
//! name, version, help copy, extensions, and config-file name.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use anyhow::{Result, bail};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell as ClapShell;

use crate::adapters::baseline::{self, BaselineSnapshot};
use crate::adapters::config::{self, FileConfig};
use crate::adapters::reporters;
use crate::adapters::reporters::json::DeltaContext;
use crate::core::{AnalysisOutput, AnalyzeOptions};
use crate::domain::delta::{self, AnalysisDelta, DeltaView};
use crate::domain::threshold::{ThresholdConfig, ThresholdPreset, is_valid_threshold};
use crate::domain::types::{AnalysisDiagnostics, ComplexityMetric};
use crate::domain::view::{self, GroupKey, SortKey};
use crate::ports::{ComplexityPort, CoveragePort, ParseDiagnostic};

mod delta_args;
mod init;
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
    /// SARIF v2.1.0 — for GitHub Code Scanning (upload-sarif@v3)
    Sarif,
    /// Agent-oriented JSON with Diagnostic remediation hints (experimental)
    Advice,
    /// Single mokumo-scorecard `Row::CrapDelta` JSON object — for scorecard
    /// aggregator consumption (mokumo schema_version=2).
    ScorecardRow,
    /// Self-contained HTML dashboard with summary stats, risk
    /// distribution, and per-file collapsible function tables. Inline
    /// CSS, no external assets, mobile-responsive.
    Html,
}

/// One requested output: a format and an optional file destination.
///
/// Parsed from `--format X` (stdout) or `--format X:FILE` (write to file).
/// `--format` accepts a comma-separated list of these specs so a single
/// analysis pass can fan out to multiple shapes.
#[derive(Debug, Clone)]
pub struct FormatSpec {
    pub format: FormatArg,
    pub output: Option<PathBuf>,
}

impl std::str::FromStr for FormatSpec {
    type Err = String;

    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        let (fmt_str, output) = match spec.split_once(':') {
            Some((f, path)) if !path.is_empty() => (f, Some(PathBuf::from(path))),
            Some((_, _)) => return Err(format!("empty file path in `--format {spec}`")),
            None => (spec, None),
        };
        let format = FormatArg::from_str(fmt_str, true)
            .map_err(|e| format!("invalid format `{fmt_str}`: {e}"))?;
        Ok(FormatSpec { format, output })
    }
}

/// Clap value parser for `FormatSpec` — delegates to the `FromStr` impl.
fn parse_format_spec(s: &str) -> Result<FormatSpec, String> {
    s.parse()
}

/// Sort key for the displayed view.
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

/// Reverse mapping for saved view presets — preset stores
/// domain `SortKey`, but `FilterArgs.sort_by` is the clap-side wrapper.
///
/// `SortKey` is `#[non_exhaustive]` for cross-crate consumers, but
/// the cli module lives in the same crate as the domain `SortKey`
/// definition, so the compiler treats the match as exhaustive
/// without a wildcard arm. New domain variants must still land with a
/// paired CLI variant in the same PR — clippy's missing-pattern error
/// is now the loud failure point (the formerly-required wildcard arm
/// triggered `unreachable_patterns` post-relocation).
impl From<SortKey> for SortKeyArg {
    fn from(key: SortKey) -> Self {
        match key {
            SortKey::Crap => SortKeyArg::Crap,
            SortKey::Coverage => SortKeyArg::Coverage,
            SortKey::Complexity => SortKeyArg::Complexity,
            SortKey::Path => SortKeyArg::Path,
        }
    }
}

/// Group key for the displayed view.
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

/// Reverse mapping for saved view presets. See `From<SortKey>`
/// above for the wildcard-arm rationale.
impl From<GroupKey> for GroupByArg {
    fn from(key: GroupKey) -> Self {
        match key {
            GroupKey::File => GroupByArg::File,
        }
    }
}

/// Sort key for the delta block.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DeltaSortKeyArg {
    /// Magnitude of change descending — regressions first (default)
    ScoreDelta,
    /// Current CRAP score descending; `Removed` rows last
    CurrentCrap,
    /// Baseline CRAP score descending; `Added` rows last
    BaselineCrap,
    /// Alphabetical by file_path then qualified_name
    Path,
}

impl From<DeltaSortKeyArg> for crate::domain::delta::DeltaSortKey {
    fn from(arg: DeltaSortKeyArg) -> Self {
        use crate::domain::delta::DeltaSortKey;
        match arg {
            DeltaSortKeyArg::ScoreDelta => DeltaSortKey::ScoreDelta,
            DeltaSortKeyArg::CurrentCrap => DeltaSortKey::CurrentCrap,
            DeltaSortKeyArg::BaselineCrap => DeltaSortKey::BaselineCrap,
            DeltaSortKeyArg::Path => DeltaSortKey::Path,
        }
    }
}

/// Change-kind subset for `--delta-only`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DeltaKindArg {
    Added,
    Removed,
    Modified,
}

impl From<DeltaKindArg> for crate::domain::delta::ChangeKind {
    fn from(arg: DeltaKindArg) -> Self {
        use crate::domain::delta::ChangeKind;
        match arg {
            DeltaKindArg::Added => ChangeKind::Added,
            DeltaKindArg::Removed => ChangeKind::Removed,
            DeltaKindArg::Modified => ChangeKind::Modified,
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

/// Shell name for completion script generation.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellArg {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
    Nushell,
}

/// Top-level subcommands. Optional — when absent, the analyzer runs
/// the default analysis path that requires `--coverage`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Generate a shell completion script to stdout.
    Completions {
        #[arg(value_enum)]
        shell: ShellArg,
    },
    /// Generate a starter config TOML in the current directory.
    ///
    /// Interactive by default (asks for a threshold preset);
    /// `--non-interactive` uses defaults for CI/scripts. Refuses to
    /// overwrite an existing config unless `--force` is passed.
    Init {
        /// Overwrite an existing config file in this directory.
        #[arg(long)]
        force: bool,
        /// Skip the interactive prompt and use defaults (preset =
        /// "default"). CI-friendly.
        #[arg(long)]
        non_interactive: bool,
    },
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Input")]
pub struct InputArgs {
    /// Path to the coverage file (adapter-specific format).
    /// Required for analysis; not required for the `completions` subcommand.
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub coverage: Option<PathBuf>,

    /// Root directory of source files to analyze [default: src]
    #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
    pub src: Option<PathBuf>,

    /// Complexity metric to use [default: cognitive]
    #[arg(long, value_enum)]
    pub metric: Option<MetricArg>,

    /// Path to config file (default: auto-discover the adapter's config TOML)
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub config: Option<PathBuf>,

    /// Resolve and apply a saved view preset from the adapter's config TOML.
    ///
    /// The preset's fields (`top`, `min_coverage`, `max_coverage`, `sort`,
    /// `only_failing`, `no_fail`, `group_by`, `minimal_view`) are folded
    /// into the parsed CLI before the report is shaped. CLI flags
    /// override the preset's `Option<T>` fields. Bare-bool flags
    /// OR-merge with the preset (an explicit `--no-fail` adds to a
    /// preset's value but cannot turn off `no_fail = true`).
    #[arg(long, value_name = "NAME")]
    pub view: Option<String>,

    /// Path to a previously-emitted JSON envelope, used as the baseline
    /// for delta analysis.
    ///
    /// The analyzer runs the current analysis as usual, then compares
    /// against the baseline's `result` block to produce a `delta` block
    /// in the output (see `--format json`, `--format markdown` for
    /// rendering). Generate the baseline file by piping a previous run:
    /// `<binary> --coverage <file> --format json > baseline.json`.
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
    /// Output format(s).
    ///
    /// Accepts a single format (`--format json`) for stdout, or a comma-
    /// separated list to fan out a single analysis pass to multiple
    /// destinations (`--format json:envelope.json,markdown:report.md`).
    /// Each entry is `FORMAT` (stdout) or `FORMAT:FILE` (write to file).
    /// Multi-format invocations require every entry to specify a file —
    /// stdout cannot multiplex.
    #[arg(
        short,
        long,
        value_delimiter = ',',
        default_value = "table",
        value_parser = parse_format_spec
    )]
    pub format: Vec<FormatSpec>,

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
    /// `--quiet` for silent success in CI. With `--delta-gate`, also
    /// overrides the delta-gate exit-code translation (truth still in
    /// `delta.summary.passed`).
    #[arg(long)]
    pub no_fail: bool,

    /// Fail the build (exit 1) when the baseline comparison introduces
    /// new threshold violations.
    ///
    /// Off by default — delta is informational unless this flag is set.
    /// Drives off `delta.summary.passed`, which is true iff
    /// `new_violations == 0`. Pre-existing violations (functions that
    /// already exceeded threshold in the baseline) do NOT contribute,
    /// so re-running with no code changes never trips the gate. Only
    /// meaningful with `--baseline`. Composes with `--no-fail` (which
    /// overrides BOTH gates).
    #[arg(long, requires = "baseline")]
    pub delta_gate: bool,

    /// Omit the denormalized `view.shown` row array from JSON output.
    ///
    /// Payload-size escape hatch for very large codebases. The
    /// envelope's `result` block (the gate) is unaffected; `view.spec`,
    /// `view.eligible_count`, `view.truncated`, and `view.shown_summary`
    /// remain so consumers retain full scope context. Only meaningful
    /// with `--format json`.
    #[arg(long)]
    pub minimal_view: bool,

    /// Emit a single-line analysis verdict instead of the full report.
    ///
    /// Format: `<STATUS>: <N> functions | <M> above threshold (<T>) | worst: <W> | avg: <A>`
    /// (e.g., `PASS: 1082 functions | 0 above threshold (25) | worst: 13.0 | avg: 1.6`).
    /// Short-circuits `--format`: when set, the format dispatch is
    /// skipped and only the summary line is printed to stdout.
    /// Composes with `--no-fail` (exit 0 always when set, summary still
    /// emitted) and `--quiet` (quiet wins — no output, exit code only).
    /// Matches crap4ts's `--summary` shape byte-for-byte for the shared
    /// subset so a CI line-template can match either tool.
    #[arg(long)]
    pub summary: bool,
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
    /// Useful for CI PR gating: `<binary> --coverage <file> --diff main`
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

    /// Truncate the delta block to the top N rows by `--delta-sort`.
    /// `--delta-top 0` means "no limit". Independent of `--top`, which
    /// truncates the analysis view (`view.shown`).
    ///
    /// `allow_hyphen_values`: parses `--delta-top -3` as a value (not
    /// an unknown flag) so the error attribution to `--delta-top` is
    /// readable.
    #[arg(long, allow_hyphen_values = true, value_name = "N")]
    pub delta_top: Option<u32>,

    /// Sort key for the delta block.
    ///
    /// `score-delta` (default) — magnitude of change descending
    /// (regressions first). `current-crap` — current CRAP descending,
    /// `Removed` rows last. `baseline-crap` — baseline CRAP descending,
    /// `Added` rows last. `path` — alphabetical by file then qualified
    /// name.
    #[arg(long, value_enum, value_name = "KEY")]
    pub delta_sort: Option<DeltaSortKeyArg>,

    /// Comma-separated list of change kinds to include in the delta
    /// block: `added`, `removed`, `modified`. Default: all three.
    #[arg(long, value_delimiter = ',', value_name = "KINDS")]
    pub delta_only: Vec<DeltaKindArg>,
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

    /// Render the full per-function table in markdown output.
    ///
    /// By default `--format markdown` produces a compact summary plus a
    /// top-N table (failures if any exist, otherwise the worst by CRAP).
    /// This flag appends the legacy row-per-function table — useful when
    /// piping into a longer document instead of a PR comment. Has no
    /// effect on other output formats.
    #[arg(long)]
    pub md_full_table: bool,

    /// Number of rows in the markdown top-N table (default 10).
    ///
    /// Bounds the failures list (or worst-by-CRAP list when nothing
    /// exceeds threshold). The summary block is unaffected — its stats
    /// always reflect the full unshapeable analysis.
    #[arg(long, value_name = "N", default_value_t = 10)]
    pub md_top: usize,
}

// ── Top-level CLI ───────────────────────────────────────────────────

// `long_version` is overridden at runtime in `cli::run` so each binary's
// build script can splice the git hash + build date into its `--version`
// output without forcing crap-core to read an env var that's only set
// during the binary's compile. The derive's `version` here resolves to
// the **adapter** crate's `CARGO_PKG_VERSION` because clap captures the
// env at the macro expansion site — that's the binary crate's version
// when compiling the binary, but the lib crate's version when compiling
// the lib. Production callers always reach `cli::run` through the binary,
// so `--version` displays the adapter's version. Tests that go through
// the lib see crap-core's version, which is fine for tests.
//
// Consumer-visible version strings flow as parameters via `AdapterMeta`
// from the binary where `env!` resolves against the bin's package, not
// against this module's home crate.

// `about` / `long_about` / `after_help` are intentionally generic
// here — adapter-flavored copy (language name, AST library, coverage
// toolchain, runnable examples) is injected at runtime by
// `build_command` from `AdapterMeta`. Library tests that
// `try_parse_from` `Cli` directly see this generic default; the
// binary always overrides.
#[derive(Debug, Parser)]
#[command(
    version,
    author,
    about = "CRAP score analyzer",
    long_about = "CRAP (Change Risk Anti-Patterns) score analyzer. \
                  Combines complexity analysis with line-coverage data to \
                  identify functions that are both complex and under-tested. \
                  Adapter-specific binaries (crap4rs for Rust, crap4ts for \
                  TypeScript) wire language-specific complexity walkers and \
                  coverage parsers behind the same orchestrator."
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

/// Adapter-supplied runtime metadata that crap-core threads through
/// `parse_args`, `run`, and the reporter call sites.
///
/// All fields are `&'static` because every production caller supplies
/// `env!(...)` / `build.rs`-stamped literals or `const &[&str]` slices.
/// Tests construct from `&'static str` literals too. The lifetime
/// parameter was dropped in #161 — no caller ever needed non-static
/// metadata, and the `<'a>` ripple polluted 15 function signatures for
/// no payoff. The struct stays `Copy` so threading is trivial.
///
/// Reporters keep a flat `(tool_name, tool_version)` call boundary —
/// the struct only travels through orchestration code.
#[derive(Debug, Clone, Copy)]
pub struct AdapterMeta {
    /// Adapter binary name (e.g., `"crap4rs"`, `"crap4ts"`). Drives
    /// clap's `--version` output, the `name` field in SARIF, and the
    /// header line in table/markdown/html reporters.
    pub tool_name: &'static str,
    /// Short version string (e.g., `"0.5.0"`). Threaded to every
    /// reporter alongside `tool_name`.
    pub tool_version: &'static str,
    /// Long version string for `--version --long` (e.g.,
    /// `"0.5.0 (abc1234 2026-05-09)"`).
    pub long_version: &'static str,
    /// Short adapter-flavored help text (one-line, shown by `--help`).
    pub about: &'static str,
    /// Long adapter-flavored help text (multi-paragraph, shown by
    /// `--help` in full mode).
    pub long_about: &'static str,
    /// `after_help` block with adapter-specific examples
    /// (`crap4rs --coverage lcov.info ...` etc.). May be empty.
    pub after_help: &'static str,
    /// Coverage-tool hint shown when `--coverage` points at a file
    /// with no `SF:` / `DA:` records. Adapter-specific because the
    /// remediation depends on the coverage toolchain (Rust: `cargo
    /// llvm-cov --lcov`; TS: `c8 --reporter=lcov`).
    pub coverage_hint: &'static str,
    /// File extensions the walker should pick up (e.g.,
    /// `&["rs"]` for crap4rs; `&["ts","tsx","js","jsx","mjs","cjs"]`
    /// for crap4ts). Adapter binaries supply a `const &[&str]`; copied
    /// into `AnalyzeOptions.extensions` at the orchestration boundary.
    pub extensions: &'static [&'static str],
    /// Adapter repo URL spliced into SARIF's
    /// `runs[0].tool.driver.informationUri`. Adapter-specific so
    /// crap4ts SARIF output links to crap4ts's repo, not crap4rs's.
    pub tool_info_uri: &'static str,
    /// Adapter rule-help URL spliced into SARIF's
    /// `runs[0].tool.driver.rules[0].helpUri`. Adapter-specific for
    /// the same reason as `tool_info_uri`.
    pub rule_help_uri: &'static str,
    /// Conventional config file name the adapter binary auto-discovers
    /// in the working directory (e.g., `"crap4rs.toml"` for the Rust
    /// adapter; `"crap4ts.toml"` for the TS adapter). Threaded through
    /// to `discover_config` and surfaced in `--view <preset>`
    /// error hints so users see the right file name to create.
    pub config_file_name: &'static str,
    /// Commented-out exclude patterns emitted by `init` into the
    /// generated config (e.g., `&["tests/**", "benches/**", "examples/**"]`
    /// for Rust; `&["node_modules/**", "dist/**", "coverage/**"]` for
    /// TS). Adapter-specific because the convention for "where tests
    /// and ignorable artifacts live" differs per ecosystem. May be
    /// empty — init then emits the `# exclude = [ … ]` block without
    /// per-language entries.
    pub default_excludes: &'static [&'static str],
    /// The default complexity metric the adapter binary uses when
    /// neither CLI nor config file specifies one. crap4rs sets
    /// `Cognitive`; crap4ts sets `Cyclomatic` (the only metric crap4ts
    /// currently supports per `CrapError::MetricNotSupported`).
    /// Threaded through `merge_effective_inputs` so the binary's
    /// default flips per adapter without re-litigating the shared CLI
    /// fallthrough. See ADR (d) `adr-adapter-meta-default-metric.md`
    /// for the design rationale (per-adapter defaults surface through
    /// `AdapterMeta`, not crap-core configuration).
    pub default_metric: ComplexityMetric,
}

impl AdapterMeta {
    /// Allocate an owned `Vec<String>` from `extensions` for inclusion
    /// in `AnalyzeOptions` (which owns its config rather than borrowing
    /// from the meta, decoupling analysis lifetime from CLI lifetime).
    pub fn extensions_owned(&self) -> Vec<String> {
        self.extensions.iter().map(|e| (*e).to_string()).collect()
    }

    /// Trip on construction with empty required strings. `extensions`
    /// is allowed to be empty — `core::ensure_source_files_found`
    /// surfaces a parser-neutral diagnostic when no files match. Other
    /// fields are mandatory for help/SARIF/`--version` rendering, and a
    /// silent empty string here would produce malformed output that's
    /// hard to trace back to the meta. Debug-only so release builds
    /// stay zero-cost; production binaries should never hit these
    /// (their meta is `env!()` / `const`).
    pub(crate) fn debug_assert_required_fields(&self) {
        debug_assert!(
            !self.tool_name.is_empty(),
            "AdapterMeta.tool_name must not be empty"
        );
        debug_assert!(
            !self.tool_version.is_empty(),
            "AdapterMeta.tool_version must not be empty"
        );
        debug_assert!(
            !self.long_version.is_empty(),
            "AdapterMeta.long_version must not be empty"
        );
        debug_assert!(
            !self.about.is_empty(),
            "AdapterMeta.about must not be empty"
        );
        debug_assert!(
            !self.long_about.is_empty(),
            "AdapterMeta.long_about must not be empty"
        );
        debug_assert!(
            !self.coverage_hint.is_empty(),
            "AdapterMeta.coverage_hint must not be empty"
        );
        debug_assert!(
            !self.tool_info_uri.is_empty(),
            "AdapterMeta.tool_info_uri must not be empty"
        );
        debug_assert!(
            !self.rule_help_uri.is_empty(),
            "AdapterMeta.rule_help_uri must not be empty"
        );
        debug_assert!(
            !self.config_file_name.is_empty(),
            "AdapterMeta.config_file_name must not be empty"
        );
    }
}

/// Parse process args into `Cli`, splicing the adapter's runtime
/// metadata into clap's help / `--version` output.
///
/// Split out from `run` purely to keep the parse step monomorphic and
/// off the binary's hot path on `--help` / `--version` (clap intercepts
/// those before `parse_args` returns). The adapter binary supplies its
/// coverage adapter to `run` as a factory closure that's invoked once
/// after CLI/config-file merging resolves the effective source root —
/// pre-construction lets the coverage parser strip the wrong prefix
/// from per-file records when `cli.input.src` is `None` because `src`
/// came from the adapter's config TOML rather than the CLI.
///
/// `AdapterMeta::{tool_version, long_version, about, long_about,
/// after_help}` flow into clap's help / `--version` output at runtime
/// so the binary's build-script metadata reaches the help text — the
/// derive macro's `version` reads `CARGO_PKG_VERSION` at lib-crate
/// compile time (crap-core's `0.1.0`); the adapter binary's own
/// `CARGO_PKG_VERSION` and `<ADAPTER>_LONG_VERSION` only resolve in
/// the binary's compile and reach us by parameter.
pub fn parse_args(meta: &AdapterMeta) -> Cli {
    meta.debug_assert_required_fields();
    let cmd = build_command(meta);
    let matches = cmd.get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

/// Read the adapter binary's name from `argv[0]`, falling back to
/// `meta.tool_name` when argv[0] is unavailable (extreme edge cases
/// like execve with empty argv). The clap-derive `Cli::command()`
/// defaults to `CARGO_PKG_NAME` of the lib crate (crap-core), which
/// would print `--version` lines with the wrong identifier and shape
/// generated completion scripts for the wrong binary; runtime
/// detection ensures the displayed name matches whichever adapter
/// binary actually ran.
fn current_bin_name(meta_fallback: &str) -> String {
    std::env::args()
        .next()
        .and_then(|first| {
            // `file_stem()` (not `file_name()`) so Windows builds drop
            // the `.exe` suffix — without it `--version` would print
            // `<binary>.exe <version>` and break scripts (and the
            // version-stamp integration tests) that match `^<binary> `.
            // No-op on Linux/macOS.
            std::path::PathBuf::from(first)
                .file_stem()
                .map(|os| os.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| meta_fallback.to_string())
}

/// Build the clap `Command` with the binary's runtime metadata
/// spliced in. Used by `parse_args`; `emit_completions` reads the
/// bin name through `current_bin_name` directly because
/// `clap_complete::generate` takes the bin name as a separate arg.
///
/// `name` / `bin_name` need clap's `string` feature
/// (`impl From<String> for clap::builder::Str`) because
/// `current_bin_name` constructs the bin name at runtime from
/// `argv[0]` and returns `String` — without the feature, the only way
/// to satisfy `From<&'static str>` from a runtime `String` is
/// `Box::leak` (the pre-#161 workaround). The remaining fields are
/// `&'static str` on `AdapterMeta`, so they pass through clap's
/// default `Into<Str>` impl with zero heap allocations.
fn build_command(meta: &AdapterMeta) -> clap::Command {
    let bin_name = current_bin_name(meta.tool_name);
    let mut cmd = Cli::command()
        .name(bin_name.clone())
        .bin_name(bin_name)
        .version(meta.tool_version)
        .long_version(meta.long_version)
        .about(meta.about)
        .long_about(meta.long_about);
    if !meta.after_help.is_empty() {
        cmd = cmd.after_help(meta.after_help);
    }
    cmd
}

/// Run the CRAP CLI pipeline end-to-end.
///
/// Takes a `coverage_factory` closure rather than a constructed
/// coverage adapter so the parser receives the effective source root
/// *after* CLI / config-file / preset merging — pre-construction
/// canonicalized against the bare CLI value (or the default `src`) and
/// the LCOV parser silently stripped the wrong prefix from `SF:`
/// records. The factory is invoked once inside `run` after
/// `merge_effective_inputs` resolves the final `src`, receives the
/// **canonicalized** effective source root (so adapter factories stay
/// dumb — orchestration owns the canonicalize concern), and is
/// short-circuited entirely on the `completions` subcommand (clap's
/// `--help` / `--version` exit even earlier, inside `parse_args`).
///
/// Generic over `P: ParseDiagnostic` so the same orchestrator drives
/// every adapter crate's binary (per ADR D9, mixed-dispatch). The
/// `'static` bound on `P` is the standard trait-object well-formedness
/// requirement when the closure returns `Box<dyn …>`; concrete adapter
/// diagnostic types (`LcovParseDiagnostic`, `IstanbulParseDiagnostic`)
/// satisfy it trivially.
///
/// `meta` carries the adapter binary's runtime identity (name,
/// version, help copy, extensions, config-file name, SARIF URIs).
/// The binary's own `tool_version` (e.g. crap4rs's `CARGO_PKG_VERSION`
/// resolves to `0.5.0`, not crap-core's `0.1.0`) feeds the JSON
/// envelope's `tool_version` field, the SARIF run metadata, the
/// markdown / HTML headers, and clap's long-version splice. See
/// `AdapterMeta` for the per-field rationale.
pub fn run<P, F>(
    cli: Cli,
    complexity: &dyn ComplexityPort,
    coverage_factory: F,
    meta: &AdapterMeta,
) -> ExitCode
where
    P: ParseDiagnostic + std::fmt::Display + 'static,
    F: FnOnce(&Path) -> Box<dyn CoveragePort<Diagnostic = P>>,
{
    match run_inner(cli, complexity, coverage_factory, meta) {
        Ok(true) => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            render_error(&e, meta);
            ExitCode::from(2)
        }
    }
}

/// Render an end-of-pipeline error to stderr, special-casing the
/// `MetricNotSupported` variant so the user sees adapter-specific
/// phrasing without the generic `error:` prefix (per breadboard W-5
/// + `metric_unsupported.feature` scenario 1 exact-string contract).
///
/// `anyhow::Error::downcast_ref` walks the source chain looking for a
/// concrete `CrapError`; we use it (not `is::<CrapError>()`) so an
/// error that was `.context(...)`-wrapped along the way is still
/// detected. Every other error type falls through to the default
/// `error: {e:#}` rendering — `{:#}` uses anyhow's alternate Display
/// which prints the full cause chain.
fn render_error(err: &anyhow::Error, meta: &AdapterMeta) {
    if let Some(crap_err) = err.downcast_ref::<crate::domain::types::CrapError>()
        && let crate::domain::types::CrapError::MetricNotSupported { metric } = crap_err
    {
        // Adapter-specific message: `tool_name` + `default_metric`
        // hint + `tool_info_uri`. The domain layer's variant message
        // stays adapter-agnostic; adapter-named phrasing lives at
        // this rendering boundary only. `metric` (input) +
        // `meta.default_metric` (hint) both use `ComplexityMetric`'s
        // `Display` impl which yields lowercase wire tokens —
        // matching CLI input (`cognitive`, not Debug `Cognitive`).
        eprintln!(
            "{}: complexity metric `{}` is not yet supported. Use `--metric {}` (the default for {}) or track support at {}.",
            meta.tool_name, metric, meta.default_metric, meta.tool_name, meta.tool_info_uri,
        );
        return;
    }
    eprintln!("error: {err:#}");
}

fn run_inner<P, F>(
    mut cli: Cli,
    complexity: &dyn ComplexityPort,
    coverage_factory: F,
    meta: &AdapterMeta,
) -> Result<bool>
where
    P: ParseDiagnostic + std::fmt::Display + 'static,
    F: FnOnce(&Path) -> Box<dyn CoveragePort<Diagnostic = P>>,
{
    match cli.command {
        Some(Command::Completions { shell }) => {
            emit_completions(shell, &current_bin_name(meta.tool_name));
            return Ok(true);
        }
        Some(Command::Init {
            force,
            non_interactive,
        }) => {
            init::handle_init(force, non_interactive, meta)?;
            return Ok(true);
        }
        None => {}
    }

    let prep = prepare_pipeline(&mut cli, complexity, coverage_factory, meta)?;

    // Build the spec, then shape the result through the View pipeline.
    // V1b: `--only-failing` flows through `Filters::only_failing` here.
    // W2 fills in `--top`, `--min/max-coverage`, `--sort-by`. The
    // underlying `result` is never mutated — the gate is unshapeable.
    let spec = view_args::build_view_spec(&cli);
    let view = view::apply(&prep.analysis.result, spec);

    // Shape the delta. Spec is built from --delta-top / --delta-sort /
    // --delta-only (VS4); defaults match the dominant scorecard use
    // case (regressions first, all kinds, no truncation). `Option::map`
    // is `FnOnce`, so the closure moves the spec rather than cloning —
    // `DeltaView` owns its `spec` field, no further uses upstream.
    let delta_spec = delta_args::build_delta_view_spec(&cli);
    let delta_view: Option<DeltaView<'_>> = prep
        .delta_state
        .as_ref()
        .map(move |s| delta::apply(&s.delta, delta_spec));

    if !cli.display.quiet {
        print_formatted_output(
            &cli,
            &view,
            delta_view.as_ref(),
            prep.delta_state.as_ref(),
            &prep.analysis,
            &prep.inputs,
            meta,
        )?;
    }

    // Exit code derives from `view.full.passed` — i.e., the underlying
    // analysis. The View shapes the display, never the gate.
    //
    // Delta is informational by default.
    // `--delta-gate` opts in: a passing analysis with delta regressions
    // that introduce new violations will exit 1 when `--delta-gate` is
    // set. `--no-fail` overrides BOTH gates — truth lives in JSON
    // (`result.passed` and `delta.summary.passed`) so consumers can
    // still detect "would have failed."
    Ok(compute_exit_code(
        &cli,
        prep.analysis.result.passed,
        prep.delta_state.as_ref(),
    ))
}

// ── Run-inner orchestration helpers ────────────────────────────────

/// Effective inputs after CLI / config-file / preset / default merging.
/// Everything `core::analyze` needs except the coverage path (which is
/// validated separately and may be borrowed from `cli`).
struct EffectiveInputs {
    src: PathBuf,
    metric: ComplexityMetric,
    threshold_config: ThresholdConfig,
    threshold: f64,
    exclude: Vec<String>,
}

/// In-flight pipeline state assembled by `prepare_pipeline`. Owns the
/// analysis output and the optional delta state so the dispatch layer
/// borrows through references. Generic over `P: ParseDiagnostic` so
/// `AnalysisOutput<P>` and `DeltaState<P>` carry the adapter's diagnostic
/// shape (LCOV, future Istanbul, …) end-to-end.
struct PipelinePrep<P: ParseDiagnostic> {
    inputs: EffectiveInputs,
    analysis: AnalysisOutput<P>,
    delta_state: Option<DeltaState<P>>,
}

/// Merge CLI flags, optional file config, and adapter defaults into a
/// concrete `EffectiveInputs`. `meta.default_metric` is the
/// load-bearing fallthrough — each adapter binary picks its own
/// sensible default (crap4rs: `Cognitive`; crap4ts: `Cyclomatic`) so
/// the shared CLI stays adapter-agnostic. See ADR (d).
fn merge_effective_inputs(
    cli: &Cli,
    file_config: &Option<FileConfig>,
    meta: &AdapterMeta,
) -> EffectiveInputs {
    let src = cli
        .input
        .src
        .clone()
        .or_else(|| file_config.as_ref().and_then(|c| c.src.clone()))
        .unwrap_or_else(|| PathBuf::from("src"));
    let metric: ComplexityMetric = cli
        .input
        .metric
        .map(Into::into)
        .or_else(|| file_config.as_ref().and_then(|c| c.metric))
        .unwrap_or(meta.default_metric);
    let (threshold_config, threshold) = merge_threshold(cli, file_config, metric);
    let exclude = merge_exclude(cli, file_config);
    EffectiveInputs {
        src,
        metric,
        threshold_config,
        threshold,
        exclude,
    }
}

fn validate_runtime_inputs<'a>(
    cli: &'a Cli,
    inputs: &EffectiveInputs,
    meta: &AdapterMeta,
) -> Result<&'a Path> {
    // `--coverage` is required on the analysis path; subcommands like
    // `completions` skip this branch. Clap can't express "required
    // unless subcommand X" in derive, so we enforce it here.
    let Some(coverage_path) = cli.input.coverage.as_deref() else {
        bail!(
            "--coverage <FILE> is required (run `{name} --help` for usage, or `{name} completions <SHELL>` for shell completion scripts)",
            name = meta.tool_name,
        );
    };

    validate_inputs(
        coverage_path,
        &inputs.src,
        inputs.threshold,
        meta.coverage_hint,
    )?;

    if let Some(diff_ref) = cli.filter.diff.as_deref() {
        validate_diff_ref(diff_ref)?;
        preflight_git_worktree(&inputs.src)?;
    }

    Ok(coverage_path)
}

fn build_analyze_options(
    cli: &Cli,
    inputs: &EffectiveInputs,
    coverage: &Path,
    meta: &AdapterMeta,
) -> AnalyzeOptions {
    AnalyzeOptions {
        src: inputs.src.clone(),
        coverage: coverage.to_path_buf(),
        threshold_config: inputs.threshold_config.clone(),
        metric: inputs.metric,
        exclude: inputs.exclude.clone(),
        respect_gitignore: !cli.filter.no_gitignore,
        diff_ref: cli.filter.diff.clone(),
        extensions: meta.extensions_owned(),
        compute_diagnostics: cli
            .output
            .format
            .iter()
            .any(|s| matches!(s.format, FormatArg::Advice | FormatArg::Sarif)),
        ..AnalyzeOptions::default()
    }
}

fn apply_diagnostics<P: ParseDiagnostic + std::fmt::Display>(
    cli: &Cli,
    diagnostics: &AnalysisDiagnostics<P>,
) {
    // Always warn about non-fatal issues (details require --verbose)
    warn_if_issues(diagnostics);
    if cli.display.verbose {
        print_diagnostics(diagnostics);
    }
}

/// Validates inputs, merges effective config, runs the analyzer, and
/// resolves the optional baseline delta. The bulk of `run_inner`'s
/// pre-render work lives here so `run_inner` itself stays a flat dispatch.
///
/// Constructs the coverage adapter via `coverage_factory` *after*
/// `merge_effective_inputs` resolves the final source root, so the
/// coverage parser strips the correct prefix from per-file records
/// even when `src` came from the adapter's config TOML rather than
/// the CLI.
fn prepare_pipeline<P, F>(
    cli: &mut Cli,
    complexity: &dyn ComplexityPort,
    coverage_factory: F,
    meta: &AdapterMeta,
) -> Result<PipelinePrep<P>>
where
    P: ParseDiagnostic + std::fmt::Display + 'static,
    F: FnOnce(&Path) -> Box<dyn CoveragePort<Diagnostic = P>>,
{
    validate_display_flags(cli)?;
    apply_color(cli.display.color);

    // Load config file (explicit path or auto-discovered). Path is
    // kept alongside the loaded config so downstream diagnostics
    // (e.g., unknown `--view` preset) can point the user at the
    // exact file to edit.
    let (file_config, config_path) = load_file_config(cli, meta.config_file_name)?.unzip();

    // Resolve `--view <NAME>` before validate_view_args runs
    // so preset fields participate in the same validation pass as CLI
    // flags. `apply_preset_to_cli` mutates `cli` in place: CLI explicit
    // values win on `Option<T>` fields, bools OR-merge.
    view_args::resolve_view_preset(
        cli,
        file_config.as_ref(),
        config_path.as_deref(),
        meta.config_file_name,
    )?;
    view_args::validate_view_args(cli)?;

    let inputs = merge_effective_inputs(cli, &file_config, meta);
    let coverage_path = validate_runtime_inputs(cli, &inputs, meta)?;

    // Canonicalize the effective `src` (post-config-merge) and hand
    // it to the adapter's factory closure. `validate_runtime_inputs`
    // already gated on existence; `canonicalize_src`'s fallback path
    // is purely defensive against TOCTOU between the two `metadata`
    // calls and emits a warning on the error arm so the regression is
    // observable instead of silent.
    let src_canonical = crate::core::canonicalize_src(&inputs.src);
    let coverage = coverage_factory(&src_canonical);

    // Adapter-aware pre-flight runs after construction so
    // `CoveragePort::validate` can apply its own structural check
    // (LCOV: SF/DA records; future Istanbul: non-empty statementMap)
    // before the full parse pass. See ADR D-coverage-validate.
    preflight_checks(coverage_path, &*coverage, meta)?;

    let options = build_analyze_options(cli, &inputs, coverage_path, meta);

    let analysis = crate::core::analyze(&options, complexity, &*coverage)?;
    apply_diagnostics(cli, &analysis.diagnostics);

    // Resolve --baseline: load a previously-emitted JSON
    // envelope and compute the AnalysisDelta. None when --baseline is
    // absent — the JSON envelope omits the `delta` block entirely so
    // existing consumers see byte-identical output.
    let delta_state = load_delta_state(cli, &analysis.result)?;

    Ok(PipelinePrep {
        inputs,
        analysis,
        delta_state,
    })
}

// ── Format dispatch ────────────────────────────────────────────────

fn format_as_json<P: ParseDiagnostic>(
    cli: &Cli,
    view: &view::AnalysisView<'_>,
    delta_view: Option<&DeltaView<'_>>,
    delta_state: Option<&DeltaState<P>>,
    analysis: &AnalysisOutput<P>,
    inputs: &EffectiveInputs,
    meta: &AdapterMeta,
) -> Result<String> {
    let delta_ctx = delta_state.zip(delta_view).map(|(s, dv)| DeltaContext {
        view: dv,
        baseline_tool_version: &s.snapshot.tool_version,
        baseline_timestamp: &s.snapshot.timestamp,
        baseline_diagnostics: s.snapshot.diagnostics.as_ref(),
    });
    let config = reporters::json::JsonConfig {
        tool_version: meta.tool_version.to_string(),
        metric: inputs.metric,
        threshold: inputs.threshold,
        timestamp: now_unix_epoch(),
        diagnostics: cli.display.verbose.then_some(&analysis.diagnostics),
        diff_ref: cli.filter.diff.as_deref(),
        minimal_view: cli.output.minimal_view,
        delta: delta_ctx,
    };
    reporters::json::format_json(view, &config).map_err(Into::into)
}

/// ScorecardRow projects the unshaped analysis + delta into a mokumo
/// `Row::CrapDelta` JSON object. View shaping does NOT
/// alter scorecard-row — the aggregator consumes truth, not a filtered
/// subset.
fn format_as_scorecard_row<P: ParseDiagnostic>(
    delta_state: Option<&DeltaState<P>>,
    result: &crate::domain::types::AnalysisResult,
    threshold: f64,
) -> String {
    let baseline_result = delta_state.map(|s| &s.snapshot.result);
    let delta_inputs = delta_state.map(|s| (&s.delta.summary, s.delta.changes.as_slice()));
    let row_data = crate::domain::summary::project_crap_delta_row(
        result,
        baseline_result,
        delta_inputs,
        threshold.round() as u32,
    );
    reporters::format_scorecard_row(&row_data)
}

// 8-arg dispatch is the cost of threading `<P>` + `meta` through the
// format match without restructuring the per-reporter call sites
// (which carry heterogeneous, irreducible signatures per `adapters.md`
// rule 1). Bundling them into a context struct would shadow the per-arm
// argument list that's the whole point of this match. Tracked under v1.0
// follow-up for the broader cli refactor.
#[allow(clippy::too_many_arguments)]
fn render_format<P: ParseDiagnostic>(
    cli: &Cli,
    spec: &FormatSpec,
    view: &view::AnalysisView<'_>,
    delta_view: Option<&DeltaView<'_>>,
    delta_state: Option<&DeltaState<P>>,
    analysis: &AnalysisOutput<P>,
    inputs: &EffectiveInputs,
    meta: &AdapterMeta,
) -> Result<String> {
    Ok(match spec.format {
        FormatArg::Table => reporters::format_table_with_explain(
            view,
            delta_view,
            inputs.threshold,
            cli.display.breakdown,
            cli.display.explain,
            meta.tool_name,
            meta.tool_version,
        ),
        FormatArg::Json | FormatArg::Advice => {
            format_as_json(cli, view, delta_view, delta_state, analysis, inputs, meta)?
        }
        FormatArg::Markdown => reporters::format_markdown(
            view,
            delta_view,
            inputs.threshold,
            cli.display.breakdown,
            cli.display.explain,
            cli.display.md_full_table,
            cli.display.md_top,
            meta.tool_name,
            meta.tool_version,
        ),
        FormatArg::Csv => reporters::format_csv(view, delta_view, inputs.metric),
        // SARIF is a gate translation, not a display: it iterates
        // `view.full.functions` internally regardless of how the View
        // was shaped. `--top`, `--sort-by`, `--only-failing`, and
        // `--baseline` do NOT alter SARIF output — PR annotations
        // must reflect truth.
        FormatArg::Sarif => reporters::format_sarif(
            view,
            meta.tool_name,
            meta.tool_version,
            meta.tool_info_uri,
            meta.rule_help_uri,
        ),
        FormatArg::ScorecardRow => {
            format_as_scorecard_row(delta_state, &analysis.result, inputs.threshold)
        }
        FormatArg::Html => {
            reporters::format_html(view, inputs.threshold, meta.tool_name, meta.tool_version)
        }
    })
}

fn print_formatted_output<P: ParseDiagnostic>(
    cli: &Cli,
    view: &view::AnalysisView<'_>,
    delta_view: Option<&DeltaView<'_>>,
    delta_state: Option<&DeltaState<P>>,
    analysis: &AnalysisOutput<P>,
    inputs: &EffectiveInputs,
    meta: &AdapterMeta,
) -> Result<()> {
    // `--summary` short-circuits `--format` dispatch entirely. `--quiet`
    // already gates this entire function at the caller (run_inner), so
    // the precedence is `--quiet > --summary > --format`. Mirrors
    // crap4ts's implicit precedence (its formatSummaryLine bypasses the
    // reporter switch when set).
    if cli.output.summary {
        let line = reporters::format_summary_line(view.full, inputs.threshold);
        println!("{line}");
        return Ok(());
    }

    for spec in &cli.output.format {
        let output = render_format(
            cli,
            spec,
            view,
            delta_view,
            delta_state,
            analysis,
            inputs,
            meta,
        )?;
        match &spec.output {
            Some(path) => std::fs::write(path, &output)
                .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?,
            None => print!("{output}"),
        }
    }

    // Advice's stderr summary fires once even if Advice appears multiple
    // times in `--format`. SARIF stays silent — its primary deliverable
    // is the `.sarif` file uploaded to Code Scanning; stderr would noise
    // up CI logs.
    if cli
        .output
        .format
        .iter()
        .any(|s| matches!(s.format, FormatArg::Advice))
    {
        let mut stderr = std::io::stderr();
        let _ = reporters::render_advice_summary(view, &mut stderr);
    }

    Ok(())
}

fn compute_exit_code<P: ParseDiagnostic>(
    cli: &Cli,
    passed: bool,
    delta_state: Option<&DeltaState<P>>,
) -> bool {
    let delta_passed = delta_state.map(|s| s.delta.summary.passed).unwrap_or(true);
    let combined_passed = passed && (!cli.output.delta_gate || delta_passed);
    combined_passed || cli.output.no_fail
}

// ── Delta orchestration ─────────────────────────────────────────────

/// In-flight delta state — owned baseline metadata + computed delta.
/// `cli/mod.rs` keeps this for the lifetime of `run_inner` so reporters
/// can borrow through it. Constructed once per invocation when
/// `--baseline` is set; absent otherwise. Generic over `P:
/// ParseDiagnostic` so the snapshot's `BaselineSnapshot<P>` matches the
/// adapter's diagnostic shape.
struct DeltaState<P: ParseDiagnostic> {
    snapshot: BaselineSnapshot<P>,
    delta: AnalysisDelta,
}

fn load_delta_state<P: ParseDiagnostic>(
    cli: &Cli,
    current: &crate::domain::types::AnalysisResult,
) -> Result<Option<DeltaState<P>>> {
    let Some(path) = cli.input.baseline.as_ref() else {
        return Ok(None);
    };
    let snapshot = baseline::load::<P>(path).map_err(|e| anyhow::anyhow!("{e}"))?;
    // delta::compute consumes both — we own snapshot.result, clone the
    // current analysis so the surrounding pipeline keeps its handle.
    let delta = delta::compute(snapshot.result.clone(), current.clone());
    Ok(Some(DeltaState { snapshot, delta }))
}

fn validate_display_flags(cli: &Cli) -> Result<()> {
    let any_table = cli
        .output
        .format
        .iter()
        .any(|s| matches!(s.format, FormatArg::Table));
    if cli.display.explain && any_table && !cli.display.breakdown {
        bail!("--explain requires --breakdown for table output");
    }
    validate_format_destinations(&cli.output.format)?;
    Ok(())
}

/// Multi-format invocations require every entry to specify a file —
/// stdout cannot multiplex.
fn validate_format_destinations(specs: &[FormatSpec]) -> Result<()> {
    if specs.len() > 1 {
        let stdout_specs: Vec<_> = specs
            .iter()
            .filter(|s| s.output.is_none())
            .map(|s| format_arg_kebab(s.format).to_string())
            .collect();
        if !stdout_specs.is_empty() {
            bail!(
                "multi-format `--format` requires every entry to specify a file (e.g. `json:envelope.json`); stdout-only entries: {}",
                stdout_specs.join(", ")
            );
        }
    }
    Ok(())
}

/// User-facing kebab-case name for a `FormatArg` (matches the clap CLI
/// surface `--format X`). Defaults to `Debug` lowercased if clap's
/// `ValueEnum` registry can't resolve a name.
fn format_arg_kebab(arg: FormatArg) -> String {
    use clap::ValueEnum;
    arg.to_possible_value()
        .map(|v| v.get_name().to_string())
        .unwrap_or_else(|| format!("{arg:?}").to_lowercase())
}

// ── Config loading & merging ───────────────────────────────────────

/// Load the on-disk config file (explicit `--config` path or
/// auto-discovered by adapter convention) and return it paired with
/// the path it came from. The path is threaded into downstream error
/// hints (e.g., the `--view` unknown-preset diagnostic) so the user
/// sees the exact file to edit — not just the conventional name.
fn load_file_config(
    cli: &Cli,
    config_file_name: &str,
) -> Result<Option<(FileConfig, std::path::PathBuf)>> {
    if let Some(path) = &cli.input.config {
        let cfg = config::load_config(path)?;
        Ok(Some((cfg, path.clone())))
    } else {
        match config::discover_config(config_file_name)? {
            Some(path) => {
                let cfg = config::load_config(&path)?;
                Ok(Some((cfg, path)))
            }
            None => Ok(None),
        }
    }
}

/// Merge CLI threshold with config file. Returns (ThresholdConfig, effective_display_threshold).
///
/// Resolution order (first match wins). Every tier-derived value is
/// keyed on the resolved `metric` via [`ThresholdPreset::threshold`],
/// so a cutoff calibrated for one metric is never applied to the
/// other metric's (different-magnitude) scores. `metric` is the
/// already-resolved effective metric (CLI > config > adapter default).
/// 1. `--threshold N`   — explicit CLI value (a literal cutoff; metric-independent)
/// 2. `--strict`        → `ThresholdPreset::Strict.threshold(metric)`
/// 3. `--lenient`       → `ThresholdPreset::Lenient.threshold(metric)`
/// 4. config `preset`   → `preset.threshold(metric)`
/// 5. config `threshold` — explicit literal cutoff (metric-independent)
/// 6. no-flag default   → `ThresholdPreset::Default.threshold(metric)`
///    (cyclomatic-metric runs → 16; cognitive-metric runs → 25)
fn merge_threshold(
    cli: &Cli,
    file_config: &Option<FileConfig>,
    metric: ComplexityMetric,
) -> (ThresholdConfig, f64) {
    let global = cli
        .output
        .threshold
        .or_else(|| {
            cli.output
                .strict
                .then(|| ThresholdPreset::Strict.threshold(metric))
        })
        .or_else(|| {
            cli.output
                .lenient
                .then(|| ThresholdPreset::Lenient.threshold(metric))
        })
        .or_else(|| {
            file_config
                .as_ref()
                .and_then(|c| c.preset)
                .map(|p| p.threshold(metric))
        })
        .or_else(|| file_config.as_ref().and_then(|c| c.threshold))
        .unwrap_or(ThresholdPreset::Default.threshold(metric));

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
    coverage_hint: &str,
) -> Result<()> {
    match std::fs::metadata(coverage) {
        Ok(m) if m.is_file() => {}
        Ok(_) => bail!(
            "coverage path is not a file: {}\n  \
             hint: pass --coverage pointing to a coverage file, not a directory",
            coverage.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "coverage file not found: {}\n  hint: {coverage_hint}",
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
             hint: pass --src <DIR> pointing to your source root",
            src.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "source directory not found: {}\n  \
             hint: pass --src <DIR> pointing to your source root",
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

/// Adapter-aware coverage pre-flight: read the coverage file once and
/// delegate the structural check to `CoveragePort::validate`. The
/// source-directory check is handled by `core::ensure_source_files_found`
/// during the analyze pipeline — see ADR D-preflight-walker-reconcile.
fn preflight_checks<P>(
    coverage: &std::path::Path,
    coverage_port: &dyn CoveragePort<Diagnostic = P>,
    meta: &AdapterMeta,
) -> Result<()>
where
    P: ParseDiagnostic,
{
    check_coverage_has_data(coverage, coverage_port, meta.coverage_hint)
}

fn check_coverage_has_data<P>(
    path: &std::path::Path,
    coverage_port: &dyn CoveragePort<Diagnostic = P>,
    coverage_hint: &str,
) -> Result<()>
where
    P: ParseDiagnostic,
{
    // The adapter's `validate` streams the file itself (LCOV) or
    // slurps (Istanbul, post-implementation) — whichever is cheaper
    // for that format. We do NOT pre-read here: `core::analyze` will
    // read the file again for the full parse pass, and slurping twice
    // for large workspaces (100 MB+ LCOV) is a memory regression.
    //
    // The validation reason (e.g. `"no SF/DA records"`) is surfaced
    // alongside the path so the user knows whether the file was
    // syntactically empty, malformed, or just missing data points.
    if let Err(reason) = coverage_port.validate(path) {
        bail!(
            "no coverage data found in {} ({reason})\n  hint: {}",
            path.display(),
            coverage_hint,
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

fn warn_if_issues<P: ParseDiagnostic>(diag: &AnalysisDiagnostics<P>) {
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

fn print_diagnostics<P: ParseDiagnostic + std::fmt::Display>(diag: &AnalysisDiagnostics<P>) {
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
///
/// `bin_name` is the adapter binary's name (`crap4rs`, future
/// `crap4ts`, …) inferred at runtime from `argv[0]` — generated
/// completion scripts should reference the binary the user invoked,
/// not crap-core's library name.
fn emit_completions(shell: ShellArg, bin_name: &str) {
    let mut cmd = Cli::command();
    let stdout = &mut std::io::stdout();
    match shell {
        ShellArg::Bash => clap_complete::generate(ClapShell::Bash, &mut cmd, bin_name, stdout),
        ShellArg::Zsh => clap_complete::generate(ClapShell::Zsh, &mut cmd, bin_name, stdout),
        ShellArg::Fish => clap_complete::generate(ClapShell::Fish, &mut cmd, bin_name, stdout),
        ShellArg::Powershell => {
            clap_complete::generate(ClapShell::PowerShell, &mut cmd, bin_name, stdout)
        }
        ShellArg::Elvish => clap_complete::generate(ClapShell::Elvish, &mut cmd, bin_name, stdout),
        ShellArg::Nushell => {
            clap_complete::generate(clap_complete_nushell::Nushell, &mut cmd, bin_name, stdout)
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
    // `DEFAULT_THRESHOLD` is no longer referenced by `merge_threshold`
    // (it reads `meta.default_threshold` post-#218); only the tests
    // assert against the value crap4rs's `AdapterMeta` carries.
    use crate::domain::threshold::DEFAULT_THRESHOLD;
    use std::path::Path;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        // argv[0] is a clap placeholder — kept adapter-agnostic
        // (`"test-adapter"`, not any real adapter binary's name) so
        // crap-core source has zero hardcoded references to its
        // consumers.
        let mut full = vec!["test-adapter"];
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
        assert_eq!(cli.output.format.len(), 1);
        assert!(matches!(cli.output.format[0].format, FormatArg::Table));
        assert!(cli.output.format[0].output.is_none());
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
        assert_eq!(cli.output.format.len(), 1);
        assert!(matches!(cli.output.format[0].format, FormatArg::Json));
        assert!(cli.output.format[0].output.is_none());
    }

    #[test]
    fn format_sarif() {
        let cli = parse(&["--coverage", "lcov.info", "--format", "sarif"]).unwrap();
        assert_eq!(cli.output.format.len(), 1);
        assert!(matches!(cli.output.format[0].format, FormatArg::Sarif));
    }

    #[test]
    fn format_with_file_destination() {
        let cli = parse(&["--coverage", "lcov.info", "--format", "json:env.json"]).unwrap();
        assert_eq!(cli.output.format.len(), 1);
        assert!(matches!(cli.output.format[0].format, FormatArg::Json));
        assert_eq!(cli.output.format[0].output, Some(PathBuf::from("env.json")));
    }

    #[test]
    fn format_multi_with_files() {
        let cli = parse(&[
            "--coverage",
            "lcov.info",
            "--format",
            "json:env.json,markdown:report.md",
        ])
        .unwrap();
        assert_eq!(cli.output.format.len(), 2);
        assert!(matches!(cli.output.format[0].format, FormatArg::Json));
        assert_eq!(cli.output.format[0].output, Some(PathBuf::from("env.json")));
        assert!(matches!(cli.output.format[1].format, FormatArg::Markdown));
        assert_eq!(
            cli.output.format[1].output,
            Some(PathBuf::from("report.md"))
        );
    }

    #[test]
    fn format_multi_without_files_rejected() {
        let cli = parse(&["--coverage", "lcov.info", "--format", "json,markdown"]).unwrap();
        let err = validate_display_flags(&cli).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("multi-format"));
        assert!(msg.contains("file"));
    }

    #[test]
    fn format_empty_path_rejected() {
        let err = parse(&["--coverage", "lcov.info", "--format", "json:"]).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("empty file path"));
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
    fn validate_missing_coverage_file_uses_adapter_hint() {
        let err = validate_inputs(
            Path::new("nonexistent.info"),
            Path::new("src"),
            DEFAULT_THRESHOLD,
            "run `cargo llvm-cov --lcov --output-path lcov.info` first",
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("coverage file not found"));
        // Adapter-supplied hint flows through; crap-core itself stays neutral.
        assert!(msg.contains("cargo llvm-cov"));
    }

    #[test]
    fn validate_missing_src_dir() {
        let err = validate_inputs(
            Path::new("Cargo.toml"),
            Path::new("nonexistent_dir"),
            DEFAULT_THRESHOLD,
            "test-hint",
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("source directory not found"));
    }

    #[test]
    fn validate_negative_threshold() {
        let err = validate_inputs(Path::new("Cargo.toml"), Path::new("src"), -5.0, "test-hint")
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_zero_threshold() {
        let err = validate_inputs(Path::new("Cargo.toml"), Path::new("src"), 0.0, "test-hint")
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_infinity_threshold() {
        let err = validate_inputs(
            Path::new("Cargo.toml"),
            Path::new("src"),
            f64::INFINITY,
            "test-hint",
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_src_is_file_not_dir() {
        let err = validate_inputs(
            Path::new("Cargo.toml"),
            Path::new("Cargo.toml"),
            DEFAULT_THRESHOLD,
            "test-hint",
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("source path is not a directory"));
    }

    #[test]
    fn validate_coverage_is_dir_not_file() {
        let err = validate_inputs(
            Path::new("src"),
            Path::new("src"),
            DEFAULT_THRESHOLD,
            "test-hint",
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("coverage path is not a file"));
    }

    #[test]
    fn format_short_flag() {
        let cli = parse(&["--coverage", "lcov.info", "-f", "json"]).unwrap();
        assert!(matches!(cli.output.format[0].format, FormatArg::Json));
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
        let (config, display) = merge_threshold(&cli, &file_config, ComplexityMetric::Cognitive);
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
        let (config, display) = merge_threshold(&cli, &file_config, ComplexityMetric::Cognitive);
        assert_eq!(config.global, 12.0);
        assert_eq!(display, 12.0);
    }

    #[test]
    fn merge_threshold_preserves_overrides() {
        use crate::domain::threshold::ThresholdOverride;
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let file_config = Some(FileConfig {
            threshold: Some(10.0),
            overrides: vec![ThresholdOverride {
                pattern: "domain/**".to_string(),
                threshold: 5.0,
            }],
            ..FileConfig::default()
        });
        let (config, _) = merge_threshold(&cli, &file_config, ComplexityMetric::Cognitive);
        assert_eq!(config.overrides.len(), 1);
        assert_eq!(config.overrides[0].pattern, "domain/**");
    }

    #[test]
    fn merge_threshold_no_config() {
        let cli = parse(&["--coverage", "lcov.info", "--threshold", "20.0"]).unwrap();
        let (config, display) = merge_threshold(&cli, &None, ComplexityMetric::Cognitive);
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
        let (config, display) = merge_threshold(&cli, &file_config, ComplexityMetric::Cognitive);
        assert_eq!(
            config.global, 8.0,
            "explicit CLI default must override config"
        );
        assert_eq!(display, 8.0);
    }

    #[test]
    fn merge_threshold_no_flag_default_is_metric_keyed() {
        // Replaces the pre-fix `merge_threshold_no_cli_no_config_uses_hardcoded_default`.
        // The no-flag/no-config fallthrough is the `Default` tier
        // resolved against the effective metric — NOT a single shared
        // scalar. Cognitive runs get 25; cyclomatic runs get 16. A
        // cognitive-tuned 25 applied to cyclomatic scores would
        // under-gate (the bug this keys against).
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let (cog, cog_disp) = merge_threshold(&cli, &None, ComplexityMetric::Cognitive);
        assert_eq!(cog.global, 25.0);
        assert_eq!(cog_disp, 25.0);
        let (cyc, cyc_disp) = merge_threshold(&cli, &None, ComplexityMetric::Cyclomatic);
        assert_eq!(cyc.global, 16.0);
        assert_eq!(cyc_disp, 16.0);
    }

    #[test]
    fn merge_threshold_strict_lenient_are_metric_keyed() {
        // `--strict` / `--lenient` were previously metric-blind (always
        // the cognitive 15 / 40). They now resolve per metric too, so
        // no preset path silently applies a cognitive cutoff to
        // cyclomatic scores.
        let strict = parse(&["--coverage", "lcov.info", "--strict"]).unwrap();
        assert_eq!(
            merge_threshold(&strict, &None, ComplexityMetric::Cognitive).1,
            15.0
        );
        assert_eq!(
            merge_threshold(&strict, &None, ComplexityMetric::Cyclomatic).1,
            8.0
        );
        let lenient = parse(&["--coverage", "lcov.info", "--lenient"]).unwrap();
        assert_eq!(
            merge_threshold(&lenient, &None, ComplexityMetric::Cognitive).1,
            40.0
        );
        assert_eq!(
            merge_threshold(&lenient, &None, ComplexityMetric::Cyclomatic).1,
            30.0
        );
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

    // Synthetic adapter values for tests — match the placeholder used
    // throughout the in-crate test suite. Real adapters supply real
    // values via `AdapterMeta`.
    const TEST_COVERAGE_HINT: &str =
        "ensure tests ran with coverage enabled (test-tool's `--coverage` flag)";

    /// Stub `CoveragePort` whose `validate` returns whatever the caller
    /// configured. `parse` panics — these tests exercise the CLI-layer
    /// preflight wrapper, not the adapter's parsing path.
    struct StubCoveragePort {
        validate_result: Result<(), String>,
    }

    impl CoveragePort for StubCoveragePort {
        type Diagnostic = crate::test_strategies::DummyParseDiagnostic;

        fn parse(
            &self,
            _data: &str,
        ) -> Result<crate::ports::ParseOutput<Self::Diagnostic>, crate::domain::types::CrapError>
        {
            unreachable!("preflight tests never invoke parse")
        }

        fn validate(&self, _path: &std::path::Path) -> Result<(), String> {
            self.validate_result.clone()
        }
    }

    fn stub_ok() -> StubCoveragePort {
        StubCoveragePort {
            validate_result: Ok(()),
        }
    }

    fn stub_err(reason: &str) -> StubCoveragePort {
        StubCoveragePort {
            validate_result: Err(reason.to_string()),
        }
    }

    #[test]
    fn preflight_surfaces_hint_when_adapter_reports_no_data() {
        let dir = tempfile::tempdir().unwrap();
        let cov = dir.path().join("empty.info");
        std::fs::write(&cov, "").unwrap();

        let err =
            check_coverage_has_data(&cov, &stub_err("no records"), TEST_COVERAGE_HINT).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("no coverage data found"));
        // The adapter's structural reason is surfaced alongside the
        // path so the user knows whether the file was empty,
        // malformed, or just missing data points.
        assert!(msg.contains("no records"), "expected reason in msg: {msg}");
        assert!(msg.contains(TEST_COVERAGE_HINT));
    }

    #[test]
    fn preflight_passes_when_adapter_accepts_data() {
        let dir = tempfile::tempdir().unwrap();
        let cov = dir.path().join("ok.info");
        std::fs::write(&cov, "any contents — adapter decides").unwrap();

        assert!(check_coverage_has_data(&cov, &stub_ok(), TEST_COVERAGE_HINT).is_ok());
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
        use crate::domain::threshold::STRICT_THRESHOLD;
        let cli = parse(&["--coverage", "lcov.info", "--strict"]).unwrap();
        let (config, display) = merge_threshold(&cli, &None, ComplexityMetric::Cognitive);
        assert_eq!(config.global, STRICT_THRESHOLD);
        assert_eq!(display, STRICT_THRESHOLD);
    }

    #[test]
    fn merge_threshold_lenient_flag() {
        use crate::domain::threshold::LENIENT_THRESHOLD;
        let cli = parse(&["--coverage", "lcov.info", "--lenient"]).unwrap();
        let (config, display) = merge_threshold(&cli, &None, ComplexityMetric::Cognitive);
        assert_eq!(config.global, LENIENT_THRESHOLD);
        assert_eq!(display, LENIENT_THRESHOLD);
    }

    #[test]
    fn merge_threshold_toml_preset_used_when_no_cli_flag() {
        use crate::domain::threshold::{STRICT_THRESHOLD, ThresholdPreset};
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let file_config = Some(FileConfig {
            preset: Some(ThresholdPreset::Strict),
            ..FileConfig::default()
        });
        let (config, _) = merge_threshold(&cli, &file_config, ComplexityMetric::Cognitive);
        assert_eq!(config.global, STRICT_THRESHOLD);
    }

    #[test]
    fn merge_threshold_cli_threshold_overrides_toml_preset() {
        use crate::domain::threshold::ThresholdPreset;
        let cli = parse(&["--coverage", "lcov.info", "--threshold", "50.0"]).unwrap();
        let file_config = Some(FileConfig {
            preset: Some(ThresholdPreset::Strict),
            ..FileConfig::default()
        });
        let (config, _) = merge_threshold(&cli, &file_config, ComplexityMetric::Cognitive);
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

    // ── merge_effective_inputs tests ───────────────────────────────────

    #[test]
    fn merge_effective_inputs_default_src() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let inputs = merge_effective_inputs(&cli, &None, &fake_meta());
        assert_eq!(inputs.src, PathBuf::from("src"));
    }

    #[test]
    fn merge_effective_inputs_cli_src_wins_over_config() {
        let cli = parse(&["--coverage", "lcov.info", "--src", "crates/"]).unwrap();
        let file_config = Some(FileConfig {
            src: Some(PathBuf::from("from-config/")),
            ..FileConfig::default()
        });
        let inputs = merge_effective_inputs(&cli, &file_config, &fake_meta());
        assert_eq!(inputs.src, PathBuf::from("crates/"));
    }

    #[test]
    fn merge_effective_inputs_config_src_when_cli_absent() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let file_config = Some(FileConfig {
            src: Some(PathBuf::from("from-config/")),
            ..FileConfig::default()
        });
        let inputs = merge_effective_inputs(&cli, &file_config, &fake_meta());
        assert_eq!(inputs.src, PathBuf::from("from-config/"));
    }

    #[test]
    fn merge_effective_inputs_uses_adapter_default_metric_cognitive() {
        // Replaces the pre-W2.5 `merge_effective_inputs_default_metric_is_cognitive`
        // test. Now that the fallthrough comes from `meta.default_metric`
        // (not `ComplexityMetric::default()`), the assertion is "the
        // adapter's default flows through when neither CLI nor config
        // override it." crap4rs sets this to `Cognitive`.
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let meta = AdapterMeta {
            default_metric: ComplexityMetric::Cognitive,
            ..fake_meta()
        };
        let inputs = merge_effective_inputs(&cli, &None, &meta);
        assert!(matches!(inputs.metric, ComplexityMetric::Cognitive));
    }

    #[test]
    fn merge_effective_inputs_uses_adapter_default_metric_cyclomatic() {
        // Replaces the pre-W2.5 `merge_effective_inputs_default_metric_is_cognitive`
        // test. Mirror of the Cognitive case for the crap4ts adapter
        // (locked decision #2: crap4ts default = cyclomatic).
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let meta = AdapterMeta {
            default_metric: ComplexityMetric::Cyclomatic,
            ..fake_meta()
        };
        let inputs = merge_effective_inputs(&cli, &None, &meta);
        assert!(matches!(inputs.metric, ComplexityMetric::Cyclomatic));
    }

    #[test]
    fn merge_effective_inputs_cli_metric_overrides_config() {
        let cli = parse(&["--coverage", "lcov.info", "--metric", "cyclomatic"]).unwrap();
        let file_config = Some(FileConfig {
            metric: Some(ComplexityMetric::Cognitive),
            ..FileConfig::default()
        });
        let inputs = merge_effective_inputs(&cli, &file_config, &fake_meta());
        assert!(matches!(inputs.metric, ComplexityMetric::Cyclomatic));
    }

    #[test]
    fn merge_effective_inputs_default_threshold_follows_adapter_metric_cognitive() {
        // End-to-end wiring: an adapter whose default metric is
        // cognitive, with no `--threshold`/`--metric`/config, resolves
        // the no-flag gate to the cognitive `Default` cutoff (25).
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let meta = AdapterMeta {
            default_metric: ComplexityMetric::Cognitive,
            ..fake_meta()
        };
        let inputs = merge_effective_inputs(&cli, &None, &meta);
        assert!(matches!(inputs.metric, ComplexityMetric::Cognitive));
        assert_eq!(inputs.threshold, 25.0);
    }

    #[test]
    fn merge_effective_inputs_default_threshold_follows_adapter_metric_cyclomatic() {
        // Mirror for a cyclomatic-default adapter (crap4ts): the no-flag
        // gate must be the cyclomatic `Default` cutoff (16), not the
        // cognitive 25 — a single shared default applied the wrong
        // metric's cutoff before this fix.
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let meta = AdapterMeta {
            default_metric: ComplexityMetric::Cyclomatic,
            ..fake_meta()
        };
        let inputs = merge_effective_inputs(&cli, &None, &meta);
        assert!(matches!(inputs.metric, ComplexityMetric::Cyclomatic));
        assert_eq!(inputs.threshold, 16.0);
    }

    #[test]
    fn merge_effective_inputs_exclude_combines_cli_and_config() {
        let cli = parse(&["--coverage", "lcov.info", "--exclude", "tests/**"]).unwrap();
        let file_config = Some(FileConfig {
            exclude: Some(vec!["benches/**".to_string()]),
            ..FileConfig::default()
        });
        let inputs = merge_effective_inputs(&cli, &file_config, &fake_meta());
        assert_eq!(inputs.exclude, vec!["tests/**", "benches/**"]);
    }

    #[test]
    fn merge_effective_inputs_config_metric_wins_over_adapter_default() {
        // Config-file metric should still beat the adapter default —
        // adapter default is the FINAL fallthrough, below CLI and
        // config-file precedence.
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        let file_config = Some(FileConfig {
            metric: Some(ComplexityMetric::Cyclomatic),
            ..FileConfig::default()
        });
        let meta = AdapterMeta {
            default_metric: ComplexityMetric::Cognitive,
            ..fake_meta()
        };
        let inputs = merge_effective_inputs(&cli, &file_config, &meta);
        assert!(matches!(inputs.metric, ComplexityMetric::Cyclomatic));
    }

    // ── compute_exit_code tests ────────────────────────────────────────
    //
    // delta_state=None covers the analysis-only paths; the delta-gate +
    // delta_state=Some interactions are exercised end-to-end in
    // delta_gate_integration.rs (where AnalysisDelta is built through
    // the real `delta::compute` path rather than mocked).

    #[test]
    fn compute_exit_code_passing_no_delta() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(compute_exit_code::<
            crate::test_strategies::DummyParseDiagnostic,
        >(&cli, true, None));
    }

    #[test]
    fn compute_exit_code_failing_no_delta() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(!compute_exit_code::<
            crate::test_strategies::DummyParseDiagnostic,
        >(&cli, false, None));
    }

    #[test]
    fn compute_exit_code_no_fail_overrides_failure() {
        let cli = parse(&["--coverage", "lcov.info", "--no-fail"]).unwrap();
        assert!(compute_exit_code::<
            crate::test_strategies::DummyParseDiagnostic,
        >(&cli, false, None));
    }

    #[test]
    fn compute_exit_code_delta_gate_without_runtime_baseline_treats_delta_as_passed() {
        // delta_state=None → delta_passed defaults to true even with
        // --delta-gate; this matches the runtime behavior when the
        // baseline file is missing or unreadable. Clap requires
        // --baseline to accompany --delta-gate at parse time, so we
        // pass a sentinel path to satisfy the parser without exercising
        // the file load (compute_exit_code only inspects the resolved
        // delta state, not cli.input.baseline).
        let cli = parse(&[
            "--coverage",
            "lcov.info",
            "--delta-gate",
            "--baseline",
            "/dev/null",
        ])
        .unwrap();
        assert!(compute_exit_code::<
            crate::test_strategies::DummyParseDiagnostic,
        >(&cli, true, None));
    }

    #[test]
    fn compute_exit_code_no_fail_with_delta_gate() {
        // --no-fail is the master override even when --delta-gate
        // is set.
        let cli = parse(&[
            "--coverage",
            "lcov.info",
            "--delta-gate",
            "--baseline",
            "/dev/null",
            "--no-fail",
        ])
        .unwrap();
        assert!(compute_exit_code::<
            crate::test_strategies::DummyParseDiagnostic,
        >(&cli, false, None));
    }

    // ── AdapterMeta unit tests (#161) ──────────────────────────────

    fn fake_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "fake-adapter",
            tool_version: "9.9.9",
            long_version: "9.9.9 (test 2099-01-01)",
            about: "Fake adapter for tests",
            long_about: "Fake adapter for tests — verifies AdapterMeta plumbing without binding crap-core to any real adapter.",
            after_help: "",
            coverage_hint: "no coverage tool — fake adapter",
            extensions: &["fake"],
            tool_info_uri: "https://example.invalid/fake-adapter",
            rule_help_uri: "https://example.invalid/fake-adapter#rules",
            config_file_name: "fake-adapter.toml",
            default_excludes: &["fixtures/**"],
            // `Cognitive` preserves the pre-W2.5 fallthrough semantics
            // for tests that don't care which default they get (the two
            // tests that DO care construct their own AdapterMeta with
            // an explicit `default_metric`).
            default_metric: ComplexityMetric::Cognitive,
        }
    }

    #[test]
    fn adapter_meta_extensions_owned_roundtrips_to_owned_strings() {
        let meta = AdapterMeta {
            extensions: &["ts", "tsx", "js"],
            ..fake_meta()
        };
        let owned = meta.extensions_owned();
        assert_eq!(
            owned,
            vec!["ts".to_string(), "tsx".to_string(), "js".to_string()]
        );
        // Round-trip via Vec<&str> back to a slice-equivalent shape.
        let back: Vec<&str> = owned.iter().map(String::as_str).collect();
        assert_eq!(back, &["ts", "tsx", "js"]);
    }

    #[test]
    fn adapter_meta_extensions_owned_handles_empty_slice() {
        // `extensions` is allowed to be empty; the diagnostic surfaces
        // downstream in `core::ensure_source_files_found`.
        let meta = AdapterMeta {
            extensions: &[],
            ..fake_meta()
        };
        assert!(meta.extensions_owned().is_empty());
    }

    #[test]
    #[should_panic(expected = "tool_name must not be empty")]
    fn adapter_meta_debug_assert_trips_on_empty_tool_name() {
        let meta = AdapterMeta {
            tool_name: "",
            ..fake_meta()
        };
        meta.debug_assert_required_fields();
    }

    #[test]
    #[should_panic(expected = "config_file_name must not be empty")]
    fn adapter_meta_debug_assert_trips_on_empty_config_file_name() {
        let meta = AdapterMeta {
            config_file_name: "",
            ..fake_meta()
        };
        meta.debug_assert_required_fields();
    }

    #[test]
    fn adapter_meta_debug_assert_passes_on_all_fields_set() {
        // Smoke test: a meta with every required field populated should
        // pass the debug_assert sweep without panicking.
        fake_meta().debug_assert_required_fields();
    }
}
