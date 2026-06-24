# crap4ts

[![crates.io](https://img.shields.io/crates/v/crap4ts.svg)](https://crates.io/crates/crap4ts)
[![npm](https://img.shields.io/npm/v/crap4ts.svg)](https://www.npmjs.com/package/crap4ts)
[![docs.rs](https://img.shields.io/docsrs/crap4ts)](https://docs.rs/crap4ts)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

CRAP (Change Risk Anti-Patterns) score analyzer for TypeScript and JavaScript, powered by Rust. It finds the functions that are both complex and under-tested.

`crap4ts` combines [oxc](https://oxc.rs/)-driven AST complexity with [Istanbul](https://istanbul.js.org/) JSON coverage. One pass, one CI gate, one number per function.

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

High complexity plus low coverage yields a high CRAP score. See [understanding CRAP](https://breezy-bays-labs.github.io/crap-rs/book/understanding-crap.html) for the math and the unit conventions.

> **JavaScript / Node consumers**: install [`crap4ts` from npm](https://www.npmjs.com/package/crap4ts) — that package ships pre-built Node addons for every supported platform. Its [npm README](https://github.com/breezy-bays-labs/crap-rs/blob/main/packages/crap4ts/README.md) covers Node-side usage.
>
> **This crates.io page** documents the Rust crate: the standalone CLI binary and the `cdylib` artifact that backs the npm package.

## Install (CLI)

```bash
cargo install crap4ts
```

Pre-built standalone CLI binaries are tracked for a future release; the napi `cdylib` for the npm package ships pre-built today, the CLI artifact does not yet. Use `cargo install` for now, or install the [npm package](https://www.npmjs.com/package/crap4ts) if you consume from Node. See [installation](https://breezy-bays-labs.github.io/crap-rs/book/installation.html) for all install paths.

## Quick run

```bash
# Generate Istanbul JSON coverage (e.g. via Vitest + istanbul provider)
vitest run --coverage --coverage.reporter=json

# Analyze
crap4ts --coverage coverage/coverage-final.json --src src

# Gate CI strictly
crap4ts --coverage coverage/coverage-final.json --src src --strict
```

`--coverage` is required at runtime. The full flag surface lives in the [CLI reference](https://breezy-bays-labs.github.io/crap-rs/book/cli-reference.html); see [quick start](https://breezy-bays-labs.github.io/crap-rs/book/quick-start.html) for a guided first run.

## Why cyclomatic complexity (only)

`crap4ts` ships cyclomatic complexity as the only supported metric. Two reasons:

1. **Classic CRAP semantics.** Cyclomatic decision-point count is the original CRAP metric and aligns with how TypeScript and JavaScript quality tools report complexity (for example ESLint's `complexity` rule). CI gates and reviewer expectations transfer cleanly.
2. **AST signal density differs from Rust.** TypeScript code leans less on `match`-style branching than idiomatic Rust, so the cognitive-vs-cyclomatic divergence is smaller. Cognitive may follow in a later release if ecosystem demand justifies the additional walker logic.

Passing `--metric cognitive` errors out cleanly with `MetricNotSupported`.

The companion Rust analyzer [`crap4rs`](https://crates.io/crates/crap4rs) defaults to cognitive complexity for the inverse reason — Rust idioms benefit from it. The shared CRAP formula, risk bands, and envelope shape are identical across both adapters; only the complexity number entering the formula differs.

## Threshold gate and risk bands

The default gate (no flag) is CRAP ≤ 15. The two preset flags shift it:

| Flag | Gate | Use for |
|---|---|---|
| `--strict` | CRAP ≤ 8 | safety-critical, high-quality libraries |
| *(default)* | CRAP ≤ 15 | typical app / library code |
| `--lenient` | CRAP ≤ 25 | legacy / transitional codebases |

The threshold gate and the score-based risk bands are distinct axes that share the same numbers today:

| CRAP score | Risk band |
|---|---|
| ≤ 8 | Low |
| ≤ 15 | Acceptable |
| ≤ 25 | Moderate |
| > 25 | High |

The `8`/`15`/`25` values are a calibration convention, not empirically derived. Override or define per-codebase presets in [configuration](https://breezy-bays-labs.github.io/crap-rs/book/configuration.html).

## Output formats

Table (TTY default), markdown, GitHub annotations, JSON, CSV, SARIF, scorecard-row, and an interactive HTML report all ship today. The output gallery and per-format details live in [output formats](https://breezy-bays-labs.github.io/crap-rs/book/output-formats.html). Multiple formats compose in one pass.

The JSON wire envelope is byte-identical to `crap4rs`'s output — same fields, same risk-band strings, same delta-gate semantics — so a multi-language monorepo can drive a single CI gate across both ecosystems. Delta gates, rename detection, and threshold-epsilon jitter suppression are covered in the [CLI reference](https://breezy-bays-labs.github.io/crap-rs/book/cli-reference.html) and [limitations and FAQ](https://breezy-bays-labs.github.io/crap-rs/book/limitations-and-faq.html).

## Two artifacts from one crate

`crap4ts` is the TypeScript / JavaScript adapter in the [`crap-rs`](https://github.com/breezy-bays-labs/crap-rs) workspace. It compiles to two artifacts:

- A standalone Rust CLI binary, built from source with `cargo install crap4ts` (this page). Prebuilt binaries and `cargo binstall` support are tracked for a future release.
- A [napi-rs](https://napi.rs/) `cdylib` Node addon, distributed via [npm](https://www.npmjs.com/package/crap4ts).

Both share the same `oxc` walker, the same Istanbul JSON coverage parser, and the same [`crap-core`](https://crates.io/crates/crap-core) scoring and reporter pipeline as the Rust adapter [`crap4rs`](https://crates.io/crates/crap4rs).

## Library use (Rust)

```toml
[dependencies]
crap4ts = "2"
```

Most users want the CLI or the npm package; the library crate is for downstream tooling that needs programmatic TypeScript walking and scoring without spawning a subprocess.

## Stability

`crap4ts` is in the `2.0.0-rc.x` release-candidate series ahead of the GA `2.0.0` cut. The CLI surface, configuration shape, and scorecard envelope are locked across the rc series; rc bumps fix bugs and tighten the walker. See the [changelog](https://github.com/breezy-bays-labs/crap-rs/blob/main/packages/crap4ts/CHANGELOG.md) for per-version history.

## See also

- **Repository**: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- **npm package**: [`crap4ts` on npm](https://www.npmjs.com/package/crap4ts) — for JS/Node consumers
- **Rust analyzer**: [`crap4rs`](https://crates.io/crates/crap4rs)
- **Shared core library**: [`crap-core`](https://crates.io/crates/crap-core)
- **Issues**: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
