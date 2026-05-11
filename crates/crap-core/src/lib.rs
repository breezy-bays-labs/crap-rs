//! crap-core — language-agnostic CRAP analyzer foundation.
//!
//! Domain types, port traits, and shared invariants used by every
//! per-language CRAP adapter (`crap4rs`, `crap4ts`, …). The `domain/`
//! and `ports/` modules pull no AST-library dependency and no coverage
//! parser; `core/` and `adapters/` add filesystem walking, git diffs,
//! and reporter rendering, all language-agnostic. Coverage adapters
//! supply a concrete `ParseDiagnostic` impl; the analyzer pipeline
//! flows that type through `ParseOutput<P>` and
//! `AnalysisDiagnostics<P>`.
//!
//! `core::analyze<P>` and `cli::run<P>` are language-agnostic; the
//! per-adapter binaries inject their own `ComplexityPort` +
//! `CoveragePort<Diagnostic = P>` and adapter metadata via
//! `cli::AdapterMeta`.

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
