# Installation

crap4rs analyzes Rust; crap4ts analyzes TypeScript and JavaScript. Both share the `crap-core` foundation, which also ships the `crap-render` binary used for multi-language reports. Pick the analyzer that matches your project, then install the matching coverage prerequisites below.

## Rust analyzer — crap4rs

Install from crates.io with a Rust toolchain present:

```bash
cargo install crap4rs
```

Prefer a prebuilt binary over a source build with `cargo binstall`. crap4rs ships `[package.metadata.binstall]`, so binstall resolves the matching release tarball and falls back to `cargo install` only when no prebuilt target exists:

```bash
cargo binstall crap4rs
```

The binary name is `crap4rs`.

## TypeScript / JavaScript analyzer — crap4ts

Install from npm:

```bash
npm install -g crap4ts
```

The npm package ships a native binding built with napi-rs for `linux` and `darwin` on `x64` and `arm64`; it requires Node 18 or later.

> The `crap4ts` CLI binary is not yet published to crates.io — only the npm package carries a working release. To run the CLI on a platform the npm package does not cover, install from source with a Rust toolchain:
>
> ```bash
> cargo install crap4ts
> ```

The binary name is `crap4ts`.

## Multi-language renderer — crap-render

`crap-render` composes per-language JSON envelopes into a combined report (HTML today). It ships inside `crap-core`:

```bash
cargo install crap-core      # installs the crap-render binary
cargo binstall crap-core     # prebuilt, with cargo install fallback
```

You only need `crap-render` directly when combining Rust and TypeScript results by hand. See [multi-language analysis](multi-language.md) for its usage; in CI the [composite action](ci-integration.md) invokes it for you.

## Coverage prerequisites

crap4rs and crap4ts compute CRAP from your existing coverage data. Generate it with the tooling for your language before running an analysis.

### Rust — LCOV

crap4rs reads LCOV (`lcov.info`). Generate it with [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov), which needs the `llvm-tools-preview` rustup component:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
cargo llvm-cov --lcov --output-path lcov.info
```

### TypeScript / JavaScript — Istanbul JSON

crap4ts reads Istanbul-format coverage (`coverage-final.json`). crap4ts consumes the istanbul provider's JSON only — Vitest defaults to the v8 provider, whose JSON is a different shape the adapter does not read, so set the provider explicitly. Any Istanbul-compatible producer works — for example [c8](https://github.com/bcoe/c8), Vitest with the istanbul provider, or `nyc`. Configure your runner to emit the `json` reporter:

```bash
c8 --reporter=json npm test            # writes coverage/coverage-final.json
vitest run --coverage --coverage.provider=istanbul --coverage.reporter=json
```

## Next steps

- [Quick start](quick-start.md) — run your first analysis.
- [CLI reference](cli-reference.md) — every flag.
- [CI integration](ci-integration.md) — the scorecard action and coverage setup on a runner.
