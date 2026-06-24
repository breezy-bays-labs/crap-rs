# crap-core

[![crates.io](https://img.shields.io/crates/v/crap-core.svg)](https://crates.io/crates/crap-core)
[![docs.rs](https://img.shields.io/docsrs/crap-core)](https://docs.rs/crap-core)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

Language-agnostic foundation for the CRAP (Change Risk Anti-Patterns) analyzer family. Domain types, port traits, threshold and risk logic, reporters, and the shared scorecard envelope used by every CRAP adapter.

## What this is

`crap-core` is the shared backbone for analyzers that compute CRAP scores across source languages:

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

It owns:

- The CRAP formula and score-based risk classification (Low / Acceptable / Moderate / High).
- The `ComplexityPort` and `CoveragePort` traits that language adapters implement.
- The locked wire envelope (`scorecard`, `delta`, `crap-delta` shapes) consumed by reporters and downstream tooling.
- The reporters — `table`, `markdown`, `json`, `csv`, `sarif`, `scorecard-row`, `github-annotations`, and `html` (single-language `format_html` and multi-language `format_html_multi`, the latter driven by the `crap-render` binary).
- Threshold presets, configuration parsing, and delta-gate semantics.

If you depend on `crap-core` directly, you are probably building a new language adapter. End users want one of the adapter crates instead:

- [`crap4rs`](https://crates.io/crates/crap4rs) — Rust analyzer (`syn` complexity + LCOV coverage; cognitive complexity default).
- [`crap4ts`](https://crates.io/crates/crap4ts) — TypeScript / JavaScript analyzer (`oxc` complexity + Istanbul JSON coverage; cyclomatic complexity only).

Both link `crap-core` and produce byte-identical scorecard envelopes for the same `(complexity, coverage)` inputs.

## Install

```sh
cargo add crap-core
```

## Quick example

```rust
use crap_core::domain::crap::compute_crap;

// compute_crap takes coverage as a percent in [0, 100] and returns a
// Result<CrapScore, CrapError>; CrapScore exposes value and risk_level.
let score = compute_crap(15, 90.0).unwrap();
assert_eq!(format!("{:.2}", score.value), "15.23");
```

For the port traits and how adapters wire complexity + coverage data into a `Scorecard`, see the [API docs on docs.rs](https://docs.rs/crap-core).

The risk bands (score-based `classify_risk`) and the threshold gate (default 15) are distinct axes that share the same `8/15/25` numbers — the book covers both in [understanding-crap.md](https://breezy-bays-labs.github.io/crap-rs/book/understanding-crap.html). The reporter output gallery lives in [output-formats.md](https://breezy-bays-labs.github.io/crap-rs/book/output-formats.html), and the `crap-render` multi-language render path in [multi-language.md](https://breezy-bays-labs.github.io/crap-rs/book/multi-language.html).

## Stability

`crap-core` is at `0.x` and follows pre-1.0 semver — breaking changes can land on minor bumps; adapter crates pin against a specific minor. The wire envelope schema is locked once published: patch releases never change envelope shape; minor releases may add fields under `#[serde(default)]`.

## See also

- Repository: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- Project README: [workspace overview](https://github.com/breezy-bays-labs/crap-rs#readme)
- Issues: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
