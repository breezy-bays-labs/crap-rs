# crap4rs

[![crates.io](https://img.shields.io/crates/v/crap4rs.svg)](https://crates.io/crates/crap4rs)
[![docs.rs](https://img.shields.io/docsrs/crap4rs)](https://docs.rs/crap4rs)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

CRAP (Change Risk Anti-Patterns) score analyzer for Rust. Joins [`syn`](https://crates.io/crates/syn)-driven AST complexity (cognitive by default, cyclomatic available) with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) LCOV output to identify functions that are both complex and under-tested — the ones most likely to break when changed.

## What you get

For each function in your crate (or workspace), `crap4rs` reports complexity, coverage, and a CRAP score:

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

Functions above the configured threshold fail the CI gate. Several output formats are supported — `table` (TTY default), `markdown` (PR comments), `scorecard` (CI delta gating), `github-annotations` (inline PR review annotations), `json`, `csv`, `sarif`.

## Install

```bash
# Pre-built binaries (recommended)
cargo binstall crap4rs

# From source
cargo install crap4rs
```

## Usage

```bash
# 1. Generate coverage with cargo-llvm-cov
cargo llvm-cov --lcov --output-path lcov.info

# 2. Analyze
crap4rs --coverage lcov.info

# 3. Or gate CI on a threshold
crap4rs --coverage lcov.info --threshold strict
```

Full documentation — including the threshold presets (`strict` / default / `lenient`), delta-gate behavior, baseline-comparison mode, configuration file (`crap-rs.toml`), and the GitHub Actions composite scorecard action — lives in the [workspace README](https://github.com/breezy-bays-labs/crap-rs#readme).

## Library use

```toml
[dependencies]
crap4rs = "0.5"
```

`crap4rs` re-exports `crap-core`'s public API and adds the Rust-specific adapters (syn walker, LCOV parser). If you only need the scoring/envelope/reporter logic without Rust-specific I/O, depend on [`crap-core`](https://crates.io/crates/crap-core) directly.

## Stability

`crap4rs` is at `0.x` and follows pre-1.0 semver. The scorecard wire envelope is locked once published.

## See also

- **Repository**: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- **TypeScript / JavaScript analyzer**: [`crap4ts`](https://crates.io/crates/crap4ts) (crates.io) · [npm package](https://www.npmjs.com/package/crap4ts)
- **Shared core library**: [`crap-core`](https://crates.io/crates/crap-core)
- **Issues**: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
