Feature: Branch coverage dark infrastructure

  BRDA record parsing, branch domain types, and branch matching
  are wired internally but do not affect CRAP scoring or CLI output.
  Dark infrastructure for future --coverage-metric branch support.

  # ── BRDA Parsing ───────────────────────────────────────────────────

  @unwired
  Scenario: Valid BRDA records are parsed alongside DA records
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given an LCOV file with SF, DA, and BRDA records for "lib.rs"
    When the coverage is parsed
    Then ParseOutput.branches contains an entry for "lib.rs"
    And ParseOutput.coverage contains line data as before

  @unwired
  Scenario: BRDA taken value "-" maps to None
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a BRDA record "BRDA:10,0,0,-"
    When the record is parsed
    Then the taken value is None

  @unwired
  Scenario: BRDA numeric taken value maps to Some
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a BRDA record "BRDA:10,0,0,5"
    When the record is parsed
    Then the taken value is Some(5)

  @unwired
  Scenario: BRDA taken value "0" maps to Some(0)
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a BRDA record "BRDA:10,0,0,0"
    When the record is parsed
    Then the taken value is Some(0)

  @unwired
  Scenario: Duplicate BRDA records are merged by summing taken
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given two BRDA records with the same line, block, and branch
      | record          |
      | BRDA:10,0,0,3   |
      | BRDA:10,0,0,7   |
    When the coverage is parsed
    Then the merged branch has taken value Some(10)

  @unwired
  Scenario: Duplicate BRDA with "-" and numeric sums only the numeric
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given two BRDA records with the same key
      | record          |
      | BRDA:10,0,0,-   |
      | BRDA:10,0,0,5   |
    When the coverage is parsed
    Then the merged branch has taken value Some(5)

  @unwired
  Scenario: Malformed BRDA records are skipped with diagnostic
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given an LCOV file containing "BRDA:not,valid"
    When the coverage is parsed
    Then a MalformedRecord diagnostic is emitted
    And parsing continues without error

  @unwired
  Scenario: LCOV file with no BRDA records produces None branches
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given an LCOV file with only SF and DA records
    When the coverage is parsed
    Then ParseOutput.branches is None

  # ── Branch Domain Types ────────────────────────────────────────────

  @unwired
  Scenario: BranchCoverage carries only line position and execution count
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a BranchCoverage with line 10 and taken Some(3)
    Then it exposes a line number and an optional execution count
    And no format-specific identifiers are stored

  @unwired
  Scenario: CoverageMetric enum has Line and Branch variants
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given the CoverageMetric enum
    Then it has variant Line
    And it has variant Branch
    And the default is Line

  # ── Branch Matching ────────────────────────────────────────────────

  @unwired
  Scenario: Branch points within function span are matched
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a function spanning lines 5-15
    And branch data with entries at lines 7, 10, and 12
    When functions are matched with branch data
    Then branch_coverage covers 3 total branches

  @unwired
  Scenario: Branch points outside function span are excluded
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a function spanning lines 5-15
    And branch data with entries at lines 3, 10, and 20
    When functions are matched with branch data
    Then branch_coverage covers 1 total branch

  @unwired
  Scenario: Branch boundaries are inclusive
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a function spanning lines 5-15
    And branch data with entries at lines 5 and 15
    When functions are matched with branch data
    Then branch_coverage covers 2 total branches

  @unwired
  Scenario: Branches with taken None are excluded from ratio
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
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

  @unwired
  Scenario: Function with no branch points gets None branch_coverage
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a function spanning lines 5-15
    And no branch data entries within that span
    When functions are matched with branch data
    Then FunctionCoverage.branch_coverage is None

  @unwired
  Scenario: No cross-file branch leakage
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given function "foo" in "a.rs" spanning lines 1-10
    And function "bar" in "b.rs" spanning lines 1-10
    And branch data for "a.rs" at line 5 with taken Some(1)
    And no branch data for "b.rs"
    When functions are matched with branch data
    Then "foo" has branch_coverage with total 1
    And "bar" has branch_coverage None

  # ── Core Plumbing ──────────────────────────────────────────────────

  @unwired
  Scenario: analyze() passes branch data through without affecting scores
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given a project with LCOV containing both DA and BRDA records
    When analyze() is called with default options
    Then CRAP scores are computed from line coverage only
    And FunctionCoverage carries branch_coverage data

  @unwired
  Scenario: AnalyzeOptions defaults to Line coverage metric
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given default AnalyzeOptions
    Then coverage_metric is CoverageMetric::Line

  # ── Property Invariants ────────────────────────────────────────────

  @unwired
  Scenario: Arbitrary BRDA input never panics
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given arbitrary strings matching "BRDA:*" patterns
    When parsed by the BRDA parser
    Then no panic occurs

  @unwired
  Scenario: Branch ratios are always in [0, 100]
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given any set of BranchCoverage entries matched to a function
    When branch coverage is computed
    Then the percent is between 0.0 and 100.0 inclusive

  @unwired
  Scenario: Branch covered is always <= total
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given any set of BranchCoverage entries matched to a function
    When branch coverage is computed
    Then covered is less than or equal to total

  @unwired
  Scenario: No cross-file branch leakage under arbitrary input
    # tracked: crap-rs#169 — branch-coverage cucumber harness not yet built
    Given arbitrary branch data across multiple files
    And arbitrary function spans
    When functions are matched with branch data
    Then each function's branch_coverage only reflects its own file
