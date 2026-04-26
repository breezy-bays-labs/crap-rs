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
| `--format <type>` | table | Output format: `table`, `json`, `markdown`, or `csv` |
| `--exclude <glob>` | — | Exclude paths matching glob (repeatable) |
| `--verbose` | — | Print analysis diagnostics to stderr |
| `--breakdown` | — | Show per-contributor complexity breakdown for failing functions in table output |
| `--explain` | — | With `--breakdown`, explain nested cognitive increments in table output |
| `--only-failing` | — | Display only functions exceeding the threshold (full analysis still drives the gate) |
| `--top <n>` | — | Truncate the report to the top `n` highest-CRAP rows (`--top 0` means no limit) |
| `--min-coverage <pct>` | — | Drop functions whose `coverage_percent` falls below the bound |
| `--max-coverage <pct>` | — | Drop functions whose `coverage_percent` exceeds the bound |
| `--sort-by <key>` | `crap` | Reorder rows by `crap`, `coverage`, `complexity`, or `path` |
| `--group-by <key>` | — | Aggregate the displayed view by a key. Today: `file` (per-file summaries). Under grouping, `--top` and `--sort-by` key at the file level. |
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

# Top 10 worst files by average CRAP — find which files to refactor first
crap4rs --coverage lcov.info --group-by file --top 10
```

Under `--group-by file`, `--top N` truncates to the top N **files** (not functions) and `--sort-by` keys at the file level: `crap` orders by average CRAP descending, `coverage` by average coverage ascending, `complexity` by max complexity descending, `path` alphabetically. The full per-function row list still appears in JSON `view.shown` for drill-down (`jq '.view.shown[] | select(.scored.identity.file_path == "src/blob.rs")'`); pair with `--minimal-view` to drop it for size-sensitive consumers. CSV's column schema shifts under grouping — pin your flags if you script on column position.

### Saved view presets (`--view <NAME>`)

When the same flag set repeats across runs (CI, investigation, paste-into-issue), bake it into `crap4rs.toml` under a `[views.<name>]` block and invoke it with `crap4rs --view <NAME>`:

```toml
# crap4rs.toml
[views.ci]
top = 20
min_coverage = 0
max_coverage = 90
sort = "coverage"
only_failing = true
group_by = "file"
minimal_view = true

[views.investigate]
sort = "complexity"
top = 10
```

```bash
# CI invocation — exits 1 on violations, 0 otherwise; minimal JSON for log parsing
crap4rs --coverage lcov.info --view ci --format json

# Investigation — preset's top=10 + sort by complexity, override with --top 25 inline
crap4rs --coverage lcov.info --view investigate --top 25
```

**Override priority:** defaults < preset < CLI flags. CLI explicit `Option<T>` values (`--top`, `--min-coverage`, `--sort-by`, `--group-by`) override the preset; bare bool flags (`--no-fail`, `--only-failing`, `--minimal-view`) OR-merge with the preset — an explicit CLI flag adds to the preset's `true` value but cannot turn off a preset's `true`. The gate keystone holds: a preset cannot change `result.passed`, only the displayed view. Unknown preset names exit `2` listing the available presets; invalid preset fields (out-of-range coverage, bad sort string, typos) fail fast at config load with the offending preset's name in the message.

The JSON envelope reflects the same separation: `result.*` always describes the full analysis (gate); `view.*` describes what the operator chose to see (display). An agent or dashboard can act on `result.passed`, `result.summary`, and `result.functions` while rendering only `view.shown`.

### Comparing two analyses (`--baseline <FILE>`)

To track regressions across runs, capture a baseline JSON envelope (typically from `main`) and compare your working tree to it:

```bash
# Capture the baseline (CI: cache or upload as an artifact)
crap4rs --coverage lcov.info --format json > baseline.json

# Compare working tree to baseline (informational by default)
crap4rs --coverage lcov.info --baseline baseline.json
```

The output adds a "Delta vs baseline" block under the analysis table — per-change rows with kind (added/removed/modified), baseline/current scores, and signed delta. Functions are paired by `(file_path, qualified_name)`; line shifts don't disrupt matching. Renames across files surface as Add+Remove pairs until rename detection ships.

By default, delta is **informational** — the exit code follows the analysis gate alone. Add `--delta-gate` to fail (exit 1) when the comparison introduces new threshold violations:

```bash
# CI usage: fail the build on new violations only (pre-existing ones don't trip it)
crap4rs --coverage lcov.info --baseline baseline.json --delta-gate
```

`new_violations` counts threshold breaches *introduced* by this change — `Added` rows that exceed threshold, plus `Modified` rows where baseline was passing and current isn't. Pre-existing violations (Modified rows where the baseline already exceeded) never contribute, so re-running on unchanged code never trips the gate.

`--no-fail` overrides BOTH gates (analysis + delta), but truth still lives in JSON: consumers see `result.passed` and `delta.summary.passed` regardless of the exit code, so a CI job can post a "would have failed" comment while still exiting 0.

For PR-comment scorecards, pipe the markdown reporter:

```bash
# Drop into a PR comment body verbatim (status, counts, regressions table, new-violations table)
crap4rs --coverage lcov.info --baseline baseline.json --format markdown
```

The shaping flags `--delta-top`, `--delta-sort` (`score-delta` (default) | `current-crap` | `baseline-crap` | `path`), and `--delta-only` (`added,removed,modified`) drive a sibling `DeltaViewSpec` independent of the View shaping. The JSON envelope's additive `delta` block carries the full summary plus `delta.shown` for renderer drill-down — the gate keystone holds for delta as it does for the analysis.

## Output formats

`--format markdown` produces GitHub-flavored Markdown (pipe-syntax table plus a Summary block) — paste it into a PR comment, an issue body, or a doc page. `--format csv` produces RFC 4180 CSV with a fixed header row, suitable for piping into spreadsheets, BI tools, or `awk`/`jq` pipelines that prefer tabular input. Both honor every shaping flag (`--top`, `--sort-by`, `--only-failing`, `--min-coverage` / `--max-coverage`).

### SARIF for GitHub Code Scanning (`--format sarif`)

`--format sarif` emits SARIF v2.1.0 JSON. Pipe it into a `.sarif` file and upload via `github/codeql-action/upload-sarif@v3` — every function whose CRAP score exceeds the threshold becomes an inline annotation on the exact line range in the PR diff. Reviewers see the findings without running crap4rs themselves.

| Risk level     | SARIF `level` |
| -------------- | ------------- |
| `high`         | `error`       |
| `moderate`     | `warning`     |
| `acceptable`   | `note`        |
| `low`          | `note`        |

Unlike the table / JSON / Markdown / CSV reporters, SARIF is a **gate translation**, not a display: it iterates the unshapeable analysis. `--top`, `--sort-by`, `--only-failing`, and `--baseline` do **not** alter SARIF output — PR annotations must reflect truth, not a presentation choice. `--no-fail` overrides the exit code only; the `results[]` array still lists every finding.

```yaml
# .github/workflows/crap-scan.yml
name: crap-scan
on: pull_request
jobs:
  scan:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      security-events: write   # required for upload-sarif
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-llvm-cov
      - run: cargo llvm-cov --lcov --output-path lcov.info
      - uses: cargo-bins/cargo-binstall@main
      - run: cargo binstall -y crap4rs
      # Always emit SARIF, even on failure, so annotations land on the PR.
      - run: crap4rs --coverage lcov.info --format sarif --no-fail > crap.sarif
      - uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: crap.sarif
```

## Shell completions

Print a completion script for your shell to stdout — the subcommand does no file I/O, so redirect it wherever your shell expects completions:

```bash
# bash
crap4rs completions bash > /usr/local/etc/bash_completion.d/crap4rs

# zsh — adjust to a directory in your $fpath
crap4rs completions zsh > ~/.zsh/completions/_crap4rs

# fish
crap4rs completions fish > ~/.config/fish/completions/crap4rs.fish

# nushell — append the printed module to your config
crap4rs completions nushell >> ~/.config/nushell/config.nu

# powershell
crap4rs completions powershell | Out-String | Invoke-Expression

# elvish
crap4rs completions elvish > ~/.config/elvish/lib/crap4rs.elv
```

`crap4rs completions <SHELL>` does not need `--coverage`. Unknown shell names exit `2`.

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
