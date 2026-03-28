Feature: Terminal table reporter

  The table reporter formats CRAP analysis results as a colored,
  sorted terminal table for developers to quickly identify risky functions.

  # ── Sorting ────────────────────────────────────────────────────────

  Scenario: Functions are sorted by CRAP score descending
    Given an analysis with functions scoring 5.0, 45.2, and 8.1
    When the table is formatted
    Then the first row shows the function scoring 45.2
    And the second row shows the function scoring 8.1
    And the third row shows the function scoring 5.0

  # ── Risk Level Coloring ────────────────────────────────────────────
  # Boundary values (5.0, 8.0, 30.0) tested in domain/crap.rs unit tests

  Scenario Outline: Risk level coloring
    Given a function with a CRAP score of <score>
    When the table is formatted
    Then the risk column shows "<level>" in <color>

    Examples:
      | score | level      | color    |
      | 3.0   | low        | green    |
      | 6.5   | acceptable | no color |
      | 15.0  | moderate   | yellow   |
      | 45.0  | high       | bold red |

  # ── Coverage Coloring ──────────────────────────────────────────────
  # Boundary values (50%, 80%) deferred to unit tests

  Scenario Outline: Coverage coloring
    Given a function with <percent>% coverage
    When the table is formatted
    Then the coverage column appears in <color>

    Examples:
      | percent | color  |
      | 30      | red    |
      | 65      | yellow |
      | 90      | green  |

  # ── Threshold Highlighting ─────────────────────────────────────────

  Scenario: CRAP scores exceeding threshold appear in bold red
    Given a threshold of 8.0
    And a function with a CRAP score of 15.0
    When the table is formatted
    Then the CRAP score column shows "15.00" in bold red

  Scenario: CRAP scores within threshold appear without emphasis
    Given a threshold of 8.0
    And a function with a CRAP score of 5.0
    When the table is formatted
    Then the CRAP score column shows "5.00" without emphasis

  # ── Table Columns ──────────────────────────────────────────────────

  Scenario: Table contains all required columns
    Given an analysis with one function
    When the table is formatted
    Then the table header contains "File", "Function", "CC", "Cov%", "CRAP", and "Risk"

  Scenario: Function details appear in correct columns
    Given a function "parse_record" in "src/adapters/coverage/mod.rs" with complexity 6, coverage 72.5%, and CRAP score 8.13
    When the table is formatted
    Then the row shows file "src/adapters/coverage/mod.rs"
    And the row shows function "parse_record"
    And the row shows complexity "6"
    And the row shows coverage "72.5"
    And the row shows CRAP "8.13"

  # ── Header ─────────────────────────────────────────────────────────

  Scenario: Table starts with a version header
    Given an analysis result
    When the table is formatted
    Then the output starts with "crap4rs v" followed by the tool version

  # ── Summary Line ───────────────────────────────────────────────────

  Scenario: Summary shows function count and threshold violations
    Given an analysis with 42 functions where 3 exceed threshold 8.0
    And the worst CRAP score is 45.2
    When the table is formatted
    Then the summary line contains "42 functions"
    And the summary line contains "3 above threshold (8.0)"
    And the summary line contains "worst: 45.2"

  Scenario: Summary shows pass when no functions exceed threshold
    Given an analysis where no functions exceed the threshold
    When the table is formatted
    Then the summary line contains "PASS"

  Scenario: Summary shows fail when functions exceed threshold
    Given an analysis where 2 functions exceed the threshold
    When the table is formatted
    Then the summary line contains "FAIL"

  Scenario: Second summary line shows statistics and distribution
    Given an analysis with average CRAP 7.3, median 5.1, and distribution low=30 acceptable=9 moderate=2 high=1
    When the table is formatted
    Then the second summary line contains "avg: 7.3"
    And the second summary line contains "median: 5.1"
    And the second summary line contains "low: 30"
    And the second summary line contains "acceptable: 9"
    And the second summary line contains "moderate: 2"
    And the second summary line contains "high: 1"

  # ── Empty Results ──────────────────────────────────────────────────

  Scenario: Empty analysis shows informational message
    Given an analysis with no functions
    When the table is formatted
    Then the output contains "No functions analyzed"
    And no table rows are present

  # ── Decimal Precision ──────────────────────────────────────────────

  Scenario: CRAP scores display with two decimal places
    Given a function with a CRAP score of 5.0
    When the table is formatted
    Then the CRAP column shows "5.00"

  Scenario: Coverage displays with one decimal place
    Given a function with 85.0% coverage
    When the table is formatted
    Then the coverage column shows "85.0"
