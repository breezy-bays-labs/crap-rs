# `crap-scorecard` action

Runs `crap4rs` on a coverage file and emits a markdown scorecard. Composable
into aggregator PR-comment bots; can also post its own sticky comment.

## Why composite, not standalone

The scorecard is typically one row in a richer "PR metrics" comment
that aggregates multiple quality signals (coverage, CRAP, mutation,
module size, architecture violations, etc.). This action exposes the
rendered markdown as an **output** so an aggregator job can drop it
into a larger sticky comment without two bots fighting over the same
comment.

For repos that only care about CRAP, set `comment-mode: sticky` and the
action manages its own sticky comment.

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
| `languages` | `rust` \| `typescript` \| `rust,typescript` \| `all` | Single-language supersedes `language:`; multi-language (PR β #293) lands separately |

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

## Minimal usage — analysis only, no delta

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

## Standalone PR-comment bot — analysis + baseline + sticky

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

### Sticky comments and fork PRs

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

## Aggregator pattern — one row of a richer metrics comment

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
| `coverage` | (required) | Path to LCOV (`.info`) for Rust, Istanbul JSON for TS |
| `src` | `.` | Source root passed to the analyzer |
| `baseline` | `''` | Path to a previously-captured CRAP JSON envelope. Empty = no delta |
| `threshold` | `15` | Threshold for violations (user-visible default; the action.yml default is empty so the preset resolver can tell explicit from default — see [Presets — Raw wins on conflict](#raw-wins-on-conflict)) |
| `config` | `''` | Path to `crap4rs.toml` |
| `delta-gate` | `false` | Exit non-zero on new violations (only when baseline supplied). User-visible default; empty internally — see [Presets](#presets) |
| `analysis-gate` | `false` | Exit non-zero if the analysis itself fails. User-visible default; empty internally — see [Presets](#presets) |
| `comment-mode` | `none` | `none` (outputs only) or `sticky` (post/update sticky comment) |
| `comment-header` | `crap-scorecard` | Sticky-comment identifier |
| `version` | `latest` | crap4rs version to install (`latest` or pinned tag) |
| `annotations` | `false` | When `true`, emit `::warning` workflow commands so findings render inline on the PR Files Changed tab (see [Inline annotations](#inline-annotations)) |
| `annotation-limit` | `''` | Cap on emitted annotations when `annotations: true`. Empty defers to the adapter's default (10) or `[output] annotation_limit` from `config`. Range 1..=100 |
| `threshold-preset` | `''` | (preset) `strict` (8) \| `default` (15) \| `lenient` (25). Derives `threshold`; raw `threshold:` wins on conflict + warning. See [Presets](#presets) |
| `run-mode` | `''` | (preset) `full` \| `delta` \| `both` — drives baseline expectations. `delta`/`both` require `baseline:` set. See [Presets](#presets) |
| `gate-mode` | `''` | (preset) `report-only` \| `gate-on-analysis` \| `gate-on-delta` \| `gate-on-both`. Atomic `analysis-gate` + `delta-gate` pair; raw inputs win on conflict + warning. See [Presets](#presets) |
| `languages` | `''` | (preset) `rust` \| `typescript` \| `rust,typescript` \| `all`. Single-language supersedes `language:`; multi-language lands in PR β #293. See [Presets](#presets) |

## Outputs

| Name | Notes |
|---|---|
| `markdown` | The rendered scorecard — drop into aggregator comments verbatim |
| `row-json` | `Row::CrapDelta` JSON object — for aggregators that re-render with full layout control. See [Structured row output](#structured-row-output-outputsrow-json) |
| `json-envelope-path` | Path to the full JSON envelope on the runner |
| `analysis-passed` | `true` / `false` |
| `delta-passed` | `true` / `false`, or empty string when no baseline |
| `new-violations` | Count of functions exceeding threshold but not in baseline |
| `regressions` | Count of modified functions whose CRAP increased above rendering precision |
| `threshold-resolved` | The threshold value the action's `Resolve presets` step resolved (preset-derived or raw passthrough). See [Presets — Threshold-sync invariant](#threshold-sync-invariant) |

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
appends a trailing `::notice::N more functions exceed threshold; see
scorecard for the full list` line so reviewers know findings were
dropped. The full set always appears in `outputs.markdown`.

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
