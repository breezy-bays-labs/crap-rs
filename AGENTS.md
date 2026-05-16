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

## Mutation testing

`cargo mutants` is the surviving-mutant gate on a **dual-file
surface** — the two highest-leverage files in the workspace:

| File | Why it's gated | Crate |
|------|----------------|-------|
| `crates/crap-core/src/domain/view.rs` | highest-complexity function family in `crap-core` (view-projection / column logic) | `crap-core` |
| `crates/crap4ts/src/adapters/walker/mod.rs` | every decision-point decision + every function-discovery decision + every span-to-line conversion in the TS adapter routes through it (crap-rs#209) | `crap4ts` |

CI runs both **sequentially within one `mutants` job** on every PR (the
`view.rs` step then the walker step); locally you'll want parallelism
so a single file's gate finishes in minutes rather than half an hour.

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

CI's `mutants` job runs **two sequential steps** on a single
ubuntu-latest worker:

```bash
cargo mutants    --package crap-core --file crates/crap-core/src/domain/view.rs      --no-shuffle --in-place
cargo mutants -j 4 --package crap4ts --file crates/crap4ts/src/adapters/walker/mod.rs --no-shuffle
```

**The two steps deliberately differ in execution mode.** The
`view.rs` step (~30 mutants) uses `--in-place` single-worker — fast
enough, no tree copy. The walker step has ~5× the mutant count (157);
single-worker `--in-place` on 157 mutants is ~1 h wall-clock, so it
uses `-j 4` copy mode instead (parallel workers, each reusing the
prebuilt `target/` via `.cargo/mutants.toml`'s `copy_target = true`).
`-j N` and `--in-place` are mutually exclusive by design (see the
section below), which is why the walker step drops `--in-place`. The
crap-rs#209 ≤8-min budget AC is **not met by single-worker in-place on
the walker** — that is a recorded plan-of-record deviation, not a
regression; `-j 4` copy mode is the chosen mitigation. `--no-shuffle`
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
