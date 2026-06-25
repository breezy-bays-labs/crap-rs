# crap4rs

[![crates.io](https://img.shields.io/crates/v/crap4rs.svg)](https://crates.io/crates/crap4rs)
[![docs.rs](https://img.shields.io/docsrs/crap4rs)](https://docs.rs/crap4rs)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

CRAP (Change Risk Anti-Patterns) score analyzer for Rust. It finds the functions that are both complex and under-tested — the ones most likely to break when changed.

`crap4rs` joins [`syn`](https://crates.io/crates/syn)-driven AST complexity with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) LCOV output, matching functions to coverage by line range. One pass, one CI gate, one number per function.

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

High complexity plus low coverage yields high CRAP, which means high change risk. See [understanding-crap.md](https://breezy-bays-labs.github.io/crap-rs/book/understanding-crap.html) for the math, units, and why the default metric is cognitive complexity.

## Install

```bash
cargo binstall crap4rs   # pre-built binary
cargo install crap4rs    # from source
```

See [installation.md](https://breezy-bays-labs.github.io/crap-rs/book/installation.html) for prerequisites and `cargo-llvm-cov` setup.

## Quick run

```bash
cargo llvm-cov --lcov --output-path lcov.info   # generate coverage
crap4rs --coverage lcov.info                     # analyze (default gate: CRAP > 15)
crap4rs --coverage lcov.info --strict            # gate strictly (CRAP > 8)
crap4rs --coverage lcov.info --format markdown   # PR-comment friendly
```

`--coverage` is required at runtime. The full flag surface lives in [cli-reference.md](https://breezy-bays-labs.github.io/crap-rs/book/cli-reference.html); the report gallery (table, markdown, JSON, GitHub annotations, CSV, SARIF, HTML) lives in [output-formats.md](https://breezy-bays-labs.github.io/crap-rs/book/output-formats.html).

## Thresholds and risk tiers

The no-flag gate fails on CRAP above 15. `--strict` lowers it to 8, `--lenient` raises it to 25. Override with `--threshold <N>`. Risk bands (score-based classification) and the threshold gate are distinct axes that happen to share the same numbers today.

| Risk band | CRAP score | Preset that gates here |
|---|---|---|
| Low | ≤ 8 | `--strict` |
| Acceptable | ≤ 15 | *(default)* |
| Moderate | ≤ 25 | `--lenient` |
| High | > 25 | — |

These numbers are a calibration convention, not an empirically derived constant. Set per-codebase defaults in `crap.toml` — see [configuration.md](https://breezy-bays-labs.github.io/crap-rs/book/configuration.html). How missing-from-coverage files are scored is a configurable choice; see [limitations-and-faq.md](https://breezy-bays-labs.github.io/crap-rs/book/limitations-and-faq.html).

## CI gates

Fail a PR only on newly introduced or newly elevated violations with `--baseline` plus `--delta-gate`; relocations and threshold-border jitter are handled so migrations and refactors don't trip the gate. The delta walkthrough lives in [cli-reference.md](https://breezy-bays-labs.github.io/crap-rs/book/cli-reference.html) and [limitations-and-faq.md](https://breezy-bays-labs.github.io/crap-rs/book/limitations-and-faq.html); the composite GitHub Action is documented in [ci-integration.md](https://breezy-bays-labs.github.io/crap-rs/book/ci-integration.html).

## Library use

```bash
cargo add crap4rs
```

`crap4rs` re-exports `crap-core`'s public API and adds the Rust-specific adapters (syn walker, LCOV parser). If you only need scoring, envelope, and reporter logic without Rust-specific I/O, depend on [`crap-core`](https://crates.io/crates/crap-core) directly (`cargo add crap-core`).

## Stability

`crap4rs` is at `0.x` and follows pre-1.0 semver. The scorecard wire envelope is locked once published — patch releases never change envelope shape; minor releases may add fields under `#[serde(default)]`.

## See also

- Documentation: the [crap-rs book](https://breezy-bays-labs.github.io/crap-rs/book/)
- Repository: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- TypeScript / JavaScript analyzer: [`crap4ts`](https://crates.io/crates/crap4ts) (crates.io) · [npm](https://www.npmjs.com/package/crap4ts) — cyclomatic metric; see [multi-language.md](https://breezy-bays-labs.github.io/crap-rs/book/multi-language.html)
- Shared core library: [`crap-core`](https://crates.io/crates/crap-core)
- Issues: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
