//! Thin TypeScript adapter binary — wires `crap_core::cli::run<P>` with
//! the oxc walker and Istanbul coverage ports.
//!
//! Mirrors `crates/crap4rs/src/main.rs` exactly so future maintenance
//! sees one shape. clap dispatch lives in `crap_core::cli`; the
//! `AdapterMeta` plumbing renders TS-flavored help text.
//!
//! The coverage adapter is supplied as a factory closure so
//! `crap_core::cli::run` can construct the Istanbul parser *after*
//! CLI/config-file merging resolves the effective source root —
//! pre-construction can silently strip the wrong path prefix from
//! coverage records when `--src` came from the config file rather
//! than the CLI.

use std::process::ExitCode;

use crap_core::cli::AdapterMeta;
use crap_core::domain::types::ComplexityMetric;
use crap4ts::adapters::coverage::IstanbulCoverage;
use crap4ts::adapters::walker::OxcWalker;
use crap4ts::{DEFAULT_EXCLUDES, EXTENSIONS, FORCED_EXCLUDES};

const ABOUT: &str = "CRAP score analyzer for TypeScript / JavaScript";
const LONG_ABOUT: &str = "CRAP (Change Risk Anti-Patterns) score analyzer for TypeScript / \
                         JavaScript codebases.\n\n\
                         Combines AST complexity (via oxc) with Istanbul JSON coverage to \
                         identify functions that are both complex and under-tested. Default \
                         metric is cyclomatic complexity; cognitive is not yet supported on \
                         crap4ts.";

const AFTER_HELP: &str = "\
EXAMPLES:
  crap4ts --coverage coverage/coverage-final.json
  crap4ts --coverage coverage/coverage-final.json --threshold 15 --metric cyclomatic
  crap4ts --coverage coverage/coverage-final.json --format sarif --no-fail > crap.sarif

crap4ts is the TypeScript adapter for crap-rs: \
https://github.com/breezy-bays-labs/crap-rs";

const COVERAGE_HINT: &str = "ensure tests ran with coverage enabled (e.g. `c8 --reporter=json` or `vitest --coverage`) — \
     crap4ts parses Istanbul's `coverage-final.json`, not LCOV";

fn main() -> ExitCode {
    let meta = AdapterMeta {
        tool_name: env!("CARGO_PKG_NAME"),
        tool_version: env!("CARGO_PKG_VERSION"),
        long_version: env!("CRAP4TS_LONG_VERSION"),
        about: ABOUT,
        long_about: LONG_ABOUT,
        after_help: AFTER_HELP,
        coverage_hint: COVERAGE_HINT,
        extensions: EXTENSIONS,
        tool_info_uri: "https://github.com/breezy-bays-labs/crap-rs",
        rule_help_uri: "https://github.com/breezy-bays-labs/crap-rs#crap-formula",
        config_file_name: "crap4ts.toml",
        default_excludes: DEFAULT_EXCLUDES,
        // `.d.ts` declaration files contain ambient types only (no
        // executable code); skip them at the discovery boundary so
        // they never reach the AST walker (crap-rs#253). Operators
        // cannot opt back in via `crap4ts.toml` — if a corpus
        // legitimately needs declaration-file CRAP, fork the adapter
        // or file a follow-up to add an opt-out flag.
        forced_excludes: FORCED_EXCLUDES,
        // crap4ts ships --metric cyclomatic as the only supported
        // metric in 2.0.0; cognitive returns CrapError::MetricNotSupported
        // from the walker (D5 + locked decision #2).
        default_metric: ComplexityMetric::Cyclomatic,
    };
    let cli = crap_core::cli::parse_args(&meta);
    let complexity = OxcWalker::new();
    crap_core::cli::run(
        cli,
        &complexity,
        |src| Box::new(IstanbulCoverage::new(src.to_path_buf())),
        &meta,
    )
}
