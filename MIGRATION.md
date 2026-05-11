# Migration Guide

Per-release migration notes for `crap4rs` consumers. Read the section
that matches how you consume the tool.

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
([crap4rs#138](https://github.com/breezy-bays-labs/crap4rs/pull/<S6>)),
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
