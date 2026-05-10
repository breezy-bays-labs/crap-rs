//! crap-core — language-agnostic CRAP analyzer foundation.
//!
//! Domain types, port traits, and shared invariants used by every
//! per-language CRAP adapter (`crap4rs`, future `crap4ts`, …). Pure
//! domain — no `syn`, no LCOV, no I/O. Coverage adapters supply a
//! concrete `ParseDiagnostic` impl; the analyzer pipeline flows that
//! type through `ParseOutput<P>` and `AnalysisDiagnostics<P>`.
//!
//! Imported in S2 ([crap4rs#134](https://github.com/breezy-bays-labs/crap4rs/issues/134)).
//! Adapter relocation (reporters + baseline + config + diff + filesystem
//! walker) landed in S3
//! ([crap4rs#135](https://github.com/breezy-bays-labs/crap4rs/issues/135)).
//! Core orchestration + CLI dispatch landed in S4
//! ([crap4rs#136](https://github.com/breezy-bays-labs/crap4rs/issues/136)) —
//! `core::analyze<P>` and `cli::run<P>` are language-agnostic; the
//! per-adapter binaries (`crap4rs`, future `crap4ts`) inject their own
//! `ComplexityPort` + `CoveragePort<Diagnostic = P>`.

pub mod adapters;
pub mod cli;
pub mod core;
pub mod domain;
pub mod ports;

/// Shared proptest strategies + a stub `ParseDiagnostic` impl, exposed
/// for downstream adapter tests via the `test-helpers` feature. Always
/// available inside crap-core's own tests; gated for external consumers
/// so production builds don't pull `proptest`.
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_strategies;
