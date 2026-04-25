# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **`--min-coverage` / `--max-coverage` range filter** — drop functions whose `coverage_percent` falls outside `[min, max]` (inclusive). Either bound is optional; the unspecified side defaults to `0.0` or `100.0`. Invalid bounds (out-of-range or `min > max`) exit `2` with flag-attributed stderr. The full unfiltered analysis still drives the gate (exit code), so a filter that hides every violation does not change the outcome. JSON envelope echoes the resolved range under `view.filters.coverage_range`. (#63)

### Changed
- **`--only-failing` summary semantics** — the summary line now reflects the full unfiltered analysis (correctness fix). Previously, `--only-failing` mutated `result.functions` in-place via `retain`, so `total_functions` and `exceeding_threshold` reflected the post-mutation count while `average_crap`, `median_crap`, `max_crap`, and `distribution` retained pre-mutation values — an internally inconsistent state. The flag's row-level filter behavior is unchanged; only the printed summary is now coherent. (#78 follow-up)

### Internal
- `--only-failing` migrated from `OutputArgs.only_failing` (top-level `result.functions.retain`) to `FilterArgs.only_failing` flowing through `domain::view::Filters`. CLI behavior of the flag itself is unchanged.

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
