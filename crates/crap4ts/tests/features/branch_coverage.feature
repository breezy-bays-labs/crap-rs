Feature: Istanbul branch coverage flows through to the scorecard
  As a TypeScript developer with branch-coverage measurement enabled
  I want crap4ts to surface per-function branch coverage alongside line coverage
  So that I can identify functions whose branches are tested but whose statements are not (or vice versa)

  Background:
    # Per CAO advisory A-3, branch coverage uses the EXISTING
    # ParseOutput.branches: Option<HashMap<String, Vec<BranchCoverage>>> slot
    # already defined in crap-core::ports::ParseOutput. No new parallel seam.
    # W2.3 lands this: extend IstanbulCoverage::parse to consume `b` records
    # alongside `s` records.

  @unwired
  Scenario: A coverage-final.json with branch records populates ParseOutput.branches
    # tracked: crap-rs#173 — W2.3 branch coverage extension; harness lands in W3.3
    Given an Istanbul `coverage-final.json` whose entries include `b` and `branchMap` records
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes a branch-coverage column for every covered function
    And the branch-coverage entries are keyed by workspace-relative paths
    And every branch record in the input is paired with its `branchMap` entry

  @unwired
  Scenario: A coverage-final.json with NO branch records leaves ParseOutput.branches as None
    # tracked: crap-rs#173 — W2.3 default branches=None when adapter doesn't emit; harness lands in W3.3
    Given an Istanbul `coverage-final.json` whose entries have empty `b` and empty `branchMap`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the scorecard renders with no branch-coverage column
    And the JSON envelope omits the `branches` field (or sets it to null)

  @unwired
  Scenario: A function's branch coverage joins its line coverage in the report
    # tracked: crap-rs#173 — W2.3 per-function branch coverage join; harness lands in W3.3
    Given a TypeScript function with cyclomatic complexity 3 (one if/else, one ternary)
    And a coverage-final.json showing 4 of 6 branches hit (66% branch coverage)
    And the same function shows 100% line coverage in the `s` record
    When the operator runs `crap4ts --coverage coverage-final.json --src src --format json`
    Then the function's `lineCoverage` is 100.0
    And the function's `branchCoverage` is 66.7 (rounded one decimal)
    And both values appear in the JSON envelope's row for the function

  @unwired
  Scenario: A b record references a branchId not in branchMap emits BranchMismatch
    # tracked: crap-rs#173 — W2.3 mismatch surfaces as diagnostic, not silent drop; harness lands in W3.3
    Given an Istanbul `coverage-final.json` whose `b` references branchId `42` and `branchMap` omits `42`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the parser emits an `IstanbulParseDiagnostic` with kind `branch-mismatch`
    And the function's `branchCoverage` is `null` for the affected entry
    And the rest of the scorecard still produces line coverage for the file
