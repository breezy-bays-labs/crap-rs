# crap4ts

[![crates.io](https://img.shields.io/crates/v/crap4ts.svg)](https://crates.io/crates/crap4ts)
[![npm](https://img.shields.io/npm/v/crap4ts.svg)](https://www.npmjs.com/package/crap4ts)
[![docs.rs](https://img.shields.io/docsrs/crap4ts)](https://docs.rs/crap4ts)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

Rust-powered CRAP (Change Risk Anti-Patterns) score analyzer for TypeScript and JavaScript. Joins [oxc](https://oxc.rs/)-driven AST complexity with [Istanbul](https://istanbul.js.org/) JSON coverage to identify functions that are both complex and under-tested.

> **JavaScript / Node consumers**: install [`crap4ts` from npm](https://www.npmjs.com/package/crap4ts) — that package ships pre-built Node addons for every supported platform. Its [npm README](https://github.com/breezy-bays-labs/crap-rs/blob/main/packages/crap4ts/README.md) covers Node-side usage.
>
> **This crates.io page** documents the Rust crate: the standalone CLI binary and the `cdylib` artifact that backs the npm package.

## Install (CLI)

```bash
cargo binstall crap4ts
# or from source
cargo install crap4ts
```

## Usage

```bash
# 1. Generate Istanbul JSON coverage (e.g. via Vitest + istanbul provider)
vitest run --coverage --coverage.reporter=json

# 2. Analyze
crap4ts --coverage coverage/coverage-final.json --src src

# 3. Gate CI on the default threshold
crap4ts --coverage coverage/coverage-final.json --src src --threshold default
```

`crap4ts` emits the same scorecard envelope as `crap4rs` — same fields, same risk tiers, same delta-gate semantics — so a multi-language monorepo can drive a single CI gate across both ecosystems.

## What this is

`crap4ts` is the TypeScript / JavaScript adapter in the [`crap-rs`](https://github.com/breezy-bays-labs/crap-rs) workspace. It compiles to two artifacts from one crate:

- A standalone Rust CLI binary, distributed via crates.io (this page) and `cargo binstall`
- A [napi-rs](https://napi.rs/) `cdylib` Node addon, distributed via [npm](https://www.npmjs.com/package/crap4ts)

Both share the same walker (`oxc` for complexity), the same Istanbul JSON coverage parser, and the same [`crap-core`](https://crates.io/crates/crap-core) scoring/reporter pipeline as the Rust adapter [`crap4rs`](https://crates.io/crates/crap4rs).

## Library use (Rust)

```toml
[dependencies]
crap4ts = "2"
```

Most users want the CLI or the npm package; the library crate is intended for downstream tooling that needs programmatic access to TypeScript walking + scoring without spawning a subprocess.

## Stability

`crap4ts` is in the `2.0.0-rc.x` release-candidate series ahead of the GA `2.0.0` cut. The CLI surface, configuration shape, and scorecard envelope are locked across the rc series; rc bumps fix bugs and tighten the walker. See the [changelog](https://github.com/breezy-bays-labs/crap-rs/blob/main/packages/crap4ts/CHANGELOG.md) for the per-version history.

## See also

- **Repository**: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- **npm package**: [`crap4ts` on npm](https://www.npmjs.com/package/crap4ts) — for JS/Node consumers
- **Rust analyzer**: [`crap4rs`](https://crates.io/crates/crap4rs)
- **Shared core library**: [`crap-core`](https://crates.io/crates/crap-core)
- **Issues**: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
