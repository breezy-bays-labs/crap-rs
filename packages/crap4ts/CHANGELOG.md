# Changelog

All notable changes to `crap4ts` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`crap4ts` is the TypeScript adapter of the CRAP analyzer — a
napi-rs-built Node addon that binds against the language-agnostic
`crap-core` library. It ships to npm; the underlying Rust crate
lives in the same workspace as `crap4rs` (the Rust adapter) for
shared-core development.

## [Unreleased]

## [2.0.0-rc.3](https://github.com/breezy-bays-labs/crap-rs/compare/crap4ts-v2.0.0-rc.2...crap4ts-v2.0.0-rc.3) - 2026-05-24

### Added

- *(crap-core)* askama-templated HTML + markdown reporters + Sakura
  HTML redesign. ([#307](https://github.com/breezy-bays-labs/crap-rs/pull/307))
- `crap4ts init` subcommand generates a starter `crap4ts.toml`
  in the current directory. Auto-detects `src/`
  → `crates/` → falls back to `src` with a hint comment. Interactive
  by default (one prompt mapping `s|d|l` to strict/default/lenient
  preset); `--non-interactive` for CI; `--force` to overwrite an
  existing config. Emits TS-ecosystem exclude defaults
  (`node_modules/**`, `dist/**`, `coverage/**`). (#73)

### Fixed

- Threshold cutoffs are now calibrated per complexity metric instead
  of a single shared scalar. A cyclomatic count and a cognitive count
  have different magnitudes for the same function, so one cutoff
  cannot fit both — applying the cognitive cutoff to cyclomatic scores
  silently mis-gated. The strict/default/lenient presets now resolve
  to cyclomatic `8/16/30` or cognitive `15/25/40` based on the
  effective metric. User-visible behavior change: `crap4ts` with no
  threshold flag now gates at `16` (was `25`), `--strict` at `8`
  (was `15`), `--lenient` at `30` (was `40`). The generated
  `crap4ts.toml` threshold comment now states which metric the
  printed cutoffs apply to. (#218)

### Changed

- Config-file `threshold = N` now takes precedence over
  `preset = "..."` when both are set in the same `crap4ts.toml`.
  This makes config-file resolution consistent with
  CLI semantics, where an explicit `--threshold N` already overrides
  `--strict` / `--lenient`. Users who had both fields set will now get
  the literal value; previously the preset silently won. The
  `init`-generated config never writes both, so the blast radius is
  limited to hand-edited configs. (#218)
- Istanbul `coverage-final.json` parser now models only the
  fields it actually consumes (`path`, `s`, `statementMap.start.line`,
  `b`, `branchMap`). Unconsumed fields (`f`/`fnMap`, statement/branch
  `end` positions, `column`, branch `type`) are no longer deserialized,
  so emitter-side `null` or shape drift in those fields can no longer
  abort the whole-file parse. Forward-looking: every captured jest 29 /
  vitest-istanbul 4 / nyc 17 / c8 10 fixture already parsed cleanly, so
  no current producer triggered this; the change removes a latent
  whole-file-bail vector and locks the four producers as regression
  fixtures. (#214)
- Functions declared inside a TypeScript `namespace` now report with
  a namespace-qualified name — `Foo.bar`, `A.B.f` (dotted and
  block-nested forms both qualify), `Svc.Repo.find` for a class method
  inside a namespace — instead of the bare local name. This mirrors
  the existing class-method qualification (`C.m`) and changes the
  `function` field in the JSON envelope and the table/markdown
  reporters for any namespaced function. Qualification is shallow:
  only direct namespace members carry the prefix; functions nested
  inside them stay bare (`inner`, not `A.inner`), matching how
  class-nested functions already behave. Forward-looking: no
  first-party fixture corpus emits namespaced output today (the
  wire-snapshot corpus has no `namespace`), so no captured snapshot
  drifts; the change disambiguates namespace output and makes it
  consistent with class output ahead of crap4ts@2.0.0. (#221)

## [2.0.0-rc.2] - 2026-05-21

Corrective re-release of the `crap4ts` 2.x release candidate.
`2.0.0-rc.1` declared `"libc": ["glibc"]` in `package.json`; npm
evaluates `libc` on every platform, so the field blocked installation
on macOS (`EBADPLATFORM` — macOS has no glibc). `2.0.0-rc.1` is
deprecated on npm; the 48–72 h soak window restarts on this release.

### Fixed

- Removed the `"libc"` constraint from the npm package so the
  single-package multi-OS tarball installs on macOS as well as
  Linux/glibc. ([#242](https://github.com/breezy-bays-labs/crap-rs/issues/242))

## [2.0.0-rc.1] - 2026-05-19

First release candidate of the from-scratch `crap4ts` 2.x line —
a [napi-rs](https://napi.rs/) Node addon that replaces the
JavaScript-only `crap4ts` 1.x. CRAP formula, scorecard envelope, and
reporter shapes are now shared with the Rust adapter `crap4rs` via
the language-agnostic `crap-core` library. **48–72 h soak window
before promoting to `crap4ts 2.0.0` GA.**

This release cuts only the `crap4ts` npm package — `crap-core` and
`crap4rs` are not tagged to crates.io as part of this release. But
because the published cdylib is built from workspace `HEAD`, every
workspace-level change queued under [Unreleased] IS shipped inside
this cdylib, including:

- #214 — Istanbul parser narrowing
- #221 — namespace-qualified naming
- #218 — threshold metric calibration
- #73 — `init` subcommand

### Added

- New `crap4ts` npm package (v2.0.0-rc.1) shipping the napi-rs-built
  cdylib alongside a single-package runtime dispatcher. Exposes one
  `analyze({ sourceRoot, coveragePath, threshold?, metric? })`
  function returning the analysis output (functions + summary +
  diagnostics) as a JSON string. ([#192](https://github.com/breezy-bays-labs/crap-rs/issues/192))
- Native bindings for macOS arm64, macOS x64, and Linux x64 (glibc).
  All three live in the same tarball; `index.js` selects the right
  `.node` at require-time via `process.platform` + `process.arch`.
- `.github/workflows/publish.yml` — tag-triggered (`crap4ts-v*`)
  matrix workflow with `id-token: write` for npm OIDC trusted
  publishing (`npm publish --provenance`).
- `MIGRATION.md` gains a `crap4ts@1.x → crap4ts@2.0.0` section with
  the three reasons scores may diverge (threshold default `12 → 16`,
  TS-specific calibration not yet validated, arrow-function coverage
  handling) and subpath-export replacement recipes.

### Migration

See `MIGRATION.md` "crap4ts@1.x → crap4ts@2.0.0" section. Short
version: a `2.0.0-rc.1` install replaces a `1.x` install; scores may
differ for the three compounding reasons documented there.

## [2.0.0-alpha.1] - 2026-05-10

Initial extraction from the `crap4rs` workspace.

### Added

- `crap4ts` 2.0.0-alpha.1 — TypeScript adapter shell crate
  scaffolding the napi-rs `cdylib` + Rust `bin` surface for the future
  Node.js / TypeScript binding. Walker and Istanbul coverage parser
  are stub `unimplemented!()` adapters; the real walker pipeline ships
  in a future pipeline. **NOT published to crates.io or npm**
  (`package.json` is `"private": true` and release-publishing is
  disabled). See PR
  [#153](https://github.com/breezy-bays-labs/crap4rs/pull/153). The
  v1.x line of the legacy TypeScript implementation at
  [`breezy-bays-labs/crap4ts`](https://github.com/breezy-bays-labs/crap4ts)
  enters maintenance-only mode.

[Unreleased]: https://github.com/breezy-bays-labs/crap-rs/compare/crap4ts-v2.0.0-rc.3...HEAD
[2.0.0-rc.3]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/crap4ts-v2.0.0-rc.3
[2.0.0-rc.2]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/crap4ts-v2.0.0-rc.2
[2.0.0-rc.1]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/crap4ts-v2.0.0-rc.1
[2.0.0-alpha.1]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/crap4ts-v2.0.0-alpha.1
