# crap4rs

[![CI](https://github.com/breezy-bays-labs/crap4rs/actions/workflows/ci.yml/badge.svg)](https://github.com/breezy-bays-labs/crap4rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/crap4rs.svg)](https://crates.io/crates/crap4rs)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

CRAP (Change Risk Anti-Patterns) score analyzer for Rust codebases. Finds complex, under-tested functions.

## What is CRAP?

The CRAP metric combines **complexity** and **code coverage** into a single risk score:

```
CRAP(complexity, coverage) = complexity^2 * (1 - coverage)^3 + complexity
```

High complexity + low coverage = high CRAP score = high risk of bugs when changed.

| CRAP Score | Risk Level |
|------------|------------|
| ≤ 5 | Low |
| ≤ 8 | Acceptable |
| ≤ 30 | Moderate |
| > 30 | High |

## Usage

```bash
# Generate coverage data
cargo llvm-cov --lcov --output-path lcov.info

# Run CRAP analysis
crap4rs --src src/ --coverage lcov.info
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--src <path>` | `src` | Path to Rust source files |
| `--coverage <path>` | required | Path to LCOV coverage file |
| `--threshold <n>` | 25 | CRAP score threshold (exit 1 if exceeded) |
| `--metric <type>` | cognitive | Complexity metric: `cognitive` or `cyclomatic` |
| `--format <type>` | table | Output format: `table` or `json` |
| `--exclude <glob>` | — | Exclude paths matching glob (repeatable) |
| `--verbose` | — | Print analysis diagnostics to stderr |
| `--breakdown` | — | Show per-contributor complexity breakdown for failing functions in table output |
| `--explain` | — | With `--breakdown`, explain nested cognitive increments in table output |
| `--only-failing` | — | Display only functions exceeding the threshold (full analysis still drives the gate) |
| `--top <n>` | — | Truncate the report to the top `n` highest-CRAP rows (`--top 0` means no limit) |
| `--min-coverage <pct>` | — | Drop functions whose `coverage_percent` falls below the bound |
| `--max-coverage <pct>` | — | Drop functions whose `coverage_percent` exceeds the bound |
| `--sort-by <key>` | `crap` | Reorder rows by `crap`, `coverage`, `complexity`, or `path` |
| `--no-fail` | — | Always exit `0`; `result.passed` in JSON still reflects the truthful state |

Threshold presets are Rust-specific:

- `--strict` = `15`
- default = `25`
- `--lenient` = `40`

These do not match `crap4ts` exactly. The long-term goal is shared CRAP math and shared analysis concepts via `crap-core`, with language-specific adapters and threshold policy above that core.

### Why cognitive by default?

Rust's `match` expressions with many arms inflate cyclomatic complexity without adding real risk. A flat 20-arm match is cyclomatic 20 but cognitive 1. Cognitive complexity better reflects actual Rust code risk.

## Investigation patterns

The shaping flags (`--only-failing`, `--top`, `--min-coverage`, `--max-coverage`, `--sort-by`) reorder, filter, and truncate the **displayed report** without ever touching the **underlying analysis**. The gate is unshapeable: `result.passed` and the exit code always reflect the full unfiltered codebase, so a filter that hides every violation does not change the outcome. `--no-fail` overrides only the gate-to-exit-code translation; `result.passed` in JSON still tells the truth, so consumers can detect "would have failed" even when the process exits `0`.

```bash
# First-run scan: keep the report short
crap4rs --coverage lcov.info --top 20

# Worst partially-covered functions, sorted by coverage ascending,
# never fail the build — useful when investigating an untested codebase
crap4rs --coverage lcov.info \
  --min-coverage 1 --max-coverage 90 \
  --sort-by coverage --top 10 \
  --no-fail
```

The JSON envelope reflects the same separation: `result.*` always describes the full analysis (gate); `view.*` describes what the operator chose to see (display). An agent or dashboard can act on `result.passed`, `result.summary`, and `result.functions` while rendering only `view.shown`.

## Coverage notes

### `cargo llvm-cov --lib` only instruments unit-testable code

`cargo llvm-cov --lib` instruments code that is invoked by `#[test]` functions in the same crate. It does **not** cover:

- **Axum handlers** — only called via HTTP in integration tests, not unit tests
- **Tauri entry points** — only called by the Tauri runtime
- **BDD-tested code** — cucumber-rs scenarios run as separate processes, outside `--lib`

These functions will show **0% line coverage**, even if they are thoroughly tested. This is expected, not a bug.

**Mitigation:** use `--exclude` to skip paths where 0% coverage is unavoidable:

```toml
# crap4rs.toml — monorepo example (SvelteKit + Axum)
# Only analyze unit-testable crates.

preset = "strict"

exclude = [
  "services/api/src/**",   # Axum handlers — integration-only, no unit test surface
  "apps/desktop/**",       # Tauri entry point — no unit test surface
  "**/tests/**",           # Test helpers have 0% coverage by definition
]
```

When more than half of analyzed files show 0% coverage, `crap4rs` will print a warning with this hint automatically.

## Installation

```bash
# From crates.io (requires Rust toolchain)
cargo install crap4rs

# Or clone and build
git clone https://github.com/breezy-bays-labs/crap4rs.git
cd crap4rs
cargo build --release
```

## Prerequisites

- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) for generating LCOV coverage data

## Architecture

Hexagonal (ports & adapters) design for future extraction into a polyglot `crap-core` library:

```
domain/    Pure logic: CRAP formula, thresholds, types
ports/     Trait definitions (ComplexityPort, CoveragePort)
adapters/  syn walker, LCOV parser, reporters
core/      Wires adapters through ports
cli/       clap argument parsing
```

## Extraction roadmap

This repo is the Rust implementation today, but the longer-term direction is a shared multi-language CRAP toolchain:

- `crap-core` — shared CRAP math, thresholds model, result types, and language-agnostic analysis interfaces
- `crap4rs` — Rust-specific complexity and coverage adapters plus Rust-facing CLI/package surfaces
- `crap4ts` — TypeScript-specific complexity and coverage adapters plus npm-facing package surfaces

That split means:

- shared analysis concepts should converge in `crap-core`
- language parsers, coverage formats, and default threshold policy remain language-specific
- matching `crap4rs` and `crap4ts` behavior does not require identical thresholds

The current directory layout already reflects that extraction boundary:

- `domain/`, `ports/`, and `core/` are the future `crap-core` seam
- `adapters/` is the Rust-specific layer
- `cli/` is the Rust delivery surface that may later become part of a unified monorepo layout

## Self-check

The self-referential CI check runs at `--strict` (15) against `src`, excluding `cli/**`.

## Related

- [crap4ts](https://github.com/breezy-bays-labs/crap4ts) — CRAP analyzer for TypeScript

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
