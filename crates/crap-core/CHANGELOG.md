# Changelog

All notable changes to `crap-core` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`crap-core` is the language-agnostic foundation shared by the
`crap4rs` (Rust) and `crap4ts` (TypeScript) adapters.

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
