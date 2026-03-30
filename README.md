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
| `--threshold <n>` | 8 | CRAP score threshold (exit 1 if exceeded) |
| `--metric <type>` | cognitive | Complexity metric: `cognitive` or `cyclomatic` |
| `--format <type>` | table | Output format: `table` or `json` |
| `--exclude <glob>` | — | Exclude paths matching glob (repeatable) |
| `--verbose` | — | Print analysis diagnostics to stderr |

### Why cognitive by default?

Rust's `match` expressions with many arms inflate cyclomatic complexity without adding real risk. A flat 20-arm match is cyclomatic 20 but cognitive 1. Cognitive complexity better reflects actual Rust code risk.

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

threshold = 8

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

## Known limitations

`parse_unified_diff` and `analyze()` are the two most complex functions in crap4rs itself and currently exceed the `--strict` threshold (15). The tool passes its own default gate (threshold=25). This is tracked in [#54](https://github.com/breezy-bays-labs/crap4rs/issues/54) and is not a blocker for v0.1.0.

## Related

- [crap4ts](https://github.com/breezy-bays-labs/crap4ts) — CRAP analyzer for TypeScript

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
