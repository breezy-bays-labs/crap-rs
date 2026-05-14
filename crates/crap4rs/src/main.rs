//! Thin Rust adapter binary — wires `crap_core::cli::run<P>` with the
//! LCOV/`syn` ports and the version metadata baked in by `build.rs`.
//!
//! The coverage adapter is supplied as a factory closure so
//! `crap_core::cli::run` can construct the LCOV parser *after*
//! CLI/config-file merging resolves the effective source root —
//! pre-construction can silently strip the wrong path prefix from
//! `SF:` records when `--src` came from the config file rather than
//! the CLI.
//!
//! Adapter-specific copy (help text, examples, coverage hint, repo
//! URLs, source extensions) is bundled in `AdapterMeta` and passed
//! once to crap-core's parse + run entry points. crap-core itself is
//! adapter-agnostic.

use std::process::ExitCode;

use crap_core::cli::AdapterMeta;
use crap_core::domain::types::ComplexityMetric;
use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::adapters::coverage::LcovParser;

const ABOUT: &str = "CRAP score analyzer for Rust";
const LONG_ABOUT: &str = "CRAP (Change Risk Anti-Patterns) score analyzer for Rust codebases.\n\n\
                         Combines complexity analysis (via syn) with line coverage data \
                         (LCOV from cargo-llvm-cov) to identify functions that are both \
                         complex and under-tested.\n\n\
                         Default metric is cognitive complexity (not cyclomatic), which \
                         better captures Rust idioms like match arms and nested control flow.";

const AFTER_HELP: &str = "\
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
  crap4rs --coverage lcov.info --view ci

  # GitHub Code Scanning: emit SARIF and let upload-sarif annotate the PR
  # diff inline. Use --no-fail so the gate exit code doesn't skip the
  # upload step on regressions.
  crap4rs --coverage lcov.info --format sarif --no-fail > crap.sarif

COMPARING TWO ANALYSES:
  # Capture a baseline (e.g., from main):
  crap4rs --coverage lcov.info --format json > baseline.json

  # Then compare the working tree to it (informational by default):
  crap4rs --coverage lcov.info --baseline baseline.json

  # CI usage: fail the build when new threshold violations land
  crap4rs --coverage lcov.info --baseline baseline.json --delta-gate

  # PR-comment scorecard (markdown — drop into the comment body verbatim)
  crap4rs --coverage lcov.info --baseline baseline.json --format markdown";

const COVERAGE_HINT: &str = "ensure tests ran with coverage enabled (`cargo llvm-cov --lcov`)";

const EXTENSIONS: &[&str] = &["rs"];

/// Common Rust-project directories `init` writes as commented-out
/// excludes — tests, benches, examples typically run their own coverage
/// passes and shouldn't count against the source CRAP budget.
const DEFAULT_EXCLUDES: &[&str] = &["tests/**", "benches/**", "examples/**"];

fn main() -> ExitCode {
    let meta = AdapterMeta {
        tool_name: env!("CARGO_PKG_NAME"),
        tool_version: env!("CARGO_PKG_VERSION"),
        long_version: env!("CRAP4RS_LONG_VERSION"),
        about: ABOUT,
        long_about: LONG_ABOUT,
        after_help: AFTER_HELP,
        coverage_hint: COVERAGE_HINT,
        extensions: EXTENSIONS,
        tool_info_uri: "https://github.com/breezy-bays-labs/crap-rs",
        rule_help_uri: "https://github.com/breezy-bays-labs/crap-rs#crap-formula",
        config_file_name: "crap4rs.toml",
        default_excludes: DEFAULT_EXCLUDES,
        // crap4rs supports both cognitive and cyclomatic. Cognitive
        // remains the default (locked decision #2) — explicit here so
        // the per-adapter default flows through `AdapterMeta` for both
        // adapters uniformly (no implicit `ComplexityMetric::default()`
        // fallthrough anywhere in crap-core).
        default_metric: ComplexityMetric::Cognitive,
    };
    let cli = crap_core::cli::parse_args(&meta);
    let complexity = SynComplexityAdapter::new();
    crap_core::cli::run(
        cli,
        &complexity,
        |src| Box::new(LcovParser::new(src.to_path_buf())),
        &meta,
    )
}
