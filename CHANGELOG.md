# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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
