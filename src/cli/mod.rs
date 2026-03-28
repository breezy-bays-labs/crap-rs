//! CLI entry point — thin shell over the library crate.
//!
//! Parses args with clap, validates inputs, delegates to `core::analyze()`.
//! No business logic lives here.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use anyhow::{Result, bail};
use clap::{Args, Parser, ValueEnum, ValueHint};

use crap4rs::adapters::reporters;
use crap4rs::core::AnalyzeOptions;
use crap4rs::domain::threshold::DEFAULT_THRESHOLD;
use crap4rs::domain::types::ComplexityMetric;

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

#[derive(Debug, Args)]
#[command(next_help_heading = "Input")]
pub struct InputArgs {
    /// Path to LCOV coverage file (from `cargo llvm-cov --lcov`)
    #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
    pub coverage: PathBuf,

    /// Root directory of Rust source files to analyze
    #[arg(long, value_name = "DIR", default_value = "src", value_hint = ValueHint::DirPath)]
    pub src: PathBuf,

    /// Complexity metric to use
    #[arg(long, value_enum, default_value_t = MetricArg::Cognitive)]
    pub metric: MetricArg,
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Output")]
pub struct OutputArgs {
    /// Output format
    #[arg(short, long, value_enum, default_value_t = FormatArg::Table)]
    pub format: FormatArg,

    /// CRAP score threshold — functions above this fail the check
    // allow_hyphen_values: lets clap parse `--threshold -5` as a value
    // (not a flag), so our validate_inputs can give an actionable error.
    #[arg(long, default_value_t = DEFAULT_THRESHOLD, allow_hyphen_values = true)]
    pub threshold: f64,

    /// Only show functions that exceed the threshold
    #[arg(long)]
    pub only_failing: bool,
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
}

// ── Top-level CLI ───────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(
    version,
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
  crap4rs --coverage lcov.info --exclude \"tests/**\" --exclude \"benches/**\""
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
    let cli = Cli::parse();

    apply_color(cli.display.color);
    validate_inputs(&cli)?;
    preflight_checks(&cli)?;

    let options = AnalyzeOptions {
        src: cli.input.src.clone(),
        coverage: cli.input.coverage.clone(),
        threshold: cli.output.threshold,
        metric: cli.input.metric.into(),
        exclude: cli.filter.exclude.clone(),
        respect_gitignore: !cli.filter.no_gitignore,
    };

    let mut result = crap4rs::core::analyze(&options)?;
    let passed = result.passed;

    // Filter to only failing functions if requested (summary stays unfiltered)
    if cli.output.only_failing {
        result.functions.retain(|v| v.exceeds);
    }

    if !cli.display.quiet {
        let output = match cli.output.format {
            FormatArg::Table => reporters::format_table(&result, cli.output.threshold),
            FormatArg::Json => {
                let config = reporters::json::JsonConfig {
                    tool_version: env!("CARGO_PKG_VERSION").to_string(),
                    metric: cli.input.metric.into(),
                    threshold: cli.output.threshold,
                    timestamp: now_unix_epoch(),
                };
                reporters::format_json(&result, &config)?
            }
        };
        print!("{output}");
    }

    Ok(passed)
}

// ── Validation ──────────────────────────────────────────────────────

fn validate_inputs(cli: &Cli) -> Result<()> {
    match std::fs::metadata(&cli.input.coverage) {
        Ok(m) if m.is_file() => {}
        Ok(_) => bail!(
            "coverage path is not a file: {}\n  \
             hint: pass --coverage pointing to an LCOV file, not a directory",
            cli.input.coverage.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "coverage file not found: {}\n  \
             hint: run `cargo llvm-cov --lcov --output-path lcov.info` first",
            cli.input.coverage.display()
        ),
        Err(e) => bail!(
            "cannot access coverage file: {}: {e}\n  \
             hint: check file permissions",
            cli.input.coverage.display()
        ),
    }
    match std::fs::metadata(&cli.input.src) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => bail!(
            "source path is not a directory: {}\n  \
             hint: pass --src <DIR> pointing to your Rust source root",
            cli.input.src.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!(
            "source directory not found: {}\n  \
             hint: pass --src <DIR> pointing to your Rust source root",
            cli.input.src.display()
        ),
        Err(e) => bail!(
            "cannot access source directory: {}: {e}\n  \
             hint: check directory permissions",
            cli.input.src.display()
        ),
    }
    if !cli.output.threshold.is_finite() || cli.output.threshold <= 0.0 {
        bail!(
            "threshold must be a finite positive number, got: {}",
            cli.output.threshold
        );
    }
    Ok(())
}

// ── Pre-flight checks ──────────────────────────────────────────────

fn preflight_checks(cli: &Cli) -> Result<()> {
    check_coverage_has_data(&cli.input.coverage)?;
    check_src_has_rust_files(&cli.input.src)?;
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

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut full = vec!["crap4rs"];
        full.extend_from_slice(args);
        Cli::try_parse_from(full)
    }

    #[test]
    fn required_coverage_arg() {
        let err = parse(&[]).unwrap_err();
        assert!(err.to_string().contains("--coverage"));
    }

    #[test]
    fn minimal_valid_args() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert_eq!(cli.input.coverage, PathBuf::from("lcov.info"));
        assert_eq!(cli.input.src, PathBuf::from("src"));
    }

    #[test]
    fn default_metric_is_cognitive() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(matches!(cli.input.metric, MetricArg::Cognitive));
    }

    #[test]
    fn default_format_is_table() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(matches!(cli.output.format, FormatArg::Table));
    }

    #[test]
    fn default_threshold_matches_domain() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert_eq!(cli.output.threshold, DEFAULT_THRESHOLD);
    }

    #[test]
    fn default_color_is_auto() {
        let cli = parse(&["--coverage", "lcov.info"]).unwrap();
        assert!(matches!(cli.display.color, ColorArg::Auto));
    }

    #[test]
    fn metric_cyclomatic() {
        let cli = parse(&["--coverage", "lcov.info", "--metric", "cyclomatic"]).unwrap();
        assert!(matches!(cli.input.metric, MetricArg::Cyclomatic));
    }

    #[test]
    fn format_json() {
        let cli = parse(&["--coverage", "lcov.info", "--format", "json"]).unwrap();
        assert!(matches!(cli.output.format, FormatArg::Json));
    }

    #[test]
    fn custom_threshold() {
        let cli = parse(&["--coverage", "lcov.info", "--threshold", "15.5"]).unwrap();
        assert_eq!(cli.output.threshold, 15.5);
    }

    #[test]
    fn custom_src() {
        let cli = parse(&["--coverage", "lcov.info", "--src", "crates/"]).unwrap();
        assert_eq!(cli.input.src, PathBuf::from("crates/"));
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
        assert!(cli.output.only_failing);
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
        let cli = parse(&["--coverage", "nonexistent.info"]).unwrap();
        let err = validate_inputs(&cli).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("coverage file not found"));
        assert!(msg.contains("cargo llvm-cov"));
    }

    #[test]
    fn validate_missing_src_dir() {
        // Use a file that exists for coverage but a nonexistent src dir
        let cli = parse(&["--coverage", "Cargo.toml", "--src", "nonexistent_dir"]).unwrap();
        let err = validate_inputs(&cli).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("source directory not found"));
    }

    #[test]
    fn validate_negative_threshold() {
        let cli = parse(&["--coverage", "Cargo.toml", "--threshold", "-5"]).unwrap();
        let err = validate_inputs(&cli).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_zero_threshold() {
        let cli = parse(&["--coverage", "Cargo.toml", "--threshold", "0"]).unwrap();
        let err = validate_inputs(&cli).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_infinity_threshold() {
        let cli = parse(&["--coverage", "Cargo.toml", "--threshold", "inf"]).unwrap();
        let err = validate_inputs(&cli).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("threshold must be a finite positive number"));
    }

    #[test]
    fn validate_src_is_file_not_dir() {
        let cli = parse(&["--coverage", "Cargo.toml", "--src", "Cargo.toml"]).unwrap();
        let err = validate_inputs(&cli).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("source path is not a directory"));
    }

    #[test]
    fn validate_coverage_is_dir_not_file() {
        let cli = parse(&["--coverage", "src", "--src", "src"]).unwrap();
        let err = validate_inputs(&cli).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("coverage path is not a file"));
    }

    #[test]
    fn format_short_flag() {
        let cli = parse(&["--coverage", "lcov.info", "-f", "json"]).unwrap();
        assert!(matches!(cli.output.format, FormatArg::Json));
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
}
