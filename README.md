# crap-rs

[![CI](https://github.com/breezy-bays-labs/crap-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/breezy-bays-labs/crap-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/crap4rs.svg)](https://crates.io/crates/crap4rs)
[![npm](https://img.shields.io/npm/v/crap4ts.svg)](https://www.npmjs.com/package/crap4ts)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

CRAP (Change Risk Anti-Patterns) score analysis with a shared Rust core and language-specific adapters. A CRAP score fuses a function's complexity with its line coverage into one risk number — high complexity plus low coverage means high risk when the code changes. The published formula is `CRAP = complexity² × (1 − coverage)³ + complexity`, where `coverage` is a fraction in `[0, 1]`.

## Two adapters, one core

| Crate / package | Analyzes | Default metric | Coverage format | Published to |
|-----------------|----------|----------------|-----------------|--------------|
| `crap4rs` | Rust | cognitive | LCOV (`lcov.info`) | [crates.io](https://crates.io/crates/crap4rs) |
| `crap4ts` | TypeScript / JavaScript | cyclomatic | Istanbul JSON (`coverage-final.json`) | [npm](https://www.npmjs.com/package/crap4ts) — see its [package README](packages/crap4ts/README.md) |
| `crap-core` | shared library | — | — | CRAP formula, thresholds, reporters, analysis types |

Both adapters link the same `crap-core`, so the formula, wire envelope, and reporters are identical across languages. CRAP scores are not directly comparable across languages — the combined view ranks by CRAP/threshold ratio and risk band, not by raw score.

## Risk bands

Every score lands in one of four risk bands. These bands (score-based) and the build gate (threshold-based) are distinct axes that share the same numbers today — see [understanding CRAP](https://breezy-bays-labs.github.io/crap-rs/book/understanding-crap.html).

| CRAP score | Risk band |
|------------|-----------|
| ≤ 8 | Low |
| ≤ 15 | Acceptable |
| ≤ 25 | Moderate |
| > 25 | High |

## Install

```bash
# Rust analyzer (binstall preferred, falls back to a source build)
cargo binstall crap4rs
cargo install crap4rs

# TypeScript / JavaScript analyzer
npm install -g crap4ts
```

crap4rs reads LCOV coverage from [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) (`cargo llvm-cov --lcov --output-path lcov.info`); install it alongside the analyzer. See [installation](https://breezy-bays-labs.github.io/crap-rs/book/installation.html) for all install paths and coverage prerequisites.

## Try it locally

The repo ships a polyglot sample at `crates/crap-examples/` — four Rust and four TypeScript modules spanning every risk band, each isolating one term of the CRAP formula. See `crates/crap-examples/README.md` for the worked-example heatmap.

```bash
git clone https://github.com/breezy-bays-labs/crap-rs.git
cd crap-rs

# Rust
crap4rs --src ./crates/crap-examples/src --coverage crates/crap-examples/lcov.info

# TypeScript
crap4ts --src ./crates/crap-examples/ts --coverage crates/crap-examples/coverage-final.json --exclude '*.test.ts'
```

The committed coverage fixtures use paths relative to each adapter's `--src` root. To regenerate them, follow `crates/crap-examples/README.md`.

## Documentation

Full documentation lives in the [book](https://breezy-bays-labs.github.io/crap-rs/book/introduction.html). Each chapter owns its scope:

| Chapter | Covers |
|---------|--------|
| [Installation](https://breezy-bays-labs.github.io/crap-rs/book/installation.html) | Install paths, prerequisites, toolchain |
| [Quick start](https://breezy-bays-labs.github.io/crap-rs/book/quick-start.html) | First analysis end to end |
| [Understanding CRAP](https://breezy-bays-labs.github.io/crap-rs/book/understanding-crap.html) | The formula, risk bands vs. the gate, why cognitive by default |
| [CLI reference](https://breezy-bays-labs.github.io/crap-rs/book/cli-reference.html) | Every flag, subcommand, and exit code; shell completions |
| [Configuration](https://breezy-bays-labs.github.io/crap-rs/book/configuration.html) | `crap.toml` schema, presets, per-path thresholds, saved views |
| [Output formats](https://breezy-bays-labs.github.io/crap-rs/book/output-formats.html) | table, json, markdown, csv, sarif, html, advice, scorecard-row, github-annotations |
| [CI integration](https://breezy-bays-labs.github.io/crap-rs/book/ci-integration.html) | The composite scorecard action, SARIF upload, `--baseline` / `--delta-gate` |
| [Multi-language](https://breezy-bays-labs.github.io/crap-rs/book/multi-language.html) | Unified Rust + TypeScript reports via `crap-render` |
| [Limitations & FAQ](https://breezy-bays-labs.github.io/crap-rs/book/limitations-and-faq.html) | Line-range matching, integration/BDD coverage, `--lib` caveats, threshold calibration |

## Workspace layout

crap-rs is a Cargo workspace built on a hexagonal (ports & adapters) core.

| Crate | Role |
|-------|------|
| `crap-core` | Language-agnostic core — CRAP formula, threshold model, result types, reporters, analysis orchestration (`domain/` → `ports/` → `core/`). |
| `crap4rs` | Rust adapter — `syn`-based complexity walker, LCOV parser, and the Rust CLI. |
| `crap4ts` | TypeScript / JavaScript adapter — `oxc`-based complexity walker, Istanbul JSON parser, published to npm as a napi-rs addon. |

Each adapter supplies its own `ComplexityPort` and `CoveragePort`; `crap-core` never imports a language toolchain. An `ast-purity` CI gate enforces this. The CRAP math is shared; threshold policy stays language-specific.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms
or conditions.
