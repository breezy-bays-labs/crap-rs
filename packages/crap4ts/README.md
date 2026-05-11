# crap4ts (alpha)

**This package is alpha and not yet published to npm.** It's a scaffold for the upcoming `crap4ts@2.x` line — a Rust-powered CRAP (Change Risk Anti-Patterns) score analyzer for TypeScript, distributed as a [napi-rs](https://napi.rs/) Node addon backed by the [oxc](https://oxc.rs/) parser.

## Status

| Component | Status |
|---|---|
| Workspace skeleton, Cargo wiring, CI build | landed in [crap4rs#137](https://github.com/breezy-bays-labs/crap4rs/issues/137) |
| `oxc`-based TypeScript walker | not yet implemented (`unimplemented!()`) |
| Istanbul JSON coverage parser | not yet implemented (`unimplemented!()`) |
| napi-rs Node bindings beyond the `alphaStatus()` placeholder | not yet implemented |
| npm publish | gated on the items above |

The `package.json` is intentionally marked `"private": true` so `npm publish` is a no-op until the walker lands.

## For working CRAP analysis on TypeScript today

Use the previous, JavaScript-implemented analyzer:

```
npm install --save-dev crap4ts@1
```

The `v1.x-maintenance-announcement` release on [`breezy-bays-labs/crap4ts`](https://github.com/breezy-bays-labs/crap4ts) documents the deprecation timeline.

## Why a v2

The v2 analyzer shares its CRAP formula, scorecard envelope, and reporter shapes with [`crap4rs`](https://github.com/breezy-bays-labs/crap-rs) by linking the same `crap-core` Rust library. That gives TypeScript projects identical analysis semantics to Rust projects (per-function complexity, line-range matching against coverage data, deltas vs a baseline, JSON / Markdown / SARIF / HTML / scorecard-row reporters) — at native speed, with no separate engine to maintain.

## Tracking

- Skeleton + scaffolding: [crap4rs#137](https://github.com/breezy-bays-labs/crap4rs/issues/137)
- Follow-up walker / parser pipeline: tracked separately (see the issue thread for the pipeline name once filed).
