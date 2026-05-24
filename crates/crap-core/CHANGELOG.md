# Changelog

All notable changes to `crap-core` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`crap-core` is the language-agnostic foundation shared by the
`crap4rs` (Rust) and `crap4ts` (TypeScript) adapters.

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
