# crap-core

[![crates.io](https://img.shields.io/crates/v/crap-core.svg)](https://crates.io/crates/crap-core)
[![docs.rs](https://img.shields.io/docsrs/crap-core)](https://docs.rs/crap-core)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

Language-agnostic foundation for the CRAP (Change Risk Anti-Patterns) analyzer family. Domain types, port traits, threshold/risk logic, reporters, and the shared scorecard envelope used by every CRAP adapter.

## What this is

`crap-core` is the shared backbone for analyzers that compute CRAP scores — `complexity² × (1 − coverage)³ + complexity` — across different source languages. It owns:

- The CRAP formula and risk-tier classification (Low / Acceptable / Moderate / High)
- The `ComplexityPort` and `CoveragePort` traits that language adapters implement
- The wire envelope (`scorecard`, `delta`, `crap-delta` shapes) consumed by reporters and downstream tooling
- All reporters (`json`, `markdown`, `table`, `csv`, `sarif`, `scorecard`, `scorecard-row`, `github-annotations`)
- Threshold presets, configuration parsing, and delta-gate semantics

If you're using `crap-core` directly, you're probably building a new language adapter. Most users want one of the adapter crates instead:

- **[`crap4rs`](https://crates.io/crates/crap4rs)** — Rust analyzer (syn-based complexity + LCOV coverage)
- **[`crap4ts`](https://crates.io/crates/crap4ts)** — TypeScript / JavaScript analyzer (oxc-based complexity + Istanbul JSON coverage)

Both link `crap-core` and produce byte-identical scorecard envelopes for the same `(complexity, coverage)` inputs — so cross-language CI gates and reports stay consistent.

## Install

```toml
[dependencies]
crap-core = "0.4"
```

## Quick example

```rust
use crap_core::domain::crap::compute_crap;

let score = compute_crap(15, 0.90);
assert_eq!(format!("{:.2}", score), "15.23");
```

For the port traits and how adapters wire complexity + coverage data into a `Scorecard`, see the [API docs on docs.rs](https://docs.rs/crap-core).

## Stability

`crap-core` is at `0.x` and follows pre-1.0 semver: breaking changes can land on minor bumps. Adapter crates pin against a specific minor version. The wire envelope schema is locked once published — patch releases never change envelope shape; minor releases may add fields under `#[serde(default)]`.

## See also

- **Repository**: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- **Project README**: [workspace overview](https://github.com/breezy-bays-labs/crap-rs#readme) — explains CRAP scoring, risk tiers, and threshold presets
- **Issues**: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
