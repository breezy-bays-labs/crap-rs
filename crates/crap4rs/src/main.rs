//! Thin Rust adapter binary — wires `crap_core::cli::run<P>` with the
//! LCOV/`syn` ports and the version metadata baked in by `build.rs`.
//!
//! The coverage adapter is supplied as a factory closure so
//! `crap_core::cli::run` can construct the LCOV parser *after*
//! CLI/config-file merging resolves the effective source root —
//! pre-construction was the silent path-strip mismatch fixed in #150.

use std::process::ExitCode;

use crap4rs::adapters::complexity::SynComplexityAdapter;
use crap4rs::adapters::coverage::LcovParser;

fn main() -> ExitCode {
    let cli = crap_core::cli::parse_args(env!("CARGO_PKG_VERSION"), env!("CRAP4RS_LONG_VERSION"));
    let complexity = SynComplexityAdapter::new();
    crap_core::cli::run(
        cli,
        &complexity,
        |src| Box::new(LcovParser::new(src.to_path_buf())),
        env!("CARGO_PKG_VERSION"),
    )
}
