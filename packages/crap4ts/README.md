# crap4ts

Rust-powered [CRAP](https://www.artima.com/weblogs/viewpost.jsp?thread=210575) (Change Risk Anti-Patterns) score analyzer for TypeScript and JavaScript. Combines [oxc](https://oxc.rs/)-driven AST complexity with [Istanbul](https://istanbul.js.org/) JSON coverage to identify functions that are both complex and under-tested.

`crap4ts@2.x` is the TypeScript adapter for [`crap-rs`](https://github.com/breezy-bays-labs/crap-rs), distributed as a [napi-rs](https://napi.rs/) Node addon. The CRAP formula, scorecard envelope, and reporter shapes are shared with [`crap4rs`](https://github.com/breezy-bays-labs/crap-rs) (the Rust adapter) so TypeScript projects get identical semantics to Rust projects.

## Status

This is `2.0.0-rc.1` — a release candidate in a 48–72 h soak window before the GA `2.0.0` cut. Try it on a sandbox project; file issues against [`breezy-bays-labs/crap-rs`](https://github.com/breezy-bays-labs/crap-rs/issues) if you hit anything.

## Install

```sh
npm install crap4ts@2.0.0-rc.1
# or
pnpm add crap4ts@2.0.0-rc.1
```

Published platforms in `2.0.0-rc.1`:
- macOS (arm64 + x64)
- Linux (x64 / glibc)

Windows and Linux arm64 / musl are tracked for a later release (see [`crap-rs`#154](https://github.com/breezy-bays-labs/crap-rs/issues/154)).

## Usage

```js
const { analyze } = require('crap4ts');

const json = analyze({
  sourceRoot: 'src',
  coveragePath: 'coverage/coverage-final.json',
  // Optional:
  // threshold: 16,            // metric-correct default (cyclomatic: 16, cognitive: 25)
  // metric: 'cyclomatic',     // 'cyclomatic' (default) or 'cognitive' (not yet supported)
});

const { result, diagnostics } = JSON.parse(json);
console.log(result.summary);
```

Generate `coverage-final.json` via your test runner with Istanbul coverage enabled (`jest --coverage`, `vitest --coverage`, `c8 --reporter=json`, etc.).

## Migrating from `crap4ts@1.x`

See [`MIGRATION.md`](https://github.com/breezy-bays-labs/crap-rs/blob/main/MIGRATION.md) for the full upgrade guide. Short version: scores may differ for three compounding reasons (calibrated threshold default, TS-specific calibration not yet validated, arrow-function coverage handling), and the subpath exports (`crap4ts/formula`, `crap4ts/complexity`, `crap4ts/coverage`) are not re-exposed in 2.x — call `analyze()` and read fields from the returned JSON.

## License

MIT OR Apache-2.0.
