# CLI reference

`crap4rs` (Rust) and `crap4ts` (TypeScript) share one flag surface. crap-core drives both binaries; each binary supplies only its language metadata, default complexity metric, and help examples. Every flag below works identically on both unless noted.

Run a binary's own `--help` for the live, version-stamped surface. This chapter is the durable reference.

```bash
crap4rs --coverage lcov.info
crap4ts --coverage coverage-final.json
```

`--coverage` is required for analysis (see [Input](#input)). For the file formats themselves, see [output formats](output-formats.md); for the CRAP math, [understanding CRAP](understanding-crap.md); for config-file keys, [configuration](configuration.md).

## Subcommands

When a subcommand is present, the analysis path is skipped and `--coverage` is not required.

| Subcommand | Effect |
|---|---|
| `completions <SHELL>` | Print a shell completion script to stdout. `<SHELL>` is one of `bash`, `zsh`, `fish`, `powershell`, `elvish`, `nushell`. |
| `init` | Write an exhaustive, annotated starter config to the canonical config file name in the current directory. Refuses to overwrite an existing file unless `--force` is passed. |

```bash
crap4rs completions zsh > _crap4rs
crap4rs init            # writes crap.toml (annotated; every key documented)
crap4rs init --force    # overwrite an existing config
```

`init` writes the same deterministic document regardless of flags. `--non-interactive` is accepted for back-compat but no longer changes the output. The generated config documents every supported key; trim it to what you use. Key reference: [configuration](configuration.md).

## Input

| Flag | Value | Default | Notes |
|---|---|---|---|
| `--coverage` | `FILE` | — | Coverage file in the adapter's format (`lcov.info` for Rust, `coverage-final.json` for TS). Required at runtime for analysis; clap does not mark it required so subcommands work without it. |
| `--src` | `DIR` | `src` | Source root to analyze. **Repeatable** — pass more than once to union several roots into one report against the shared `--coverage`. Falls back to the config `src` list, then `src`. |
| `--metric` | `cognitive` \| `cyclomatic` | per-adapter | See below. |
| `--missing-coverage-policy` | `pessimistic` \| `optimistic` \| `skip` | `pessimistic` | How to score a function whose source file is absent from the coverage data. The pessimistic default scores such functions as 0% covered (`CRAP = c² + c`) — a deliberate choice that never hides risk, not a measured truth. |
| `--config` | `FILE` | auto-discover | Path to the config TOML. Default discovers the adapter's config file in the working directory. See [configuration](configuration.md). |
| `--view` | `NAME` | — | Apply a saved view preset from the config TOML. CLI flags override the preset's fields. See [configuration](configuration.md). |
| `--baseline` | `FILE` | — | A previously emitted JSON envelope, used as the baseline for delta analysis. Delta is informational unless `--delta-gate` is set. |

### `--metric` default differs per adapter

`crap4rs` defaults to **cognitive** complexity (it captures match arms and nested control flow better for Rust) and also accepts `--metric cyclomatic`.

`crap4ts` is **cyclomatic-only**. It defaults to cyclomatic; passing `--metric cognitive` exits 2 with an "is not yet supported" error.

The complexity metric and the CRAP threshold both differ in magnitude between metrics; the threshold defaults are calibrated per metric.

## Output

| Flag | Value | Default | Notes |
|---|---|---|---|
| `--format` / `-f` | spec list | `table` | Comma-separated list of `FORMAT` (stdout) or `FORMAT:FILE` (write to file). One analysis pass fans out to multiple sinks. At most one stdout entry. Formats and samples: [output formats](output-formats.md). |
| `--threshold` | `FLOAT` | `15` | CRAP cutoff — functions above it fail the gate. Defaults to the calibrated cutoff for the active metric. Mutually exclusive with `--strict` / `--lenient`. |
| `--strict` | flag | — | Strict preset cutoff (`8`). Mutually exclusive with `--threshold` / `--lenient`. |
| `--lenient` | flag | — | Lenient preset cutoff (`25`). Mutually exclusive with `--threshold` / `--strict`. |
| `--no-fail` | flag | off | Always exit 0, even with violations. See [exit codes](#exit-codes). |
| `--delta-gate` | flag | off | Fail (exit 1) when the `--baseline` comparison introduces new violations. Requires `--baseline`. |
| `--threshold-epsilon` | `EPS` | `0.0` | Border-jitter suppression band for the delta gate. Requires `--baseline`. Finite, non-negative. |
| `--minimal-view` | flag | off | Omit the denormalized `view.shown` row array from JSON. The gate is unaffected. JSON only. |
| `--summary` | flag | off | Emit a single-line verdict instead of the full report. Short-circuits `--format`. |
| `--annotation-limit` | `N` (1–100) | `10` | Cap on `::warning` annotations emitted by `--format github-annotations`. Ignored by every other format. |

There is **no `--threshold-preset` flag** on the binary. Choose a preset with `--strict` / `--lenient`, or set `preset` in the config TOML ([configuration](configuration.md)). The composite action exposes a separate `threshold-preset` input ([CI integration](ci-integration.md)).

### Threshold gate vs risk bands

The threshold gate (`--threshold` / `--strict` / `--lenient`) decides exit code and the per-function `exceeds` flag, whereas the score-based risk bands are a separate axis used for display and ranking; [understanding CRAP](understanding-crap.md) covers how the two differ and why the `8 / 15 / 25` cutoffs are a calibration convention.

## Filtering

| Flag | Value | Default | Notes |
|---|---|---|---|
| `--exclude` | glob | — | Glob to exclude from analysis. Repeatable. `target/` (Rust) and similar build output are excluded via `.gitignore`; test files are **not** excluded by default. |
| `--no-gitignore` | flag | off | Analyze all files regardless of `.gitignore`. |
| `--diff` | `REF` | — | Only analyze functions in files changed since the given git ref. |
| `--only-failing` | flag | off | Display only functions above threshold. **Display-only** — see below. |
| `--min-coverage` | `PCT` | — | Lower bound (inclusive) on coverage percent for the displayed view. Display-only. |
| `--max-coverage` | `PCT` | — | Upper bound (inclusive) on coverage percent for the displayed view. Display-only. |
| `--sort-by` | `crap` \| `coverage` \| `complexity` \| `path` | `crap` | Sort key for the displayed view. Display-only. |
| `--top` | `N` | no limit | Truncate the displayed view to the top N rows (`0` = no limit). Display-only. |
| `--group-by` | `file` | — | Aggregate the displayed view by file. `--top` then truncates files, not functions. Display-only. |
| `--delta-top` | `N` | no limit | Truncate the delta block (independent of `--top`). |
| `--delta-sort` | `score-delta` \| `current-crap` \| `baseline-crap` \| `path` | `score-delta` | Sort key for the delta block. |
| `--delta-only` | kinds | all | Comma-separated `added`, `removed`, `modified` for the delta block. |

### Display-only flags never move the gate

`--top`, `--sort-by`, `--only-failing`, `--min-coverage`, `--max-coverage`, and `--group-by` shape what the report *shows*. They never alter the analysis that drives the exit code. `result.passed` in JSON output, every aggregate (`average_crap`, `median_crap`, `distribution`), and the exit code always reflect the **full, unfiltered** codebase. Truncating violations out of the view with `--top 5` does not make a failing run pass.

When you need the gate to see a narrower set, narrow the *analysis* input instead — scope `--src`, `--exclude`, or `--diff`.

## Display

| Flag | Value | Default | Notes |
|---|---|---|---|
| `--color` | `auto` \| `always` \| `never` | `auto` | Terminal color. `auto` colorizes only when writing to a TTY. |
| `--verbose` / `-v` | flag | off | Show parse diagnostics and matching statistics. |
| `--quiet` / `-q` | flag | off | Suppress report output; set exit code only. Wins over `--summary`. |
| `--breakdown` | flag | off | Show complexity contributors for above-threshold functions in table output. JSON always includes contributors. |
| `--explain` | flag | off | Explain nested breakdown increments. Table only, and only with `--breakdown`. |
| `--md-full-table` | flag | off | Append the full per-function table to `--format markdown`. Markdown only. |
| `--md-top` | `N` | `10` | Rows in the markdown top-N table. Markdown only. |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Analysis passed — no function exceeds the threshold. Also returned by `init` and `completions`, and by any run with `--no-fail`. |
| `1` | At least one function exceeds the threshold (or, with `--delta-gate`, the baseline comparison introduced new violations). |
| `2` | The run errored — missing `--coverage`, an unreadable file, an unsupported `--metric`, or any other failure. |

`--no-fail` overrides the exit-code translation only: a failing analysis exits `0`, but `result.passed` (and `delta.summary.passed` under `--delta-gate`) still reports the truthful state, so a consumer can detect a would-have-failed run from the JSON. Compose `--no-fail` with `--quiet` for silent success in CI.
