//! Thin TypeScript adapter binary — wires `crap_core::cli::run<P>` with
//! the oxc/Istanbul ports (currently stubs).
//!
//! Mirrors `crates/crap4rs/src/main.rs` exactly so future maintenance
//! sees one shape. ALPHA: invoking the real analysis path runtime-
//! panics inside the walker / coverage parser stubs. `--help` and
//! `--version` work because clap dispatch lives in `crap_core::cli`.

use std::path::PathBuf;
use std::process::ExitCode;

use crap4ts::adapters::coverage::IstanbulCoverage;
use crap4ts::adapters::walker::OxcWalker;

fn main() -> ExitCode {
    let cli = crap_core::cli::parse_args(env!("CARGO_PKG_VERSION"), env!("CRAP4TS_LONG_VERSION"));
    let src = cli
        .input
        .src
        .clone()
        .unwrap_or_else(|| PathBuf::from("src"));
    let coverage = IstanbulCoverage::new(src.canonicalize().unwrap_or(src));
    let complexity = OxcWalker::new();
    crap_core::cli::run(cli, &complexity, &coverage, env!("CARGO_PKG_VERSION"))
}
