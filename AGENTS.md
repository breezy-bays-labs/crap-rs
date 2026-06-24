# crap4rs Agent Notes

- This repo uses a shared Cargo target directory via `.cargo/config.toml`; let worktrees inherit it normally.
- Preserve any worktree the user identifies as active.

## Repo Context

- Architecture: hexagonal (ports & adapters) — `domain/` → `ports/` → `adapters/` → `core/` → `cli/`. Never import "inward."
- Testing: `cargo nextest run` for tests, `cargo clippy -- -D warnings` for lints, `cargo fmt --check` for formatting. Quick verify: all three chained. The org **Testing Strategy** (`~/Github/ops/standards/testing-strategy.md` — the three axes, the Boundary Rule, the quadrants, the five-leg health dashboard) is canonical; crap4rs's per-level tool-map + the Boundary Rule are in `.claude/rules/testing.md`.
- Property tests use `proptest` — regression files in `proptest-regressions/` are committed to git, never gitignored.
- Safety: do not push directly to `main`, always branch + PR. Do not modify `.github/workflows/*` unless the task clearly requires CI changes.
- The `domain/` and `ports/` layers stay language-agnostic (no `syn`/`oxc`, no LCOV/Istanbul types) — they live in the shared `crap-core` library that every adapter links (`crap4rs` via `syn`, `crap4ts` via `oxc`); `crap-core` also ships the `crap-render` binary. Purity is enforced by the four-layer `ast-purity` CI job + `deny.toml`; the only allowed proc-macro-chain derives are `serde`, `thiserror`, and `askama` (the `Template` derive backing the HTML/markdown reporters).

## BDD hygiene

Gherkin specs live in `crates/crap4rs/tests/features/*.feature` and are
exercised by cucumber-rs harnesses under `crates/crap4rs/tests/*_cucumber.rs`.
The conventions below keep the spec corpus honest about what is and
isn't actually verified, so a future BDD-quality monitor can dogfood
on us without false positives.

**Lexicon source of truth**: `crates/crap4rs/tests/features/TAGS.toml`.

### Rules

1. **One status tag per scenario.** Every `Scenario` and `Scenario
   Outline` across every `.feature` file carries exactly one tag from
   `[status]` (`@wired`, `@unwired`, or `@wip`). Tags are applied at
   the scenario level, never the feature level — a single feature
   commonly mixes wired and unwired scenarios while harness coverage
   grows.

   Mechanical enforcement: the same `scripts/bdd-tracked-lint.py` that
   owns Rule 2 (run from `lefthook.yml` pre-push and the
   `bdd-tracked-lint` CI job) also rejects any scenario carrying ZERO
   status tags or MORE THAN ONE. Documentation rots; CI doesn't.

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

   Mechanical enforcement: `scripts/bdd-tracked-lint.py` (run from
   `lefthook.yml` pre-push and the `bdd-tracked-lint` CI job)
   rejects any `@unwired`/`@wip` scenario whose body lacks a
   `# tracked: crap-rs#<n>` comment. Documentation rots; CI doesn't.

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

### BDD Boundary-Rule lint

`scripts/bdd-mislevel-lint.py` is the mechanical enforcer of the
*mechanizable shadow* of the org Testing Strategy **Boundary Rule**
("test each behavior once, at the lowest level that fully captures it;
promote to acceptance/BDD only for product-level contracts"). It is the
sibling of `bdd-tracked-lint.py` — wired identically into the
`bdd-mislevel-lint` CI job and a `lefthook.yml` pre-push command
(single source of truth in the script; documentation rots, CI doesn't).
It joins each `crates/*/tests/*_cucumber.rs` harness to its bound
`crates/*/tests/features/*.feature` spec.

**The honest boundary:** the keystone Boundary-Rule decision — *is this
behavior a product-level contract a consumer relies on?* — is
irreducibly **judgment** and is NOT mechanized; it stays a CQO / council
BDD audit. The lint enforces only the provable shadow:

1. **RULE A (FAIL) — narration/execution mismatch.** A `@wired`/`@wip`
   scenario whose step prose narrates a backtick-quoted CLI invocation
   of an analyzer binary (`crap4rs` / `crap4ts` / `crap-render`) while
   its bound harness has ZERO process-spawn markers (`cargo_bin(` /
   `CARGO_BIN_EXE_`) — it advertises a CLI contract but executes a
   library/adapter call. The fix is honest narration: rewrite to
   adapter-level prose (e.g. `When the oxc walker analyzes the source`,
   as `cyclomatic_walker.feature` does, or `When the JSON is formatted`
   as `json_reporter.feature` does) — or actually spawn the binary if
   it genuinely IS a CLI contract.
2. **RULE B (FAIL) — feature-link soundness.** Every harness declares
   exactly one resolvable, on-disk `(filter_)run_and_exit(
   "tests/features/<X>.feature")` path (the join RULE A relies on; a
   missing join would silently no-op the gate).

**Annotation conventions** the lint validates (mirroring the
`# tracked:` em-dash shape):

- `// bdd-lint: lib-direct-by-design — tracked: crap-rs#<n> — <reason>`
  on a harness DECLARES it is deliberately library-level. Its shape is
  enforced (RULE A-OPTOUT, FAIL if malformed); and a harness carrying it
  must NOT have a bound feature that still narrates a CLI run (RULE
  A-COHERENCE, FAIL) — the opt-out admits lib-level, so the prose must
  stop claiming a CLI run rather than paper over a lying narration.
- `// bdd-asserts-only: <crate>::<path> — tracked: crap-rs#<n> —
  <reason>` is a contributor's *voluntary, honest* declaration that the
  harness knowingly mirrors a named lower-level test. Presence-only,
  shape-validated (RULE D, WARN); absence is never a violation. This is
  the honest mechanizable form of "this scenario duplicates a named unit
  test" — the *inference* of that duplication is below the false-match
  floor and is deliberately NOT attempted.

The lint ships `--self-test`; CI runs it as a separate preceding step so
the rule logic is itself regression-guarded.

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

2. **Floating-branch actions get pinned to a branch-HEAD SHA — but
   Dependabot can't bump them, so manual refresh is required.** Some
   actions (e.g. `dtolnay/rust-toolchain@stable`,
   `dtolnay/rust-toolchain@1.93`) publish version-channel branches
   rather than tagged releases — `@stable` is a branch that bakes in
   "whatever Rust release is current" and advances every ~6 weeks.
   These get pinned to the current branch-HEAD SHA with a comment
   naming the branch:

   ```yaml
   - uses: dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8 # tracks @stable branch
   ```

   **Dependabot's `github-actions` ecosystem tracks updates on an
   action's default branch (and on tagged releases) — it does NOT
   track HEAD advances on non-default branches.** For
   `dtolnay/rust-toolchain` the default branch is `master` (which
   has different `action.yml` content per version-channel branch),
   so a Dependabot-proposed SHA from `master` would silently break
   the action. The pin therefore needs a manual refresh on a
   quarterly cadence (or before any major Rust release the project
   depends on). The conscious tradeoff: reproducible CI (SHA-locked)
   at the cost of manual upkeep on the toolchain-action pin.

3. **Resolve SHAs via `gh api repos/foo/bar/commits/<tag>`.** This
   form returns the underlying **commit SHA** for any tag —
   lightweight or annotated — without the caller having to know the
   difference:

   ```bash
   gh api repos/foo/bar/commits/vX --jq '.sha'
   ```

   The alternative `gh api repos/foo/bar/git/ref/tags/<tag> --jq
   '.object.sha'` returns the *tag object* SHA for annotated tags
   (only the commit SHA for lightweight ones), which mixes
   representations across actions and risks pin drift. Use the
   `commits/<tag>` form unconditionally — both `@v4`-style
   floating-major and `@v1.18.1`-style pinned-release tags work.
   For branch-pinned actions:

   ```bash
   gh api repos/foo/bar/branches/<branch> --jq '.commit.sha'
   ```

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
   audits surfaced by zizmor that we scope-defer (e.g. a workflow-wide
   convention change too large for the current PR) land in
   `.github/zizmor.yml` with a `# tracked: crap-rs#N` comment naming
   the follow-up issue. When the follow-up lands, the ignore is
   removed and the audit fires unconditionally — same accountability
   pattern as the rest of the repo's exclusions (mirrors
   `~/.claude/rules/exclusions.md`). The file is intentionally absent
   in steady state: it appears only when an audit is in flight to a
   tracked issue. Inline ignores in composite actions or in workflow
   spans use `# zizmor: ignore[<audit>]` on the identified line; if
   the suppression is permanent (intentional + bounded), no
   `tracked:` comment is needed.

7. **`persist-credentials: false` on every `actions/checkout`.**
   Workflows that never push (every job in this repo's `ci.yml` +
   the build/publish jobs in `release-plz.yml`) keep the GH App
   checkout token out of the runner's `.git/config`. Already wired
   on every checkout repo-wide; the `artipacked` audit fails any
   new checkout that omits it.

8. **Scoped per-job `permissions:` blocks (least privilege).** Each
   workflow declares a top-level `permissions: contents: read`
   default; jobs that need more elevate explicitly (e.g.
   `release-plz.yml/release-plz-release` needs `contents: write` +
   `id-token: write` for the OIDC publish path; `ci.yml/scorecard-smoke`
   needs `pull-requests: write` for the sticky-comment dogfood). Never
   let a job inherit the runner's default workflow permissions silently;
   the `excessive-permissions` audit fails any job missing a
   `permissions:` block.

9. **Cache-poisoning fix shape for release-relevant workflows.**
   Workflows whose downstream jobs publish to crates.io / npm or
   upload signed binaries (every job in `release-plz.yml`) must
   neither restore nor save caches: a poisoned cache from a malicious
   push would otherwise become part of the published artifact.
   The pattern: the `setup-rust` composite action accepts
   `enable-cache: "false"` which skips its embedded `Swatinem/rust-cache`
   step via `if:`. Release-relevant callers pass that input.
   `actions/setup-node` is opted-out by simply omitting the `cache:`
   input (which would otherwise enable caching), and carries an inline
   `# zizmor: ignore[cache-poisoning]` because zizmor's heuristic
   flags setup-node defensively. See
   https://docs.zizmor.sh/audits/#cache-poisoning.

10. **Prefer the `gh` CLI to a third-party release action.** GitHub
    releases are created via `gh release create "$TAG" --generate-notes
    artifacts/*.tar.gz` under `permissions: contents: write` and
    `GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}`. One less external action
    to SHA-pin and bump, and the `superfluous-actions` audit closes
    cleanly. The same principle applies whenever the gh CLI covers
    the third-party action's feature set.

### When the rules bite

- Adding a new workflow / composite action → SHA-pin every external
  `uses:` reference from day one, add `persist-credentials: false`
  to every checkout, declare a scoped `permissions:` block per job,
  and pass `enable-cache: "false"` to `setup-rust` if the workflow
  is tag-triggered. The `zizmor` CI gate fails the PR otherwise.
- Adding a new step that triggers a new zizmor audit → fix the
  finding directly OR file a follow-up issue and add a tracked
  entry to `.github/zizmor.yml`. Never silently expand the ignore
  list without a tracking issue.
- Reviewing a Dependabot bump PR → eyeball the upstream release
  notes for the bumped action; merge if benign. Multi-action group
  PRs may need staging if one bump is more contentious than the
  others.

## Composite scorecard action

`.github/actions/scorecard/action.yml` is the public dogfood surface
for both adapters. It auto-detects the language from the `coverage`
input's file extension (`.info` / `.lcov` → crap4rs, `.json`
(Istanbul) → crap4ts) and dispatches the matching adapter binary.
Callers may override detection by setting `inputs.language` to
`rust` or `typescript` explicitly.

### Multi-root `--src` + the two-scorecard model (crap-rs#336)

`--src` is **repeatable** (`Vec<PathBuf>`): a single run can analyze
several source roots and union their functions into one scorecard,
joined against a single shared `--coverage`. The action's `src:` /
`src-ts:` inputs accept a newline-separated list (YAML `|` block scalar)
and split it into repeated `--src`. A single root stays byte-identical
to before multi-root existed.

**`IdentityBase` (one-liner):** function `file_path` (and coverage-SF
normalization) is relativized to a run identity base decided ONCE from
the root count — `len()==1 ⇒ src-relative` (byte-identical back-compat;
load-bearing for #334's config `src=[...]`), `len()>1 ⇒
git-toplevel-relative` (globally-unique keys across crates sharing
internal names like `adapters/mod.rs`; no coverage bleed). Multi-root
outside a git work tree is a HARD ERROR, never a silent basename strip.
Resolved in `cli::prepare_pipeline` between input validation and the
coverage-factory call, threaded to BOTH consumers (identity +
coverage). ADR: `adr-multi-root-identity-base.md`.

**Two scorecards, two surfaces:** the repo runs a **production** CRAP
gate and an **examples** dogfood as distinct sticky comments.
- *Production* — `scorecard-production` job in `.github/workflows/ci.yml`,
  `gate-mode: gate-on-analysis`, multi-root over the three prod crate
  src roots (`crap-core` + `crap4rs` + `crap4ts`),
  `comment-header: crap-scorecard-production`. Builds + stages THIS PR's
  binary on PATH (the published binary lacks repeatable `--src`), so the
  gate dogfoods the PR's own analyzer.
- *Examples* — the invocation in `.github/workflows/quick-start-smoke.yml`,
  `gate-mode: report-only`, multi-language over `crap-examples`,
  `comment-header: crap-scorecard-quickstart-smoke`.

Each carries a `comment-preamble` labeling its scope (production-crates
vs intentionally-bad teaching sample). The preamble is the stopgap
disambiguator until #334 lands `[output].title`; it prepends to the
sticky body and leaves an empty preamble byte-identical. The composed
body is also exposed on the action's `sticky-message` output.

### Cross-adapter `--format scorecard-row` parity

`crap4rs` and `crap4ts` both route `--format scorecard-row` through
crap-core's shared
`cli::format_as_scorecard_row` → `domain::summary::project_crap_delta_row`
→ `reporters::format_scorecard_row` pipeline. The Row JSON shape —
top-level keys, `type`/`id`/`label`/`anchor` literals, `status`
enum, numeric types — is therefore structurally guaranteed to be
identical across both adapters, and consumers of the composite
action's `outputs.row-json` can rely on a single schema regardless
of which adapter the action dispatched.

**Mechanical enforcement**:
`crates/crap-core/tests/scorecard_row_parity.rs` runs both bins
against representative fixtures and asserts byte-identical key
sets on both the Green (no violations) and Red (above-threshold)
branches plus value-shape invariants for fields the locked
schema fragment
(`crates/crap4rs/tests/fixtures/scorecard/schema.json`) relies on.
"Documentation rots; CI doesn't" — same pattern as
`scripts/bdd-tracked-lint.py` and `scripts/mutants-skip-lint.py`.

### Cross-adapter `RiskLevel` consistency canary

`crap4rs` and `crap4ts` both derive every function's `RiskLevel`
from one shared `crap_core::domain::crap::classify_risk` (score →
band) and serialize it through one shared serde derive on
`crap_core::domain::types::RiskLevel`. Cross-adapter consistency
therefore holds by construction — there is no per-language
risk-classification step. This is the dimensional-consistency
invariant ζ's Combined-view ranking depends on (risk-level desc,
then CRAP/threshold ratio desc within band).

Two axes are deliberately kept distinct here, since #317's prose
conflates them: `RiskLevel` **bands** are score-based, fixed in
`classify_risk` (≤8 Low / ≤15 Acceptable / ≤25 Moderate / else High),
metric-agnostic, and independent of `--threshold`; the
**preset thresholds** (strict/default/lenient, flat `8/15/25` across
both metrics today, routed through metric-keyed infrastructure so a
future per-metric recalibration stays a one-line change) drive the
`--threshold` GATE (`exceeds` / scorecard-row `status`), which is the
separate axis the scorecard-row + default-gate canaries own. The
RiskLevel canary pins the band axis only. The two axes share the same
`8/15/25` numbers today but remain conceptually distinct: the band is
score-based via `classify_risk`, the gate is `preset.threshold(metric)`.

**Mechanical enforcement**:
`crates/crap-core/tests/risk_level_cross_adapter.rs` (test fn
`risk_level_envelope_parity`) runs both bins' `--format json`
envelopes and asserts, for every function in either envelope, that
the serialized `risk_level` is (a) one of the four canonical
`RiskLevel::as_wire_str` values and (b) the band the shared
`classify_risk` oracle re-derives from the wire `crap.value`. Because
`risk_level` and `crap.value` are serialized independently, the
oracle is a genuine round-trip check, not a tautology; running the
same shared oracle against both adapters is what makes it a
cross-adapter consistency proof. A future per-adapter
risk-classification step or a serde rename on one path fails the
canary loudly. The fn name carries the `envelope` substring so the
existing `--skip envelope` token in `.cargo/mutants.toml` covers it
(see the Mutation testing section). "Documentation rots; CI doesn't"
— same pattern as the scorecard-row parity + wire-envelope canaries.

### crap4ts install constraint

Until crap4ts publishes a working binary release to crates.io
(currently `2.0.0-alpha.1` on crates.io is the pre-walker stub;
the working `2.0.0-rc.x` ships only to npm via napi-rs and the
crates.io publish path is tracked under release-plz adoption),
the composite action's typescript branch does **not** attempt a
`cargo binstall`. External TS consumers must pre-install `crap4ts`
on `PATH` themselves (e.g. by cargo-building from a fork or
shipping a binary artifact); the action fails with an actionable
error message naming the same constraint when the binary is
missing. The CI smoke in this repo dogfoods the typescript branch
using the workspace-built `target/release/crap4ts` prepended to
`PATH`.

### Multi-language unified HTML render

`html-report: true` + `languages: rust,typescript` (or `all`) routes
through the `crap-render` binary that ships with `crap-core` (≥
0.7.0). The action runs three additive steps gated on
`steps.presets.outputs.is_multi == 'true'`:

1. `Install crap-render` via `taiki-e/install-action@v2` — resolves
   through `crap-core`'s `[package.metadata.binstall]` block to the
   pre-built `crap-render-<target>.tar.gz` uploaded by the
   `build-crap-core-binaries` matrix in `release-plz.yml`.
2. `Render unified HTML` composes per-language envelopes via
   `crap-render --input rust=... --input typescript=... --format html`.
3. `Upload unified HTML report` uploads the rendered document as
   `crap-scorecard-report-<suffix>` (default suffix
   `-${{ runner.os }}`).

The unified URL is surfaced on the new `outputs.html-artifact-url`
action output. The legacy per-language outputs
(`html-artifact-url-{rust,typescript}`) now resolve to
`<unified-url>#<lang>` deep-link anchors in multi-language mode and
the action emits a `::warning::` deprecation notice. Single-language
mode preserves byte-identical γ behavior — the three new steps are
all skipped.

The renderer's library entry points live alongside the existing
reporter and domain types:
`crap_core::domain::multi_lang::{MultiLangContext, LanguageBlock,
CombinedSummary}`, `crap_core::core::compose::compose_multi_lang`,
`crap_core::adapters::reporters::format_html_multi`. ADR for the
placement decision: `~/Github/ops/decisions/crap-rs/adr-multi-lang-renderer-placement.md`.

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
cross-adapter RiskLevel canary
(`crates/crap-core/tests/risk_level_cross_adapter.rs`, test fn
`risk_level_envelope_parity`) shells both bins too; its fn name carries
the `envelope` substring deliberately so the same `--skip envelope`
token covers it without a new token. The
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

## Release envelope publication

Every release-plz tag run publishes two JSON envelope assets to
every release page in the run — `crap4rs-envelope.json` and
`crap4ts-envelope.json`. Each envelope describes its adapter's
analysis of the fixed polyglot sample at `crates/crap-examples/`,
so the same corpus is described across releases and consumers can
diff envelope-vs-envelope to see how the analyzer's output drifts
across crap-rs versions. The dogfood smoke
(`.github/workflows/quick-start-smoke.yml`) and any consumer who
wires the action's Pattern 2b recipe fetch the latest envelope via
`gh release download` and pass it as `--baseline` to render an
enabled Delta tab.

The pattern is wired by two jobs in `.github/workflows/release-plz.yml`:

| Job | Purpose | Cardinality |
|-----|---------|-------------|
| `build-crap-examples-envelopes` | Build adapter bins, lint committed fixtures for absolute paths, run each adapter against `crates/crap-examples/` to produce both envelopes | single ubuntu-latest runner (envelopes are platform-agnostic JSON) |
| `upload-crap-examples-envelopes` | Iterate `jq -r '.[].tag'` over the release-plz `releases` output; upload BOTH envelopes per tag in one `gh release upload "$TAG" file1 file2 --clobber` call; then re-read each tag's assets and assert both envelopes landed | single ubuntu-latest runner; one atomic multi-asset upload per tag |

### Rules

1. **Collapsed-mirror pattern (one runner, not a matrix).** The
   envelopes are platform-agnostic JSON — a 3-target matrix would be
   3× cost for byte-identical output. The binary builds above stay
   matrix-driven because their tarballs ARE per-platform; envelopes
   don't have that property.

2. **Every release page carries both envelopes.** The upload job
   iterates `jq -r '.[].tag'` (not `select(.package_name=="X")`) so
   every release page in a multi-package tag run gets both envelopes
   — `crap-core-v*`, `crap4rs-v*`, `crap4ts-v*` tags are symmetric
   from the consumer's point of view. A `--repo
   breezy-bays-labs/crap-rs --pattern 'crap4rs-envelope.json'` fetch
   resolves to the most recent release of any flavor.

3. **Atomic publication per release page.** `gh release upload
   "$TAG" file1 file2 --clobber` is one API call — either both
   envelopes land on the release page or neither does. The earlier
   shape (parallel one-job-per-envelope) carried upload-side
   asymmetry risk; the single multi-asset upload closes it.

4. **Post-upload verification.** A `gh release view "$TAG" --json
   assets` step re-reads each release page's assets and asserts both
   envelopes are present via `jq -e contains([...])`. Replaces the
   "verified manually post-merge by reading the release page" gate
   with an in-job assertion.

5. **First-fire grace via `continue-on-error: true`.** The dogfood
   smoke's envelope-fetch step continues on failure (no
   envelope-bearing release exists yet during the bootstrap window).
   The fallback path emits a `::warning::` naming the cause so a
   steady-state regression is visually distinguishable from a normal
   first-fire fallback.

6. **Empty-file guard before every `jq` parse.** Every
   `<adapter> ... > envelope.json && jq -e ...` shape in the
   release-plz envelope-build steps carries an explicit
   `[ -s envelope.json ] || { echo "::error::..."; exit 1; }` guard
   between the adapter and the jq parse. Without it, a silent
   adapter failure (empty file) surfaces as `jq: parse error:
   Invalid numeric literal` — opaque. With it, the CI message names
   the cause.

7. **`gh release download --dir` over `--output` for pattern-mode.**
   `--output` is single-file; pattern-mode downloads should use
   `--dir` so multi-asset matches across releases don't collide on
   the output filename.

8. **CI lint blocks absolute paths in committed fixtures.** The
   `build-crap-examples-envelopes` job lints
   `crates/crap-examples/lcov.info` for `^SF:/` lines and
   `crates/crap-examples/coverage-final.json` for absolute path keys.
   Absolute paths in committed fixtures diverge between contributors
   and CI runners and silently produce envelopes with empty
   coverage maps — fail loudly here rather than ship a malformed
   envelope.

### When the rules bite

- Adding a new analyzer or envelope shape → mirror the existing
  build + upload pattern. Don't add a new matrix without confirming
  the output is platform-dependent first (envelopes aren't).
- Editing `crates/crap-examples/` source → regen fixtures
  per `crates/crap-examples/README.md`. The smoke job's staleness
  check warns (does not fail) on drift, but a CI-visible warning
  beats a forgotten regen.
- Triaging a smoke failure that says "::warning::release XYZ
  missing envelope assets" → check the
  `upload-crap-examples-envelopes` job log for the matching tag.
  That step's post-upload verification step would have failed
  loudly first; a warning here without an upload-job failure means
  the post-upload assertion is being silently skipped.

### Coverage staleness guardrail

The forcing function that keeps the committed `crap-examples` coverage
fixtures in sync with the sample source lives in one place:
`scripts/coverage-staleness-check.sh`. The `quick-start-smoke.yml`
smoke job calls it; the script header carries the full rationale
(drift → stale baseline envelope → silently-wrong consumer Delta tab)
and the warn-not-fail / null-SHA / unreachable-base hardening.

It **warns, never fails** — by design, so a transient base-ref glitch
can't wedge the smoke. The logic is guarded on every PR by
`crates/crap-core/tests/coverage_staleness_check.rs`, which exercises
every branch (empty/all-zero ref, unreachable ref, no-merge-base ref,
drift, clean) in a hermetic temp git repo. That regression test proves
the *logic*;
the *integration* layer — that GitHub's runtime actually supplies a
null SHA on an orphan first-push, that a force-push leaves the base
reachable on a PR, etc. — was validated empirically against live
Actions runs (the synthetic-push validation). Keep the script as the
single implementation: never re-inline the bash into the workflow, or
the regression test stops guarding what CI runs.

## Version ownership

**release-plz owns every `[package] version` bump.** Feature PRs must
NOT hand-edit a crate's `[package] version` (or `[workspace.package]
version`), and must not finalize a versioned CHANGELOG section
(`[Unreleased]` entries are fine; the `[x.y.z]` heading is release-plz's
to write). Versions are bumped exactly once, in release-plz's release
PR, in lockstep with the tag + publish.

### Why

Hand-bumping a version inside a feature PR desyncs the version line from
the publish cadence and strands intermediate versions: they get
changelogged on `main` but are never tagged or published. crap-core
climbed 0.5 → 0.6 → 0.7 → 0.8 on `main` via feature PRs with no release
cutting them (crap-rs#448), so 0.6.0/0.7.0 became phantom versions and
release #373 surfaced the confusing jump. The 0.9.0 bump that looked
"breaking" was a *separate* signal — `cargo-semver-checks` correctly
flagging an accidentally-`pub` internal type (see the public-API
boundary policy) — not a version-ownership violation.

### Mechanical enforcement

`scripts/version-edit-lint.py` (the `version-edit-lint` CI job +
`lefthook.yml` pre-push command — single source of truth in the script)
fails a NON-release PR that changes a crate's parsed `[package] version`
or `[workspace.package] version`. The lint:

- **compares parsed TOML values, not diff lines** — a `[dependencies]` /
  `[dev-dependencies]` / inline `{ version = "..." }` change can never
  false-positive, because only the real published-version field is read;
- **exempts brand-new crates** (no prior `[package]` → not a bump) and
  **deleted manifests**;
- is **gated to non-release branches**: release-plz's own release PR
  (head ref `release-plz-*`) is skipped at the job/hook level, so the
  bumps it makes are never flagged;
- **fails open** on a missing base ref / TOML parse error (a release
  gate must not wedge on a shallow checkout, and invalid TOML is the
  build's job to catch).

The CI job checks out with `fetch-depth: 0` and passes the PR base SHA
(`BASE_SHA`); the pre-push hook diffs against `origin/main`. A
`--self-test` flag guards the comparison logic itself
(documentation rots; CI doesn't — same pattern as
`scripts/bdd-tracked-lint.py` and `scripts/mutants-skip-lint.py`).

There is **no inline override**: the only legitimate feature-PR version
touch is a new crate (exempt). A genuine one-off bump belongs on the
release-plz path. The decision is captured in
`adr-release-process-and-public-api-boundary`.
