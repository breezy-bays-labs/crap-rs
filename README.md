# crap-rs

[![CI](https://github.com/breezy-bays-labs/crap-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/breezy-bays-labs/crap-rs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/crap4rs.svg)](https://crates.io/crates/crap4rs)
[![npm](https://img.shields.io/npm/v/crap4ts.svg)](https://www.npmjs.com/package/crap4ts)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**CRAP (Change Risk Anti-Patterns) score analysis with a shared Rust core and language-specific adapters.** A CRAP score fuses a function's complexity with its test coverage into a single number — high complexity plus low coverage means high risk when the code changes.

This workspace ships two analyzers over one shared core:

| Crate / package | Analyzes | Coverage format | Published to |
|-----------------|----------|-----------------|--------------|
| **`crap4rs`** | Rust | LCOV | [crates.io](https://crates.io/crates/crap4rs) |
| **`crap4ts`** | TypeScript / JavaScript | Istanbul JSON | [npm](https://www.npmjs.com/package/crap4ts) — see its [package README](packages/crap4ts/README.md) |
| **`crap-core`** | _shared library_ | — | internal: CRAP formula, thresholds, reporters, analysis types |

Both adapters link the same `crap-core`, so Rust and TypeScript / JavaScript projects get identical CRAP semantics — the same formula, envelope, and reporters.

## What is CRAP?

The CRAP metric combines **complexity** and **code coverage** into a single risk score:

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

where `coverage` is the fraction in `[0, 1]` (50% → `0.5`). High complexity + low coverage = high CRAP score = high risk of bugs when changed.

### Risk classification

Every score lands in one of four risk levels:

| CRAP Score | Risk Level |
|------------|------------|
| ≤ 8        | Low        |
| ≤ 15       | Acceptable |
| ≤ 25       | Moderate   |
| > 25       | High       |

These boundaries also anchor the threshold presets: `--strict` fires at the Low → Acceptable boundary, the default fires at Acceptable → Moderate, and `--lenient` fires at Moderate → High — so every preset corresponds to "gate at the next risk tier up." See [Threshold (the gate)](#threshold-the-gate) for how the gate works and how the two views relate.

## Try it locally

Want a hands-on tour without setting up a real project? The repo ships a polyglot pedagogical sample at `crates/crap-examples/` carrying four Rust + four TypeScript modules picked to span every risk band on a single analysis. Each module isolates one term of the CRAP formula `c² × (1 − coverage)³ + c` — see `crates/crap-examples/README.md` for the worked-example heatmap.

```bash
# Install (pick one):
cargo install crap4rs                  # Rust analyzer — published to crates.io
npm install -g crap4ts                 # TypeScript analyzer — published to npm

# Clone + analyze the sample
git clone https://github.com/breezy-bays-labs/crap-rs.git
cd crap-rs
crap4rs --src ./crates/crap-examples/src --coverage crates/crap-examples/lcov.info
# Expected: 4 risk bands hit (Low, Acceptable, Moderate, High);
# worst function is config_merger::merge_configs in the High band.

# Same shape for TypeScript:
crap4ts --src ./crates/crap-examples/ts --coverage crates/crap-examples/coverage-final.json --exclude '*.test.ts'
```

The committed coverage fixtures use paths relative to each adapter's `--src` root. If you regen them yourself, follow `crates/crap-examples/README.md` § Regenerating the fixtures — the regen recipe normalizes paths so the analyzer's coverage matcher joins them cleanly.

The same envelope shape ships as a release asset on every crap-rs release page (`crap4rs-envelope.json` + `crap4ts-envelope.json`); the composite `crap-scorecard` action's [Pattern 2b](.github/actions/scorecard/README.md#pattern-2b--cross-release-envelope-baseline) recipe fetches them as a `--baseline` so the Delta tab renders enabled against the analyzer's cross-release drift.

## crap4rs — the Rust analyzer

Everything from here down documents the `crap4rs` Rust CLI. For TypeScript / JavaScript projects, see the [crap4ts package README](packages/crap4ts/README.md).

## Usage

```bash
# Generate coverage data
cargo llvm-cov --lcov --output-path lcov.info

# Run CRAP analysis
crap4rs --src src/ --coverage lcov.info
```

### Commands

| Command | Purpose |
|---------|---------|
| `crap4rs` (no subcommand) | Run the analyzer. Requires `--coverage <FILE>`. |
| `crap4rs init` | Write the exhaustive annotated `crap.toml` (every option, documented — see [`crap.example.toml`](crap.example.toml)) in the current directory; trim it down to your needs. Pair with `--force` to overwrite. |
| `crap4rs completions <SHELL>` | Print a shell completion script to stdout. See [Shell completions](#shell-completions). |
| `crap4rs help [SUBCOMMAND]` | Long-form help for the binary or a subcommand. |

### Options

Run `crap4rs --help` for the canonical full reference. Grouped here as in `--help` output:

**Input**

| Flag | Default | Description |
|------|---------|-------------|
| `--coverage <FILE>` | required for analysis | Path to the LCOV coverage file. Not required for `completions`/`init`. |
| `--src <DIR>` | `src` | Root directory of source files to analyze. **Repeatable** — pass `--src` more than once (`--src crates/a/src --src crates/b/src`) to union several roots into one report against a single `--coverage`. A single `--src` is unchanged; multiple roots key function paths relative to the git toplevel (requires a git work tree). |
| `--metric <METRIC>` | `cognitive` | `cognitive` (default) or `cyclomatic`. |
| `--config <FILE>` | auto-discovered | Explicit config file path; bypasses `crap.toml` auto-discovery. |
| `--view <NAME>` | — | Resolve a saved view preset from the config (see [Saved view presets](#saved-view-presets---view-name)). |
| `--baseline <FILE>` | — | Compare against a previously-emitted JSON envelope (see [Comparing two analyses](#comparing-two-analyses---baseline-file)). |

**Output**

| Flag | Default | Description |
|------|---------|-------------|
| `--format <FORMAT[,…]>` | `table` | One or more output formats. Each entry is `FORMAT` (stdout) or `FORMAT:FILE` (write to file); a comma-separated list fans out a single analysis pass to multiple destinations (`json:env.json,markdown:report.md` or `markdown:scorecard.md,github-annotations`). Supported formats: `table`, `json`, `markdown`, `csv`, `sarif`, `advice` (experimental), `scorecard-row`, `github-annotations`, `html`. Multi-format invocations may include at most one stdout entry; additional entries must specify a file. |
| `--annotation-limit <N>` | `10` | Cap on `::warning` lines emitted by `--format github-annotations`. Range `1..=100`; also configurable via `[output] annotation_limit` in the TOML config. The CLI flag wins when both are set. |
| `--threshold <N>` | metric-correct `default` preset (cognitive 15, cyclomatic 15 today) | CRAP score above which a function fails the check. The default is resolved against the effective metric, not a hard-coded scalar. |
| `--strict` | — | Use the `strict` preset (cognitive 8, cyclomatic 8 today). Mutually exclusive with `--lenient`. |
| `--lenient` | — | Use the `lenient` preset (cognitive 25, cyclomatic 25 today). |
| `--no-fail` | — | Always exit `0`; `result.passed` in JSON still reflects truth. Composes with `--delta-gate` (overrides BOTH gates). |
| `--delta-gate` | off | Fail the build (exit `1`) when `--baseline` introduces new threshold violations. |
| `--minimal-view` | — | Omit `view.shown[]` from JSON output (payload-size escape hatch for large codebases). |
| `--summary` | — | Emit a single-line analysis verdict instead of the full report; short-circuits `--format`. |

**Filtering**

| Flag | Default | Description |
|------|---------|-------------|
| `--exclude <GLOB>` | — | Exclude paths matching glob (repeatable). |
| `--no-gitignore` | respect | Do NOT skip paths in `.gitignore`. |
| `--diff <REF>` | — | Only analyze functions in files changed since `REF` (CI PR-gating). |
| `--only-failing` | — | Display only functions exceeding the threshold (gate still ranges over everything). |
| `--top <N>` | — | Truncate the displayed report to the top `N` highest-CRAP rows. |
| `--min-coverage <PCT>` | — | Drop displayed rows whose coverage falls below `PCT`. |
| `--max-coverage <PCT>` | — | Drop displayed rows whose coverage exceeds `PCT`. |
| `--sort-by <KEY>` | `crap` | `crap` (default), `coverage`, `complexity`, or `path`. |
| `--group-by <KEY>` | — | Today: `file` (per-file summaries). Under grouping, `--top` and `--sort-by` key at the file level. |
| `--delta-top <N>` | — | Truncate the delta block to top `N` changes. |
| `--delta-sort <KEY>` | `score-delta` | `score-delta`, `current-crap`, `baseline-crap`, or `path`. |
| `--delta-only <KINDS>` | all | Comma-separated subset of `added`, `removed`, `modified`. |

**Display**

| Flag | Default | Description |
|------|---------|-------------|
| `--color <COLOR>` | `auto` | `auto`, `always`, or `never`. |
| `-v`, `--verbose` | — | Show parse diagnostics and matching statistics. |
| `-q`, `--quiet` | — | Suppress report output, only set exit code. |
| `--breakdown` | — | Show per-contributor complexity breakdown for failing functions in table output. |
| `--explain` | — | With `--breakdown`, explain nested cognitive increments in table output. |
| `--md-full-table` | — | Append the full per-function table to markdown output (default markdown is a compact top-N summary). |
| `--md-top <N>` | `10` | Number of rows in the markdown top-N table. |

### Config file

The canonical config file name is **`crap.toml`** — a single language-neutral file shared by both `crap4rs` and `crap4ts`, auto-discovered in the working directory. `crap4rs init` writes it, and `--config <FILE>` overrides discovery with an explicit path.

Every supported option, with prose for each field, lives in **[`crap.example.toml`](crap.example.toml)** — the exhaustive annotated reference. It is what `crap4rs init` writes verbatim (trim it down to your real config), is generated from the config type (a sync test keeps it from rotting), and is **not** loaded by the tool — it exists purely as the canonical option reference. The editor-validation schema for `crap.toml` is **[`crap.schema.json`](crap.schema.json)** (point your editor's `$schema` at it for autocomplete + inline validation).

This repo dogfoods its own config: **[`crap.toml`](crap.toml)** at the root is a real, trimmed-down working example (the kind you'd actually commit, not the exhaustive dump). The production CRAP scorecard CI job runs with no analysis flags in the workflow and lets this file own the knobs via auto-discovery — `preset`, the per-language `metric`, and the multi-root `src` array. It also carries a `[views.ci]` preset as a worked reference; that preset only shapes a report when a run opts in with `--view ci` (the production job does not), so it documents the report-shaping surface without gating today. Read it alongside `crap.example.toml` to see the difference between "every option documented" and "a real project's config."

For back-compat, the legacy per-adapter names **`crap4rs.toml`** (and `crap4ts.toml` for crap4ts) are still discovered when no `crap.toml` is present, but are **deprecated aliases**: the tool prints a one-line warning nudging you to rename. A present `crap.toml` always takes precedence over a co-present legacy file (the legacy file is then reported as safe to remove). The config examples below use `crap4rs.toml`; they apply identically to `crap.toml`.

### Threshold (the gate)

`--threshold` is the line above which a function trips the build gate (exit `1` unless `--no-fail`). Tier presets are aligned with the [risk classification](#risk-classification) — each preset fires at the next risk-tier boundary:

| Preset      | Cognitive | Cyclomatic | Risk boundary it gates at |
|-------------|-----------|------------|---------------------------|
| `--strict`  | `8`       | `8`        | Low → Acceptable          |
| default     | `15`      | `15`       | Acceptable → Moderate     |
| `--lenient` | `25`      | `25`       | Moderate → High           |

Cognitive and cyclomatic columns currently hold the same values. The dual-column infrastructure inside `crap-core` is preserved because the two metrics can diverge in magnitude for the same code (cognitive is nesting-weighted; cyclomatic counts decision points); a future per-metric recalibration is a one-line change without an API churn.

A function with CRAP `5` is `Low` and never trips any preset. A function with CRAP `40` is `High` and trips every preset. A function with CRAP `12` is `Acceptable` (the risk classification) but trips `--strict` (the gate); the two views agree at boundaries but the gate moves earlier than risk reclassification.

Risk classification feeds SARIF severity (`high → error`, `moderate → warning`, `acceptable/low → note`); threshold feeds the exit code. Scorecard-row status (`Red`/`Yellow`/`Green`) — see [docs/scorecard-row-contract.md](docs/scorecard-row-contract.md) — is minted from the threshold gate, not from risk classification.

`crap4rs` and `crap4ts` share the CRAP formula and analysis concepts through `crap-core`; threshold policy is exposed per analyzer and may diverge in the future.

#### Per-path threshold overrides

`crap4rs.toml` accepts a `[thresholds]` block with per-glob overrides — useful when a directory has stricter or looser requirements than the project default. The most-specific (latest-matching) override wins:

```toml
# crap4rs.toml — domain/ must stay under the strict cutoff, even when
# the project default is more permissive.
preset = "default"   # global cutoff = cognitive 15

[[thresholds.overrides]]
pattern = "src/domain/**"
threshold = 8        # gate at the strict tier inside domain/
```

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

### Agent advice (`--format advice`) — experimental

`--format advice` emits the same JSON envelope as `--format json`, but with a populated `Diagnostic` on every over-threshold `view.shown[]` entry. The diagnostic is AST-derived — coverage gaps, complexity drivers, suggested actions, and a flat `root_cause` scalar — so coding agents can read findings and propose remediations without re-walking the source.

```jsonc
{
  "schema_version": 1,
  "view": {
    "shown": [{
      "scored": { "identity": { "qualified_name": "branchy_fail", ... } },
      "exceeds": true,
      "diagnostic": {
        "coverage_gaps": [{ "start": 11, "end": 19 }],
        "complexity_drivers": [{ "kind": "if-branch", "line": 12, "nesting_depth": 1, ... }],
        "suggested_actions": [
          { "kind": "add_tests_for_lines", "lines": [...], "applicability": "unspecified" },
          { "kind": "extract_function", "candidates": [
            { "line_range": { "start": 12, "end": 16 }, "complexity_contribution": 3,
              "branch_path": "if-branch", "kind": "deepest_nesting", "recommended": true },
            ...
          ], "applicability": "unspecified" }
        ],
        "root_cause": "both"
      }
    }]
  }
}
```

A grep-friendly stderr summary streams alongside stdout — one line per over-threshold function in `view.shown[]` order:

```text
[crap=56.00] src/lib.rs:5-21 branchy_fail [actions: add_tests_for_lines,extract_function]
```

`--format sarif` carries the same `Diagnostic` shape under `result.properties.diagnostic`, so SARIF consumers (and the [`/cut-the-crap` Claude Code skill](#claude-code-skill-cut-the-crap)) read the same advice as `--format advice`.

> **Stability:** `--format advice` is **experimental in v0.3.x**. The shape may grow additively (new fields, new `SuggestedAction` variants under `#[non_exhaustive]`), but `schema_version` stays at `1` and existing fields will not change meaning. The shape stabilises at v0.4.0 with `schema_version: 2`.

### SARIF for GitHub Code Scanning (`--format sarif`)

`--format sarif` emits SARIF v2.1.0 JSON. Pipe it into a `.sarif` file and upload via `github/codeql-action/upload-sarif@v3` — every function whose CRAP score exceeds the threshold becomes an inline annotation on the exact line range in the PR diff. Reviewers see the findings without running crap4rs themselves.

| Risk level     | SARIF `level` |
| -------------- | ------------- |
| `high`         | `error`       |
| `moderate`     | `warning`     |
| `acceptable`   | `note`        |
| `low`          | `note`        |

Severity is the [risk classification](#risk-classification), not the threshold gate — a `Moderate` function is a SARIF `warning` regardless of which preset is in effect.

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

### GitHub Actions inline annotations (`--format github-annotations`)

`--format github-annotations` emits one
[GitHub Actions `::warning` workflow command](https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions)
per function above threshold. The runner intercepts the commands from
stdout and renders them as inline annotations on the PR "Files
Changed" tab — same UX as SARIF, but with no GitHub Advanced Security
or Code Scanning subscription required and no per-line annotation
cap (other than the per-step UI cap below).

```bash
crap4rs --coverage lcov.info --format github-annotations
# ::warning file=src/lib.rs,line=42,title=CRAP 32.5::Function `tangled` has CRAP 32.50 (complexity=14, coverage=20.0%) which exceeds threshold 15.0
```

Like SARIF, this is a **gate translation**, not a display: the
reporter iterates the unshapeable analysis. `--top`, `--sort-by`,
`--only-failing`, and `--baseline` do **not** alter what's emitted —
PR annotations must reflect truth, not a presentation choice.

GitHub silently drops annotations past a per-step UI cap (10 warning
/ 10 error / 10 notice per step; 50 per job; 50 per workflow). Use
`--annotation-limit N` (default `10`, range `1..=100`) to cap
emission; over-cap eligible findings surface as a trailing
`::notice::N more functions exceed threshold; see scorecard for the
full list` line so reviewers know findings were dropped. Configurable
per project via `[output] annotation_limit` in `crap4rs.toml`; the
CLI flag wins when both are set.

The composite scorecard action exposes this via `annotations: true`
(see [`.github/actions/scorecard/README.md`](.github/actions/scorecard/README.md#inline-annotations)).
For a workflow that ships annotations alongside the markdown
scorecard in a single analyzer invocation, combine the formats:

```yaml
- run: crap4rs --coverage lcov.info --format markdown:scorecard.md,github-annotations --no-fail
  # `markdown:scorecard.md` writes the rendered scorecard to a file;
  # `github-annotations` flows through to stdout where the runner
  # intercepts the workflow commands.
```

### Scorecard row (`--format scorecard-row`)

`--format scorecard-row` emits exactly one JSON object — a
[`Row::CrapDelta`](crates/crap4rs/tests/fixtures/scorecard/schema.json)
— for consumption by scorecard aggregators (any CI workflow, PR-comment
bot, or dashboard that composes per-gate verdicts into a unified
scorecard). The object conforms to the locked `definitions/Row` schema
fragment owned by this repo (`schema_version: 1`; see
[`docs/scorecard-row-contract.md`](docs/scorecard-row-contract.md)).

Status is producer-side (crap4rs mints it from `--baseline` + `--threshold`):

| Status   | Trigger |
| -------- | ------- |
| `Red`    | A new threshold violation lands (Added function exceeds, or Modified function crosses the threshold). `failure_detail_md` carries the violator list. |
| `Yellow` | No new violations, but at least one modified function's CRAP score regressed. |
| `Green`  | Otherwise. |

```bash
# Run against a PR base baseline + emit a single Row JSON for the aggregator.
crap4rs \
  --src . \
  --coverage lcov.info \
  --baseline pr-base-envelope.json \
  --threshold 15 \
  --format scorecard-row > crap-delta-row.json
```

Sample output (Red):

```json
{
  "type": "CrapDelta",
  "id": "crap_delta",
  "label": "CRAP Δ",
  "anchor": "crap-delta",
  "status": "Red",
  "threshold": 15,
  "delta_count": 2,
  "delta_text": "5 → 7 (+2)",
  "failure_detail_md": "**New CRAP threshold violations (>15):**\n- ..."
}
```

When `--baseline` is omitted, the row reports the absolute count of
over-threshold functions (`"N over threshold (no baseline)"` in
`delta_text`); status is Green when the count is zero, Red otherwise.

> **Tradeoff:** the threshold lives in `crap4rs.toml` / `--threshold`,
> not in an aggregator's config. CRAP-metric parameters belong to the
> producer. If a future repo wants aggregator-side CRAP threshold
> tuning, that's a non-breaking evolution at the wire level — the row
> shape stays identical; status-minting moves from producer to
> aggregator. See [`docs/scorecard-row-contract.md`](docs/scorecard-row-contract.md)
> for the full operator-tunability boundary.

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

## Claude Code skill (`/cut-the-crap`)

crap4rs ships a reference [Claude Code](https://claude.com/claude-code) skill that consumes `--format advice` and drives a cover-then-split remediation loop on every over-threshold function. It lives in [`skills/cut-the-crap/`](skills/cut-the-crap/) — copy it into your user skills directory once and it's available from any Claude Code session:

```bash
# from the repo root
cp -r skills/cut-the-crap ~/.claude/skills/
```

Then in any Claude Code session:

```text
/cut-the-crap                       # cover-then-split, apply changes
/cut-the-crap --explain-only        # produce plan, do not modify
/cut-the-crap --threshold 15        # custom CRAP threshold
```

The skill handles the agent-loop side of remediation (covering uncovered branches first, naming proposed extractions, writing a plan to `tmp/cut-the-crap-plan.md` before applying); the crap4rs binary stays a unix-style mechanical emitter. See [`skills/cut-the-crap/SKILL.md`](skills/cut-the-crap/SKILL.md) for the full process specification.

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
git clone https://github.com/breezy-bays-labs/crap-rs.git
cd crap-rs
cargo build --release
```

## Prerequisites

- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) for generating LCOV coverage data

## Workspace layout

crap-rs is a Cargo workspace built on a hexagonal (ports & adapters) core:

| Crate | Role |
|-------|------|
| `crap-core` | Language-agnostic core — the CRAP formula, threshold model, result types, reporters, and analysis orchestration (`domain/` → `ports/` → `core/`). |
| `crap4rs` | Rust adapter — `syn`-based complexity walker, LCOV coverage parser, and the Rust CLI. |
| `crap4ts` | TypeScript / JavaScript adapter — `oxc`-based complexity walker, Istanbul JSON coverage parser, published to npm as a napi-rs addon. |

Each adapter supplies its own `ComplexityPort` and `CoveragePort` implementations; `crap-core` never imports a language toolchain. An `ast-purity` CI gate enforces this — it bans `syn`, `oxc`, and coverage-format types from `crap-core/src/`. The CRAP math is shared; threshold policy stays language-specific, so `crap4rs` and `crap4ts` need not use identical thresholds.

## Self-check

crap-rs analyzes its own source as a CI gate — `crap4rs` runs at `--strict` against each workspace crate (`crap-core`, `crap4rs`, and `crap4ts`).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms
or conditions.
