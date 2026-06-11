# Changelog

All notable changes to `crap-core` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`crap-core` is the language-agnostic foundation shared by the
`crap4rs` (Rust) and `crap4ts` (TypeScript) adapters.

## [Unreleased]

### Added

- `format_markdown` always emits a hidden
    `<!-- {tool_name}:scorecard -->` HTML comment as the first output
    line — a per-adapter dedupe anchor for sticky-PR-comment tooling
    (it precedes a configured `[output] title`, and the scorecard
    action's heading-offset rewrite leaves it untouched). When
    `--breakdown` is active, the per-line complexity contributors of the
    above-threshold functions collect into one
    `<details><summary>Show breakdown</summary>` collapsible rendered
    BELOW the scorecard table, each function keyed by a markdown code
    span. The collapsible sits below the table (rather than interleaved
    between rows) because a `<details>` HTML block terminates a GFM
    table — an inline block would drop every row after the first to
    literal "pipe-text". Verified against GitHub's own `/markdown`
    renderer. (crap-rs#275, crap-rs#397)

### Changed

- Config-file auto-discovery now **walks upward**: when no explicit
    `--config` is given, the loader searches the run's anchor directory
    (the first `--src` root, or the working directory when `--src` is
    empty) and every ancestor up to the filesystem root, and the
    nearest directory holding any candidate wins. This lets
    `crap4rs --src crates/foo` (run from a repo root) discover the
    repo-root `crap.toml`, and `cd crates && crap4rs --src foo` discover
    a `crap.toml` one level up. The previous behavior only inspected the
    working directory. Within each directory the canonical-over-legacy
    ordering and the same-directory shadow notice are unchanged; a file
    in a parent directory is never reported as shadowed. Pass an explicit
    `--config <path>` to bypass discovery entirely. (crap-rs#339)
    - Edge case: the walk has no `.git` / workspace-root boundary, so a
        stray `crap.toml` in `$HOME` (or any ancestor above your project)
        is discovered when no nearer config exists. Use `--config` to
        pin the file explicitly if an ancestor config is unwanted.

### Internal

- The config loader is now `anyhow`-free: `discover_config`,
    `load_config`, and the parse/validate helpers return a typed
    `crap_core::adapters::config::ConfigError`
    (`thiserror` + `#[non_exhaustive]`); the CLI boundary lifts it into
    `anyhow` so user-facing output is unchanged. (crap-rs#340)
- The parsed config POD types (`FileConfig`, `OutputConfig`,
    `LangConfig`, `ViewPreset`) moved to
    `crap_core::domain::config`; they are re-exported from
    `crap_core::adapters::config` so existing import paths keep working.
    (crap-rs#341)

## [0.8.0]

### Added

- View axis (Current / Delta tabs) on the multi-language unified HTML
    report. Each per-language panel renders a Current/Delta tab pair
    when its block carries a baseline; languages without a baseline
    render the Delta tab disabled with a no-baseline tooltip so the
    asymmetric state is visible without suppressing other languages'
    deltas. The Combined panel exposes its own Current/Delta tabs;
    Combined → Delta surfaces a cross-adapter ranked table of
    regressions and new functions sorted by risk band desc then
    CRAP/threshold ratio desc within band (matches the dimensional-
    consistency rule that governs the Current-view Combined
    ranking). (crap-rs#326)
- New domain types in `crap_core::domain::multi_lang` to support the
    Combined Delta aggregate: `CombinedDelta`,
    `CombinedDeltaSummary`, `RankedDeltaRow`, `RankedDeltaKind`,
    `DeltaRowSnapshot`. The types are N-adapter-agnostic — adding a
    new adapter that supplies a baseline contributes to the
    Combined Delta without code changes outside the calling site.
    (crap-rs#326)
- New library entry point
    `crap_core::core::compose::compose_combined_delta(blocks) ->
    Option<CombinedDelta>`. Returns `None` precisely when no
    language supplied a baseline — the renderer reads this signal
    to suppress the View axis nav entirely so the no-baseline
    multi-language render path stays equivalent to the v0.7.0
    output. (crap-rs#326)
- `crap-render --baseline <LANG>=<FILE>` CLI flag (additive, fully
    optional). Each baseline pairs by language key with one of the
    `--input` envelopes; a baseline whose language key has no
    matching input is an error. The flag is repeatable; same
    duplicate-language guard as `--input`. (crap-rs#326)
- Two-axis URL hash routing in the multi-language report's inline
    `<script>`. URL hash format is `#<lang>:<view>` where `<lang>` is
    one of the Language nav buttons and `<view>` is `current` or
    `delta`. Default on first load with no hash: `#combined:current`.
    Switching Language preserves View where possible (e.g. on
    `#rust:delta`, clicking TypeScript navigates to
    `#typescript:delta`, not `#typescript:current`). Navigating to a
    disabled tab silently falls back to `current` and logs to the
    browser console — no error toast. (crap-rs#326)

### Changed

- `format_html_multi` template context (`HtmlMultiReport`) gained
    `has_view_axis: bool` and `combined_delta_panel:
    Option<Box<CombinedDeltaPanel>>` fields; `LangPanel` gained
    `has_delta: bool`, `current_tab_count`, `delta_tab_count`,
    `delta_has_news`, and `delta_panel: Option<Box<DeltaPanel>>`.
    These are internal template-context types — the public
    `format_html_multi` signature is unchanged.
- Single-language passthrough (`multi.languages.len() == 1`)
    continues to delegate to `format_html` for byte-identical
    output. The View axis plumbing in the multi-lang glue does not
    leak into the n=1 short-circuit even when a baseline is supplied
    — a new test (`multi_lang_single_language_passthrough_byte_identical_with_baseline`)
    locks this invariant in addition to the existing
    no-baseline passthrough test.

## [0.7.0]

### Added

- New multi-language domain types in `crap_core::domain::multi_lang`:
    `MultiLangContext`, `LanguageBlock`, `CombinedSummary`,
    `RankedFunction`, `WorstRatio`. The types are N-agnostic — adapter
    identity (`tool_name`, `display_name`, `language`, `tool_version`)
    is carried as owned `String` so envelope-loaded data composes
    cleanly, and `compose_multi_lang` takes `Vec<LanguageBlock>` with
    no hardcoded language list. Adding a future adapter (e.g.
    `crap4go`, `crap4py`) is purely additive — no domain or library
    changes required. (crap-rs#315)
- `crap_core::core::compose::compose_multi_lang(blocks)` — pure
    function that aggregates per-adapter blocks into a
    `MultiLangContext`. Combined-view ranking applies the
    dimensional-consistency-aware sort: risk level descending
    (per-adapter calibrated), then CRAP/threshold ratio descending
    within band. Raw CRAP scores are NOT used as the primary sort
    because complexity metrics (cognitive vs cyclomatic) scale
    differently across adapters; per-tier risk + ratio is
    dimensionally honest. (crap-rs#315)
- `crap_core::adapters::reporters::format_html_multi(multi, threshold,
    options)` — renders a `MultiLangContext` as a unified HTML
    document. Single-language passthrough (when
    `multi.languages.len() == 1`) delegates to `format_html` for
    byte-identical output. Multi-language input renders the
    `html_multi_report.html` template with `.segmented` Language nav
    (Rust / TypeScript / Combined), Combined-default panel, per-row
    adapter badges in the ranked-CRAP table, and a per-adapter
    Adapters provenance grid in the footer. (crap-rs#315)
- New `crap-render` `[[bin]]` target in `crap-core`. CLI shape:
    `crap-render --input <LANG>=<FILE> [--input <LANG>=<FILE>...]
    --format html [--output <PATH>]`. Validates envelope
    `schema_version ∈ {1, 2}` (mirrors the baseline loader's accepted
    range) and refuses duplicate language keys. Consumed by the
    composite scorecard action in multi-language mode; also invocable
    manually for debugging. (crap-rs#315)
- `[package.metadata.binstall]` block in `crap-core/Cargo.toml`
    mirroring the `crap4rs` pattern. `cargo binstall crap-core` (or
    `taiki-e/install-action with tool: crap-render`) resolves to the
    pre-compiled `crap-render-<target>.tar.gz` uploaded by the
    `build-crap-core-binaries` matrix in `release-plz.yml`.
    (crap-rs#315)

### Changed

- `crap-core` now ships a binary alongside the library. This
    diverges from the `crap4rs` peer-crate convention of "library
    crate, never a binary"; the divergence is intentional — the
    multi-language renderer needs both `crap4rs` and `crap4ts` data
    in one process, which neither adapter binary can satisfy alone,
    and placing the renderer in `crap-core` keeps the rendering
    pipeline language-neutral. The change is non-breaking for
    existing library consumers — the `[[bin]]` target compiles only
    when explicitly built.
- The `crap-core` description and categories pick up the new
    `command-line-utilities` category so crates.io discovery surfaces
    the renderer.

### Adapter schema compatibility

`crap-render` enforces that all input envelopes carry a
`schema_version` in `{1, 2}`. If you upgrade one adapter (e.g.
`crap4rs` to a version emitting `schema_version: 3`) but leave the
other on an older version emitting `schema_version: 1` or `2`,
`crap-render` will fail fast with an actionable error rather than
silently produce a mangled combined view. Keep `crap4rs` and
`crap4ts` reasonably up-to-date; major version bumps are documented
in each crate's CHANGELOG.

## [0.6.0]

### Added

- HTML reporter: optional delta tab (Current vs baseline view) when
    `--baseline` is set. The tabs nav appears between the topbar and
    the body, the Current panel opens by default, and a second
    `<div class="tab-panel" data-tab="delta">` follows with a 4-tile
    delta KPI grid (Exceeding threshold · Max CRAP · Average CRAP ·
    Avg coverage) plus Regressions / Improvements / New-functions
    tables. The Sakura mock's 5th "Functions" tile is dropped
    (counts already surface in the verdict line), the empty
    Removed-zero panel is dropped, and the Unchanged section trims
    to a single-line note. URL hash `#delta` deep-links into the
    delta tab via an inline `<script>` hook so CI sticky-comment
    links land users in the right view. (crap-rs#306)

### Changed

- `reporters::format_html` signature widened from
    `(view, threshold, &AdapterMeta, ComplexityMetric)` to
    `(view, delta: Option<&DeltaView<'_>>, threshold, &AdapterMeta, ComplexityMetric)`.
    Additive parameter (callers that wire `None` produce
    **byte-identical** v0.5.0 output — the no-baseline contract is
    preserved end-to-end), but the signature change is breaking for
    any external caller and motivates the minor bump.
- HTML template gains a `{% if has_delta %}` gate on the tabs nav,
    a second `<div class="tab-panel" data-tab="delta">` block,
    delta-tab CSS layered into the inline `<style>`, and a
    `tab-switcher` IIFE in the inline `<script>`. All four
    additions are gated on `has_delta`; the no-baseline render
    emits zero new bytes.

## [0.5.0]

### Changed

- HTML and Markdown reporters now render through askama compile-time
  templates. Templates live at `crates/crap-core/templates/` and are
  checked at compile time by `#[derive(Template)]`. Refactor under
  the hood; no behavioral change to the markdown output (snapshots
  byte-identical), HTML redesigned per the Sakura Reports design
  system. (#260)
- HTML reporter redesigned around the **Sakura Reports** design
  system: a verdict-stamped header, 4 KPI tiles (down from 6), a
  risk distribution bar, up to 4 worst offenders (down from 6), a
  `<details>` card per file with a real `<table>` for function-level
  data, light + optional dark mode, and a per-adapter footer that
  carries metric / coverage / threshold provenance. Inline `<script>`
  now permitted (theme toggle, file-list filter, `/` keyboard
  shortcut); external assets still rejected. (#260)
- `reporters::format_html` signature widened from
  `(view, threshold, tool_name: &str, tool_version: &str)` to
  `(view, threshold, meta: &AdapterMeta, effective_metric: ComplexityMetric)`.
  The new arguments thread adapter identity + runtime-resolved
  complexity metric into the per-adapter footer without leaking
  domain state into the template. (#260)
- `reporters::format_markdown` signature shifted its last two
  `(&str, &str)` arguments to `(&AdapterMeta, ComplexityMetric)` for
  signature symmetry with `format_html` — markdown does not yet
  surface the metric label but the bundle is threaded uniformly.
  (#260)
- `AdapterMeta` gains a `display_name: &'static str` field carrying
  the human-readable language label (`"Rust"` / `"TypeScript"`)
  used by the HTML reporter's per-adapter footer row. Adapter
  binaries must initialize this field. (#260)

## [0.4.0]

This release covers two breaking changes to the public API, bundled
into a single migration window.

### Changed

- `AdapterMeta` gains a `default_excludes: &'static [&'static str]`
  field — required by adapter binaries so `<adapter> init` can emit
  per-ecosystem exclude defaults. Existing struct-literal
  constructions in adapter `main.rs` files need this field added;
  there is no default impl to fall back on (the type stays `Copy` /
  per-field literal-init to keep zero-cost). Affects only adapter
  binary crates; library consumers of `crap-core` do not construct
  `AdapterMeta` directly. (#73)
- `ThresholdPreset::threshold` now takes a `ComplexityMetric`
  argument and returns the metric-calibrated cutoff
  (`fn threshold(self, metric: ComplexityMetric) -> f64`). Callers
  resolving a preset to a numeric cutoff must pass the effective
  metric. No external consumers. (#218)
- `CoveragePort::parse` now takes `&Path` instead of `&str` so each
  adapter owns its slurp-vs-stream decision internally. The shared
  pre-read in `crap_core::core::Analyzer::parse_coverage` is removed,
  eliminating the double-read trap where the orchestrator slurped a
  100 MB LCOV file before handing the buffer to a parser that could
  have streamed. `LcovParser` and `IstanbulCoverage` both slurp via
  `std::fs::read_to_string` internally (size ceilings well below
  peak-RSS concern); future streaming adapters drop in unchanged.
  External `CoveragePort` impls — likely none today, the port has
  been internal — migrate by changing the `parse` signature to
  `fn parse(&self, path: &Path) -> Result<…>` and adding a slurp
  or stream at the top of the body. Each impl owns its read
  strategy; the trait makes no commitment. (#179)
- `CrapError::LcovParse(String)` renamed to
  `CrapError::CoverageParse(String)`. The variant was always
  adapter-agnostic in intent — both the LCOV and Istanbul parsers
  today either succeed (emitting per-record issues as non-fatal
  `ParseOutput.diagnostics`) or fail via `CrapError::SourceParse`
  for malformed top-level structure, so the rename has no
  user-facing impact on current adapters. The variant stays as the
  stable error surface for future adapter-format parse failures
  that don't fit either bucket; tool-prefixed messages
  (`"lcov: …"` / `"istanbul: …"`) render at construction sites.
  `CrapError` remains `#[non_exhaustive]` so external matchers with
  `_ => …` arms continue to compile; explicit
  `CrapError::LcovParse(_)` matches need a one-line rename.
  Mirrors the same anti-pattern class that closed #161. (#178)

## [0.1.0] - 2026-05-10

Initial extraction from the `crap4rs` crate as part of the
workspace-extraction milestone.

### Added

- `crap-core` 0.1.0 — language-agnostic shared library extracted
  from crap4rs. Contains domain types (`AnalysisResult`,
  `ScoredFunction`, `RiskLevel`, `ContributorKind`, CRAP formula,
  delta + summary), port traits (`ComplexityPort`, `CoveragePort`,
  `DiffPort`, `ParseDiagnostic`), the eight reporters (JSON, SARIF,
  CSV, markdown, HTML, scorecard-row, table, advice-summary), the
  baseline / config / diff adapters, the `walker` orchestration, and
  the CLI dispatch shell. Designed for future TypeScript / multi-
  language adapters to bind against the same domain core. See PRs
  [#146](https://github.com/breezy-bays-labs/crap4rs/pull/146) (domain
  + ports), [#149](https://github.com/breezy-bays-labs/crap4rs/pull/149)
  (adapters), [#151](https://github.com/breezy-bays-labs/crap4rs/pull/151)
  (core + cli).

[0.4.0]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/crap-core-v0.4.0
[0.1.0]: https://github.com/breezy-bays-labs/crap-rs/releases/tag/crap-core-v0.1.0
