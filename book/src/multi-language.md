# Multi-language analysis

A polyglot repository has one CRAP score per function, in two scales that don't share a number line. crap4rs scores Rust functions (cognitive complexity by default); crap4ts scores TypeScript functions (cyclomatic complexity only). `crap-render` composes their JSON envelopes into one HTML report with a Combined view and an optional Delta view — without pretending the two scales are interchangeable.

For the analyzer flags that produce each envelope, see [CLI reference](cli-reference.md). For the CRAP formula itself, see [understanding CRAP](understanding-crap.md). This chapter owns the cross-language layer only: envelope production, the `crap-render` CLI, and the aggregation rules.

## Per-language coverage formats

Each adapter consumes the coverage format native to its toolchain. The formats are not interchangeable; pass each adapter the file its language emits.

| Language | Coverage file | Producer |
|----------|---------------|----------|
| Rust | `lcov.info` | `cargo llvm-cov --lcov` |
| TypeScript | `coverage-final.json` | Istanbul (`nyc`, `c8`, Jest, Vitest) |

The composite action auto-detects the adapter from the `coverage` input's extension (`.info`/`.lcov` → Rust, `.json` → TypeScript). See [CI integration](ci-integration.md).

## Producing envelopes

The unified report composes per-adapter JSON envelopes — the same `--format json` output each analyzer already emits. Run each analyzer over its own source root and coverage file:

```bash
crap4rs --coverage lcov.info --src crates/core/src --format json > rust.json
crap4ts --coverage coverage-final.json --src apps/web/src --format json > typescript.json
```

Each envelope carries its adapter's `tool_version`, `language` tag, complexity `metric`, and `threshold`, so `crap-render` reconstructs the per-adapter footer without re-running analysis.

## The crap-render CLI

`crap-render` ships with crap-core. It composes envelopes; it never analyzes source.

```bash
crap-render --input rust=rust.json --input typescript=typescript.json --format html --output report.html
```

| Flag | Meaning |
|------|---------|
| `--input <LANG>=<FILE>` | Pair an envelope with its language key. Repeatable; at least one required. |
| `--baseline <LANG>=<FILE>` | Optional baseline envelope, matched to the `--input` of the same key. Repeatable. |
| `--format html` | Output format. `html` is the only value today; the flag exists so future formats extend without a breaking change. |
| `--output <PATH>` | Write target. Omit to write to stdout. |
| `--threshold <N>` | Workspace threshold echoed in the scope banner. Defaults to the maximum across all per-envelope thresholds. |

The language key in `--input rust=...` — not the envelope's own `language` field — is the routing source of truth: it drives the segmented Language nav and the URL hash (`#rust:current`, `#typescript:delta`, `#combined:current`). Routing stays explicit so the renderer never has to trust per-envelope identity to place a block.

`<LANG>` is an arbitrary key. `rust` maps to the `crap4rs` / Rust labels and `typescript` to `crap4ts` / TypeScript; any other key falls through to itself as both tool name and display label. Adding a language is purely additive — supply another `--input` pair.

### Guards

`crap-render` fails fast on operator errors:

- Two envelopes for the same language key are refused (the common "passed two `rust.json`" mistake), on both `--input` and `--baseline`.
- A `--baseline <LANG>=...` whose key has no matching `--input` is an error.
- An envelope whose `schema_version` is outside the supported range fails with a message naming the path and the offending value. Upgrade the emitting adapter or the renderer.
- An `--input` envelope that omits `metric` or `threshold` renders with the defaults (cognitive / `0`) and a `note:` on stderr, so a hand-built envelope can't silently misrender.

## The unified report: Combined and Delta views

The report has two axes. The **Language** axis is a segmented nav (one panel per `--input`, plus a Combined panel). The **View** axis toggles **Current** vs **Delta** within a panel.

The **Combined** panel aggregates across every adapter into one scorecard plus one workspace-wide ranked function table. The **Delta** axis appears only when at least one `--baseline` was supplied; it shows what changed since the baseline, per language and combined.

### Cross-language aggregation rules

Aggregation is deliberately conservative. Raw CRAP scores are **not** comparable across adapters — cognitive and cyclomatic complexity scale differently, so a Rust function at CRAP 20 and a TypeScript function at CRAP 20 are not equivalent risk. The composer never sorts on raw cross-language CRAP.

**Additive counts (disjoint trees).** Each adapter scans a disjoint source tree, so `total_functions`, `exceeding_threshold`, `total_files`, and the per-tier risk distribution sum cleanly. "Exceeds" is decided per-adapter against that adapter's own threshold first, then counted — binary per function, so the sum is honest.

**Ranking by risk band, then ratio.** The Combined ranked table sorts by risk level descending, then by CRAP/threshold **ratio** descending within each band, with a stable tie-break on qualified name then file path. The ratio ("how far over this function's own threshold") is dimensionally consistent across adapters in a way raw CRAP is not. The same rule orders the Combined Delta regression table. Risk **bands** (the score-based classification) and the threshold **gate** are distinct axes that happen to share the 8/15/25 numbers today — see [understanding CRAP](understanding-crap.md).

**Consistent by construction.** Both adapters derive every function's risk level from one shared crap-core classifier and serialize it through one shared type. There is no per-language risk-classification step to drift, so the Combined ranking's band axis is consistent across adapters by construction, not by convention.

### Asymmetric baselines

Baselines are independent per language. Supplying `--baseline rust=...` but not `--baseline typescript=...` is allowed and well-defined:

- Languages **with** a baseline get a working Delta tab and contribute to the Combined Delta ranking.
- Languages **without** a baseline render the Delta tab **disabled** (a tooltip points reviewers to supply one); their Current view is unaffected.
- The Combined Delta panel lists `contributing_languages` and `missing_baseline_languages` so reviewers see that the Combined Delta represents fewer languages than the Combined Current view.

A baseline scored with a **different complexity metric** than its input is not diffed — cognitive and cyclomatic are incomparable scales, so the recomputed delta would be confident garbage. That language degrades to the disabled-Delta state with a metric-mismatch tooltip, a stderr warning, and an unchanged exit code; other languages are unaffected. An envelope that merely *omits* `metric` carries no evidence of disagreement, so absence never trips the guard.

Combined Delta is regression-focused: it ranks regressions and new functions. Improvements and removed functions are summarized but not ranked.

## CI and consumers

In CI, the composite action drives `crap-render` for you — set `html-report: true` with `languages: rust,typescript` (or `all`) and pass per-language `src`/`coverage` inputs. The action fetches the latest released envelope as a baseline to populate the Delta tab. See [CI integration](ci-integration.md). The manual `crap-render` invocation above is for local debugging and bespoke pipelines.
