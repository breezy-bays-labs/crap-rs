Feature: Branch coverage dark infrastructure

  BRDA record parsing, branch domain types, and branch matching
  are wired internally but do not affect CRAP scoring or CLI output.
  Dark infrastructure for future --coverage-metric branch support.

  # ── BRDA Parsing ───────────────────────────────────────────────────

  Scenario: Valid BRDA records are parsed alongside DA records
    Given an LCOV file with SF, DA, and BRDA records for "lib.rs"
    When the coverage is parsed
    Then ParseOutput.branches contains an entry for "lib.rs"
    And ParseOutput.coverage contains line data as before

  Scenario: BRDA taken value "-" maps to None
    Given a BRDA record "BRDA:10,0,0,-"
    When the record is parsed
    Then the taken value is None

  Scenario: BRDA numeric taken value maps to Some
    Given a BRDA record "BRDA:10,0,0,5"
    When the record is parsed
    Then the taken value is Some(5)

  Scenario: BRDA taken value "0" maps to Some(0)
    Given a BRDA record "BRDA:10,0,0,0"
    When the record is parsed
    Then the taken value is Some(0)

  Scenario: Duplicate BRDA records are merged by summing taken
    Given two BRDA records with the same line, block, and branch
      | record          |
      | BRDA:10,0,0,3   |
      | BRDA:10,0,0,7   |
    When the coverage is parsed
    Then the merged branch has taken value Some(10)

  Scenario: Duplicate BRDA with "-" and numeric sums only the numeric
    Given two BRDA records with the same key
      | record          |
      | BRDA:10,0,0,-   |
      | BRDA:10,0,0,5   |
    When the coverage is parsed
    Then the merged branch has taken value Some(5)

  Scenario: Malformed BRDA records are skipped with diagnostic
    Given an LCOV file containing "BRDA:not,valid"
    When the coverage is parsed
    Then a MalformedRecord diagnostic is emitted
    And parsing continues without error

  Scenario: LCOV file with no BRDA records produces None branches
    Given an LCOV file with only SF and DA records
    When the coverage is parsed
    Then ParseOutput.branches is None

  # ── Branch Domain Types ────────────────────────────────────────────

  Scenario: BranchCoverage carries only line position and execution count
    Given a BranchCoverage with line 10 and taken Some(3)
    Then it exposes a line number and an optional execution count
    And no format-specific identifiers are stored

  Scenario: CoverageMetric enum has Line and Branch variants
    Given the CoverageMetric enum
    Then it has variant Line
    And it has variant Branch
    And the default is Line

  # ── Branch Matching ────────────────────────────────────────────────

  Scenario: Branch points within function span are matched
    Given a function spanning lines 5-15
    And branch data with entries at lines 7, 10, and 12
    When functions are matched with branch data
    Then branch_coverage covers 3 total branches

  Scenario: Branch points outside function span are excluded
    Given a function spanning lines 5-15
    And branch data with entries at lines 3, 10, and 20
    When functions are matched with branch data
    Then branch_coverage covers 1 total branch

  Scenario: Branch boundaries are inclusive
    Given a function spanning lines 5-15
    And branch data with entries at lines 5 and 15
    When functions are matched with branch data
    Then branch_coverage covers 2 total branches

  Scenario: Branches with taken None are excluded from ratio
    Given a function spanning lines 5-15
    And branch data:
      | line | taken   |
      | 7    | Some(3) |
      | 10   | None    |
      | 12   | Some(0) |
    When functions are matched with branch data
    Then branch_coverage.total is 2
    And branch_coverage.covered is 1
    And the None branch is excluded from the ratio

  Scenario: Function with no branch points gets None branch_coverage
    Given a function spanning lines 5-15
    And no branch data entries within that span
    When functions are matched with branch data
    Then FunctionCoverage.branch_coverage is None

  Scenario: No cross-file branch leakage
    Given function "foo" in "a.rs" spanning lines 1-10
    And function "bar" in "b.rs" spanning lines 1-10
    And branch data for "a.rs" at line 5 with taken Some(1)
    And no branch data for "b.rs"
    When functions are matched with branch data
    Then "foo" has branch_coverage with total 1
    And "bar" has branch_coverage None

  # ── Core Plumbing ──────────────────────────────────────────────────

  Scenario: analyze() passes branch data through without affecting scores
    Given a project with LCOV containing both DA and BRDA records
    When analyze() is called with default options
    Then CRAP scores are computed from line coverage only
    And FunctionCoverage carries branch_coverage data

  Scenario: AnalyzeOptions defaults to Line coverage metric
    Given default AnalyzeOptions
    Then coverage_metric is CoverageMetric::Line

  # ── Property Invariants ────────────────────────────────────────────

  Scenario: Arbitrary BRDA input never panics
    Given arbitrary strings matching "BRDA:*" patterns
    When parsed by the BRDA parser
    Then no panic occurs

  Scenario: Branch ratios are always in [0, 100]
    Given any set of BranchCoverage entries matched to a function
    When branch coverage is computed
    Then the percent is between 0.0 and 100.0 inclusive

  Scenario: Branch covered is always <= total
    Given any set of BranchCoverage entries matched to a function
    When branch coverage is computed
    Then covered is less than or equal to total

  Scenario: No cross-file branch leakage under arbitrary input
    Given arbitrary branch data across multiple files
    And arbitrary function spans
    When functions are matched with branch data
    Then each function's branch_coverage only reflects its own file
