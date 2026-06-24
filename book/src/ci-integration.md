# CI integration

The `crap-scorecard` composite action runs a CRAP analysis inside your CI and turns it into a markdown scorecard, an optional gate that fails the build, an optional sticky PR comment, and an optional hosted HTML report. This chapter covers it from a consumer's point of view. For the CRAP math see [understanding-crap.md](understanding-crap.md); for the flag surface it wraps see [cli-reference.md](cli-reference.md); for the report layouts it can render see [output-formats.md](output-formats.md).

The action reference uses `@main` and `@<sha>` interchangeably in snippets below. Pin to a release SHA in real workflows (`@<sha> # vN`); `@main` tracks the unstable tip.

## What the action does and doesn't do

The action wraps the analyzer binary; it does not orchestrate coverage or git. Two things are the caller's job, by design:

- **Coverage.** Produce the coverage file before the action runs. Rust: `cargo llvm-cov --workspace --lcov --output-path lcov.info`. TypeScript: an Istanbul `coverage-final.json`. The path is `--coverage` (runtime-required — the action errors if it's missing or unreadable).
- **Checkout and base refs.** The action never runs `actions/checkout` and never reaches into git to fetch a base ref. Delta mode (below) needs you to produce the baseline yourself.

Drawing the boundary there keeps the action a pure function of its inputs: the same coverage file plus the same flags produce the same scorecard, with no hidden git state.

## Minimal workflow

```yaml
name: CRAP scorecard
on:
  pull_request:
jobs:
  scorecard:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@<sha>
        with:
          persist-credentials: false
      - run: cargo llvm-cov --workspace --lcov --output-path lcov.info
      - uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
        id: crap
        with:
          coverage: lcov.info
      - run: echo "${{ steps.crap.outputs.markdown }}"
```

That run is report-only: it renders the scorecard to `outputs.markdown` and never fails the build. The sections below add gating, a PR comment, deltas, and a hosted report.

## Core inputs

| Input | Purpose |
|---|---|
| `coverage` | Path to the coverage file. Runtime-required. LCOV (`.info`/`.lcov`) for Rust, Istanbul JSON (`.json`) for TypeScript. |
| `language` | `auto` (default — infers from the `coverage` extension), `rust`, or `typescript`. |
| `src` | Source root passed to the analyzer. Newline-separated list for multi-root. **Empty by default** — when omitted, `--src` is not forwarded, so the analyzer's own precedence wins (a `crap.toml` `src = [...]`, else its built-in `["src"]`). Pass `src: .` to scan the repo root. |
| `config` | Path to a `crap.toml`. Forwarded only when set. |

When `src`, `threshold`, and `threshold-preset` are all omitted, the action forwards no corresponding flag and lets a `crap.toml` own those knobs (see [configuration.md](configuration.md)). A wrapper that injected a tool default as an explicit flag would invert that cascade, so the action does not.

The default complexity metric is the adapter's own: crap4rs uses cognitive, crap4ts uses cyclomatic. The action never passes `--metric`.

## Presets

Four optional preset inputs cover the common-case configuration in one line each. Presets are additive — every raw input still works, and an explicit raw input wins when set alongside a preset (the action emits a `::warning::` only on actual conflict, since a GitHub Actions composite cannot tell "caller passed the default" from "caller omitted it").

| Input | Values | Derives |
|---|---|---|
| `threshold-preset` | `strict` \| `default` \| `lenient` | `threshold` = 8 / 15 / 25 (cognitive calibration) |
| `run-mode` | `full` \| `delta` \| `both` | Baseline expectations (delta/both require `baseline:`) |
| `gate-mode` | `report-only` \| `gate-on-analysis` \| `gate-on-delta` \| `gate-on-both` | The atomic `analysis-gate` + `delta-gate` pair |
| `languages` | `rust` \| `typescript` \| `rust,typescript` \| `all` | Single value supersedes `language:`; a multi-value set triggers multi-language mode |

The 8/15/25 thresholds are a calibration convention, not empirically derived values, and they are the cognitive scale; crap4ts applies its own cyclomatic calibration automatically. `threshold-preset` and the score-based risk bands are distinct axes that happen to share 8/15/25 today — see [understanding-crap.md](understanding-crap.md). The full input and output tables live in the [action README](https://github.com/breezy-bays-labs/crap-rs/blob/main/.github/actions/scorecard/README.md); this chapter names the inputs a consumer reaches for first.

`languages: rust,typescript` (or `all`) runs both adapters in one invocation and pairs the Rust inputs (`coverage`, `src`, `baseline`) with their `-ts` counterparts (`coverage-ts`, `src-ts`, `baseline-ts`). It produces one combined sticky comment and split outputs. See [multi-language.md](multi-language.md) for the paired-input contract and how the combined view ranks across languages (by CRAP-to-threshold ratio and risk band — cross-language scores are not directly comparable).

## The gate-mode dial

By default the action is measurement-only: it renders the scorecard and exits 0 even when functions sit above the threshold. `gate-mode` is the one dial that turns measurement into a hard gate. It sets `analysis-gate` and `delta-gate` together so the two cannot drift.

| `gate-mode` | Fails the build when… |
|---|---|
| `report-only` (default behavior) | Never — render only. |
| `gate-on-analysis` | Any analyzed function exceeds the threshold. |
| `gate-on-delta` | A new violation lands relative to the baseline (requires `baseline:`). |
| `gate-on-both` | Either of the above. |

When a gate trips, the action exits non-zero after the scorecard is rendered, so `outputs.markdown` and the sticky comment are still produced — the gate fails the job; it does not suppress the report.

```yaml
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
  with:
    coverage: lcov.info
    gate-mode: gate-on-analysis
```

## Sticky PR comments

Set `comment-mode: sticky` to post (and update in place on each push) a single PR comment carrying the scorecard. This needs `pull-requests: write` on the calling job.

```yaml
permissions:
  contents: read
  pull-requests: write
# ...
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
  with:
    coverage: lcov.info
    comment-mode: sticky
    comment-header: crap-scorecard
```

`comment-header` is the sticky identity — vary it to run two distinct scorecards on one PR (e.g. a gated production card and a report-only one) without their comments colliding. `comment-preamble` prepends a labeling line so a reader can tell two cards apart at a glance. The composed body is also exposed verbatim on the `sticky-message` output for aggregator bots that compose their own comment.

## Baseline and delta

A baseline is a previously captured JSON envelope (`crap4rs … --format json`). Supply it on `baseline:` to render a delta scorecard — what changed versus the baseline — and to enable `gate-on-delta`. Producing the baseline is the caller's job (the action does not auto-checkout the base ref):

```yaml
steps:
  - uses: actions/checkout@<sha>
    with:
      fetch-depth: 0
      persist-credentials: false
  - name: Capture baseline on the PR base
    run: |
      git switch --detach origin/${{ github.base_ref }}
      cargo llvm-cov --workspace --lcov --output-path /tmp/base.lcov
      cargo install --locked crap4rs
      crap4rs --coverage /tmp/base.lcov --src . --format json --no-fail > /tmp/baseline.json
      git switch -
  - run: cargo llvm-cov --workspace --lcov --output-path lcov.info
  - uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
    with:
      coverage: lcov.info
      baseline: /tmp/baseline.json
      gate-mode: gate-on-delta
      comment-mode: sticky
```

The delta is recomputed from the two envelopes, so the baseline must carry the parameters the gate uses (threshold, epsilon). The action threads those through; a hand-built baseline that omits them recomputes against defaults. For the JSON envelope shape see [output-formats.md](output-formats.md).

## HTML report and GitHub Pages

`html-report: true` renders the full file-by-file HTML report (KPI tiles, distribution bar, file cards, optional dark mode), uploads it as a workflow artifact, and — under `comment-mode: sticky` — appends a download link to the sticky comment. A supplied `baseline:` activates the report's Delta tab automatically. The artifact name is per language (`crap4rs-report-<suffix>` / `crap4ts-report-<suffix>`), suffixed with `-${{ runner.os }}` by default; override `html-artifact-name-suffix` for matrix legs beyond OS, or `actions/upload-artifact` fails on the second leg uploading to the same name.

A workflow-artifact link makes the reviewer download a zip. `pages-publish: true` instead pushes the rendered HTML to a GitHub Pages branch and links the **live hosted page** in the sticky comment — one click, no download.

```yaml
permissions:
  contents: write       # required for the gh-pages push
  pull-requests: write  # required for the sticky comment
# ...
- uses: breezy-bays-labs/crap-rs/.github/actions/scorecard@<sha>
  with:
    coverage: lcov.info
    baseline: baseline.json   # optional — enables the Delta tab
    html-report: true         # required for pages-publish
    comment-mode: sticky
    pages-publish: ${{ github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository }}
    pages-deploy-path: pr-${{ github.event.number }}
    pages-url: https://acme.github.io/my-repo/pr-${{ github.event.number }}/
```

Mechanics that matter:

- **`pages-publish` requires `html-report: true`** (there must be a rendered report to publish) and both `pages-deploy-path` (the path within the branch, e.g. `pr-123`) and `pages-url` (the full public URL, used verbatim as the sticky link). The action does not derive the URL — you pass it, so custom domains and project-vs-user Pages all work.
- **Publish before post.** The push runs before the sticky comment is composed under `set -euo pipefail`, so a push failure reds the job rather than posting a dead link.
- **First-run ordering.** When the Pages branch (default `gh-pages`) does not exist, the action creates it as an orphan branch with a `.nojekyll` marker on the first publish. Creating the branch does not make GitHub serve it — enable Pages on the branch once (Settings → Pages → Deploy from a branch → `gh-pages` → `/ (root)`). Until Pages is enabled and has built once, the link 404s even though the file was pushed.
- **Content is preserved.** The publish copies into `<pages-deploy-path>/index.html`; it does not wipe the branch. Other PRs' directories, the root report, and `baselines/` survive. Per-PR directories accumulate, so ship a `pull_request: types: [closed]` cleanup workflow that removes `pr-<N>/` on close, and give every job that pushes to the branch a shared `concurrency` group with `cancel-in-progress: false`.

## Key outputs

| Output | Use |
|---|---|
| `markdown` | The bare rendered scorecard. Consume directly in aggregator bots. |
| `sticky-message` | The composed sticky body (preamble + scorecard + report link). |
| `row-json` | The single-row `Row::CrapDelta` JSON for aggregator workflows that re-render with full layout control. |
| `json-envelope-path` | Path on the runner to the full JSON envelope. |
| `analysis-passed` / `delta-passed` | `true`/`false` gate results. `delta-passed` is empty with no baseline. |
| `new-violations` / `regressions` | Counts feeding `gate-on-delta`. |
| `threshold-resolved` | The threshold the action resolved from its inputs. When the caller defers to `crap.toml`, this still reports the action's `15` fallback, not the analyzer's effective gate — read the envelope for that. |
| `html-artifact-url` | Report URL when `html-report: true` (resolves only for GitHub-authenticated requests). |

In multi-language mode `row-json` is empty (the action warns) and you read the split `row-json-rust` / `row-json-typescript` instead — see [multi-language.md](multi-language.md).

## Caveats

- **Fork PRs get a read-only token.** GitHub issues a read-only `GITHUB_TOKEN` for `pull_request` events from forks regardless of the job's `permissions:` block. The `comment-mode: sticky` step silently no-ops, and `pages-publish` would 403 — gate it to same-repo events (as above) so a fork PR degrades cleanly to a summary-only card with no hosted link. Two standard workarounds restore fork coverage: a `pull_request_target` trigger with hardened checkout, or a separate `workflow_run`-triggered comment workflow with its own writable token. The advanced privileged-handoff topology (the `fork-handoff` input plus a base-repo `workflow_run` publisher) is documented in [`docs/pages-fork-reports.md`](https://github.com/breezy-bays-labs/crap-rs/blob/main/docs/pages-fork-reports.md). Same-repo branches are unaffected.
- **crap4ts is not auto-installed.** The action installs crap4rs via `cargo binstall`, but the TypeScript branch does **not** attempt to install crap4ts — it requires the binary pre-installed on `PATH` and fails with an actionable message when it's missing. Prepend its directory to `PATH` in a step that runs before the action.
- **Integration / BDD / feature-gated code scores `c² + c`.** Code with little or no line coverage scores its complexity squared. For analysis-only crates that is expected, not a bug — gate them `report-only` or scope `src`/`config` to exclude them rather than chasing a passing gate.
- **`threshold-resolved` is the resolved input, not always the analyzer's gate** (see the outputs table) — only relevant when you let `crap.toml` own the threshold.

The complete input and output tables, the aggregator pattern, and the deep multi-language and fork topologies stay in the [action README](https://github.com/breezy-bays-labs/crap-rs/blob/main/.github/actions/scorecard/README.md).
