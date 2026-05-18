# crap4ts@1.x Parity Oracle Fixture

This directory snapshots crap4ts@1.x's TypeScript source tree at a known commit
plus the matching Istanbul-format coverage data and v1.x's own CRAP scorecard
output. Used by `crates/crap4ts/tests/parity_v1.rs` (W3.2) to validate that
crap4ts@2.x produces results consistent with v1.x within a documented tolerance
band, while distinguishing legitimate improvements (e.g., the crap4ts#37 bug
class — span-overlap-ratio mis-matching for long-parameter functions) from
score regressions.

## Captured state

- **Commit**: `44eb2c2` — `chore: normalize package metadata` (`breezy-bays-labs/crap4ts` @ v1.0.1)
- **Captured on**: 2026-05-14
- **Toolchain**:
  - node: `v25.9.0`
  - pnpm: `10.33.0`
  - vitest: `3.2.4`
  - @vitest/coverage-istanbul: `3.2.4` (pinned to match `vitest@3.2.4`; pnpm
    will otherwise resolve `latest` to `4.x` which requires `vitest@4.x` and
    fails with `SyntaxError: ... 'BaseCoverageProvider'`)
  - crap4ts: `1.0.1`

## Capture procedure

The capture is reproducible against any future v1.x commit by following these
steps. **All modifications to `~/Github/crap4ts/` are local-only for capture
purposes — do NOT commit or push them to the v1.x repo (which is in
maintenance mode pending the v2.x cutover, see crap4ts#38).**

1. Add v1.x's CLI + Istanbul coverage provider as devDeps. Pin the Istanbul
   provider to the same minor as the project's installed vitest:

   ```bash
   cd ~/Github/crap4ts
   pnpm add -D crap4ts@^1.0.1 '@vitest/coverage-istanbul@^3.2.4'
   ```

   If the project upgrades vitest to a different major in a future capture,
   bump the Istanbul provider pin to match (`@vitest/coverage-istanbul@^4.x`
   etc.). Cross-major combinations error out at coverage init.

2. Flip vitest to Istanbul format (V8 is the default; we need Istanbul because
   crap4ts@2.x's W2.4 parser handles only Istanbul today — V8 support is
   tracked as a follow-up for v2.1):

   ```bash
   # Edit ~/Github/crap4ts/vitest.config.ts:
   #   provider: "v8" → provider: "istanbul"
   ```

3. Run coverage:

   ```bash
   pnpm test:coverage
   # Produces ~/Github/crap4ts/coverage/coverage-final.json in Istanbul format
   ```

4. Capture v1.x's CRAP scorecard against the same Istanbul coverage:

   ```bash
   pnpm exec crap4ts --format json > crap4ts-v1-reference.json
   ```

   v1.x's CLI auto-detects `coverage/coverage-final.json` per its
   `detect.ts` logic. The CLI exits **non-zero (1)** when functions exceed the
   threshold; that's expected behaviour and does NOT mean the capture failed
   — verify by parsing the JSON output. At commit `44eb2c2` the v1.x oracle
   reports 137 functions, 8 exceeding the default threshold of 12 (so the
   CLI exits 1 with `passed: false`).

5. Copy artifacts into the crap-rs fixture (paths relative to crap-rs worktree):

   ```bash
   rsync -av --exclude='__tests__/' --exclude='*.test.ts' --exclude='dist/' \
         --exclude='node_modules/' --exclude='.DS_Store' \
         ~/Github/crap4ts/src/ \
         crates/crap4ts/tests/fixtures/crap4ts-v1/src/
   cp ~/Github/crap4ts/coverage/coverage-final.json \
      crates/crap4ts/tests/fixtures/crap4ts-v1/
   cp crap4ts-v1-reference.json \
      crates/crap4ts/tests/fixtures/
   ```

   **Do NOT add `--exclude='coverage/'` back.** The build-artifact
   `coverage/` directory lives at the repo *root*
   (`~/Github/crap4ts/coverage/`), which is outside this rsync's
   `~/Github/crap4ts/src/` source root — it is never in the transfer set,
   so excluding it is unnecessary. Worse, rsync's `coverage/` pattern is
   unanchored: it matches a directory named `coverage` at *any* depth, so
   it silently drops the `src/adapters/coverage/` *source* subtree (5 files,
   26 scored functions in the oracle). That defect shipped in the first
   capture and was caught during W3.2 #190 pre-flight. The remaining
   directory excludes (`__tests__/`) are intentionally unanchored —
   co-located test directories at any depth should be dropped. `dist/`
   and `node_modules/` do not occur under `src/`; they are belt-and-braces.

6. Revert v1.x's local-only modifications:

   ```bash
   cd ~/Github/crap4ts
   git checkout package.json package-lock.json vitest.config.ts
   pnpm install   # restores node_modules to clean state
   ```

## Tolerance band (W3.2 parity gate)

The W3.2 parity harness applies the following thresholds per the
`parity_with_v1.feature` BDD contract and the CQO ADVISORY-4 audit note in the
pipeline impl plan:

| Dimension | Tolerance | Failure mode |
|---|---|---|
| Risk classification (Low/Acceptable/Moderate/High) | **Hard match required** | Zero tolerance — any mismatch fails the gate |
| Complexity exact-match rate | **≥ 95% of functions** must match CC exactly | Up to 5% may differ with documented contributor-list reason |
| CRAP score | **±0.5 absolute** acceptable IF complexity AND coverage are both unchanged | Otherwise fail per the threshold-default-change vs score-regression rule below |
| Contributor line drift | **±1 line** on matched contributors (off-by-one boundary on half-open spans) | Missing or extra contributors = hard fail |
| Function discovery | Exact match required | crap4ts@2.x must discover every function v1.x discovered |

## Three-way classification (W3.2)

When crap4ts@2.x's output differs from this reference, the parity harness
classifies the divergence into one of three buckets:

1. **Threshold-default-change** (expected) — v1.x used the default threshold
   `12`; crap4ts@2.x uses `16` per D5 calibration (impl-plan locked decision
   #2). A function that crossed the 12 → 16 line moves classification without
   the underlying score changing. **Passes.**

2. **Score regression** (must fix) — same complexity, same coverage, but
   different CRAP score; OR different complexity; OR different coverage. The
   adapter has a bug. **Fails the parity gate.**

3. **v1.x-was-buggy** (improvement) — the crap4ts#37 bug class. v1.x's
   `findCandidates` uses an 80% overlap-ratio threshold for span matching,
   which fails for functions with long parameter lists (declaration span is
   much larger than body span, so ratio < 0.8 → rejected → 0% coverage
   reported despite test data). crap4rs/crap4ts@2.x uses strict line-range
   containment (per `CLAUDE.md`'s "Line-range matching" design decision),
   which is structurally immune. When v2.x reports non-zero coverage on a
   function where v1.x reported 0%, **passes** and the divergence is logged
   in the parity report as an "improvement" entry, not a regression.

Reference: PR `breezy-bays-labs/crap4ts#37` (open; from outside contributor
ardsh) documents the v1.x matcher bug and contains the fix proposal that
v1.x will inherit before its retirement.

## What's NOT in this fixture

- **V8-format coverage**: deferred to v2.1 (tracked as a follow-up). V8 support
  in crap4ts@2.x is a minor bump per the additive CoveragePort architecture +
  the auto-detect pattern from v1.x's `src/adapters/coverage/detect.ts`.
- **Test files (`__tests__/`, `*.test.ts`)**: excluded from `src/` corpus —
  v1.x doesn't analyze its own tests for CRAP scoring, so neither should our
  parity oracle.
- **Generated artifacts** (`dist/`, `coverage/`): excluded; not source.
