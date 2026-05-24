# crap4rs Agent Notes

- This repo uses a shared Cargo target directory via `.cargo/config.toml`; let worktrees inherit it normally.
- Preserve any worktree the user identifies as active.

## Repo Context

- Architecture: hexagonal (ports & adapters) — `domain/` → `ports/` → `adapters/` → `core/` → `cli/`. Never import "inward."
- Testing: `cargo nextest run` for tests, `cargo clippy -- -D warnings` for lints, `cargo fmt --check` for formatting. Quick verify: all three chained.
- Property tests use `proptest` — regression files in `proptest-regressions/` are committed to git, never gitignored.
- Safety: do not push directly to `main`, always branch + PR. Do not modify `.github/workflows/*` unless the task clearly requires CI changes.
- The `domain/` and `ports/` layers must stay language-agnostic (no `syn`, no LCOV types) — they are designed for future extraction into a shared `crap-core` library.

## BDD hygiene

Gherkin specs live in `crates/crap4rs/tests/features/*.feature` and are
exercised by cucumber-rs harnesses under `crates/crap4rs/tests/*_cucumber.rs`.
The conventions below keep the spec corpus honest about what is and
isn't actually verified, so a future BDD-quality monitor (mokumo epic
breezy-bays-labs/mokumo#370) can dogfood on us without false positives.

**Lexicon source of truth**: `crates/crap4rs/tests/features/TAGS.toml`.

### Rules

1. **One status tag per scenario.** Every `Scenario` and `Scenario
   Outline` across every `.feature` file carries exactly one tag from
   `[status]` (`@wired`, `@unwired`, or `@wip`). Tags are applied at
   the scenario level, never the feature level — a single feature
   commonly mixes wired and unwired scenarios while harness coverage
   grows.

2. **`@unwired` and `@wip` require a tracking comment.** Inside the
   scenario block, add a Gherkin comment in this exact shape so it
   stays greppable (mirrors `~/.claude/rules/exclusions.md`):

   ```gherkin
   @unwired
   Scenario: --top N limits the report to the N highest-CRAP functions
     # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
     When the operator runs `crap4rs --coverage lcov.info --top 10`
     ...
   ```

   crap-rs#169 is this repo's persistent BDD wiring umbrella; new
   `.feature` files default to pointing at it unless they have their
   own per-file wiring issue.

   File the tracking issue **before** the tag lands. Same rule for
   `@wip` (issue must name the in-flight PR).

3. **No no-op step definitions.** If a `Given`/`When`/`Then` in a
   scenario has no executable implementation, the scenario is
   `@unwired` — not "wired with empty step defs". Forced empty step
   defs to skip past unwired Background blocks are forbidden:
   `cli_ergonomics_cucumber.rs` once carried four of these (PR #167)
   and they were removed in #168 by tagging the scenarios that
   depended on the Background as `@unwired` (so the harness's
   `@wired` filter skips them) and deleting the Background itself.

4. **`Background:` blocks must be executable.** A `Background` runs
   before every scenario in a feature, so a non-executable Background
   forces every scenario to be `@unwired` (or to carry no-op step
   defs, see rule 3). If the prose belongs in scenarios instead,
   inline it into the scenarios that need it and delete the
   `Background:` block.

5. **Cucumber harness filter pattern.** New cucumber harnesses run
   `@wired` scenarios only, via `filter_run_and_exit`:

   ```rust
   World::cucumber()
       .filter_run_and_exit("tests/features/<file>.feature", |_, _, sc| {
           sc.tags.iter().any(|t| t == "wired")
       })
       .await;
   ```

   Existing all-wired harnesses (e.g. `json_reporter_cucumber`) may
   continue using plain `run_and_exit` as long as every scenario in
   the file is `@wired`. Tags inside `sc.tags` are stored without the
   `@` prefix — verified empirically.

6. **Status migration is one-way.** `@unwired` → `@wip` → `@wired`.
   Removing a `@wired` tag back to `@unwired` requires the same
   tracking-issue ceremony as introducing it: a comment pointing at
   the issue that captures the regression.

### When the rules bite

- Adding a new spec scenario → tag `@unwired` with a `# tracked:` comment.
- Wiring a scenario → swap `@unwired` → `@wip` while implementing,
  then `@wip` → `@wired` when green.
- Adding a new harness → use the `@wired` filter pattern from rule 5.
- Touching `Background:` → re-read rule 4; non-executable Backgrounds
  are the source of most BDD hygiene debt in this repo.

## Supply-chain hygiene

Every GitHub Actions `uses:` reference in the repo — across
`.github/workflows/*.yml` AND `.github/actions/*/action.yml` — is
SHA-pinned with a trailing `# vX` (or `# tracks @<branch>`) comment,
and the freshness loop is closed by Dependabot + zizmor. The combined
pin + autobump + audit policy guards against tag-poisoning and
ref-shadowing attacks (a published `@v4` tag is mutable; a 40-char
commit SHA isn't).

### Rules

1. **SHA-pin every `uses:` reference.** Format:

   ```yaml
   - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
   ```

   The trailing comment names the human-readable ref the SHA
   resolves to so reviewers can recognize the action without a `gh
   api` call. New workflows + composite actions follow the same
   pattern; the `zizmor` CI job fails the build on any
   `unpinned-uses` finding (mechanical enforcement — "documentation
   rots; CI doesn't").

2. **Floating-branch actions get pinned to a branch-HEAD SHA.**
   Some actions (e.g. `dtolnay/rust-toolchain@stable`,
   `dtolnay/rust-toolchain@1.93`) publish version-channel branches
   rather than tagged releases — `@stable` is a branch that bakes in
   "whatever Rust release is current" and advances every ~6 weeks.
   These get pinned to the current branch-HEAD SHA with a comment
   naming the branch:

   ```yaml
   - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # tracks @stable branch
   ```

   The conscious tradeoff: reproducible CI (SHA-locked) at the cost
   of auto-tracking the Rust stable channel. Dependabot bumps the
   SHA when the upstream branch advances.

3. **Resolve SHAs via `gh api`.** For tag-pinned actions:

   ```bash
   gh api repos/foo/bar/git/ref/tags/vX --jq '.object.sha'
   ```

   For branch-pinned actions:

   ```bash
   gh api repos/foo/bar/branches/<branch> --jq '.commit.sha'
   ```

   For floating-version tags (e.g. `@v2` that points at the v2
   major-version head rather than a specific release), the same
   `git/ref/tags/<tag>` call returns the head SHA the moving tag
   currently resolves to — pin to that and Dependabot keeps it
   fresh as `@v2` advances.

4. **Local composite actions (`./.github/actions/<name>`) don't get
   pinned.** They're paths within this repo, not external
   references; the implicit version is "whatever's on the same
   branch". Their content (the `action.yml` inside) is SHA-pinned
   internally per rule 1.

5. **Dependabot for `github-actions` is enabled.** See
   `.github/dependabot.yml`. Weekly cadence; bumps land as PRs with
   `type:chore` + `priority:soon` labels. Minor/patch bumps are
   grouped into one weekly PR per ecosystem (smaller review surface);
   major bumps land as separate PRs (breaking-change review).

6. **Per-audit `zizmor` ignores get a `tracked:` comment.** New
   audits surfaced by zizmor that we scope-defer (e.g. workflow-wide
   `persist-credentials: false`, scoped per-job `permissions:`
   blocks) land in `.github/zizmor.yml` with a `# tracked: crap-rs#N`
   comment naming the follow-up issue. When the follow-up lands, the
   ignore is removed and the audit fires unconditionally — same
   accountability pattern as the rest of the repo's exclusions
   (mirrors `~/.claude/rules/exclusions.md`). Inline ignores in
   composite actions use `# zizmor: ignore[<audit>]` on the
   identified span and carry the same `tracked:` comment nearby.

### When the rules bite

- Adding a new workflow / composite action → SHA-pin every external
  `uses:` reference from day one. The `zizmor` CI gate fails the PR
  otherwise.
- Adding a new step that triggers a new zizmor audit → fix the
  finding OR file a follow-up issue and add a tracked entry to
  `.github/zizmor.yml`. Never silently expand the ignore list
  without a tracking issue.
- Reviewing a Dependabot bump PR → eyeball the upstream release
  notes for the bumped action; merge if benign. Multi-action group
  PRs may need staging if one bump is more contentious than the
  others.

## Mutation testing

`cargo mutants` is the surviving-mutant gate on a **dual-file
surface** — the two highest-leverage files in the workspace:

| File | Why it's gated | Crate |
|------|----------------|-------|
| `crates/crap-core/src/domain/view.rs` | highest-complexity function family in `crap-core` (view-projection / column logic) | `crap-core` |
| `crates/crap4ts/src/adapters/walker/mod.rs` | every decision-point decision + every function-discovery decision + every span-to-line conversion in the TS adapter routes through it (crap-rs#209) | `crap4ts` |

CI runs both in one `mutants` job but on **different cadences**: the
`view.rs` step runs **per-PR** (small, ~30 mutants, minutes); the
walker step runs **per-merge only** — gated `if: github.event_name ==
'push'`, which (given CI triggers on `push: [main]` /
`pull_request: [main]`) means it fires on merge-to-main, not on PR
pushes. Rationale: the walker's ~157-mutant pass is ~1 h even under
`-j 4`; a per-PR tax that size is not worth it for a file that changes
rarely once Cluster W (#200/#205/#207/#209) lands, while a per-merge
gate still catches walker regressions before crap4ts@2.0.0 GA. The
per-PR behavioural net for the walker is the `walker_proptest.rs`
suite (crap-rs#207); mutants is the deeper periodic gate. Locally
you'll want parallelism so a single file's gate finishes in minutes
rather than half an hour.

### Local invocation

```bash
# crap-core view.rs
cargo mutants -j 4 --package crap-core --file crates/crap-core/src/domain/view.rs
# crap4ts walker (crap-rs#209) — same flag shape, different package+file
cargo mutants -j 4 --package crap4ts --file crates/crap4ts/src/adapters/walker/mod.rs
```

The walker is ~1.2k LOC with 11 decision-point variants + nested-fn +
JSX + class-field/namespace traversal, so its mutant count is several
times `view.rs`'s. Run it **after** the `walker_proptest.rs` suite
(crap-rs#207) is green locally — the proptests kill a large fraction of
walker mutants without any per-mutant fixture work, leaving a much
smaller, more meaningful surviving set to triage.

`-j N` is CLI-only — cargo-mutants 27.x's config schema does not expose
a `jobs` field. Pick N up to your physical core count. Speedup is
hardware-dependent: on slower boxes where each mutated build dominates
wall-clock, `copy_target = true` removes the cold-cache penalty and
`-j N` cuts runtime substantially. On Apple Silicon and other
high-core-count boxes, `nextest`'s intra-test parallelism already
saturates available compute when one mutants worker is active, so the
marginal gain from `-j N` is smaller (~15-20% on M-series); the
workflow still matters because it removes the `--in-place` lock-in
that fragments local runs.

### --in-place is mutually exclusive with -j N

cargo-mutants implements parallelism by copying the source tree to N
scratch dirs (one per worker). `--in-place` skips the copy and mutates
the source tree directly, so it forbids `-j > 1` by design. Trying to
combine them errors out. See <https://mutants.rs/in-place>.

`.cargo/mutants.toml` sets `copy_target = true` so each local worker's
scratch dir gets a copy of the workspace `target/` — workers reuse the
prebuilt artifacts instead of rebuilding from cold. The trade-off is
disk usage: each worker holds its own `target/` copy in temp space, so
free disk should be ≥ `target/` size × N. The flag is ignored under
`--in-place` (no copy happens), which keeps CI behaviour unchanged.

### Crash recovery

- **Local default (no `--in-place`)**: workers mutate copies. Kill mid-run
  is safe — your source tree is never touched. Just re-run.
- **`--in-place` (CI, or local if you opt in)**: a mid-run kill can leave
  the mutated file dirty on disk. Restore with:

  ```bash
  # WARNING: discards any unrelated uncommitted changes to the file.
  # If you had in-flight edits, stash them first (`git stash push -- <file>`).
  git restore crates/crap-core/src/domain/view.rs
  ```

  If `cargo mutants` exits cleanly (success or failure), it restores
  the file itself — manual `git restore` is only needed after an
  unclean shutdown.

### CI parity

CI's `mutants` job runs **two steps on different cadences** on a
single ubuntu-latest worker:

```bash
# per-PR (no `if:`):
cargo mutants    --package crap-core --file crates/crap-core/src/domain/view.rs      --no-shuffle --in-place
# per-merge only (`if: github.event_name == 'push'`):
cargo mutants -j 4 --package crap4ts --file crates/crap4ts/src/adapters/walker/mod.rs --no-shuffle
```

**The two steps deliberately differ in cadence AND execution mode.**
The `view.rs` step (~30 mutants) runs per-PR with `--in-place`
single-worker — fast enough, no tree copy. The walker step has ~5× the
mutant count (157); a single-worker `--in-place` pass is ~1 h
wall-clock, so it (a) runs **per-merge only** (`if: github.event_name
== 'push'` — a `push` event under this repo's `on:` triggers is a
merge-to-main), keeping every PR fast, and (b) uses `-j 4` copy mode
(parallel workers, each reusing the prebuilt `target/` via
`.cargo/mutants.toml`'s `copy_target = true`) so even the per-merge
run is tractable. `-j N` and `--in-place` are mutually exclusive by
design (see the section below), which is why the walker step drops
`--in-place`. crap-rs#209's original "≤8 min per-PR" budget AC was
**amended to per-merge gating** (user-confirmed 2026-05-16) — that is
a resolved sequencing decision, not an open deviation. `--no-shuffle`
on both keeps mutant order deterministic across reruns, which makes
flaky-mutant triage tractable. The walker proptest suite
(`walker_proptest.rs`, crap-rs#207 — including the
`walker_nesting_depth_matches_oracle` invariant added specifically to
kill the `nesting.map(|n| n + 1)` arithmetic mutant class) keeps the
surviving set — hence the triage cost — small.

### Why `additional_cargo_test_args = ["--", "--skip", "envelope"]`

The wire-envelope snapshot tests
(`crates/crap-core/tests/wire_envelope_crap4rs.rs` and
`wire_envelope_crap4ts.rs`, test name `envelope`) shell out to the
`crap4rs` / `crap4ts` binaries, which a scoped `cargo mutants --package
crap-core` (or `--package crap4ts`) doesn't necessarily build. The
skip lives in the **repo-global** `.cargo/mutants.toml`, so
cargo-mutants applies it to **every** invocation in this workspace —
both the `view.rs` step and the walker step inherit it automatically;
there is no per-step inline `--skip` to maintain. Skipping it under
mutants is scoped to mutants only; every PR's `Test (linux-x86)` /
`Test (macos-arm)` / `Test (macos-x86)` job still runs both canaries
via `cargo nextest run --workspace --all-targets`, which DOES build the
bins first.

The same rule applies to any other test that shells
`cargo_bin("crap4rs"|"crap4ts")` from a `#[test]` fn body — without a
matching `--skip` substring token in `additional_cargo_test_args`, the
scoped mutants run's unmutated baseline panics on
`CARGO_BIN_EXE_<bin>` being unset, cargo-mutants exits 4 ("cargo test
failed in an unmutated tree"), and zero mutants get tested — the gate
silently goes dead. This was #224's root cause: a new test landed
without the token. The rule was documented but not enforced, so this
repo now enforces it mechanically — the `mutants-skip-lint` job in
`.github/workflows/ci.yml` and the matching `lefthook.yml` pre-push
hook both run `scripts/mutants-skip-lint.py`, which fails the build
if any in-scope `#[test]` fn shelling either adapter bin lacks a
covering `--skip` substring. Add a new shelling test → either pick a
fn name that an existing token already covers as a substring (cheap),
or add a new token to `.cargo/mutants.toml` in the same PR (also
cheap, just remember). The lint's failure message walks the
contributor through the fix.
