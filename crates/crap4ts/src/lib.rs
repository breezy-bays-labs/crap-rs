//! crap4ts — TypeScript adapter for the language-agnostic `crap_core`
//! analyzer.
//!
//! Combines AST complexity (via oxc) with Istanbul JSON coverage to
//! identify functions that are both complex and under-tested. Default
//! metric is cyclomatic complexity; cognitive surfaces
//! `CrapError::MetricNotSupported` (D5 + locked decision #2).
//!
//! Two consumer surfaces:
//! - the `crap4ts` CLI binary (`src/main.rs`) — default build, no napi
//!   linkage,
//! - the napi-rs cdylib (`src/napi.rs`) — gated behind the
//!   `napi-binding` feature, exposes a single `analyze()` JSON entry
//!   consumed from Node via the `crap4ts` npm package.

pub mod adapters;
pub mod parse_diagnostic;

#[cfg(feature = "napi-binding")]
pub mod napi;
