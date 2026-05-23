# Migration Guide

Per-release migration notes for `crap4rs`, `crap4ts`, and `crap-core`
consumers. Read the section that matches how you consume the tool.

---

## crap4ts@1.x → crap4ts@2.0.0 (npm)

The 2.x line is a from-scratch reimplementation of crap4ts on top of
[`crap-rs`](https://github.com/breezy-bays-labs/crap-rs)'s Rust core,
distributed as a [napi-rs](https://napi.rs/) Node addon. The CRAP
formula, scorecard envelope, and reporter shapes are now shared
verbatim with the Rust adapter `crap4rs` via the language-agnostic
`crap-core` library.

`2.0.0-rc.1` is the first published version of the 2.x line. There is
a 48–72 h soak window before `2.0.0` is cut.

### Install

```sh
npm install --save-dev crap4ts@2.0.0-rc.1
# or pnpm add -D crap4ts@2.0.0-rc.1
```

Published platforms in `2.0.0-rc.1`:
- macOS arm64 + x64
- Linux x64 (glibc)

Windows and Linux arm64 / musl are tracked for a later release.

### Why are my scores different?

`crap4ts@2.x` scores can diverge from `crap4ts@1.x` scores on the same
codebase. Two compounding reasons account for the gap (a third was
resolved in 2.0.0 — see below); each is documented so you can
self-diagnose rather than chase a phantom regression:

1. **The default threshold changed from `12` to `16`** (cyclomatic). This
   is an intentional calibration introduced with `crap-rs` v0.5.0 —
   `8 / 16 / 30` for `strict / default / lenient` on the cyclomatic
   metric. v1.x's `12` was an undocumented intermediate. If you depend
   on the v1.x default, pin it explicitly: `analyze({ threshold: 12, ... })`.
2. **TS-specific threshold calibration has not been independently
   validated.** The current cutoffs derive from the Rust corpus
   (`crap4rs` history). A `type:research` follow-up validates them
   against the `crap4ts@1.x` test corpus; until that completes, treat
   the gate as a guideline rather than a ground truth. Track at
   [`crap-rs`#173](https://github.com/breezy-bays-labs/crap-rs/issues/173).

**Resolved in 2.0.0 — arrow-function coverage now reads correctly.**
v2's Istanbul adapter consumes `s` / `statementMap` (not `f` / `fnMap`,
which has known variance across emitters). Earlier 2.0.0 release
candidates emitted one `LineCoverage` record per Istanbul statement,
so a single source line carrying both a `const` declaration and its
arrow body produced two records — and the matcher's per-function
rollup treated a never-invoked arrow as ~50% covered (the
declaration's module-load hit dominated the body's zero-hit signal).
`2.0.0` collapses multi-statement-per-line via `min(hits)`, matching
the implicit per-line contract the LCOV adapter already obeys. The
practical effect across the `crap4ts@1.x` parity corpus is that **v2
now reports more accurate coverage than v1 did for any function whose
body shares a source line with its declaration** — including
single-line arrows, single-line function expressions, and inline
`xs.map(arrow)` patterns. Both directions of v1-vs-v2 movement
classify as improvements in the parity harness: v2 reports **lower**
coverage when a genuinely uninvoked body was previously masked by its
declaration's module-load hit, and v2 reports **higher** coverage
when phantom duplicate per-line records previously inflated the
denominator. Tracked: [`crap-rs`#252](https://github.com/breezy-bays-labs/crap-rs/issues/252).

### Subpath export removals

`crap4ts@1.x` exposed four module entries; `crap4ts@2.x` ships a
single `analyze()` export from the package root. Replacement recipes:

| v1.x import | v2.x replacement |
|---|---|
| `import { computeCrap } from 'crap4ts/formula'` | Call `analyze()` and read `summary.crap_scores` fields from the returned JSON. |
| `import { extractComplexity } from 'crap4ts/complexity'` | Not externally exposed in 2.x. The walker is internal; if you need raw complexity data, run the `crap4ts` CLI with `--format json` and parse the per-function entries. |
| `import { parseLcov } from 'crap4ts/coverage'` | Not externally exposed in 2.x. crap4ts 2.x parses Istanbul JSON only; use the `crap4rs` Rust binary if you need LCOV. |

The 1.x subpath consumer count is functionally zero (per the npm
registry — v1.0.1 has no measurable install base), so the surface
narrowing is a net simplification rather than a breaking change for
real workloads.

### Usage

```js
const { analyze } = require('crap4ts');

const json = analyze({
  sourceRoot: 'src',
  coveragePath: 'coverage/coverage-final.json',
  // Optional:
  // threshold: 16,
  // metric: 'cyclomatic',
});

const { result, diagnostics } = JSON.parse(json);
console.log(result.summary);
```

Generate `coverage-final.json` via your test runner with Istanbul
coverage enabled — e.g. `jest --coverage`, `vitest --coverage`, or
`c8 --reporter=json`.

### Rollback

If `2.0.0-rc.1` reveals a blocking regression, pin to `crap4ts@1.0.1`
(the last 1.x release) until a corrective `2.0.0-rc.2` ships. The
`npm unpublish` window is 72 h from publish; if the issue surfaces
earlier, the broken RC can be pulled from the registry rather than
held alongside the fix.

---

## v0.5.0 (2026-05-10)

v0.5.0 is the workspace-extraction release: `crap-core` becomes a
language-agnostic foundation library and `crap4rs` narrows to the
Rust-specific adapter. **There are no required source changes for
existing v0.4 consumers** — backward-compatibility shim modules in
`crap4rs::*` preserve every v0.4 import path.

### For `cargo install crap4rs` consumers (CLI users)

**No action required.** The `crap4rs` binary's command-line surface is
unchanged in v0.5.0. `cargo install crap4rs` and `cargo binstall
crap4rs` continue to install a working binary; the binstall artifact
naming is unchanged.

**Repo rename:** the upstream repository renames from
`breezy-bays-labs/crap4rs` to `breezy-bays-labs/crap-rs` shortly after
v0.5.0 ships. GitHub publishes an HTTP redirect that keeps existing
URL references working for at least one year per GitHub's support
policy. If your install script hardcodes a release-asset URL like
`https://github.com/breezy-bays-labs/crap4rs/releases/download/...`,
the redirect handles it — no action required. Updating the URL to
`/crap-rs/` at your convenience is good hygiene but not blocking.

### For `cargo add crap4rs` consumers (library users)

**No required source changes at v0.5.0.** Every v0.4 path resolves
through a shim re-export. The following imports all keep compiling:

```rust
use crap4rs::domain::types::{RiskLevel, ContributorKind, AnalysisResult, ScoredFunction};
use crap4rs::domain::types::{ParseDiagnostic, AnalysisDiagnostics};
use crap4rs::ports::{ComplexityPort, CoveragePort, DiffPort, ParseOutput};
use crap4rs::core::{analyze, AnalyzeOptions, AnalysisOutput};
use crap4rs::cli::{run, parse_args, Args, FormatArg};
use crap4rs::adapters::reporters::{format_json, format_markdown, format_html};
use crap4rs::adapters::baseline::{load, BaselineSnapshot, BaselineError};
```

**Recommended future-proofing.** At v1.0 the shim modules will narrow.
Symbols that originally lived in `crap4rs` but now live in `crap-core`
will require a direct `crap-core` dependency. The full list of types
slated for narrowing is in the PR body for v0.5.0
([crap4rs#156](https://github.com/breezy-bays-labs/crap4rs/pull/156),
which closes [#138](https://github.com/breezy-bays-labs/crap4rs/issues/138)),
but the headline categories are:

| Category | v0.4 path | v0.5.0 path (still works) | Recommended v0.5.0+ path |
|---|---|---|---|
| CRAP formula | `crap4rs::domain::crap::compute_crap` | same, via shim | `crap_core::domain::crap::compute_crap` |
| Domain types (RiskLevel, ContributorKind, AnalysisResult, etc.) | `crap4rs::domain::types::*` | same, via shim | `crap_core::domain::types::*` |
| Port traits | `crap4rs::ports::{ComplexityPort, CoveragePort, DiffPort}` | same, via shim | `crap_core::ports::*` |
| Orchestrator | `crap4rs::core::{analyze, AnalyzeOptions}` | same, via shim | `crap_core::core::*` |
| CLI dispatch | `crap4rs::cli::*` | same, via shim | `crap_core::cli::*` |
| Reporters (markdown, HTML, SARIF, CSV, scorecard-row, advice) | `crap4rs::adapters::reporters::*` | same, via shim | `crap_core::adapters::reporters::*` |
| Baseline / config / diff adapters | `crap4rs::adapters::{baseline, config, diff}::*` | same, via shim | `crap_core::adapters::{baseline, config, diff}::*` |

**Migration recipe.**

```toml
# Cargo.toml — add crap-core alongside crap4rs.
[dependencies]
crap4rs  = "0.5"
crap-core = "0.1"
```

```rust
// Before (v0.4 paths, still work via shim):
use crap4rs::domain::types::RiskLevel;
use crap4rs::ports::ComplexityPort;

// After (recommended for future-proofing):
use crap_core::domain::types::RiskLevel;
use crap_core::ports::ComplexityPort;
```

Stay on `crap4rs::adapters::{complexity, coverage}::*` for anything
LCOV- or syn-specific (those are intentionally Rust-only and stay in
`crap4rs`).

**One v0.4-name rename to be aware of** (the alias drops at v1.0):

| v0.4 name | v0.5 canonical name | v0.5 alias |
|---|---|---|
| `crap4rs::domain::types::ParseDiagnostic` | `crap4rs::parse_diagnostic::LcovParseDiagnostic` | `crap4rs::domain::types::ParseDiagnostic = LcovParseDiagnostic` (re-exported) |

If you reference `ParseDiagnostic` by name in `crap4rs::domain::types`,
either keep using the alias (it works in v0.5.x) or switch to
`crap4rs::parse_diagnostic::LcovParseDiagnostic` (or
`crap_core::ports::ParseDiagnostic` — the trait, distinct from the LCOV
concrete impl).

**Generic-parameter exposure at v1.0.** Some shim types in v0.5 are
type aliases that hide the `<P: ParseDiagnostic>` parameter introduced
in S2 (#134) by concretizing it to `LcovParseDiagnostic`:

- `crap4rs::domain::types::AnalysisDiagnostics` (shim alias)
  → `crap_core::domain::types::AnalysisDiagnostics<P>`
- `crap4rs::ports::ParseOutput` (shim alias)
  → `crap_core::ports::ParseOutput<P>`
- `crap4rs::core::AnalysisOutput` (shim alias)
  → `crap_core::core::AnalysisOutput<P>`
- `crap4rs::adapters::baseline::BaselineSnapshot` (shim alias)
  → `crap_core::adapters::baseline::BaselineSnapshot<P>`
- `crap4rs::adapters::reporters::JsonConfig<'a>` (shim alias)
  → `crap_core::adapters::reporters::json::JsonConfig<'a, P>`

At v1.0 the `<P>` parameter will be visible to downstream consumers
that import these types from `crap_core` directly. If you're writing
new code, prefer the generic form against `crap_core::ports::ParseDiagnostic`
as the bound — you can still concretize to `LcovParseDiagnostic` for
the Rust adapter.

### For hardcoded `breezy-bays-labs/crap4rs` URL references

Many downstreams reference the repo by its URL — `mokumo` references
`breezy-bays-labs/crap4rs/.github/actions/scorecard@v0.5.0` as a
composite GitHub Action, READMEs link to issues, badges point at the
Actions tab. GitHub's repo-rename auto-redirect carries all of these
for at least one year:

- `git clone https://github.com/breezy-bays-labs/crap4rs.git` redirects
  to the new repo.
- `uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@v0.5.0` in
  GitHub Actions resolves through the redirect.
- README links to `https://github.com/breezy-bays-labs/crap4rs/...`
  redirect to the new URL.

**No immediate action is required**, but it is good hygiene to update
hardcoded URLs to `breezy-bays-labs/crap-rs` within the next year. An
example downstream update (mokumo-style):

```diff
# .github/workflows/quality.yml
 scorecard:
-  uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@v0.5.0
+  uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@v0.5.0
```

```diff
# README.md
-[CI](https://github.com/breezy-bays-labs/crap4rs/actions/workflows/ci.yml/badge.svg)
+[CI](https://github.com/breezy-bays-labs/crap-rs/actions/workflows/ci.yml/badge.svg)
```

The crates.io package name `crap4rs` is unchanged — the package is the
unit of crates.io identity, the repo is just metadata. `cargo add
crap4rs` keeps resolving exactly as before; only the GitHub-side URL
changes.

---

## Cross-references

- `CHANGELOG.md` — full v0.5.0 release notes (Added / Changed / Looking
  ahead).
- `release-checklist.md` — operational steps for cutting future releases.
- crap4ts repo (`breezy-bays-labs/crap4ts`) — the v1.x-maintenance-only
  TypeScript implementation; the v2.0 successor lives in this workspace
  as the alpha `crates/crap4ts/` shell.
