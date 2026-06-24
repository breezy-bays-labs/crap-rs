# Changelog

All notable changes to `crap4rs` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.1](https://github.com/breezy-bays-labs/crap-rs/compare/crap4rs-v0.6.0...crap4rs-v0.6.1) - 2026-06-24

### Added

- *(core)* View: subtitle line in the terminal table reporter ([#408](https://github.com/breezy-bays-labs/crap-rs/pull/408))
- *(core)* #397 — breakdown collapsible on sticky scorecards (rendered below the table) ([#398](https://github.com/breezy-bays-labs/crap-rs/pull/398))
- *(core)* #275 — markdown sticky-comment marker + breakdown collapsibles ([#395](https://github.com/breezy-bays-labs/crap-rs/pull/395))
- *(core)* #393 — metric-mismatch guard on both delta paths (warn + disabled Delta tab) ([#394](https://github.com/breezy-bays-labs/crap-rs/pull/394))
- *(core)* #379 — carry effective epsilon on the wire so crap-render's combined panel shows an active band ([#391](https://github.com/breezy-bays-labs/crap-rs/pull/391))
- *(core+cli)* #277 — configurable threshold-border epsilon (delta jitter suppression) ([#376](https://github.com/breezy-bays-labs/crap-rs/pull/376))
- *(core)* #274 — Renamed delta variant (relocation-aware delta gate) ([#375](https://github.com/breezy-bays-labs/crap-rs/pull/375))
- *(core+cli)* #278 — configurable missing-coverage policy (pessimistic|optimistic|skip) ([#374](https://github.com/breezy-bays-labs/crap-rs/pull/374))
- *(core+ci)* #260 — complete CRAP-8 campaign + flip production gate to strict-8 ([#372](https://github.com/breezy-bays-labs/crap-rs/pull/372))
- *(core+cli)* config loader hardening (#339 #340 #341 #342) ([#361](https://github.com/breezy-bays-labs/crap-rs/pull/361))
- *(adapters)* #283 — nested-mod qualified function names in the walker ([#354](https://github.com/breezy-bays-labs/crap-rs/pull/354))
- *(core)* #348 — multi-language [language.*] config schema + [output] title/subtitle + public ConfigSchema ([#355](https://github.com/breezy-bays-labs/crap-rs/pull/355))
- *(core+cli)* #345 — unified crap.toml config name + dual-discovery back-compat ([#349](https://github.com/breezy-bays-labs/crap-rs/pull/349))
- *(core+ci)* #336 — base-gated multi-root src + separate production scorecard from examples dogfood ([#344](https://github.com/breezy-bays-labs/crap-rs/pull/344))
- *(ci)* #314 — wire-envelope publication to release assets + smoke flip to baseline mode ([#332](https://github.com/breezy-bays-labs/crap-rs/pull/332))
- *(crap-core+ci)* #326 — Current/Delta View axis on multi-language unified HTML report ([#327](https://github.com/breezy-bays-labs/crap-rs/pull/327))
- *(crap-core+ci)* #315 — unified multi-language HTML report with per-language toggle ([#318](https://github.com/breezy-bays-labs/crap-rs/pull/318))

### Other

- *(crap4rs)* #434 — property tests for the LCOV path-normalization seam ([#439](https://github.com/breezy-bays-labs/crap-rs/pull/439))
- *(crap4rs)* #429 — bolero fuzz harness + syn-walker target (Q4 walking skeleton) ([#435](https://github.com/breezy-bays-labs/crap-rs/pull/435))
- *(core)* curate multi_root_src.feature — wire identity-resolution CLI contracts, keep in-process coverage ([#425](https://github.com/breezy-bays-labs/crap-rs/pull/425))
- *(core)* curate config_discovery + config_schema — delete specs owned by lower levels ([#424](https://github.com/breezy-bays-labs/crap-rs/pull/424))
- *(core)* curate format_advice.feature — wire advice CLI contracts, push coverage to lib units ([#421](https://github.com/breezy-bays-labs/crap-rs/pull/421))
- *(core)* curate saved_view_presets.feature — wire --view CLI contracts, delete integration ([#420](https://github.com/breezy-bays-labs/crap-rs/pull/420))
- *(core)* curate sarif_reporter.feature — wire SARIF CLI contracts, delete integration ([#419](https://github.com/breezy-bays-labs/crap-rs/pull/419))
- *(core)* curate group_by_file.feature — wire --group-by CLI contracts, delete integration ([#418](https://github.com/breezy-bays-labs/crap-rs/pull/418))
- *(core)* curate complexity_breakdown.feature — wire --breakdown/--explain CLI contracts ([#417](https://github.com/breezy-bays-labs/crap-rs/pull/417))
- *(core)* delete table_reporter.feature — rendering exhaustively unit-owned ([#416](https://github.com/breezy-bays-labs/crap-rs/pull/416))
- *(core)* curate diff_mode.feature — add --diff CLI-acceptance layer (keep integration for coverage) ([#415](https://github.com/breezy-bays-labs/crap-rs/pull/415))
- *(core)* curate delta.feature CLI contracts (slice 2/2) — shaping/validation/help, delete integration files ([#414](https://github.com/breezy-bays-labs/crap-rs/pull/414))
- *(core)* curate delta.feature CLI contracts (slice 1/2) — wire gate/reporter/envelope, push domain down ([#413](https://github.com/breezy-bays-labs/crap-rs/pull/413))
- *(core)* curate cli_ergonomics --help/View-line/exit + result-invariant proptest ([#412](https://github.com/breezy-bays-labs/crap-rs/pull/412))
- *(core)* curate --no-fail/--quiet + Story B BDD coverage — wire the CLI-process gaps, delete duplicates ([#411](https://github.com/breezy-bays-labs/crap-rs/pull/411))
- *(core)* curate --min/--max-coverage BDD coverage — wire envelope + validation, delete view.rs-owned filter semantics ([#409](https://github.com/breezy-bays-labs/crap-rs/pull/409))
- *(core)* curate --sort-by BDD coverage — wire the envelope echo, delete view.rs-owned orderings ([#407](https://github.com/breezy-bays-labs/crap-rs/pull/407))
- *(core)* curate --top BDD coverage — wire CLI contracts, delete view.rs-owned details ([#406](https://github.com/breezy-bays-labs/crap-rs/pull/406))
- *(core)* curate view.feature — delete spec, lift filters-AND-compose to a proptest ([#405](https://github.com/breezy-bays-labs/crap-rs/pull/405))
- *(core)* assert AnalyzeOptions default coverage_metric is Line ([#404](https://github.com/breezy-bays-labs/crap-rs/pull/404))
- #290 — vary github-annotations fixture nesting depth so CRAP scores are distinct ([#396](https://github.com/breezy-bays-labs/crap-rs/pull/396))
- *(crap4rs)* #262 — rewrite cli_init.feature to the #347 init contract + gate 4 wired cucumber harnesses ([#363](https://github.com/breezy-bays-labs/crap-rs/pull/363))
- *(bdd)* #280 — enforce one-status-tag-per-scenario + backfill delta.feature ([#362](https://github.com/breezy-bays-labs/crap-rs/pull/362))
- #331 — analyzer resolves workspace-relative coverage paths in both adapters ([#333](https://github.com/breezy-bays-labs/crap-rs/pull/333))

### Added

- `--format markdown` output now leads with a hidden
    `<!-- crap4rs:scorecard -->` HTML comment — invisible in rendered
    GFM, but a stable dedupe anchor sticky-PR-comment tooling can match
    on to update its existing comment instead of posting a new one per
    push. The marker carries the adapter name, so a Rust scorecard and
    a TypeScript scorecard can sticky to separate comments on the same
    PR. With `--breakdown`, the complexity-contributor bullets of the
    above-threshold functions render inside one collapsed
    `<details>` block below the scorecard table, keeping the default
    PR-comment view compact while keeping the markdown table itself
    intact (a `<details>` placed between table rows would terminate the
    GFM table). (crap-rs#275, crap-rs#397)

### Changed

- Config auto-discovery now walks **upward** from the `--src` anchor
    (or the working directory when `--src` is omitted): `crap4rs --src
    crates/foo` run from a repo root discovers the repo-root
    `crap.toml`, and `cd crates && crap4rs --src foo` discovers a
    `crap.toml` one level up. The previous behavior inspected only the
    working directory. Pass an explicit `--config <path>` to bypass
    discovery. Note: the walk has no `.git`/workspace stop, so a stray
    `crap.toml` in `$HOME` (or any ancestor) is discovered when no
    nearer config exists. (crap-rs#339; see `crap-core` CHANGELOG.)

## [0.6.0](https://github.com/breezy-bays-labs/crap-rs/compare/crap4rs-v0.5.0...crap4rs-v0.6.0) - 2026-05-24

### Added

- `--format github-annotations` emits GitHub Actions `::warning`
  workflow commands so threshold-exceeding functions render inline on
  the PR Files Changed tab — universal, free, no GHAS / Code Scanning
  dependency. Like SARIF, this is a gate translation: the reporter
  iterates the unshaped `view.full.functions` regardless of `--top` /
  `--sort-by` / `--only-failing`. New `--annotation-limit N` (u32,
  range 1..=100, default 10) caps emission to match the GH Actions
  per-step UI cap; over-cap eligible findings surface as a trailing
  `::notice::N more functions exceed threshold; see scorecard for the
  full list` line. Also configurable via `[output] annotation_limit`
  in `crap4rs.toml`. The composite scorecard action gained matching
  `annotations` (bool) and `annotation-limit` inputs for opt-in
  inline rendering. ([#288](https://github.com/breezy-bays-labs/crap-rs/pull/288))
- `crap4rs init` subcommand generates a starter `crap4rs.toml` in the
  current directory. Auto-detects `src/` → `crates/` → falls back to
  `src` with a hint comment. Interactive by default (one prompt
  mapping `s|d|l` to strict/default/lenient preset);
  `--non-interactive` for CI; `--force` to overwrite an existing
  config. Emits Rust-ecosystem exclude defaults (`tests/**`,
  `benches/**`, `examples/**`). Inherited by `crap4ts` via
  `AdapterMeta`. ([#171](https://github.com/breezy-bays-labs/crap-rs/pull/171))
- `--summary` flag emits a single-line analysis verdict to stdout
  (e.g. `PASS: 1082 functions | 0 above threshold (25) | worst: 13.0
  | avg: 1.6`), matching crap4ts's `formatSummaryLine` byte-for-byte.
  Short-circuits `--format`, composes with `--no-fail` (exit 0
  always, summary emitted) and `--quiet` (quiet wins — no output).
  Closes the 2026-05-08 crap4rs ↔ crap4ts parity audit's final gap.
  ([#167](https://github.com/breezy-bays-labs/crap-rs/pull/167))
- *(crap-core)* askama-templated HTML + markdown reporters + Sakura
  HTML redesign. ([#307](https://github.com/breezy-bays-labs/crap-rs/pull/307))
- *(ci)* adopt release-plz + crates.io OIDC trusted publishing. ([#301](https://github.com/breezy-bays-labs/crap-rs/pull/301))
- Composite scorecard action dispatches crap4ts + cross-adapter
  scorecard-row parity. ([#282](https://github.com/breezy-bays-labs/crap-rs/pull/282))
- *(crap-core)* [**breaking**] align CRAP thresholds + risk tiers
  (8/15/25). ([#281](https://github.com/breezy-bays-labs/crap-rs/pull/281))
- BDD tracked-comment lint + `delta.feature` spec + `colored
  set_override` poison fix. ([#279](https://github.com/breezy-bays-labs/crap-rs/pull/279))
- *(crap-core)* `CrapError::MetricNotSupported` +
  `AdapterMeta::default_metric` + crap4ts walker check + supporting
  ADR + crap-core 0.2.0. ([#203](https://github.com/breezy-bays-labs/crap-rs/pull/203))
- *(crap-core)* [**breaking**] restore `#[non_exhaustive]` on 15
  result structs (v1.0 prep). ([#166](https://github.com/breezy-bays-labs/crap-rs/pull/166))
- *(crap-core)* `AdapterMeta` + decouple from Rust/crap4rs. ([#162](https://github.com/breezy-bays-labs/crap-rs/pull/162))

### Fixed

- Threshold cutoffs are now calibrated per complexity metric instead
  of a single shared scalar. A cyclomatic count and a cognitive count
  have different magnitudes for the same function, so one cutoff
  cannot fit both — applying the cognitive cutoff to cyclomatic
  scores silently mis-gated. The strict/default/lenient presets now
  resolve to cyclomatic `8/16/30` or cognitive `15/25/40` based on
  the effective metric. User-visible behavior change: `crap4rs
  --metric cyclomatic` with no flag now gates at `16` (was `25`) and
  `--strict` at `8` (was `15`). `crap4rs`'s cognitive defaults (the
  common path) are unchanged. The generated `crap4rs.toml` threshold
  comment now states which metric the printed cutoffs apply to.
  ([#281](https://github.com/breezy-bays-labs/crap-rs/pull/281))
- *(crap4ts)* skip `.d.ts` declaration files via
  `AdapterMeta::forced_excludes`. ([#258](https://github.com/breezy-bays-labs/crap-rs/pull/258))
- Adapter-aware coverage preflight + walker reconciliation +
  config-path hint. ([#164](https://github.com/breezy-bays-labs/crap-rs/pull/164))
- *(crap-core)* late-bind coverage adapter via factory closure. ([#158](https://github.com/breezy-bays-labs/crap-rs/pull/158))

### Changed

- Multi-format `--format` invocations now permit a single stdout
  entry alongside file-targeted entries (previously every entry had
  to specify a file). Two or more stdout entries are still rejected —
  the underlying "cannot multiplex stdout" rule stands. Unblocks the
  intended composite-CI shape `--format
  markdown:scorecard.md,github-annotations`, where the markdown is
  captured as a workflow artefact and the `github-annotations`
  workflow commands flow through to the GH Actions runner. ([#288](https://github.com/breezy-bays-labs/crap-rs/pull/288))
- Config-file `threshold = N` now takes precedence over `preset =
  "..."` when both are set in the same `crap4rs.toml`. This makes
  config-file resolution consistent with CLI semantics, where an
  explicit `--threshold N` already overrides `--strict` /
  `--lenient`. Users who had both fields set will now get the literal
  value; previously the preset silently won. The `init`-generated
  config never writes both, so the blast radius is limited to
  hand-edited configs. ([#281](https://github.com/breezy-bays-labs/crap-rs/pull/281))

### Other

- Per-crate READMEs for crates.io display. ([#305](https://github.com/breezy-bays-labs/crap-rs/pull/305))
- *(ci)* AST-purity layer-4 adapter-vocabulary string-literal ban. ([#286](https://github.com/breezy-bays-labs/crap-rs/pull/286))
- *(crap-core)* port-surface cleanup (CrapError rename +
  `CoveragePort::parse` to `&Path`). ([#262](https://github.com/breezy-bays-labs/crap-rs/pull/262))
- Pre-flight scaffold for TS-adapter pipeline (gitignore +
  `delta.feature` relocation + BDD scenarios). ([#195](https://github.com/breezy-bays-labs/crap-rs/pull/195))
- *(bdd)* tag lexicon + retag scenarios + clean up Background. ([#170](https://github.com/breezy-bays-labs/crap-rs/pull/170))
- *(crap-core)* `AdapterMeta` polish + ci(ast-purity) adapter-name
  gate. ([#165](https://github.com/breezy-bays-labs/crap-rs/pull/165))

## [0.5.0] - 2026-05-10

The workspace-extraction milestone. `crap4rs` becomes one of three
crates in a workspace alongside the new language-agnostic
[`crap-core`](https://crates.io/crates/crap-core) library and the
alpha [`crap4ts`](https://github.com/breezy-bays-labs/crap4ts) shell
for the future TypeScript adapter. **No breaking changes for `cargo
install crap4rs` users**; **no required source changes for `cargo add
crap4rs` library users** — every v0.4 public path resolves through a
backward-compatibility shim re-export (per
[ADR D10](https://github.com/breezy-bays-labs/ops/tree/main/decisions/crap-rs)).

The CLI binary, output formats, JSON envelope schema (version 2,
unchanged), SARIF output, scorecard-row producer, and `/cut-the-crap`
agent skill all behave identically to v0.4.0. The wire-envelope
snapshot canary is byte-identical to the v0.4.0 baseline (the
language-agnostic adapters relocated to `crap-core` but emit the same
bytes).

The repository renames from `breezy-bays-labs/crap4rs` to
`breezy-bays-labs/crap-rs` shortly after this release ships. GitHub's
auto-redirect carries existing URL references for at least one year.
The crates.io package name `crap4rs` is unchanged.

### Added
- **Mixed dispatch architecture (ADR D9)** — generics on data
  containers (`AnalysisDiagnostics<P>`, `ParseOutput<P>`,
  `AnalysisOutput<P>`, `BaselineSnapshot<P>`, `JsonConfig<'a, P>`,
  `DeltaContext<'a, P>`); trait objects on port orchestration
  (`&dyn ComplexityPort`, `&dyn CoveragePort<Diagnostic = P>`); free
  functions for reporters preserved per
  [`adapters.md`](https://github.com/breezy-bays-labs/ops) rule 1
  (Reporter trait NOT introduced).
- **Backward-compat shim modules (ADR D10)** — `crap4rs::domain::*`,
  `crap4rs::ports::*`, `crap4rs::core::*`, `crap4rs::cli::*`, and
  `crap4rs::adapters::{baseline, config, diff, reporters}::*` are
  nested `pub mod` re-exports from `crap_core::*`. Type aliases
  concretize the `<P>` parameter to `LcovParseDiagnostic` so v0.4
  consumers' unparameterized usage keeps compiling.
- **`crap4rs::parse_diagnostic::LcovParseDiagnostic`** — the LCOV-
  specific concrete `ParseDiagnostic` impl, formerly named
  `crap4rs::domain::types::ParseDiagnostic`. The old path is preserved
  as a shim alias for v0.5.x; the alias drops at v1.0.

### Changed
- **MSRV: 1.88 → 1.93.** Building `crap4rs` from source now requires
  `rustc >= 1.93`. `cargo binstall crap4rs` and pre-built release
  artifacts are unaffected (they ship as binaries). The MSRV raise
  tracks `oxc 0.129`, which the future TypeScript walker pipeline
  needs; the v0.4 line held at `oxc 0.96` to keep MSRV at 1.88, but
  the workspace settled the tension by raising MSRV during the build
  phase (PR
  [#155](https://github.com/breezy-bays-labs/crap4rs/pull/155)).
- **`oxc` workspace pin: 0.96 → 0.129** (current at v0.5.0 ship).
  Used only by the `crap4ts` shell crate at present.
- **`crap4rs` package version: 0.4.0 → 0.5.0.** `crap-core` ships at
  `0.1.0`; `crap4ts` ships at `2.0.0-alpha.1` (not published).
- **Workspace layout.** Source files relocated:
  - `crap4rs/src/domain/` → `crap-core/src/domain/`
  - `crap4rs/src/ports/` → `crap-core/src/ports/`
  - `crap4rs/src/adapters/{reporters,baseline,config,diff}/` →
    `crap-core/src/adapters/{...}/`
  - `crap4rs/src/core/` → `crap-core/src/core/`
  - `crap4rs/src/cli/` → `crap-core/src/cli/`
  - `crap4rs/src/adapters/{complexity, coverage}/` stay in
    `crap4rs` (Rust-toolchain coupled; would fail the AST-purity gate
    in `crap-core`).
- **CI: self-CRAP runs twice** — once with `--src crates/crap-core/src`
  and once with `--src crates/crap4rs/src`, both gating PR merge.
  Mutation testing and BDD harness are split per crate. The wire-
  envelope canary stays in `crap-core`'s test surface and is
  intentionally excluded from `cargo mutants` runs.

### Looking ahead — v1.0 narrowings (file followups now)
The v0.5.0 shim re-exports preserve v0.4 paths but are explicitly
provisional. Library consumers planning past v0.5.x should anticipate:

- **Restored `#[non_exhaustive]` on 15+ result / diagnostic structs.**
  Paused during the extraction (S2 struct-literal init in `cli` /
  `core` / `adapters` blocked the attribute) and re-enabled at v1.0.
  Tracked in
  [#147](https://github.com/breezy-bays-labs/crap4rs/issues/147).
- **Shim re-exports narrow.** Symbols that originated in `crap4rs` but
  now live in `crap_core` (the domain types, port traits, reporters,
  baseline / config / diff adapters, orchestrator, CLI dispatch) are
  candidates for removal from `crap4rs::*`. Add `crap-core` as a
  direct dependency now and import from there to avoid the v1.0
  cliff. Full migration recipe in `MIGRATION.md`.
- **`crap4rs::domain::types::ParseDiagnostic` alias drops.** Use
  `crap4rs::parse_diagnostic::LcovParseDiagnostic` (concrete impl) or
  `crap_core::ports::ParseDiagnostic` (the trait) directly.
- **Type aliases concretizing `<P>` drop.** Aliases like
  `crap4rs::ports::ParseOutput`,
  `crap4rs::core::AnalysisOutput`,
  `crap4rs::domain::types::AnalysisDiagnostics`, and
  `crap4rs::adapters::baseline::BaselineSnapshot` hide the `<P:
  ParseDiagnostic>` parameter; at v1.0 the parameter is visible to
  consumers (concretize to `LcovParseDiagnostic` yourself or use a
  generic).
- **Rust-specific hardcoded strings in `crap_core::cli` parameterize
  per language adapter** (`.rs` extension check, "LCOV" / "Rust"
  diagnostic labels, clap `Box::leak` removal). Tracked in
  [#152](https://github.com/breezy-bays-labs/crap4rs/issues/152).
- **Tool-name parameterization in reporters** —
  [#148](https://github.com/breezy-bays-labs/crap4rs/issues/148).
- **LCOV-parser src divergence cleanup** —
  [#150](https://github.com/breezy-bays-labs/crap4rs/issues/150).

### Migration
See `MIGRATION.md` for the per-consumer migration recipe. TL;DR:

- **CLI users** (`cargo install crap4rs`): no action required.
- **Library users** (`cargo add crap4rs`): no required changes;
  recommended to add `crap-core = "0.1"` as a direct dependency and
  migrate `crap4rs::{domain, ports, core, cli, adapters::{baseline,
  config, diff, reporters}}::*` imports to `crap_core::*` to
  future-proof for v1.0.
- **Hardcoded `breezy-bays-labs/crap4rs` URLs** (workflows, READMEs):
  no action required; GitHub auto-redirect carries them >= 1 year.

## [0.4.0] - 2026-05-04

The agent-loop + multi-output milestone. Bundles 13 issues across three
review-passed PRs ([#124](https://github.com/breezy-bays-labs/crap4rs/pull/124),
[#125](https://github.com/breezy-bays-labs/crap4rs/pull/125),
[#126](https://github.com/breezy-bays-labs/crap4rs/pull/126)) plus the
post-0.3.0 follow-ups (`run_inner` CC reduction, scorecard composite-
action `outputs.row-json`, scorecard row-contract docs).

Three new output formats (`html`, `scorecard-row`, `advice`), multi-format
fan-out from one analysis pass (`--format json:env.json,markdown:r.md,html:r.html`),
the `/cut-the-crap` reference Claude Code skill, a cucumber-rs BDD harness,
honest end-to-end self-CRAP coverage (lifted `--exclude "cli/**"`), the
`AnalysisContext` decomposition of `core::analyze`, and full rustdoc on
`domain::view`.

JSON envelope schema bumped 1 → 2 — `ComplexityContributor.column` is now
1-based inclusive (matches `SourceSpan` + SARIF). v1 baselines remain
loadable for `--baseline` delta calculations.

**Relicensed to MIT OR Apache-2.0** (was GPL-3.0-or-later) — matches the
Rust ecosystem standard and removes copyleft friction for downstream
consumers, including the planned `crap-core` library extraction
(ops#231).

### Added
- **Multi-format output in a single run** — `--format` now accepts a
  comma-separated list with optional per-format file destinations:
  `--format json:envelope.json,markdown:report.md,html:report.html`.
  A single LCOV parse + syn walk + CRAP recompute fans out to every
  reporter, so CI no longer pays for repeated `crap4rs` invocations.
  Single-format invocations are unchanged (`--format json` still goes
  to stdout). Multi-format invocations require every entry to specify
  a file — stdout cannot multiplex. Closes #100.
- **`--format html`** emits a self-contained HTML dashboard with a
  summary block (totals, average/median/max CRAP, risk distribution,
  pass/fail badge), per-file collapsible function tables (native
  `<details>`/`<summary>`, no JS), and contributor breakdowns under
  every scored function. Inline CSS (no CDN, no external fonts), grid-
  based mobile-responsive layout, and color-coded risk levels matching
  the SARIF severity mapping. Closes #71.
- **`--format scorecard-row`** emits a single `Row::CrapDelta`
  JSON object for scorecard-aggregator consumption. Producer-mints
  status: Red on new threshold violations, Yellow on modified-function
  CRAP regression, Green otherwise. `--baseline <path>` integration
  carries the signed `delta_count`, the `delta_text` display string
  (e.g. `"5 → 7 (+2)"`), and a Red-only `failure_detail_md` listing
  violators sorted by CRAP descending. Schema round-trip pinned via
  fixture at `crates/crap4rs/tests/fixtures/scorecard/schema.json`.
  Closes #111.
- **`--format advice`** (experimental, schema_version=1) emits AST-derived
  remediation hints alongside the standard JSON envelope: a per-function
  `Diagnostic` with `coverage_gaps`, `complexity_drivers`,
  `suggested_actions` (`AddTestsForLines`, `ExtractFunction`,
  `SimplifyBranching`, `AcceptInherentComplexity`), and a flat
  `root_cause` scalar. A grep-friendly summary line per over-threshold
  function is written to stderr. Schema stabilises at v0.4.0. Closes #76.
- **SARIF `result.properties.diagnostic`** — when `--format sarif` is
  used, every result for an over-threshold verdict carries the same
  diagnostic shape as `--format advice` so GitHub Code Scanning
  consumers and the `/cut-the-crap` agent skill (#77) can read identical
  advice from either entry point. SARIF output stays byte-identical for
  runs without diagnostics.
- **`/cut-the-crap` reference Claude Code skill** — ships at
  `skills/cut-the-crap/` (install via
  `cp -r skills/cut-the-crap ~/.claude/skills/`). Consumes
  `crap4rs --format advice` and drives the cover-then-split remediation
  loop: write tests for `coverage_gaps` first when `root_cause:
  low_coverage`, name + apply the `recommended` `ProposedSplit` when
  `root_cause: high_complexity`, cover-then-split when
  `root_cause: both`. The skill emits a structured plan to
  `tmp/cut-the-crap-plan.md` before applying changes; `--explain-only`
  produces the plan without modifying code. crap4rs the binary stays a
  unix-style emitter — naming and the agent loop live in the skill.
  Closes #77.
- **Cucumber-rs harness for `tests/features/*.feature` files** —
  `cucumber = "0.22"` + `tokio` join `[dev-dependencies]`; the first
  migrated feature (`json_reporter.feature`, 12 scenarios / 59 steps)
  executes via `tests/json_reporter_cucumber.rs`. The test target uses
  `harness = false` (cucumber prints its own output) and
  `writer::Libtest::or_basic()` to remain IDE-friendly. Future feature
  files migrate one-at-a-time under the same naming convention
  (`<feature>_cucumber`). Step definitions deserialize fixture
  `AnalysisResult`s from inline JSON to stay outside `#[non_exhaustive]`
  struct-literal restrictions — no `pub(crate)` test-fixture exposure
  required. **CI integration**: `cargo nextest run --all-targets`
  excludes cucumber binaries via `.config/nextest.toml`
  (`default-filter = "not binary(/.*_cucumber$/)"`) because they don't
  speak libtest's `--list --format terse` protocol; cucumber tests run
  in a dedicated CI step (`cargo test --test json_reporter_cucumber`).
  **Migration policy**: existing `tests/*_integration.rs` files keep
  their assertions; cucumber adds Gherkin-driven coverage incrementally
  rather than replacing the integration suite wholesale. Closes #115.

### Tests
- Regression guard for the trait-default-impl boundary case (#116).
  `tests/fixtures/trait_default_override.rs` pairs a trait with a
  default `greet` body against a concrete `impl Greeter for Casual`
  override of the same method; the new
  `trait_default_and_concrete_override_have_disjoint_spans` test pins
  the walker invariant that emits two distinct `FunctionComplexity`
  entries with disjoint spans, and that contributors stay inside their
  own function's span. The walker structure
  (`visit_trait_item_fn` + `visit_impl_item_fn`) and the
  `is_viable_split` invariant
  (`range.start >= span.start_line && range.end <= span.end_line`)
  already prevent the hypothesised phantom split by construction;
  the test is the permanent guard. Closes #116.

### Changed
- `domain::types::ComplexityContributor` now records `end_line` and
  `nesting_depth` so domain helpers can answer span/nesting questions
  without re-walking the AST. Both fields default to `0` for
  forward-compat with older payloads.
- **Breaking — JSON envelope `schema_version` 1 → 2.**
  `ComplexityContributor.column` is now **1-based inclusive** (was
  0-based), matching `SourceSpan::start_column` / `end_column` and the
  SARIF spec. The bump is the wire signal that contributor column
  semantics shifted; consumers reading `column` should add `1` to v1
  values when comparing against v2. Baseline JSON files at v1 remain
  loadable for `--baseline` delta calculations (matching is identity-
  keyed, not column-keyed); `crap4rs --format json` now emits v2. The
  CLI rejects baseline envelopes with unknown `schema_version` and
  reports the accepted set (`[1, 2]`) in stderr. Closes #107.

- `core::analyze` is now a thin facade over a private
  `AnalysisContext` (`fn analyze(opts) -> AnalysisContext::new(opts).run()`).
  Phase methods (`discover_sources`, `load_diff_data`, `parse_coverage`,
  `extract_complexities`) hang off the context; diff-mode early-exit is
  surfaced via `Option<AnalysisOutput>` from
  `short_circuit_on_files` / `short_circuit_on_complexities` so the
  top-level `run()` body stays flat. Public API (`analyze`,
  `AnalyzeOptions`, `AnalysisOutput`) is unchanged; behavior is identical
  (every existing test passes unmodified). Closes #57.

- **Self-CRAP gate is honest again** — lifted `--exclude "cli/**"` from
  the self-referential CRAP step in `.github/workflows/ci.yml`. The
  exclusion was added during the M0 cli/ scaffolding sprint and outlived
  its tracking issue; with the recent `prepare_pipeline` /
  `count_cognitive_expr` splits (#121, #122) and `core::analyze`
  decomposition (#57), the worst CRAP across the entire codebase is
  13.0 — well under the strict-mode threshold of 15. The gate now
  exercises every shipped `.rs` file. Closes #109.

### Documentation
- **`domain::view` public-surface rustdoc** — the canonical pure-domain
  shaping primitive (`view::apply`) is now fully documented at the
  module level (input contract, pipeline order
  `filter → group? → sort → truncate`, gate-keystone invariant,
  `should_render_view_line` predicate, `#[non_exhaustive]` extension
  policy, `crap-core` extraction note) and per public item:
  `ViewSpec`, `Filters`, `CoverageRange`, `SortKey`, `GroupKey`,
  `GroupedView`, `AnalysisView`, `apply`, `should_render_view_line`,
  `CoverageRangeError`. No code change. Closes #94.

## [0.3.0] - 2026-04-27

The SARIF + API hardening milestone. Three breaking-but-paid-once changes
ship together so v0.3.x can stay additive: SARIF v2.1.0 output with
column-precise regions for GitHub Code Scanning (#70 / #105), the
`FunctionVerdict.diagnostic` placeholder slot for the `/cut-the-crap`
agent skill (#82), and `#[non_exhaustive]` on every public envelope type
so future field additions land as minor releases.

### Added
- **`--format sarif`** — emit SARIF v2.1.0 JSON for GitHub Code Scanning. Each function whose CRAP score exceeds the threshold becomes a SARIF `result` with `ruleId: "crap/threshold-exceeded"`, severity mapped from risk level (high → `error`, moderate → `warning`, acceptable & low → `note`), file path + start/end line, and a `partialFingerprints.functionIdentity` for cross-run dedup. SARIF output is a *gate translation*: results derive from the unshapeable analysis, so display flags (`--top`, `--sort-by`, `--only-failing`) do **not** alter SARIF output. `--no-fail` overrides the exit code only — the `results[]` array still reports every finding so PR annotations stay truthful. Pipe stdout into a `.sarif` file and upload via `github/codeql-action/upload-sarif@v3`. Closes #70.
- **Column-precise SARIF regions** — `domain::types::SourceSpan` gains additive `start_column` / `end_column` fields (1-based; `0` means "unknown"). The Rust complexity adapter populates them from `proc_macro2::Span`, and `--format sarif` emits `region.startColumn` / `endColumn` only when both columns are known. GitHub Code Scanning now underlines the exact function range in the PR diff instead of highlighting the whole line. Adapters without column data (diff hunks) emit `0`, and the reporter omits the column keys to keep half-truths off the wire. JSON envelope `schema_version` is unchanged (additive via `#[serde(default)]`). Closes #105.
- **`FunctionVerdict.diagnostic` slot** — additive `Option<Diagnostic>` field on every verdict, where `Diagnostic { summary: String }` is a placeholder for structured remediation hints. Populated later by `--format advice` (#76) and consumed by the `/cut-the-crap` reference Claude Code skill (#77). For default invocations the field is `None` and `serde` omits it from JSON output via `skip_serializing_if`, so existing consumers and snapshots are unaffected. `Diagnostic` is `#[non_exhaustive]` (so #76 can grow `kind`, `action`, line spans, and confidence additively in v0.3.x) **and** derives `Default`, so external consumers building advice in their own pipelines can use `Diagnostic { summary: "...".into(), ..Default::default() }`. Closes #82.

### Changed
- **API hardening: public envelope types are now `#[non_exhaustive]`.** `SourceSpan`, `FunctionIdentity`, `ComplexityContributor`, `CrapScore`, `ScoredFunction`, `FunctionVerdict`, `RiskDistribution`, `AnalysisSummary`, `AnalysisResult`, and `AnalysisDiagnostics` gain the `#[non_exhaustive]` attribute. Future field additions ship as minor (additive) releases instead of forcing successive major bumps. **Impact for external consumers:** the supported way to obtain instances of these types from outside the crate is to deserialize the JSON envelope produced by the CLI — that envelope is the public contract. External struct-literal construction is **intentionally restricted in v0.3.0** because line/column 0 is not a meaningful default for `SourceSpan` and there is no semantically valid empty `FunctionIdentity`/`ScoredFunction` either. The placeholder type `Diagnostic` is the deliberate exception — it derives `Default` so consumers writing advice can spread via `..Default::default()`. If you have a use case that requires constructing one of these envelope types programmatically (e.g., generating fixtures in a downstream test), file an issue and we will add a `new()` constructor or selective `Default` impl in a follow-up minor release. Field reads, `Debug`, exhaustive matches via `..` rest pattern (where you already hold an instance to match against), and serde round-tripping are unaffected. CLI behavior, JSON schema, and SARIF output are unchanged.

## [0.2.2] - 2026-04-26

### Changed
- **Markdown reporter is now summary-first.** `--format markdown` renders a compact title + multi-metric summary block (CRAP / Complexity / Coverage stats with worst, average, median + risk distribution) followed by a top-N spotlight: failures sorted CRAP-desc when violations exist, otherwise the worst-by-CRAP slice on a clean run. Designed to fit in a PR comment regardless of codebase size — a 1099-function self-analysis renders in ~1.4 KB (down from ~92 KB). The legacy row-per-function table is preserved behind `--md-full-table`.

### Added
- `--md-full-table` flag in the Display group: append the legacy row-per-function table after the summary. Useful when piping `--format markdown` into a longer document instead of a PR comment.
- `--md-top N` flag: bound the markdown spotlight table size (default 10). The summary block is unaffected — its stats always reflect the full unshapeable analysis.
- `AnalysisSummary` now carries Complexity stats (`max_complexity`, `average_complexity`, `median_complexity`) and Coverage stats (`min_coverage`, `average_coverage`, `median_coverage`) alongside the existing CRAP stats. JSON envelope additions are non-breaking; `schema_version` stays at `1`. Older baseline JSON deserializes cleanly via `#[serde(default)]`.

### Fixed
- **`cargo binstall crap4rs`** now extracts the pre-built binary instead of falling back to source build. The `[package.metadata.binstall]` `bin-dir` template was `"."`, which resolves to an empty source path under cargo-binstall ≥ 1.x; corrected to `"{ bin }{ binary-ext }"`. Closes #101.

## [0.2.1] - 2026-04-26

Patch release addressing CodeRabbit review feedback on the v0.2.0
baseline-comparison capstone (#97). No new features, no API changes —
correctness, determinism, and test-fidelity fixes.

### Fixed
- **Markdown delta scorecard suppresses sub-cent regressions** — the Regressions table filter now matches the `{:.2}` cell-rendering precision (`>= 0.005`), so functions whose CRAP delta rounds to `+0.00` no longer leak into the human-facing scorecard. Programmatic consumers (JSON, CSV) are unaffected.
- **Removed-row order is deterministic** — `domain::delta::compute` now sorts leftover baseline entries by `(file_path, qualified_name)` before emitting them as `Removed` changes. The previous code relied on `HashMap` iteration order, which is unspecified in Rust and produced run-to-run flakiness for consumers iterating `delta.changes` directly. Identity-key sort is cheap and gives a stable presentation order matching operator expectations.
- **`min_score_delta` / `max_score_delta` filters tolerate non-finite values when no bound is set** — the finiteness check now only fires when at least one bound was specified, so unspecified-bound delta views no longer silently drop rows whose `score_delta` is `NaN` (e.g., `Added`/`Removed` changes).
- **`delta_gate_passes_when_no_new_violations` actually exercises the delta gate** — the test previously combined `--no-fail` with `--threshold 5` (analysis-failing) so the assertion couldn't distinguish "gate passed" from "`--no-fail` masked the failure." Switched to `--threshold 1000` and dropped `--no-fail` so the green outcome is attributable to the delta gate alone.
- **`baseline_path_not_found_exits_2_with_actionable_message` is portable** — replaced the hard-coded `/tmp` path with a temp-dir-relative path so the test runs unchanged on Windows.
- **`baseline_unsupported_schema_version_exits_2` checks the specific error** — tightened the stderr assertion from a generic substring to the exact `unsupported baseline schema_version` message so a malformed-JSON failure that happens to mention "schema" can't silently satisfy the test.

### Internal
- **`arb_analysis_result` proptest strategy deduplicates by identity** — the strategy previously generated `(file_path, qualified_name)` collisions that violate a real invariant of `AnalysisResult` (real syn-walked output has unique identities per function). The dedup keeps the generator faithful to production data and unblocks the delta property tests.

## [0.2.0] - 2026-04-26

The reporting milestone. Five bundles ship together: pipeline-closeout
hardening (#91), additional reporter formats and shell completions
(#92), per-file aggregation (#93), saved view presets (#96), and the
baseline-comparison capstone (#97).

### Added
- **Baseline comparison: `--baseline <FILE>` + `--delta-gate`** — capture a previously-emitted JSON envelope and compare the current analysis against it. The new `domain::delta` module classifies every function as `Added`, `Removed`, or `Modified` (matched on `(file_path, qualified_name)` — span excluded so line shifts don't disrupt pairing) and rolls up an `AnalysisDelta` with summary counts (added/removed/modified, regressions/improvements, `new_violations`, `passed`). Delta is **informational by default** — `--baseline` alone never trips the exit code. Add `--delta-gate` to fail (exit 1) when new threshold violations land; pre-existing violations don't contribute, so re-running with no code changes never trips the gate. `--no-fail` overrides BOTH gates (analysis + delta); truth lives in JSON. JSON envelope grows an additive `delta` block (`schema_version` stays at `1`) with the full summary, the `DeltaView` shape (`spec`, `eligible_count`, `truncated`, `shown`), and baseline metadata (`baseline_tool_version`, `baseline_timestamp`, `baseline_diagnostics`). Three new shaping flags — `--delta-top N`, `--delta-sort {score-delta|current-crap|baseline-crap|path}` (default `score-delta` descending), `--delta-only added,removed,modified` — drive a sibling `DeltaViewSpec` that mirrors the View pipeline. Reporters render delta output in all four formats: table appends a "Delta vs baseline" block (per-change rows with kind, baseline/current scores, Δ); markdown appends a `## CRAP Scorecard` section with PASS/FAIL status, counts, and Regressions / New violations sub-tables (PR-comment ready); CSV mode-switches to row-per-change schema with `change_kind`, side-by-side `baseline_*`/`current_*` columns, and `score_delta`. CLI baseline loader validates `schema_version == 1`; unsupported envelopes exit `2` with an actionable stderr message. (#81)
- **`--view <NAME>` saved view presets** — declarative `[views.<name>]` blocks in `crap4rs.toml` bake a flag set under a single name; `crap4rs --view ci` resolves the preset and folds its values into the parsed CLI before validation. Supports every shapeable field: `top`, `min_coverage`, `max_coverage`, `sort`, `only_failing`, `no_fail`, `group_by`, `minimal_view`. **Override priority:** defaults < preset < CLI flags. CLI explicit `Option<T>` values win over preset values; bare bool flags OR-merge — an explicit `--no-fail` adds to a preset's `false` value, but bare clap booleans cannot represent "off," so a preset's `true` cannot be turned off from the CLI today. Unknown preset names exit `2` listing the available presets. Validation errors (out-of-range coverage, `min > max`, bad sort/group_by string, `deny_unknown_fields` typos) fail fast at config load with the offending preset's name in the message. The gate keystone is preserved — a preset cannot change `result.passed`, only the displayed view. (#80)
- **`--group-by file` aggregation** — shift the displayed view to per-file rows: each row is one source file with rolled-up `function_count`, `exceeding_count`, `average_crap`, `median_crap`, `max_crap`, `worst_function`, `average_coverage`, `max_complexity`, and the per-risk-level distribution. Composes naturally with `--top` and `--sort-by`, but the **semantics shift** under grouping: `--top N` truncates to the top N **files** (not functions), and `--sort-by` keys at the file level — `crap` → `average_crap` descending, `coverage` → `average_coverage` ascending, `complexity` → `max_complexity` descending, `path` → `file_path` alphabetical. The gate (exit code) is unaffected: `result.passed` always reflects the full unfiltered analysis. JSON envelope adds `view.grouped` (a sibling of `view.shown`); the per-function row list remains in `view.shown` for drill-down ergonomics, but `--minimal-view` still strips it without disturbing `view.grouped`. **CSV schema shifts**: `--format csv --group-by file` emits a different 10-column header (`file,function_count,exceeding_count,average_crap,max_crap,worst_function,distribution_low,distribution_acceptable,distribution_moderate,distribution_high`); pin your flags if you script on column position. Table and Markdown reporters render per-file rows; the summary block still derives from the underlying analysis. (#64)
- **`--format markdown` reporter** — GitHub-flavored Markdown output: pipe-syntax results table plus a Summary block (PASS/FAIL, function counts, distribution). No ANSI; safe to paste into PR comments and issue bodies. Pipes inside file paths or function names are escaped (`\|`). When `--breakdown` is set, exceeding functions get an indented bullet list of complexity contributors; `--explain` adds a trailing legend describing increment semantics. (#66)
- **`--format csv` reporter** — RFC 4180 row-per-function output with a fixed 10-column header (`file,function,start_line,end_line,complexity,complexity_metric,coverage_percent,crap_score,risk_level,exceeds_threshold`). Fields containing `,`, `"`, CR, or LF are wrapped in `"..."` with inner quotes doubled. Data-only — no summary block. (#67)
- **`crap4rs completions <SHELL>`** — new subcommand that prints a shell completion script to stdout (no file I/O — redirect into wherever your shell expects them). Supports `bash`, `zsh`, `fish`, `powershell`, `elvish`, and `nushell`. Unknown shells exit `2` with a clap value error. The subcommand does not require `--coverage`. (#69)
- **`--minimal-view` opt-in JSON shape** — omit the denormalized `view.shown` row array from JSON output for very large codebases where the per-row payload dominates. Every other view metadata key (`spec`, `eligible_count`, `truncated`, `shown_summary`) is preserved so consumers retain scope context. Default behaviour is unchanged. (#79)
- **`--no-fail` exit-code override** — force the process to exit `0` regardless of threshold violations. The underlying analysis is untouched: `result.passed` in JSON output still reflects the truthful pass/fail state, so consumers can detect "would have failed" even when the process exits 0. Composes with `--quiet` for silent success in CI; `--quiet` alone still preserves the standard exit-1 semantics on violations. (#65)
- **`--sort-by` choose sort dimension** — reorder the displayed view by `crap` (default, descending), `coverage` (ascending — lowest first surfaces investigation targets), `complexity` (descending), or `path` (alphabetical by file, then CRAP descending within file). Sorting reorders without reducing rows, so the gate (exit code) is unaffected and `--sort-by` alone does not render a "View:" banner in table output. Unknown values exit `2` with a clap value error attributed to `--sort-by`. JSON envelope echoes the resolved key under `view.spec.sort` as a lowercase string. (#68)
- **`--top N` row limit** — truncate the displayed view to the top `N` highest-CRAP rows. `--top 0` means "no limit" (canonicalised to `null` in the JSON envelope, so consumers see effective behaviour, not the literal input). Truncating violations out of the view does not change the gate (exit code) — `result.passed` always reflects the full unfiltered analysis. JSON envelope echoes the resolved limit under `view.spec.limit` and surfaces `view.truncated` / `view.eligible_count`. (#62)
- **`--min-coverage` / `--max-coverage` range filter** — drop functions whose `coverage_percent` falls outside `[min, max]` (inclusive). Either bound is optional; the unspecified side defaults to `0.0` or `100.0`. Invalid bounds (out-of-range or `min > max`) exit `2` with flag-attributed stderr. The full unfiltered analysis still drives the gate (exit code), so a filter that hides every violation does not change the outcome. JSON envelope echoes the resolved range under `view.filters.coverage_range`. (#63)

### Changed
- **`--only-failing` summary semantics** — the summary line now reflects the full unfiltered analysis (correctness fix). Previously, `--only-failing` mutated `result.functions` in-place via `retain`, so `total_functions` and `exceeding_threshold` reflected the post-mutation count while `average_crap`, `median_crap`, `max_crap`, and `distribution` retained pre-mutation values — an internally inconsistent state. The flag's row-level filter behavior is unchanged; only the printed summary is now coherent. (#78 follow-up)

### Internal
- `--only-failing` migrated from `OutputArgs.only_failing` (top-level `result.functions.retain`) to `FilterArgs.only_failing` flowing through `domain::view::Filters`. CLI behavior of the flag itself is unchanged.
- **CI gates: `cargo mutants` + per-function CC ≤ 15 on `src/domain/view.rs`** — two new jobs (`mutants`, `self-crap-view`) defend the View module's mutation kill rate and complexity ceiling. Drift in either trips CI. (#83)

## [0.1.1] - 2026-04-05

### Added

- **Breakdown explanation** (`--explain`) — optional legend for `--breakdown` table output that explains base increments, nested increments, and nesting-triggering constructs without changing default or JSON output
- **`cargo-binstall` support** — added `[package.metadata.binstall]` to `Cargo.toml` so `cargo binstall crap4rs` installs from pre-built GitHub release binaries

## [0.1.0] - 2026-03-30

### Added

- **LCOV parser** — parses `SF:` and `DA:` records from `cargo llvm-cov --lcov` output; merges repeated `SF` blocks for the same file
- **Syn complexity walker** — line-range based (not name-based) function discovery; supports both cognitive and cyclomatic metrics via `--metric` flag
- **CRAP formula** — cross-validated against crap4ts reference values; property tests verify monotonicity, boundary conditions, and oracle parity
- **Reporters** — `table` (default, ANSI-aware via comfy-table) and `json` output formats
- **CLI** — `--src`, `--coverage`, `--threshold`, `--metric`, `--format`, `--exclude`, `--verbose`, `--diff`, `--breakdown`, `--config` flags via clap
- **Config file** (`crap4rs.toml`) — TOML-based config with per-path threshold overrides and glob exclusions
- **Diff mode** (`--diff`) — filters output to functions modified in a unified diff; two-phase hunk filtering
- **Verbose mode** (`--verbose`) — diagnostic output to stderr including contributor breakdown
- **Complexity breakdown** (`--breakdown`) — per-contributor accumulation of CRAP scores
- **Threshold presets** — `--strict` (15), default (25), `--lenient` (40); configurable via TOML `preset` field
- **Zero-coverage heuristic** — warns when the majority of analyzed files show 0% coverage (common with `--lib` and integration-only code)
- **Version stamping** — `--version` includes embedded git hash and build date
- **CI/CD pipeline** — fmt, clippy, nextest, coverage gate, self-referential CRAP gate, cross-platform release builds, crates.io publish workflow
- **Self-referential test** — crap4rs analyzes its own source as an integration test; default threshold gate (25) passes

[Unreleased]: https://github.com/breezy-bays-labs/crap-rs/compare/crap4rs-v0.5.0...HEAD
[0.5.0]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/v0.5.0
[0.4.0]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/v0.4.0
[0.3.0]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/v0.3.0
[0.2.2]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/v0.2.2
[0.2.1]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/v0.2.1
[0.2.0]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/v0.2.0
[0.1.1]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/v0.1.1
[0.1.0]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/v0.1.0
