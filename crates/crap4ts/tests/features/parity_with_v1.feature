Feature: crap4ts@2 parity with crap4ts@1.x reference outputs
  As a TypeScript developer migrating from crap4ts@1.x to crap4ts@2.0.0
  I want CRAP scores that diverge only by documented, intentional reasons
  So that score regressions in CI surface real issues, not adapter swaps

  Background:
    # crap4ts@1.x source (~/Github/crap4ts/) is the primary TS test corpus
    # AND the cross-validation oracle. W3.1 snapshots the v1.x source as
    # `crates/crap4ts/tests/fixtures/crap4ts-v1/` and captures v1.x's own
    # CRAP outputs as `crap4ts-v1-reference.json` via one-shot `pnpm run crap`.
    # W3.2 cross-validates new crap4ts against the captured reference.
    #
    # Per CPO sharpening, the parity harness reports per-function CONTRIBUTOR
    # BREAKDOWNS in its diff output, not just per-function score diffs —
    # so divergence triage is one line, not manual grep + read per disagreement.

  @unwired
  Scenario: crap4ts@2 cyclomatic scores match crap4ts@1.x within tolerance
    # tracked: crap-rs#173 — W3.2 score parity harness; harness lands in W3.3
    Given the snapshotted crap4ts@1.x source corpus at `tests/fixtures/crap4ts-v1/`
    And the captured v1.x reference outputs at `tests/fixtures/crap4ts-v1-reference.json`
    When the parity harness runs `crap4ts --src tests/fixtures/crap4ts-v1/src --coverage <v1-coverage>` and compares
    Then 95%+ of functions match cyclomatic complexity within ±0 (exact match)
    And 100% of functions match risk-classification labels (Low/Acceptable/Moderate/High)
    And any divergence is reported per-function in the harness output

  @unwired
  Scenario: Divergence output shows per-function contributor breakdown, not just score diff
    # tracked: crap-rs#173 — W3.2 diff is actionable (CPO sharpening); harness lands in W3.3
    Given a function whose v1.x reference has cyclomatic 4 (contributors: 2× if-branch, 1× ternary)
    And whose v2 output has cyclomatic 3 (contributors: 2× if-branch only — ternary missed)
    When the parity harness reports the divergence
    Then the report names the function
    And the report shows v1.x contributors: `2× if-branch + 1× ternary`
    And the report shows v2 contributors: `2× if-branch`
    And the report identifies the missing contributor kind by name (ternary)

  @unwired
  Scenario: Risk classification labels match across versions (D8 invariance check)
    # tracked: crap-rs#173 — W3.2 D8 risk-cutoffs metric-invariant cross-version check; harness lands in W3.3
    Given the v1.x corpus + reference outputs
    When the parity harness compares risk labels function-by-function
    Then every function's risk classification matches v1.x to v2 exactly
    And the cutoff boundaries (5/8/30) are NOT version-sensitive

  @unwired
  Scenario: Threshold-default difference is documented, not a parity failure
    # tracked: crap-rs#173 — W3.2 threshold default 12→16 is intentional break, not a regression; harness lands in W3.3
    # Three documented compounding reasons (per MIGRATION.md): threshold 12→16
    # calibration, Rust-derived calibration awaiting TS validation, possible
    # arrow-function undercount. The parity harness must distinguish these
    # from genuine score-drift regressions.
    Given a function whose v1.x CRAP score crosses the 12 threshold but stays below 16
    When the parity harness compares pass/fail gate outcomes
    Then v1.x reports the function as `failing` (score > 12)
    And v2 reports the function as `passing` (score < 16)
    And the harness flags this as `threshold-default-change`, NOT as `score-regression`

  @unwired
  Scenario: A discovered score divergence triggers a tracked follow-up
    # tracked: crap-rs#173 — W3.2 divergence response policy; harness lands in W3.3
    Given the parity harness has identified a function with score divergence > ε
    When the divergence is NOT explained by threshold-default-change or arrow-function-undercount
    Then the harness output recommends filing a follow-up issue under epic #173
    And the recommended issue body includes the function name + v1.x contributors + v2 contributors
