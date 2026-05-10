//! Language-agnostic adapters — IO and presentation layers that make
//! no assumptions about the source language being analyzed.
//!
//! Three families live here:
//! - **`baseline`**: loads a previously-emitted JSON envelope from disk
//!   so delta reporting can compare the current run against a prior one.
//! - **`config`**: parses `crap4rs.toml` configuration (threshold
//!   overrides, view presets, exclude patterns).
//! - **`diff`**: shells out to git to compute changed line ranges per
//!   file for `--diff <ref>` filtering.
//! - **`reporters`**: render the analyzer output as JSON, Markdown,
//!   SARIF, HTML, CSV, terminal table, scorecard rows, or advice
//!   summary.
//!
//! Per-language adapters (e.g. `crap4rs::adapters::complexity`,
//! `crap4rs::adapters::coverage`) live in their parent crate and pull
//! `crap-core`'s ports / domain types as dependencies.
//!
//! Relocated from `crap4rs::adapters::*` in S3 (crap4rs#135). The v0.4
//! public surface stays buildable through the shim `pub use` re-exports
//! in `crap4rs::adapters` (see crap4rs/src/lib.rs).

pub mod baseline;
pub mod config;
pub mod diff;
pub mod reporters;
