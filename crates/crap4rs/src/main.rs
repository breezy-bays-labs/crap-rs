//! Thin Rust adapter binary — wires `crap_core::cli::run<P>` with the
//! LCOV/`syn` ports and the version metadata baked in by `build.rs`.

use std::path::PathBuf;
use std::process::ExitCode;

use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::adapters::coverage::LcovParser;

fn main() -> ExitCode {
    let cli = crap_core::cli::parse_args(env!("CARGO_PKG_VERSION"), env!("CRAP4RS_LONG_VERSION"));
    let src = cli
        .input
        .src
        .clone()
        .unwrap_or_else(|| PathBuf::from("src"));
    let coverage = LcovParser::new(src.canonicalize().unwrap_or(src));
    let complexity = SynComplexityAdapter::new();
    crap_core::cli::run(cli, &complexity, &coverage, env!("CARGO_PKG_VERSION"))
}
