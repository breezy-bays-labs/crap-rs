# `crap-scorecard` action

[![CRAP Scorecard](https://img.shields.io/badge/CRAP-scorecard-orange?logo=github)](https://github.com/breezy-bays-labs/crap-rs/blob/main/.github/workflows/examples/crap-scorecard.yml)

Runs `crap4rs` (Rust) or `crap4ts` (TypeScript) on a coverage file and
emits a markdown scorecard. Composable into aggregator PR-comment bots;
can also post its own sticky comment.

## Quick start

Copy [`.github/workflows/examples/crap-scorecard.yml`](../../workflows/examples/crap-scorecard.yml)
into your repo's `.github/workflows/` directory, change `coverage:` to
your coverage file path, push, done. The 4 preset inputs
(`threshold-preset`, `run-mode`, `gate-mode`, `languages`) cover the
common case in ~6 lines of YAML — no decisions about gate booleans,
threshold drift, or per-language metric defaults required.

Mixed Rust + TypeScript repo? Set `languages: rust,typescript` (or
`all`) and add `coverage-ts:` + `src-ts:`. Both adapters run in one
action invocation; ONE sticky comment with both scorecards stacked.
The crap4rs adapter defaults to cognitive complexity; crap4ts defaults
to cyclomatic — both language-appropriate. See [Notes](#notes) for the
invariant and [Multi-language](#multi-language) for the paired-input
contract.

Want the rendered HTML report (KPI tiles, file cards, optional dark
mode, optional delta tab)? Set `html-report: true` — uploaded as a
workflow artifact + linked in the sticky comment. See [HTML report](#html-report).

> **Fork PRs:** the templated example requests `pull-requests: write`,
> but GitHub issues a read-only `GITHUB_TOKEN` for `pull_request` events
> from forks regardless of the `permissions:` block. The sticky comment
> step silently no-ops on those runs. See
> [Pattern 2 — Standalone § Sticky comments and fork PRs](#sticky-comments-and-fork-prs)
> below for the two standard workarounds (`pull_request_target` with
> hardened checkout OR a separate `workflow_run`-triggered comment
> workflow). Same-repo branches are unaffected.

The rest of this document is reference material:
[Languages](#languages) / [Presets](#presets) /
[Multi-language](#multi-language) / [HTML report](#html-report) for
input-surface details, [Patterns](#patterns) for the three standard
integration shapes (minimal / standalone / aggregator),
[Inputs](#inputs) + [Outputs](#outputs) tables for the complete
surface, [Structured row output](#structured-row-output-outputsrow-json)
+ [Inline annotations](#inline-annotations) +
[Pinning](#pinning) + [Pre-installed binary (advanced)](#pre-installed-binary-advanced)
for power-user concerns, and [Notes](#notes) for design-decision
documentation.

## Languages

Polyglot interface. Rust (via `crap4rs`) and TypeScript (via `crap4ts`)
are both wired — the action auto-detects from the `coverage` file
extension (`.info`/`.lcov` → Rust, `.json` (Istanbul) → TypeScript) or
honors an explicit `language:` override.

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@main
  with:
    language: auto                 # rust | typescript | auto (infers from extension)
    coverage: lcov.info
```

## Presets

Four optional preset inputs cover the common-case configuration in one
line each — no decisions about gate booleans, threshold drift, or
per-language metric defaults required. Presets are **additive**: every
existing raw input continues to work unchanged, and an explicit raw
input always wins when set alongside a preset (the action emits a
`::warning::` on actual conflict so consumers notice the double-config).

| Input | Values | Derives |
|---|---|---|
| `threshold-preset` | `strict` \| `default` \| `lenient` | `threshold` = 8 / 15 / 25 (cognitive scale) |
| `run-mode` | `full` \| `delta` \| `both` | Baseline expectations (full: no baseline expected; delta/both: `baseline:` required) |
| `gate-mode` | `report-only` \| `gate-on-analysis` \| `gate-on-delta` \| `gate-on-both` | Atomic `analysis-gate` + `delta-gate` pair |
| `languages` | `rust` \| `typescript` \| `rust,typescript` \| `all` | Single-language supersedes `language:`; multi-language pairs Rust + TypeScript inputs and emits split outputs. See [Multi-language](#multi-language) |

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
  with:
    coverage: lcov.info
    threshold-preset: default
    run-mode: full
    gate-mode: report-only
    languages: rust
```

### Raw wins on conflict

When both a preset AND its corresponding raw input are set, the
explicit raw input wins. The action only emits a `::warning::` on
**actual conflict** — preset-derived value differs from the explicit
caller-supplied value — because GitHub Actions composite inputs cannot
distinguish "caller explicitly passed the default" from "caller did not
pass it and the default filled in." This sacrifices the nudge for "you
set both" while preserving the warning for "you set both and they
disagree" (the case that actually matters).

Example: `threshold-preset: strict` + `threshold: '20'` resolves to
`20` and emits a warning naming both values. `threshold-preset: default`
+ `threshold: '15'` resolves to `15` silently (no actual conflict).
Same pattern applies to `gate-mode` ↔ `analysis-gate` / `delta-gate`.

### Threshold-sync invariant

`threshold-preset` derives ONE threshold value internally; the gate
step, the inline-annotations step, and the analysis step all consume
that single derived value. Drift is structurally impossible (closes
the 6-hardcoded-places drift surface PR #282 fixed in the workflow
callers). The action exposes the resolved value via
`outputs.threshold-resolved` so consumers can audit the derivation.

### Per-language metric defaults

The action **never passes `--metric` to the adapter binary**. Each
adapter picks its language-appropriate default per the locked
`AdapterMeta::default_metric` decision: crap4rs uses cognitive, crap4ts
uses cyclomatic. The threshold values above (8 / 15 / 25) are the
**cognitive** calibration; the cyclomatic calibration is different but
applied automatically when the action dispatches to crap4ts (per
ADR (d) — see [crap-rs#218](https://github.com/breezy-bays-labs/crap-rs/issues/218)
for the metric-keyed calibration mechanism). The dogfood smoke in
PR δ (#295) asserts this invariant mechanically.

## Multi-language

For mixed Rust + TypeScript repos, set `languages: rust,typescript`
(or `languages: all`) and pair the Rust + TS coverage / source / baseline
inputs. Both adapters run in one action invocation; outputs split per
language so aggregators can drop each row in independently.

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
  id: crap
  with:
    languages: rust,typescript           # or `all`
    coverage:    lcov.info               # Rust LCOV
    coverage-ts: coverage-final.json     # TS Istanbul JSON
    src:    crates/                      # Rust source root
    src-ts: packages/                    # TS source root
    threshold-preset: default
    comment-mode: sticky
```

### Paired inputs

When multi-language mode is active, the historical input set carries
the Rust side and the new `*-ts` inputs carry the TypeScript side:

| Rust input | TypeScript input | Purpose |
|---|---|---|
| `coverage:` | `coverage-ts:` | Coverage file path (LCOV / Istanbul JSON) |
| `src:` | `src-ts:` | Source root passed to the analyzer |
| `baseline:` | `baseline-ts:` | Previously-captured CRAP JSON envelope (optional, per language) |

`coverage-ts:` and `src-ts:` are **required** when `languages:`
resolves to a multi-language set; the action errors actionably at
preset-resolution time when they're missing. `baseline-ts:` is
optional independently per language — a multi-language run can have
a Rust baseline + analysis-only TypeScript (or vice-versa, or no
baseline on either side).

### Split outputs

Two new outputs join the existing `row-json` for multi-language
aggregators:

- `outputs.row-json-rust` — populated when Rust is in the resolved
  language set (BOTH single-language `rust` and multi-language).
- `outputs.row-json-typescript` — populated when TypeScript is in the
  resolved language set.

Both conform to the same locked `Row::CrapDelta` schema as
`outputs.row-json` — see [Structured row output](#structured-row-output-outputsrow-json).

The legacy `outputs.row-json` stays populated for **single-language**
callers (back-compat: every consumer reading it today keeps working
unchanged). In **multi-language mode**, `outputs.row-json` is the
empty string and the action emits a `::warning::` directing
consumers to the split outputs. The warning surfaces the migration
path in the action's own log so a downstream `jq` parse doesn't
silently fail on empty input.

### Aggregation semantics

Per-language passed flags / counts collapse to single output values
via these rules:

| Output | Rule | Notes |
|---|---|---|
| `analysis-passed` | AND across languages | `true` only when every language passed |
| `delta-passed` | AND across languages (baseline-bearing ones only) | Empty string when no language ran with a baseline (back-compat) |
| `new-violations` | SUM across languages | Per-language counts add |
| `regressions` | SUM across languages | Per-language counts add |
| `markdown` | Combined string under `## CRAP scorecard` wrapper + per-language `### Rust (crap4rs)` / `### TypeScript (crap4ts)` H3 sections | Single-language: raw adapter markdown, no wrapper (back-compat) |
| `json-envelope-path` | Path to the FIRST language's envelope (canonical: Rust) | TS envelope at `$RUNNER_TEMP/crap4ts-envelope.json` |

Existing single-language consumers reading `analysis-passed` /
`delta-passed` see unchanged semantics. Multi-language consumers
need to know the AND/SUM contract (not "whichever ran last").

### Parsing edge cases

- `languages: "rust, typescript"` (whitespace) → tolerated; splits identically to the no-whitespace form
- `languages: "rust,rust"` → dedup collapses to single-language `rust` + `::warning::` so the input typo surfaces
- `languages: rust,typescript` without `coverage-ts:` set → preset-resolution error directing the caller to add the paired input
- `languages: garbage` → preset-resolution error naming the unsupported token

### One sticky comment, not two

`comment-mode: sticky` posts ONE comment under one `comment-header`,
containing the combined `markdown` output (both per-language sections
stacked). Aggregator workflows reading the split outputs and
re-rendering elsewhere should still set `comment-mode: 'none'`.

## HTML report

For PR reviews that benefit from the full file-by-file scorecard (KPI
tiles, distribution bar, file cards with sortable tables, optional dark
mode) — set `html-report: true`. The action renders an HTML report via
`<bin> --format html`, uploads it as a workflow artifact, and (when
`comment-mode: sticky`) appends a download link to the sticky comment
body.

When `baseline:` is supplied alongside `html-report: true`, the
rendered HTML automatically activates the Delta tab (Current vs
baseline) — no separate input needed.

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
  with:
    coverage: lcov.info
    baseline: /tmp/baseline.json
    html-report: true
    comment-mode: 'sticky'
```

| Input | Default | Notes |
|---|---|---|
| `html-report` | `false` | When `true`, render + upload + (sticky-only) inject link |
| `html-artifact-name-suffix` | `''` → `-${{ runner.os }}` | Disambiguates matrix-strategy uploads |

### Why artifact + link, not inline HTML

GitHub PR comments are rendered as **markdown only** — `<script>`,
`<style>`, inline `data:` images, and most arbitrary HTML/CSS are
stripped or sanitized. Embedding the rendered HTML in the comment
body therefore isn't structurally available. The artifact + link
pattern works with GitHub's existing UI: reviewers click the link,
GitHub authenticates them, the artifact downloads as a zip, and the
full report opens in any browser (the renderer is single-file +
self-contained, no external assets).

### Artifact naming + retention

- **Filename per language**: `crap4rs-report-<suffix>` (Rust) and/or
  `crap4ts-report-<suffix>` (TypeScript). The default suffix is
  `-${{ runner.os }}` so the common matrix-over-OS case produces
  cleanly distinguishable names (`crap4rs-report-Linux`,
  `crap4rs-report-macOS`). Override via `html-artifact-name-suffix`
  for matrix dimensions beyond OS (`-Linux-x86_64`, `-Linux-arm64`,
  …). `actions/upload-artifact@v4+` fails fatally on the second
  matrix leg attempting to upload to the same artifact name, so a
  unique suffix per leg is required for matrix-strategy workflows.
- **Retention**: 90 days (GitHub's default `actions/upload-artifact`
  retention window — long enough to cover the typical "review the PR
  within a quarter" use case). The action does not expose a
  retention override input in this release; if real consumer friction
  surfaces we can ship an additive `retention-days` input as a
  follow-up.

### Multi-language unified HTML

When `languages: rust,typescript` (or `all`) is set alongside
`html-report: true`, the action renders **one unified HTML
document** carrying both adapters' analyses with two
navigation axes:

- A **Language axis** (Sakura `.segmented` group at the top of the
  page) — switch between Rust, TypeScript, and the cross-adapter
  Combined view.
- A **View axis** (Sakura `.tabs` group inside each panel) —
  switch between **Current run** and **Delta vs baseline** within
  the active language panel. The View axis only renders when at
  least one adapter supplied a baseline. Languages without a
  baseline render their Delta tab disabled with a tooltip
  pointing at how to provide one.

URL hash routing supports both axes via the
`#<lang>:<view>` format — e.g. `#rust:delta` deep-links to the
Rust panel's Delta tab, `#combined:current` is the default first-
load target, and a deep-link to a disabled tab silently falls
back to `current`.

The sticky comment carries **one canonical link** plus two
deep-link anchors so reviewers can land directly on a per-language
panel:

```text
📊 [Open report](<unified-url>) · [Rust panel](<unified-url>#rust) · [TypeScript panel](<unified-url>#typescript)
```

The unified artifact uploads as `crap-scorecard-report-<suffix>`
(default suffix `-${{ runner.os }}`); the per-language artifact
names (`crap4rs-report-*` / `crap4ts-report-*`) are not used in
multi-language mode.

The unified URL is surfaced on the new `html-artifact-url` action
output. The legacy per-language outputs
(`html-artifact-url-rust` + `html-artifact-url-typescript`) now
resolve to `<unified-url>#rust` / `<unified-url>#typescript` deep-
link anchors and the action emits a `::warning::` deprecation
notice. Update aggregator workflows to read `html-artifact-url`
instead.

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
  id: crap
  with:
    languages: rust,typescript
    coverage:    lcov.info
    coverage-ts: coverage-final.json
    src:    crates/
    src-ts: packages/
    html-report: true
    comment-mode: 'none'             # aggregator pattern — no sticky

- name: Render combined report
  run: |
    echo "Unified:    ${{ steps.crap.outputs.html-artifact-url }}"
    echo "Rust panel: ${{ steps.crap.outputs.html-artifact-url }}#rust"
    echo "TS panel:   ${{ steps.crap.outputs.html-artifact-url }}#typescript"
```

#### How the unified render works

The unified document is produced by the `crap-render` binary that
ships with `crap-core` (≥ 0.7.0). In multi-language mode the action
runs three additive steps:

1. `Install crap-render` via `taiki-e/install-action@v2` (the binary
   is published to GitHub Releases via `crap-core`'s binstall
   packaging on each `crap-core-v<version>` tag).
2. `Render unified HTML` invokes
   `crap-render --input rust=<rs-envelope> --input typescript=<ts-envelope>
   --format html --output <runner.temp>/crap-scorecard-unified.html`
   composing the per-language envelopes already captured by the
   `Run analysis` loop above.
3. `Upload unified HTML report` uploads the rendered file as
   `crap-scorecard-report-<suffix>`.

Single-language mode (`languages: rust` or `languages: typescript`)
skips all three steps and falls back to the per-language artifact
upload pattern unchanged — back-compat invariant for existing
consumers.

#### Adapter schema compatibility

`crap-render` enforces that all input envelopes carry a
`schema_version` in `{1, 2}`. If you upgrade one adapter (e.g.
`crap4rs` to a version emitting `schema_version: 3`) but leave the
other on an older version emitting `schema_version: 1` or `2`,
`crap-render` will fail fast with an actionable error rather than
silently produce a mangled combined view. Keep `crap4rs` and
`crap4ts` reasonably up-to-date; major version bumps are documented
in each crate's CHANGELOG.

### Aggregator pattern — `comment-mode: none` + `html-report: true`

The action still uploads both artifacts and surfaces their URLs as
outputs even when `comment-mode: 'none'` — there is no sticky
comment to inject a link into, but the URLs are still available
programmatically. Aggregator workflows that compose multiple metric
rows into one comment can read the HTML-artifact URL outputs and
render their own link wherever appropriate (sticky comment, Check
Run summary, Slack message, …).

The artifact URLs only resolve for requests **authenticated with
GitHub** (per the GH API contract on artifact download URLs), so the
links are meant for human-followable surfaces (sticky comments,
Check Run summaries) — not in-job `curl` retrieval.

## Patterns

The three standard integration shapes. Pattern 1 is what the [Quick
start](#quick-start) section above expands into; Pattern 2 adds
baseline orchestration for delta-mode runs; Pattern 3 is the
aggregator pattern (one row of a richer metrics comment).

### Pattern 1 — Minimal (analysis only, no delta)

```yaml
- uses: actions/checkout@v4
- run: cargo llvm-cov --workspace --lcov --output-path lcov.info
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@main
  id: crap
  with:
    coverage: lcov.info
    threshold: '15'
- run: echo "${{ steps.crap.outputs.markdown }}"
```

### Pattern 2 — Standalone (analysis + baseline + sticky)

The caller is responsible for producing the baseline JSON envelope (typically
a coverage run on the PR base). This action does **not** auto-checkout the
base ref — that's a deliberate boundary. CI orchestrates two passes; the
analyzer doesn't reach into git.

```yaml
permissions:
  pull-requests: write   # required for sticky comments

jobs:
  scorecard:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0   # need both refs for the base coverage run

      # 1. Run coverage on the PR base, capture the JSON envelope.
      - name: Capture baseline
        run: |
          git switch --detach origin/${{ github.base_ref }}
          cargo llvm-cov --workspace --lcov --output-path /tmp/base.lcov
          cargo install --locked crap4rs
          crap4rs --coverage /tmp/base.lcov --src . --format json --no-fail > /tmp/baseline.json
          git switch -

      # 2. Run coverage on HEAD.
      - name: Capture HEAD coverage
        run: cargo llvm-cov --workspace --lcov --output-path /tmp/head.lcov

      # 3. Render the scorecard, post sticky comment.
      - uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@main
        with:
          coverage: /tmp/head.lcov
          baseline: /tmp/baseline.json
          threshold: '15'
          delta-gate: 'false'        # measurement, not gating
          comment-mode: 'sticky'
          comment-header: 'crap-scorecard'
```

#### Sticky comments and fork PRs

GitHub issues a **read-only `GITHUB_TOKEN`** for `pull_request` events
triggered from forks, regardless of the job-level `permissions:` block. The
`comment-mode: sticky` step will silently no-op (or fail to post) on those
runs. Same-repo branches are unaffected.

If you need fork-PR coverage, two well-trodden options:

- Switch the trigger to `pull_request_target` with hardened checkout (don't
  check out untrusted code in the same job that holds write tokens).
- Move the comment step to a separate `workflow_run`-triggered workflow
  that has its own writable token.

Most projects pick "no fork-PR sticky comments" until they have a concrete
need — same-repo coverage is sufficient for internal review.

### Pattern 2b — Cross-release envelope baseline

**WARNING — semantics differ from Pattern 2.** Pattern 2 fetches a
baseline that describes **your** code's prior state (typically the
PR's merge base) so the Delta tab shows what your PR changed.
Pattern 2b fetches a baseline that describes **crap-rs's** view of a
fixed pedagogical sample, published as a release asset
(`crap4rs-envelope.json` on every crap-rs release page). The Delta
tab in Pattern 2b shows what changed in the analyzer's output across
crap-rs versions — NOT what changed in your code.

If you want main-vs-PR deltas in your own codebase, use Pattern 2.

> [!WARNING]
> Pattern 2b's baseline describes crap-rs's drift, not your code's.
> Use Pattern 2 (`Capture baseline` step that runs crap4rs on the
> PR base) when you want to see what your PR changed. Pattern 2b is
> useful when (a) you're dogfooding the action against the
> pedagogical sample at `crates/crap-examples/`, or (b) you want
> the Delta tab to render in an enabled state on every run without
> a per-PR baseline-capture step.

```yaml
# === WARNING — Pattern 2b semantics =================================
# The baseline below describes crap-rs's analysis of the fixed
# crates/crap-examples/ corpus, not your codebase. The Delta tab
# renders cross-release drift in the analyzer, NOT your PR's
# code-change delta. See README § Pattern 2b for the contrast with
# Pattern 2 (per-PR baseline).
# ====================================================================
- name: Fetch crap-rs envelope baseline
  id: fetch-baseline
  continue-on-error: true   # bootstrap-window fallback (see below)
  env:
    GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  run: |
    gh release download \
      --repo breezy-bays-labs/crap-rs \
      --pattern 'crap4rs-envelope.json' \
      --dir "$RUNNER_TEMP"

- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
  with:
    coverage: lcov.info
    src: ./crates/crap-examples/src
    baseline: ${{ steps.fetch-baseline.outcome == 'success' && format('{0}/crap4rs-envelope.json', runner.temp) || '' }}
    run-mode: ${{ steps.fetch-baseline.outcome == 'success' && 'both' || 'full' }}
    gate-mode: report-only   # the sample scores poorly by design
    comment-mode: sticky
    html-report: true
```

**Bootstrap-window fallback.** `continue-on-error: true` on the
fetch step + conditional `run-mode` keeps the smoke green when no
envelope-bearing release exists yet (the first time a consumer
wires this pattern, or after a release-plz outage). Add a
`::warning::` annotation in the fetch-failure branch so a
steady-state regression (publication silently broken) is
distinguishable from a normal first-fire fallback in CI logs.

**Multi-language variant.** Add `coverage-ts:`, `src-ts:`, and a
second `gh release download` for `crap4ts-envelope.json` paired with
`baseline-ts:`. Both envelopes attach to every crap-rs release page
in one atomic upload, so they're always in sync.

### Pattern 3 — Aggregator (one row of a richer metrics comment)

When a PR-metrics aggregator composes coverage + CRAP + mutation +
module size into one sticky comment, this action contributes the CRAP
row; the aggregator owns the comment.

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@main
  id: crap
  with:
    coverage: /tmp/head.lcov
    baseline: /tmp/baseline.json
    comment-mode: 'none'           # outputs only — no comment of our own

- uses: ./.github/actions/coverage-delta
  id: coverage
  # ... whatever your coverage-delta step is

- name: Post combined metrics comment
  uses: marocchino/sticky-pull-request-comment@v2
  with:
    header: 'pr-metrics'
    message: |
      ## PR metrics

      ### Coverage
      ${{ steps.coverage.outputs.markdown }}

      ${{ steps.crap.outputs.markdown }}
```

## Inputs

| Name | Default | Notes |
|---|---|---|
| `language` | `auto` | `rust`, `typescript`, or `auto` (infer from coverage extension). Superseded by `languages` (preset) when both are set |
| `coverage` | (required) | Path to LCOV (`.info`) for Rust, Istanbul JSON for TS. In multi-language mode, carries the Rust LCOV (paired with `coverage-ts:`) |
| `coverage-ts` | `''` | (paired) TypeScript Istanbul JSON path. Required in multi-language mode; ignored in single-language mode. See [Multi-language](#multi-language) |
| `src` | `.` | Source root passed to the analyzer. In multi-language mode, carries the Rust source root |
| `src-ts` | `''` | (paired) TypeScript source root. Required in multi-language mode; ignored in single-language mode. See [Multi-language](#multi-language) |
| `baseline` | `''` | Path to a previously-captured CRAP JSON envelope. Empty = no delta. In multi-language mode, carries the Rust baseline |
| `baseline-ts` | `''` | (paired) TypeScript baseline JSON envelope. Optional even in multi-language mode (per-language); a language without a baseline contributes neutrally to AND/SUM aggregation. See [Multi-language](#multi-language) |
| `threshold` | `15` | Threshold for violations (user-visible default; the action.yml default is empty so the preset resolver can tell explicit from default — see [Presets — Raw wins on conflict](#raw-wins-on-conflict)) |
| `config` | `''` | Path to `crap4rs.toml` |
| `delta-gate` | `false` | Exit non-zero on new violations (only when baseline supplied). User-visible default; empty internally — see [Presets](#presets) |
| `analysis-gate` | `false` | Exit non-zero if the analysis itself fails. User-visible default; empty internally — see [Presets](#presets) |
| `comment-mode` | `none` | `none` (outputs only) or `sticky` (post/update sticky comment) |
| `comment-header` | `crap-scorecard` | Sticky-comment identifier |
| `version` | `latest` | crap4rs version to install (`latest` or pinned tag) |
| `annotations` | `false` | When `true`, emit `::warning` workflow commands so findings render inline on the PR Files Changed tab (see [Inline annotations](#inline-annotations)) |
| `annotation-limit` | `''` | Cap on emitted annotations when `annotations: true`. Empty defers to the adapter's default (10) or `[output] annotation_limit` from `config`. Range 1..=100 |
| `html-report` | `false` | When `true`, render `<bin> --format html` as a workflow artifact and (sticky-only) append a download link to the comment body. Per-language in multi-language mode. See [HTML report](#html-report) |
| `html-artifact-name-suffix` | `''` (→ `-${{ runner.os }}`) | Suffix appended to the HTML artifact name(s) to disambiguate matrix-strategy uploads. See [HTML report — Artifact naming + retention](#artifact-naming--retention) |
| `threshold-preset` | `''` | (preset) `strict` (8) \| `default` (15) \| `lenient` (25). Derives `threshold`; raw `threshold:` wins on conflict + warning. See [Presets](#presets) |
| `run-mode` | `''` | (preset) `full` \| `delta` \| `both` — drives baseline expectations. `delta`/`both` require `baseline:` set. See [Presets](#presets) |
| `gate-mode` | `''` | (preset) `report-only` \| `gate-on-analysis` \| `gate-on-delta` \| `gate-on-both`. Atomic `analysis-gate` + `delta-gate` pair; raw inputs win on conflict + warning. See [Presets](#presets) |
| `languages` | `''` | (preset) `rust` \| `typescript` \| `rust,typescript` \| `all`. Single-language supersedes `language:`; multi-language pairs Rust + TypeScript inputs and emits split outputs. See [Multi-language](#multi-language) |

## Outputs

| Name | Notes |
|---|---|
| `markdown` | The rendered scorecard — drop into aggregator comments verbatim. Multi-language: combined under `## CRAP scorecard` wrapper with per-language `### Rust (crap4rs)` / `### TypeScript (crap4ts)` H3 sections. Single-language: raw adapter markdown (back-compat) |
| `row-json` | `Row::CrapDelta` JSON object — for aggregators that re-render with full layout control. See [Structured row output](#structured-row-output-outputsrow-json). **Empty in multi-language mode** + `::warning::` directing consumers to `row-json-rust` / `row-json-typescript` |
| `row-json-rust` | (multi-language split) `Row::CrapDelta` JSON for the Rust dispatch when Rust is in the resolved language set. Populated in both single-language `rust` and multi-language modes; empty otherwise. See [Multi-language — Split outputs](#split-outputs) |
| `row-json-typescript` | (multi-language split) `Row::CrapDelta` JSON for the TypeScript dispatch when TypeScript is in the resolved language set. Populated in both single-language `typescript` and multi-language modes; empty otherwise. See [Multi-language — Split outputs](#split-outputs) |
| `json-envelope-path` | Path to the full JSON envelope on the runner. Multi-language: first language's envelope (canonical: Rust); TS envelope at `$RUNNER_TEMP/crap4ts-envelope.json` |
| `analysis-passed` | `true` / `false`. Multi-language: AND across languages. See [Multi-language — Aggregation semantics](#aggregation-semantics) |
| `delta-passed` | `true` / `false`, or empty string when no baseline. Multi-language: AND across languages that ran with a baseline; empty when none did |
| `new-violations` | Count of functions exceeding threshold but not in baseline. Multi-language: SUM across languages |
| `regressions` | Count of modified functions whose CRAP increased above rendering precision. Multi-language: SUM across languages |
| `threshold-resolved` | The threshold value the action's `Resolve presets` step resolved (preset-derived or raw passthrough). See [Presets — Threshold-sync invariant](#threshold-sync-invariant) |
| `html-artifact-url` | (html-report, NEW in PR ζ #315) Workflow artifact URL for the rendered HTML report. In single-language mode this is the per-language artifact URL; in multi-language mode this is the unified artifact URL. See [Multi-language unified HTML](#multi-language-unified-html) |
| `html-artifact-url-rust` | (html-report) Workflow artifact URL for the rendered Rust HTML report. **DEPRECATED in multi-language mode**: resolves to `<unified-url>#rust` deep-link anchor + `::warning::` directing consumers to `html-artifact-url`. In single-language `rust` mode the behavior is unchanged (URL of the standalone Rust artifact). See [Multi-language unified HTML](#multi-language-unified-html) |
| `html-artifact-url-typescript` | (html-report) Workflow artifact URL for the rendered TypeScript HTML report. **DEPRECATED in multi-language mode**: resolves to `<unified-url>#typescript` deep-link anchor + `::warning::`. In single-language `typescript` mode the behavior is unchanged. See [Multi-language unified HTML](#multi-language-unified-html) |

## Structured row output (`outputs.row-json`)

`outputs.row-json` is the raw stdout of `<adapter> --format scorecard-row` —
one `Row::CrapDelta` JSON object, conforming to the locked scorecard
schema fragment at
[`crates/crap4rs/tests/fixtures/scorecard/schema.json`](../../../crates/crap4rs/tests/fixtures/scorecard/schema.json)
(the `CrapDelta` member of `definitions/Row` `oneOf`). No envelope, no
metadata wrapper — the JSON object is the contract.

Use `outputs.row-json` when an aggregator workflow re-renders the
scorecard with full layout control. Use `outputs.markdown` when posting
the scorecard verbatim into a sticky comment.

Producer-side status policy (Red/Yellow/Green):

- **Red** — at least one new threshold violation landed.
- **Yellow** — no new violations, but at least one modified function's
  CRAP score regressed.
- **Green** — otherwise.

### Aggregator pattern — consume `row-json` and re-render

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@main
  id: crap
  with:
    coverage: /tmp/head.lcov
    baseline: /tmp/baseline.json
    comment-mode: 'none'           # outputs only — aggregator owns the comment

- name: Aggregate scorecard rows
  id: agg
  shell: bash
  env:
    CRAP_ROW: ${{ steps.crap.outputs.row-json }}
    # ... other producers' rows here ...
  run: |
    # Concatenate Row JSONs into a Scorecard, render however the
    # aggregator likes (a custom markdown layout, a Check Run summary,
    # a Slack message, ...). Guard against the documented version-gap
    # window (crap4rs < 0.4.0) where row-json is empty — `jq` would
    # otherwise error on a non-JSON input.
    if [ -n "$CRAP_ROW" ]; then
      printf '%s\n' "$CRAP_ROW" > /tmp/crap-row.json
      jq -r '"### " + .label + " — **" + .status + "** — " + .delta_text' /tmp/crap-row.json
    fi
```

`outputs.markdown` remains available unchanged — existing consumers
continue to read it without modification.

### Version requirement

`outputs.row-json` is populated for **`crap4rs ≥ 0.4.0`**. With older
releases the action emits an empty string and prints a workflow
warning; the rest of the action (`outputs.markdown`, gates, sticky
comment) keeps working. Pin the `version` input to `0.4.0` or later
once it publishes if your workflow consumes `row-json`.

## Inline annotations

When `annotations: true`, the action runs a second pass of the
analyzer with `--format github-annotations`. That format emits one
`::warning file=...,line=...,title=CRAP <score>::<message>` workflow
command per function above threshold; the GitHub Actions runner reads
these from the step's stdout and renders them as inline annotations
on the PR's "Files Changed" tab. No GHAS or Code Scanning subscription
needed.

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@main
  with:
    coverage: lcov.info
    threshold: '15'
    annotations: 'true'
    annotation-limit: '10'   # optional; default 10
```

GitHub silently drops annotations past a per-step UI cap (10 warning,
10 error, 10 notice per step; 50 per job; 50 per workflow). The
adapter caps emission at `annotation-limit` (default `10`) and
appends a trailing
`::notice::N more functions exceed threshold; see scorecard for the full list`
line so reviewers know findings were dropped. The full set always
appears in `outputs.markdown`.

`annotation-limit` can also live in the project's config — set
`[output] annotation_limit = N` in `crap4rs.toml` (or `crap4ts.toml`)
and pass the file via `config:`. The CLI input wins when both are set.

## Pinning

Examples above use `@main` for clarity. **Pin to a commit SHA in production
workflows** so a regression in the action can't break your CI without you
noticing:

```yaml
uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
```

When the action is folded into the future
[crap monorepo](https://github.com/breezy-bays-labs/ops/issues/231) it'll get
a Marketplace listing with proper version tags; for now `@main` is the only
moving ref and SHA-pinning is the safe default.

## Pre-installed binary (advanced)

When `version: latest` (the default) and a `crap4rs` binary is already on
`PATH` before the action runs, the install step is **skipped** and the
action uses the pre-installed binary. This supports two use cases:

- **`moonrepo/setup-rust` bins.** If a prior step already installed
  `crap4rs` via setup-rust's `bins:` parameter, the action respects it
  rather than reinstalling.
- **End-to-end self-dogfooding.** A workflow that builds `crap4rs` from
  source and copies it to `~/.cargo/bin/` before invoking the action gets
  the action to render via that binary — useful for the project's own CI
  to validate the renderer against itself.

When `version` is pinned to a specific tag (e.g. `version: '0.2.2'`), the
action **always reinstalls** that exact version, ignoring whatever happens
to be on `PATH`. Pinned-version semantics override pre-installed semantics.

> **Tag-prefix cutover.** Releases up to and including `crap4rs` 0.5.0
> live under `v{version}` tags; from the first release-plz-driven
> publish onward, the canonical install path is
> `cargo binstall crap4rs@<version>` and tarballs land under the
> per-crate `crap4rs-v{version}` tag pattern. crates.io-registered
> binstall metadata carries the right URL for each version, so users
> never need to choose the tag pattern by hand —
> `cargo binstall crap4rs@<version>` resolves correctly across the cutover.

```yaml
# Example: dogfood a workspace-local build
- run: cargo build --release
- run: install -m 0755 ./target/release/crap4rs "$HOME/.cargo/bin/crap4rs"
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@main
  with:
    coverage: lcov.info
    # version defaults to 'latest' → install step short-circuits
```

## Design notes

- **Baseline source is the caller's choice.** Auto-checkout-and-cache is a
  natural future enhancement (artifact cache keyed on base SHA), but the
  v1 contract keeps orchestration explicit because that's what made the
  v0.2.0 baseline design clean: the analyzer never reaches into git.
- **Measurement-first.** Both gates default off. The action's primary value
  is the scorecard string; gates are opt-in for repos that want hard CI
  failures.
- **`comment-mode` separates plumbing from content.** The same scorecard
  string serves standalone bots and aggregators — only the post step
  changes.

## Notes

### Metric-default invariant (per-language)

The action **never passes `--metric` to the adapter binary**. Each
adapter picks its language-appropriate default per
`AdapterMeta::default_metric`:

| Adapter | Default metric | Rationale |
|---|---|---|
| `crap4rs` | **cognitive** (SonarSource S3776 / G. Ann Campbell) | Idiomatic Rust frequently uses `match`/`if let` chains that cyclomatic complexity over-counts; cognitive complexity is calibrated for the readability cost humans actually pay |
| `crap4ts` | **cyclomatic** (McCabe 1976) | Established TypeScript/JavaScript convention (ESLint, SonarJS); the ecosystem's existing tooling and thresholds are calibrated for cyclomatic |

The cognitive-vs-cyclomatic decision is a per-language calibration —
not a knob the action exposes — because the threshold values shipped
with the preset surface (`threshold-preset: strict|default|lenient`)
are calibrated against each adapter's chosen metric. Flipping metrics
without re-calibrating thresholds would silently misreport. The
`Presets` § [Per-language metric defaults](#per-language-metric-defaults)
above documents the threshold-calibration mechanics; this section
documents the higher-level invariant.

Override is available per-crate via the `config:` input — point at a
`crap4rs.toml` (or `crap4ts.toml`) with `[analysis] metric = "..."` —
but the override carries the responsibility of re-calibrating the
threshold yourself. The repo's
[quick-start dogfood smoke](../../workflows/quick-start-smoke.yml)
mechanically asserts this invariant via
`<bin> --format json | jq -r '.metric'`, so a refactor that flips a
default fails CI loudly. See
[crap-rs#218](https://github.com/breezy-bays-labs/crap-rs/issues/218)
for the metric-keyed calibration mechanism + ADR rationale.

### Why composite, not standalone

The scorecard is typically one row in a richer "PR metrics" comment
that aggregates multiple quality signals (coverage, CRAP, mutation,
module size, architecture violations, etc.). This action exposes the
rendered markdown as an **output** so an aggregator job can drop it
into a larger sticky comment without two bots fighting over the same
comment. For repos that only care about CRAP, set
`comment-mode: sticky` and the action manages its own sticky comment.
