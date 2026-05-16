# class-validator Second-Oracle Fixture

This directory snapshots [`typestack/class-validator`](https://github.com/typestack/class-validator)'s
TypeScript source tree at a known tag plus the matching jest-native
Istanbul-format coverage data and crap4ts@2.x's own first-run CRAP
output, frozen as a regression baseline. It is the **second** oracle in
the W3 parity corpus, a sibling to `crap4ts-v1/` (W3.1, #189) — it does
NOT touch the v1.x oracle.

The v1.x oracle is crap4ts's own plain-TS source (adapters / domain /
cli — no decorators, no generics-heavy code, no JSX). class-validator
is a **decorator-heavy** validation library: the bulk of its 136
source files are property/class decorator definitions. Decorators are the
single highest-value pattern the v1.x oracle does not exercise and a
known walker watch-area (follow-up #200), so this corpus broadens the
empirical envelope the W3.2 parity harness (#190) runs against before
the W4.1a npm 2.0.0 publish.

Used by `crates/crap4ts/tests/parity_*.rs` (W3.2 / #190 — **not part of
#208**; #208 is pure corpus + frozen baseline, #190 consumes it).

## Captured state

- **Project**: `typestack/class-validator`
- **Commit**: `2e1a5c27dbd65b80e27fe96b49bd6e6641fa3603` — `chore: publish 0.15.1 (#2672)` (tag `v0.15.1`)
- **Tag rationale**: `v0.15.1` is class-validator's newest released tag
  at capture time. It retains the identical jest-native instrumentation
  (`ts-jest` preset, `jest@^29.7.0`) as earlier `v0.14.x` tags but adds
  more decorator definitions (more of the highest-value v1.x-gap
  pattern) and exercises the most current source — broadest, most
  representative signal with no instrumentation downside. Older tags
  (`v0.14.2`–`v0.14.4`) were equivalent on jest setup; the newest was
  chosen on the "broadest envelope" criterion.
- **Captured on**: 2026-05-15
- **Toolchain**:
  - node: `v25.9.0`
  - npm: `11.12.1`
  - jest: `29.7.0` (project devDep `jest@^29.7.0`)
  - ts-jest: `29.1.2` (project devDep `ts-jest@^29.1.2`; jest preset `ts-jest`)
  - typescript: `5.4.5`
  - crap4ts: `2.0.0-alpha.1` (this worktree, `cargo build --release --bin crap4ts`)

## Capture procedure

The capture is reproducible against tag `v0.14.2` by following these
steps. **All work happens against a read-only clone in `/tmp` — nothing
is committed or pushed back to the OSS project. CI never runs node/jest
(the W3.2 contract): the snapshot, coverage, and reference are frozen
committed artifacts.**

1. Clone the OSS project to a temp location and pin the released tag:

   ```bash
   git clone https://github.com/typestack/class-validator.git /tmp/oracle-src/class-validator
   cd /tmp/oracle-src/class-validator
   git checkout v0.15.1
   git rev-parse HEAD   # 2e1a5c27dbd65b80e27fe96b49bd6e6641fa3603
   ```

2. Install dependencies from the committed lockfile (offline-cache
   preferred — the corpus capture never needs to mutate the lockfile):

   ```bash
   npm ci --prefer-offline
   ```

3. Run the project's jest suite with the Istanbul JSON coverage
   reporter. class-validator's jest config (`jest.config.js`) uses the
   `ts-jest` preset, which instruments via **istanbul natively** — no
   provider flip, no V8 fallback, no config fragility. The `json`
   reporter emits `coverage/coverage-final.json` in exactly the flat
   Istanbul shape arm 1 of `IstanbulCoverage::parse` consumes (D16):

   ```bash
   node_modules/.bin/jest --coverage --coverageReporters=json --ci
   # 15 suites, 806 tests pass
   # Produces coverage/coverage-final.json — flat Istanbul shape,
   # absolute machine-local paths under .../src/
   ```

4. Snapshot the TS source into the fixture with the W3.1 rsync exclude
   set (paths relative to the crap-rs worktree):

   ```bash
   rsync -a --exclude='__tests__/' --exclude='*.test.ts' \
         --exclude='*.spec.ts' --exclude='dist/' --exclude='build/' \
         --exclude='coverage/' --exclude='node_modules/' \
         --exclude='.DS_Store' \
         /tmp/oracle-src/class-validator/src/ \
         crates/crap4ts/tests/fixtures/class-validator/src/
   cp /tmp/oracle-src/class-validator/coverage/coverage-final.json \
      crates/crap4ts/tests/fixtures/class-validator/
   ```

   class-validator co-locates `*.spec.ts` next to source under `src/`;
   the `--exclude='*.spec.ts'` rule keeps them out of the corpus (136
   `.ts` source files snapshot; 0 spec/test files). `rsync --delete`
   keeps the fixture exactly in sync if re-captured against a future
   tag (no stale files left behind).

5. Capture crap4ts@2.x's frozen baseline against the same Istanbul
   coverage:

   ```bash
   cargo build --release --bin crap4ts
   ./target/release/crap4ts \
     --src crates/crap4ts/tests/fixtures/class-validator/src \
     --coverage crates/crap4ts/tests/fixtures/class-validator/coverage-final.json \
     --format json \
     > crates/crap4ts/tests/fixtures/class-validator-reference.json
   ```

   The crap4ts CLI exits **non-zero** when functions exceed the
   threshold; that's expected and does NOT mean the capture failed —
   verify by parsing the JSON
   (`jq . crates/crap4ts/tests/fixtures/class-validator-reference.json`).
   At commit `2e1a5c2` the baseline reports 568 functions, 1 exceeding
   the default threshold of 25 (`container.ts :: getFromContainer`,
   CRAP 32.62), so the CLI exits **1** with `passed: false`. That is
   the expected frozen state.

## Baseline disposition

**No v1.x CRAP reference exists for this OSS project.**
`class-validator-reference.json` is **crap4ts@2.x's own first-run
`--format json` output, frozen as a regression baseline** — NOT a
v1.x-format reference file (contrast `crap4ts-v1-reference.json`, which
is crap4ts@1.x's own tool output and supports true cross-validation).

class-validator does not ship a CRAP scorecard, so "parity" here is
**regression detection, not cross-validation**: #190 detects
regressions by diffing future crap4ts@2.x runs against this frozen
absolute baseline (captured at commit `2e1a5c2`), NOT by comparing
against an independent tool's output. This is the "no v1 reference —
use absolute output as baseline" disposition documented in the W3.1
README's "Tolerance band" section and the #208 issue body. Any future
crap4ts@2.x change that moves a class-validator function's complexity,
coverage, or CRAP score is a baseline drift that #190's harness
surfaces for triage (legitimate improvement vs. score regression).

### Known default-threshold caveat (#218) — regenerate post-fix

This baseline was captured at crap4ts@2.x's **no-flag default
top-level gate `threshold` of `25.0`**. That `25` is crap-core's
*shared* default (crap4rs's cognitive-metric value), **NOT** the
D5-calibrated crap4ts cyclomatic default of **16** that locked
pipeline decision #2/#5 mandates. W2.5 (#188) wired the default
*metric* (cyclomatic) and the per-function/per-row threshold (`16.0`)
but the top-level *gate* threshold still falls back to `25`; the
`wire_envelope_crap4ts` canary masks this because it invokes the
binary with an explicit `--threshold 16`. This is a real crap4ts
default-threshold bug tracked as **#218** (sub-issue of #173,
`type:bug priority:soon`).

The baseline is deliberately **left frozen at the actual current
default (`threshold: 25`)** per the W3.1 "freeze reality" precedent
(the v1.x oracle likewise froze v1.x's real default threshold of 12) —
a regression baseline must capture what crap4ts@2.x produces *today*,
not its intended-after-fix behavior. **#190 (W3.2) must regenerate
this baseline after #218 lands**: the top-level `threshold`, `passed`,
each function's `exceeds`, and the `exceeding`-count will all shift
when the default corrects 25 → 16. Until then, the "exceeding default
threshold (25)" row in the Sanity-check table below reflects the
buggy-default capture, not the D5-intended gate.

## Sanity-check (captured at `2e1a5c2`)

From the frozen baseline + `--verbose` parse statistics:

| Metric | Value |
|---|---|
| Source files snapshot | 136 `.ts` |
| Files found by walker | 136 |
| Files unparseable | 0 |
| Files analyzed (with coverage) | 129 |
| Files with zero coverage | 2 |
| Functions extracted | 568 |
| Functions matched to coverage | 558 |
| Functions with no coverage | 10 |
| `parse_diagnostics` (incl. `PathUnresolved`) | **0** |
| Average CRAP | 1.91 |
| Median CRAP | 1.0 |
| Max CRAP | 32.62 (`container.ts :: getFromContainer`, high) |
| Functions exceeding default threshold (25) | 1 |
| Coverage health (non-zero / total) | 545 / 568 (95.9%) |
| Functions at 100% coverage | 517 |
| Average coverage | 94.29% |

### Zero-coverage notes (all benign — `parse_diagnostics` is empty)

The 2 fully-zero-coverage files are upstream test-coverage gaps in
class-validator itself, not crap4ts parser bugs:

- `index.ts` — the public-API barrel. class-validator's `jest.config.js`
  sets `collectCoverageFrom: [..., '!src/**/index.ts']`, so the barrel
  wrappers are deliberately uninstrumented by the OSS project.
- `validation-schema/ValidationSchemaToMetadataTransformer.ts` — the
  schema-DSL path the OSS spec suite does not exercise.

The remaining ~10 individually-zero-coverage rows are decorator-factory
inner closures not directly invoked by the spec suite (expected for a
decorator library). The worst function (`getFromContainer`, cc 8,
27% cov, CRAP 32.62, high) is a genuinely complex + under-tested OSS
function — exactly the signal a CRAP oracle should surface, not a
defect. `parse_diagnostics: []` confirms zero parser/walker
involvement, so no follow-up issue is warranted (#208 stays data-only).

**Coverage signal is healthy, not degenerate.** Despite the
coverage-final.json carrying absolute machine-local paths
(`/private/tmp/oracle-src/class-validator/src/...`) that share no
prefix with the fixture's `--src` root, the #215 suffix-fallback arm
in `normalize_path` resolved **every** entry — `parse_diagnostics` is
empty (zero `PathUnresolved`). This corpus is therefore also a live
cross-machine-portability regression check for the #215 fix.

## What's NOT in this fixture

- **A v1.x CRAP reference**: class-validator is a third-party OSS
  project, not crap4ts; there is no independent CRAP tool output to
  cross-validate against. See "Baseline disposition" above.
- **V8-format coverage**: class-validator's jest config instruments
  via istanbul natively (`ts-jest` preset), so the corpus is pure
  Istanbul flat-shape (D16 arm 1). V8 support in crap4ts@2.x is
  deferred to v2.1 (#212) and is not exercised here.
- **Test / spec files (`*.spec.ts`, `__tests__/`, `test/`)**: excluded
  from the `src/` corpus per the W3.1 exclude set — class-validator
  co-locates `*.spec.ts` next to source; we do not score test files.
- **Generated artifacts (`build/`, `dist/`, `coverage/`)**: excluded;
  not source. The instrumented `coverage-final.json` itself is the
  only coverage artifact retained, at the fixture root.
- **Harness code**: #208 is pure corpus + frozen baseline + docs. The
  parity harness that consumes both oracles and applies the three-way
  classification (threshold-default-change / score-regression /
  v1.x-was-buggy) is #190 (W3.2), which is *blocked by* #208.
