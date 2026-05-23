Feature: Istanbul branch coverage flows through to the scorecard
  As a TypeScript developer with branch-coverage measurement enabled
  I want crap4ts to surface per-function branch coverage alongside line coverage
  So that I can identify functions whose branches are tested but whose statements are not (or vice versa)

  @wired
  Scenario: A coverage-final.json with branch records populates ParseOutput.branches
    # Wired by `branch_coverage_cucumber.rs` (crap-rs#251) — the
    # surface lift to per-function envelope rows + the conditional
    # `Branch%` table column landed in the same PR.
    Given an Istanbul `coverage-final.json` whose entries include `b` and `branchMap` records
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes a branch-coverage column for every covered function
    And the branch-coverage entries are keyed by workspace-relative paths
    And every branch record in the input is paired with its `branchMap` entry

  @wired
  Scenario: A coverage-final.json with NO branch records leaves ParseOutput.branches as None
    # Wired by `branch_coverage_cucumber.rs` (crap-rs#251). The
    # `Option<f64>` + `skip_serializing_if(None)` wire model means the
    # "omits the field (or sets it to null)" alternative collapses to
    # "absent" in practice; the harness accepts both shapes.
    Given an Istanbul `coverage-final.json` whose entries have empty `b` and empty `branchMap`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the scorecard renders with no branch-coverage column
    And the JSON envelope omits the `branches` field (or sets it to null)

  @wired
  Scenario: A function's branch coverage joins its line coverage in the report
    # Wired by `branch_coverage_cucumber.rs` (crap-rs#251). The "6
    # branches" wording is Istanbul-arm-count narration (not the
    # walker's CC primitive); the harness's authored fixture exercises
    # two branchIds (a 2-arm if + a 4-arm switch-style group) with hit
    # pattern `[1,1]` + `[1,0,1,0]` = 4 of 6 arms taken → 66.7%
    # rounded one decimal.
    Given a TypeScript function with cyclomatic complexity 3 (one if/else, one ternary)
    And a coverage-final.json showing 4 of 6 branches hit (66% branch coverage)
    And the same function shows 100% line coverage in the `s` record
    When the operator runs `crap4ts --coverage coverage-final.json --src src --format json`
    Then the function's `lineCoverage` is 100.0
    And the function's `branchCoverage` is 66.7 (rounded one decimal)
    And both values appear in the JSON envelope's row for the function

  @wired
  Scenario: A b record references a branchId not in branchMap emits BranchMismatch
    # Wired by `branch_coverage_cucumber.rs` (crap-rs#251). The
    # parser's "diagnostic + skip THAT branch, never abort first-
    # record" contract (#187) means the function's row still scores
    # against the non-orphaned branches in its span; the affected
    # entry's `branchCoverage` is absent (null at the JSON-text level)
    # only when the orphan was the function's sole branch.
    Given an Istanbul `coverage-final.json` whose `b` references branchId `42` and `branchMap` omits `42`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the parser emits an `IstanbulParseDiagnostic` with kind `branch-mismatch`
    And the function's `branchCoverage` is `null` for the affected entry
    And the rest of the scorecard still produces line coverage for the file
