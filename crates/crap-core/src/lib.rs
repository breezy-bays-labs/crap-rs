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

pub mod adapters;
pub mod core;
pub mod domain;
pub mod ports;

/// Shared proptest strategies + a stub `ParseDiagnostic` impl, exposed
/// for downstream adapter tests via the `test-helpers` feature. Always
/// available inside crap-core's own tests; gated for external consumers
/// so production builds don't pull `proptest`.
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_strategies;
