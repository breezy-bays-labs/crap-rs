# crap4rs

CRAP (Change Risk Anti-Patterns) score analyzer for Rust codebases. Finds complex, under-tested functions.

> **Status:** Pre-release. MVP in progress.

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
crap4rs --src src/ --coverage lcov.info --threshold 8
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--src <path>` | required | Path to Rust source files |
| `--coverage <path>` | required | Path to LCOV coverage file |
| `--threshold <n>` | 30 | CRAP score threshold (exit 1 if exceeded) |
| `--metric <type>` | cognitive | Complexity metric: `cognitive` or `cyclomatic` |
| `--format <type>` | table | Output format: `table` or `json` |

### Why cognitive by default?

Rust's `match` expressions with many arms inflate cyclomatic complexity without adding real risk. A flat 20-arm match is cyclomatic 20 but cognitive 1. Cognitive complexity better reflects actual Rust code risk.

## Installation

```bash
# From source (requires Rust toolchain)
cargo install crap4rs

# Or clone and build
git clone https://github.com/breezy-bays-labs/crap4rs.git
cd crap4rs
cargo build --release
```

> `cargo install` will work once published to crates.io. For now, build from source.

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

## Related

- [crap4ts](https://github.com/breezy-bays-labs/crap4ts) — CRAP analyzer for TypeScript

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
