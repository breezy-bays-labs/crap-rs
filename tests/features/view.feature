Feature: View — presentation transform on analysis findings

  The View abstraction shapes how analysis findings are queried and shown.
  It separates the gate-and-summary concerns of the underlying analysis
  from the filter, sort, and truncate operations a consumer applies to
  produce a particular report. Domain-level: pure transform, no I/O.

  Background:
    Given an analysis with the following functions:
      | qualified_name        | file_path             | complexity | coverage_percent | crap   | exceeds |
      | parse_lcov            | src/adapters/lcov.rs  | 12         | 100.0            | 12.00  | false   |
      | walk_ast              | src/adapters/syn.rs   | 18         | 75.0             | 23.06  | false   |
      | render_table          | src/adapters/table.rs | 9          | 60.0             | 14.18  | false   |
      | apply_threshold       | src/domain/threshold.rs | 4        | 100.0            | 4.00   | false   |
      | sort_verdicts         | src/adapters/table.rs | 6          | 0.0              | 42.00  | true    |
      | parse_args            | src/cli/mod.rs        | 22         | 50.0             | 63.50  | true    |
    And the threshold is 25.0
    # Notes:
    # - `walk_ast` has crap=23.06 < threshold=25, so `exceeds = false`. The
    #   Rust mirror (`background_fixture()` in `src/domain/view.rs`) derives
    #   `exceeds` from `crap_value > threshold` and matches this row.
    # - `parse_args` (c=22, cov=50, threshold=25) lists `crap=63.50` as a
    #   stipulated round number for the table; the formula-faithful value
    #   (c + c² · (1 − cov/100)³ ≈ 82.50) is intentionally not used here so
    #   the row stays readable. The Rust fixture uses 63.50 to match.

  # ── Default ViewSpec — no-op invariants ────────────────────────────
  # These scenarios encode walking-skeleton invariants 1, 2, and 3
  # (Order, Identity, Summary). Each is a property-test oracle —
  # integration tests should use proptest to assert across arbitrary
  # AnalysisResults, not just the Background fixture.

  Scenario: Default spec produces a no-op view in CRAP-descending order
    When the default ViewSpec is applied to the analysis
    Then `view.full` references the original analysis result without copying
    And `view.shown` contains every function from the analysis
    And `view.shown` is ordered by CRAP score descending
    And `view.eligible_count` equals the total number of functions
    And `view.truncated` is false

  Scenario: Default spec preserves identity set
    When the default ViewSpec is applied to the analysis
    Then the set of FunctionIdentity values in `view.shown` equals the set in the original analysis

  Scenario: Default spec preserves the gate summary
    When the default ViewSpec is applied to the analysis
    Then `view.full.summary` equals the original analysis summary exactly
    And `view.shown_summary` equals `view.full.summary`

  # ── Filters ────────────────────────────────────────────────────────

  Scenario: only_failing filter retains only functions that exceed the threshold
    Given a ViewSpec with `filters.only_failing = true`
    When the spec is applied
    Then `view.shown` contains only functions where `exceeds` is true
    And every function in `view.shown` has CRAP score above the threshold

  Scenario: coverage_range filter retains functions inside the inclusive range
    Given a ViewSpec with `filters.coverage_range = CoverageRange::new(50.0, 90.0)`
    When the spec is applied
    Then `view.shown` contains only functions whose `coverage_percent` is between 50.0 and 90.0 inclusive
    And `view.eligible_count` equals the count of matching functions

  Scenario Outline: coverage_range boundaries are inclusive
    Given a ViewSpec with coverage_range from <min> to <max>
    When the spec is applied
    Then a function with coverage_percent <coverage> <inclusion> in `view.shown`

    Examples:
      | min  | max   | coverage | inclusion |
      | 50.0 | 90.0  | 50.0     | appears   |
      | 50.0 | 90.0  | 90.0     | appears   |
      | 50.0 | 90.0  | 49.9     | is absent |
      | 50.0 | 90.0  | 90.1     | is absent |
      | 0.0  | 0.0   | 0.0      | appears   |
      | 100.0 | 100.0 | 100.0   | appears   |

  Scenario: Filters AND-compose
    # Property-test oracle: for arbitrary filter subsets, the result is
    # the intersection of each filter applied alone.
    Given a ViewSpec with `filters.only_failing = true` and `coverage_range = CoverageRange::new(50.0, 100.0)`
    When the spec is applied
    Then `view.shown` contains only functions that exceed the threshold AND have coverage between 50.0 and 100.0 inclusive

  # ── CoverageRange invariants (constructor) ─────────────────────────

  Scenario Outline: CoverageRange::new validates inputs
    When `CoverageRange::new(<min>, <max>)` is called
    Then it returns <result>

    Examples:
      | min   | max   | result            |
      | 0.0   | 100.0 | Ok                |
      | 50.0  | 50.0  | Ok                |
      | 1.0   | 90.0  | Ok                |
      | -0.1  | 50.0  | Err (out of range) |
      | 50.0  | 100.1 | Err (out of range) |
      | 90.0  | 50.0  | Err (min > max)   |
      | 100.0 | 0.0   | Err (min > max)   |

  # ── Sort ───────────────────────────────────────────────────────────

  Scenario: SortKey::Crap orders by CRAP score descending
    Given a ViewSpec with `sort = SortKey::Crap`
    When the spec is applied
    Then the CRAP scores in `view.shown` are in non-increasing order

  Scenario: SortKey::Coverage orders by coverage percent ascending
    Given a ViewSpec with `sort = SortKey::Coverage`
    When the spec is applied
    Then the coverage percentages in `view.shown` are in non-decreasing order

  Scenario: SortKey::Complexity orders by complexity descending
    Given a ViewSpec with `sort = SortKey::Complexity`
    When the spec is applied
    Then the complexity values in `view.shown` are in non-increasing order

  Scenario: SortKey::Path orders alphabetically by file_path, then by CRAP descending within each file
    Given a ViewSpec with `sort = SortKey::Path`
    When the spec is applied
    Then `view.shown` is ordered by `file_path` alphabetically ascending
    And within each file, functions are ordered by CRAP score descending

  Scenario: SortKey::Path secondary-sort with multiple files
    Given functions in three files: "src/a.rs" (CRAPs 5, 30), "src/b.rs" (CRAP 10), "src/c.rs" (CRAPs 1, 50)
    And a ViewSpec with `sort = SortKey::Path`
    When the spec is applied
    Then `view.shown` order is: src/a.rs::CRAP 30, src/a.rs::CRAP 5, src/b.rs::CRAP 10, src/c.rs::CRAP 50, src/c.rs::CRAP 1

  Scenario: Sort is stable on tied keys
    # Catches the mutation `sort_by → sort_unstable_by` which would pass
    # all other sort scenarios.
    Given an analysis with two functions having identical CRAP scores
    And the input order is [foo, bar]
    When sort = SortKey::Crap is applied
    Then `view.shown` preserves input order on ties: [foo, bar]

  # ── Truncate ───────────────────────────────────────────────────────

  Scenario: limit truncates the sorted result to N entries
    Given a ViewSpec with `limit = Some(3)`
    When the spec is applied
    Then `view.shown.len()` equals 3
    And `view.eligible_count` equals 6
    And `view.truncated` is true

  Scenario: limit greater than the eligible count truncates nothing
    Given a ViewSpec with `limit = Some(100)`
    When the spec is applied
    Then `view.shown.len()` equals 6
    And `view.eligible_count` equals 6
    And `view.truncated` is false

  Scenario: limit of None means no truncation
    Given a ViewSpec with `limit = None`
    When the spec is applied
    Then `view.shown` contains every eligible function
    And `view.truncated` is false

  # ── Order of operations: filter → sort → truncate ─────────────────

  Scenario: Order is filter, then sort, then truncate
    Given a ViewSpec with `filters.only_failing = true`, `sort = SortKey::Coverage`, `limit = Some(2)`
    When the spec is applied
    Then `view.shown` contains 2 functions
    And both functions have `exceeds` equal to true
    And `view.shown` is ordered by coverage percent ascending
    And `view.eligible_count` equals the total count of failing functions

  Scenario: Truncation does not change the gate
    Given an analysis with 3 functions exceeding threshold
    And a ViewSpec with `limit = Some(1)`
    When the spec is applied
    Then `view.shown.len()` equals 1
    But `view.full.passed` is false
    And `view.full.summary.exceeding_threshold` equals 3

  Scenario: Filtering does not change the gate
    Given an analysis with 3 functions exceeding threshold
    And a ViewSpec with `filters.coverage_range = CoverageRange::new(99.0, 100.0)`
    When the spec is applied so that `view.shown` is empty
    Then `view.full.passed` is false
    And `view.full.summary.exceeding_threshold` equals 3

  # ── shown_summary ──────────────────────────────────────────────────

  Scenario: shown_summary is computed over the shown subset
    Given a ViewSpec with `filters.only_failing = true`
    When the spec is applied
    Then `view.shown_summary.total_functions` equals `view.shown.len()`
    And `view.shown_summary.exceeding_threshold` equals `view.shown.len()`
    And `view.shown_summary.average_crap` equals the arithmetic mean of CRAP scores in `view.shown` to within 1e-9
    And `view.shown_summary.median_crap` equals the median of CRAP scores in `view.shown` to within 1e-9
    And `view.shown_summary.distribution` reflects only the functions in `view.shown`

  Scenario: shown_summary differs from full summary when the view filters out functions
    Given an analysis with 6 functions, 3 exceeding threshold
    And a ViewSpec with `filters.only_failing = true`
    When the spec is applied
    Then `view.full.summary.total_functions` equals 6
    And `view.shown_summary.total_functions` equals 3

  # ── Edge cases ─────────────────────────────────────────────────────

  Scenario: Empty analysis applied with default spec produces empty view
    Given an analysis with zero functions
    When the default ViewSpec is applied
    Then `view.shown` is empty
    And `view.eligible_count` equals 0
    And `view.truncated` is false
    And `view.full.passed` is true

  Scenario: All functions filtered out produces an empty shown
    Given an analysis where no function has coverage_percent above 95.0
    And a ViewSpec with `filters.coverage_range = CoverageRange::new(95.0, 100.0)`
    When the spec is applied
    Then `view.shown` is empty
    And `view.eligible_count` equals 0
    And `view.truncated` is false

  Scenario: limit of 0 is treated as no limit
    Given a ViewSpec built with `--top 0` semantics (limit = None)
    When the spec is applied
    Then `view.shown` contains every eligible function
    And `view.truncated` is false

  # ── view.full immutability ────────────────────────────────────────

  Scenario: applying a view does not modify the analysis
    Given an analysis result owned by the caller
    When `view::apply` is called with any ViewSpec
    Then `view.full` references the original analysis result
    And the original analysis result is unchanged after the call

  # ── NaN coverage handling ─────────────────────────────────────────
  # LCOV adapters can produce coverage_percent = NaN when a function
  # has zero executable lines. The View must handle this deterministically.

  Scenario: NaN coverage is excluded from coverage_range filter
    Given a function with `coverage_percent = NaN`
    And a ViewSpec with `filters.coverage_range = CoverageRange::new(0.0, 100.0)`
    When the spec is applied
    Then the function with NaN coverage does not appear in `view.shown`

  Scenario: NaN coverage sorts last under SortKey::Coverage ascending
    Given functions with coverage percentages [10.0, NaN, 50.0, NaN, 90.0]
    And a ViewSpec with `sort = SortKey::Coverage`
    When the spec is applied
    Then `view.shown` orders the non-NaN coverages ascending first
    And NaN-coverage functions appear after all non-NaN functions in the output
