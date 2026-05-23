Feature: crap4ts@2 parity with crap4ts@1.x reference outputs
  As a TypeScript developer migrating from crap4ts@1.x to crap4ts@2.0.0
  I want CRAP scores that diverge only by documented, intentional reasons
  So that score regressions in CI surface real issues, not adapter swaps

  @wired
  Scenario: crap4ts@2 cyclomatic scores match crap4ts@1.x within tolerance
    Given the snapshotted crap4ts@1.x source corpus at `tests/fixtures/crap4ts-v1/`
    And the captured v1.x reference outputs at `tests/fixtures/crap4ts-v1-reference.json`
    When the parity harness runs `crap4ts --src tests/fixtures/crap4ts-v1/src --coverage <v1-coverage>` and compares
    Then 95%+ of functions match cyclomatic complexity within ±0 (exact match)
    And 100% of functions match risk-classification labels (Low/Acceptable/Moderate/High)
    And any divergence is reported per-function in the harness output

  @wired
  Scenario: Divergence output shows per-function contributor breakdown, not just score diff
    # The crap4ts@1.x reference JSON records a per-function cyclomatic
    # number but no contributor list, so the harness surfaces v2's
    # contributor breakdown only — there is no v1 contributor list to
    # diff against. See the parity_helpers module documentation.
    Given a function whose v1.x reference has cyclomatic 4 (contributors: 2× if-branch, 1× ternary)
    And whose v2 output has cyclomatic 3 (contributors: 2× if-branch only — ternary missed)
    When the parity harness reports the divergence
    Then the report names the function
    And the report shows v2 contributors: `2× if-branch`

  @wired
  Scenario: Risk classification labels match across versions (D8 invariance check)
    Given the v1.x corpus + reference outputs
    When the parity harness compares risk labels function-by-function
    Then every function's risk classification matches v1.x to v2 exactly
    And the cutoff boundaries (5/8/30) are NOT version-sensitive

  @wired
  Scenario: Threshold-default difference is documented, not a parity failure
    # Three documented compounding reasons (per MIGRATION.md): threshold 12→16
    # calibration, Rust-derived calibration awaiting TS validation, possible
    # arrow-function undercount. The parity harness must distinguish these
    # from genuine score-drift regressions.
    Given a function whose v1.x CRAP score crosses the 12 threshold but stays below 16
    When the parity harness compares pass/fail gate outcomes
    Then v1.x reports the function as `failing` (score > 12)
    And v2 reports the function as `passing` (score < 16)
    And the harness flags this as `threshold-default-change`, NOT as `score-regression`

  @wired
  Scenario: A discovered score divergence triggers a tracked follow-up
    Given the parity harness has identified a function with score divergence > ε
    When the divergence is NOT explained by threshold-default-change or arrow-function-undercount
    Then the harness output recommends filing a follow-up issue under epic #173
    And the recommended issue body includes the function name + v2 contributors
