//! Thin TypeScript adapter binary — wires `crap_core::cli::run<P>` with
//! the oxc/Istanbul ports (currently stubs).
//!
//! Mirrors `crates/crap4rs/src/main.rs` exactly so future maintenance
//! sees one shape. ALPHA: invoking the real analysis path runtime-
//! panics inside the walker / coverage parser stubs. `--help` and
//! `--version` work because clap dispatch lives in `crap_core::cli`.
//!
//! The coverage adapter is supplied as a factory closure so
//! `crap_core::cli::run` can construct the Istanbul parser *after*
//! CLI/config-file merging resolves the effective source root —
//! pre-construction was the silent path-strip mismatch fixed in #150.

use std::process::ExitCode;

use crap4ts::adapters::coverage::IstanbulCoverage;
use crap4ts::adapters::walker::OxcWalker;

fn main() -> ExitCode {
    let cli = crap_core::cli::parse_args(env!("CARGO_PKG_VERSION"), env!("CRAP4TS_LONG_VERSION"));
    let complexity = OxcWalker::new();
    crap_core::cli::run(
        cli,
        &complexity,
        |src| Box::new(IstanbulCoverage::new(src.to_path_buf())),
        env!("CARGO_PKG_VERSION"),
    )
}
