Feature: Complexity breakdown

  Complexity breakdown attributes a function's complexity score to specific
  constructs (if, match, loop, ?, etc.), giving developers actionable lines
  to refactor rather than an opaque number.

  # ── Contributor extraction ─────────────────────────────────────────────────

  Scenario: Base-complexity function has empty contributors
    Given a function with no decision points
    When complexity is extracted with cognitive metric
    Then the contributors list is empty
    And the complexity is 1

  Scenario: Single if-branch produces one contributor
    Given a function with a single if statement at line 3
    When complexity is extracted with cognitive metric
    Then there is 1 contributor
    And the contributor has kind "if-branch"
    And the contributor has line 3
    And the contributor has increment 1

  Scenario: Nested if produces nesting-adjusted increment
    Given a function with an if at line 3 containing an if at line 5
    When complexity is extracted with cognitive metric
    Then the contributor at line 3 has kind "if-branch" with increment 1
    And the contributor at line 5 has kind "if-branch" with increment 2

  Scenario: Match contributes as a single node in cognitive metric
    Given a function with a match expression at line 4 with 5 arms
    When complexity is extracted with cognitive metric
    Then there is 1 match-kind contributor
    And the contributor has kind "match"
    And the contributor has increment 1

  Scenario: Match arms each contribute in cyclomatic metric
    Given a function with a match expression at line 4 with 5 arms
    When complexity is extracted with cyclomatic metric
    Then there are 4 contributors with kind "match-arm"

  Scenario: Try operator contributes one contributor
    Given a function with a single ? operator at line 7
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "try"
    And the contributor has line 7
    And the contributor has increment 1

  Scenario: Let-else contributes one contributor
    Given a function with a let-else at line 5
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "let-else"
    And the contributor has increment 1

  Scenario: Infinite loop contributes with nesting increment
    Given a function with a loop at line 4 containing an if at line 6
    When complexity is extracted with cognitive metric
    Then the contributor at line 4 has kind "loop" with increment 1
    And the contributor at line 6 has kind "if-branch" with increment 2

  Scenario: For-loop contributes one contributor
    Given a function with a for loop at line 3
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "for-loop"
    And the contributor has line 3
    And the contributor has increment 1

  Scenario: While-loop contributes one contributor
    Given a function with a while loop at line 3
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "while-loop"
    And the contributor has increment 1

  Scenario: Logical operator chain contributes one contributor per sequence
    Given a function with "a && b && c" at line 5
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "logical-operator"
    And the contributor has increment 1

  Scenario: Mixed logical operator chain produces contributor per sequence switch
    Given a function with "a && b || c" starting at line 5
    When complexity is extracted with cognitive metric
    Then there are 2 contributors with kind "logical-operator"

  Scenario: Break contributes one contributor
    Given a function with a break statement at line 8 inside a loop
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "break"
    And the contributor has increment 1

  Scenario: Continue contributes one contributor
    Given a function with a continue statement at line 8 inside a loop
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "continue"
    And the contributor has increment 1

  Scenario: Closure does not contribute as a standalone contributor
    Given a function containing a closure
    When complexity is extracted with cognitive metric
    Then no contributor has kind "closure"

  Scenario: Contributors are sorted by line number
    Given a function with an if at line 10 and a match at line 5
    When complexity is extracted
    Then the first contributor is at line 5
    And the second contributor is at line 10

  # ── Property invariant ─────────────────────────────────────────────────────

  Scenario: Contributor increments sum to complexity minus one
    Given any function with complexity C and contributors list
    Then the sum of all contributor increments equals C - 1

  Scenario: Each contributor has increment of at least 1
    Given any function with at least one contributor
    Then every contributor has increment >= 1

  # ── Terminal reporter: --breakdown flag ────────────────────────────────────

  Scenario: Breakdown is off by default — no sub-rows in output
    Given an analysis where "parse_record" exceeds threshold with 3 contributors
    When the table is formatted without --breakdown
    Then the output does not contain contributor sub-rows

  Scenario: --breakdown shows contributors only for exceeding functions
    Given an analysis where "parse_record" exceeds threshold at 15.23 with contributors:
      | kind       | line | increment |
      | match      | 12   | 1         |
      | if-branch  | 18   | 2         |
      | try        | 22   | 1         |
    And "simple_fn" is within threshold
    When the table is formatted with --breakdown
    Then sub-rows appear under "parse_record"
    And no sub-rows appear under "simple_fn"

  Scenario: Sub-rows display kind and increment in tree format
    Given an exceeding function with a contributor: kind "match", line 12, increment 1
    When the table is formatted with --breakdown
    Then the sub-row contains "line 12: match (+1)"

  Scenario: Nesting increment shown with (nested) suffix
    Given an exceeding function with a contributor: kind "if-branch", line 18, increment 2
    When the table is formatted with --breakdown
    Then the sub-row contains "if-branch (+2 (nested))"

  Scenario: Last contributor uses └─ tree character
    Given an exceeding function with exactly 3 contributors
    When the table is formatted with --breakdown
    Then the first two sub-rows start with "├─"
    And the last sub-row starts with "└─"

  Scenario: Sub-rows appear sorted by line number
    Given an exceeding function with contributors at lines 22, 12, and 18
    When the table is formatted with --breakdown
    Then sub-rows appear in order: line 12, line 18, line 22

  # ── JSON reporter: contributors always present ─────────────────────────────

  Scenario: JSON output always includes contributors array
    Given an analysis with one function having 3 contributors
    When the JSON is formatted without --breakdown
    Then each function entry contains a "contributors" array
    And the contributors array has 3 entries

  Scenario: JSON contributors array is empty for base-complexity functions
    Given an analysis with one function having complexity 1
    When the JSON is formatted
    Then the function entry's "contributors" is an empty array

  Scenario: JSON contributor entry contains kind, line, column, and increment
    Given an analysis with one function having a contributor: kind "match", line 12, column 4, increment 1
    When the JSON is formatted
    Then the contributor entry contains "kind" equal to "match"
    And the contributor entry contains "line" equal to 12
    And the contributor entry contains "column" equal to 4
    And the contributor entry contains "increment" equal to 1

  Scenario: JSON contributor kind uses kebab-case
    Given an analysis with contributors of kind IfBranch and ForLoop
    When the JSON is formatted
    Then contributor kinds appear as "if-branch" and "for-loop"

  Scenario: JSON column field is null when not available
    Given a contributor with no column information
    When the JSON is formatted
    Then the contributor entry contains "column" as null

  # ── Regression: existing complexity totals unchanged ──────────────────────

  Scenario: Adding contributors does not change existing complexity values
    Given any function that previously reported complexity C
    When complexity is extracted with contributors enabled
    Then the function still reports complexity C
