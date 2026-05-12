//! Thin TypeScript adapter binary — wires `crap_core::cli::run<P>` with
//! the oxc/Istanbul ports (currently stubs).
//!
//! Mirrors `crates/crap4rs/src/main.rs` exactly so future maintenance
//! sees one shape. ALPHA: invoking the real analysis path runtime-
//! panics inside the walker / coverage parser stubs. `--help` and
//! `--version` work because clap dispatch lives in `crap_core::cli`
//! and the `AdapterMeta` plumbing renders TS-flavored help text
//! correctly even while the walker is unimplemented.
//!
//! The coverage adapter is supplied as a factory closure so
//! `crap_core::cli::run` can construct the Istanbul parser *after*
//! CLI/config-file merging resolves the effective source root —
//! pre-construction can silently strip the wrong path prefix from
//! coverage records when `--src` came from the config file rather
//! than the CLI.

use std::process::ExitCode;

use crap_core::cli::AdapterMeta;
use crap4ts::adapters::coverage::IstanbulCoverage;
use crap4ts::adapters::walker::OxcWalker;

const ABOUT: &str = "CRAP score analyzer for TypeScript (alpha)";
const LONG_ABOUT: &str = "CRAP (Change Risk Anti-Patterns) score analyzer for TypeScript / \
                         JavaScript codebases.\n\n\
                         ALPHA: the oxc-based walker and Istanbul coverage parser are stubs — \
                         invoking the analysis path will runtime-panic. `--help`, `--version`, \
                         and `completions` work end-to-end so downstream packaging can be wired \
                         up ahead of the parser ship in the next pipeline.\n\n\
                         When the real adapters land, combines AST complexity (via oxc) with \
                         Istanbul JSON coverage to identify functions that are both complex \
                         and under-tested. Default metric is cognitive complexity.";

const AFTER_HELP: &str = "\
EXAMPLES (alpha — walker not yet implemented):
  crap4ts --coverage coverage/coverage-final.json
  crap4ts --coverage coverage/coverage-final.json --threshold 15 --metric cyclomatic
  crap4ts --coverage coverage/coverage-final.json --format sarif --no-fail > crap.sarif

For working CRAP analysis on TypeScript codebases today, see crap4ts@1.x \
on npm. crap4ts@2 (this binary) is in pre-release alpha tracking \
breezy-bays-labs/crap-rs.";

const COVERAGE_HINT: &str = "ensure tests ran with coverage enabled (e.g. `c8 --reporter=json` or `vitest --coverage`) — \
     crap4ts parses Istanbul's `coverage-final.json`, not LCOV";

const EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs"];

/// Common TS/JS-project directories `init` writes as commented-out
/// excludes — `node_modules` for vendored deps, build/coverage output
/// dirs.
const DEFAULT_EXCLUDES: &[&str] = &["node_modules/**", "dist/**", "coverage/**"];

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
