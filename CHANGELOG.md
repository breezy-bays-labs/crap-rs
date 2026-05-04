# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
- **`--format scorecard-row`** emits a single mokumo `Row::CrapDelta`
  JSON object (mokumo `schema_version=2`) for scorecard-aggregator
  consumption. Producer-mints-status (Model P): Red on new threshold
  violations, Yellow on modified-function CRAP regression, Green
  otherwise. `--baseline <path>` integration carries the signed
  `delta_count`, the `delta_text` display string (e.g. `"5 → 7 (+2)"`),
  and a Red-only `failure_detail_md` listing violators sorted by CRAP
  descending. Schema round-trip pinned via vendored fixture at
  `tests/fixtures/scorecard/schema.json`. Closes #111.
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

[0.1.0]: https://github.com/breezy-bays-labs/crap4rs/releases/tag/v0.1.0
