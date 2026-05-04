# `crap-scorecard` action

Runs `crap4rs` on a coverage file and emits a markdown scorecard. Composable
into aggregator PR-comment bots; can also post its own sticky comment.

## Why composite, not standalone

The scorecard is one row in a richer "PR metrics" comment (mokumo's
[#650](https://github.com/breezy-bays-labs/mokumo/issues/650) for example
combines coverage, CRAP, mutation, module size, architecture violations).
This action exposes the rendered markdown as an **output** so an aggregator
job can drop it into a larger sticky comment without two bots fighting over
the same comment.

For repos that only care about CRAP, set `comment-mode: sticky` and the
action manages its own sticky comment.

## Languages

Polyglot interface, Rust wired today. TypeScript (via `crap4ts`) is reserved
for a future release of this action — see ops
[#231](https://github.com/breezy-bays-labs/ops/issues/231).

```yaml
- uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@main
  with:
    language: auto                 # rust | typescript | auto (infers from extension)
    coverage: lcov.info
```

## Minimal usage — analysis only, no delta

```yaml
- uses: actions/checkout@v4
- run: cargo llvm-cov --workspace --lcov --output-path lcov.info
- uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@main
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
      - uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@main
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

Mokumo's metrics-delta bot wants coverage + CRAP + mutation + module size in
one sticky comment. This action contributes the CRAP rows; the aggregator
owns the comment.

```yaml
- uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@main
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
| `language` | `auto` | `rust`, `typescript`, or `auto` (infer from coverage extension) |
| `coverage` | (required) | Path to LCOV (`.info`) for Rust, Istanbul JSON for TS |
| `src` | `.` | Source root passed to the analyzer |
| `baseline` | `''` | Path to a previously-captured CRAP JSON envelope. Empty = no delta |
| `threshold` | `15` | Threshold for violations |
| `config` | `''` | Path to `crap4rs.toml` |
| `delta-gate` | `false` | Exit non-zero on new violations (only when baseline supplied) |
| `analysis-gate` | `false` | Exit non-zero if the analysis itself fails |
| `comment-mode` | `none` | `none` (outputs only) or `sticky` (post/update sticky comment) |
| `comment-header` | `crap-scorecard` | Sticky-comment identifier |
| `version` | `latest` | crap4rs version to install (`latest` or pinned tag) |

## Outputs

| Name | Notes |
|---|---|
| `markdown` | The rendered scorecard — drop into aggregator comments verbatim |
| `row-json` | Mokumo `Row::CrapDelta` JSON object — for aggregators that re-render with full layout control. See [Structured row output](#structured-row-output-outputsrow-json) |
| `json-envelope-path` | Path to the full JSON envelope on the runner |
| `analysis-passed` | `true` / `false` |
| `delta-passed` | `true` / `false`, or empty string when no baseline |
| `new-violations` | Count of functions exceeding threshold but not in baseline |
| `regressions` | Count of modified functions whose CRAP increased above rendering precision |

## Structured row output (`outputs.row-json`)

`outputs.row-json` is the raw stdout of `crap4rs --format scorecard-row` —
one mokumo `Row::CrapDelta` JSON object, conforming to the locked
[scorecard schema](https://github.com/breezy-bays-labs/mokumo/blob/main/.config/scorecard/schema.json)
(the `CrapDelta` member of `definitions/Row` `oneOf`). No envelope, no
metadata wrapper — the JSON object is the contract.

Use `outputs.row-json` when an aggregator workflow re-renders the
scorecard with full layout control. Use `outputs.markdown` when posting
the scorecard verbatim into a sticky comment.

Producer-side status policy (Red/Yellow/Green) lives in `crap4rs`
([crap4rs#111](https://github.com/breezy-bays-labs/crap4rs/issues/111),
PR [#119](https://github.com/breezy-bays-labs/crap4rs/pull/119)):

- **Red** — at least one new threshold violation landed.
- **Yellow** — no new violations, but at least one modified function's
  CRAP score regressed.
- **Green** — otherwise.

### Aggregator pattern — consume `row-json` and re-render

```yaml
- uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@main
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
(e.g. mokumo's `crap-scorecard` job in `quality.yml`) continue to read
it without modification.

### Version requirement

`outputs.row-json` is populated for **`crap4rs ≥ 0.4.0`**. With older
releases the action emits an empty string and prints a workflow
warning; the rest of the action (`outputs.markdown`, gates, sticky
comment) keeps working. Pin the `version` input to `0.4.0` or later
once it publishes if your workflow consumes `row-json`.

## Pinning

Examples above use `@main` for clarity. **Pin to a commit SHA in production
workflows** so a regression in the action can't break your CI without you
noticing:

```yaml
uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@<sha>
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
- uses: breezy-bays-labs/crap4rs/.github/actions/scorecard@main
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
