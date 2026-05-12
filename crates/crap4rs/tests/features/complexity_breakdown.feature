Feature: Complexity breakdown

  Complexity breakdown attributes a function's complexity score to specific
  constructs (if, match, loop, ?, etc.), giving developers actionable lines
  to refactor rather than an opaque number.

  # ── Contributor extraction ─────────────────────────────────────────────────

  @unwired
  Scenario: Base-complexity function has empty contributors
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with no decision points
    When complexity is extracted with cognitive metric
    Then the contributors list is empty
    And the complexity is 1

  @unwired
  Scenario: Single if-branch produces one contributor
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a single if statement at line 3
    When complexity is extracted with cognitive metric
    Then there is 1 contributor
    And the contributor has kind "if-branch"
    And the contributor has line 3
    And the contributor has increment 1

  @unwired
  Scenario: Nested if produces nesting-adjusted increment
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with an if at line 3 containing an if at line 5
    When complexity is extracted with cognitive metric
    Then the contributor at line 3 has kind "if-branch" with increment 1
    And the contributor at line 5 has kind "if-branch" with increment 2

  @unwired
  Scenario: Match contributes as a single node in cognitive metric
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a match expression at line 4 with 5 arms
    When complexity is extracted with cognitive metric
    Then there is 1 match-kind contributor
    And the contributor has kind "match"
    And the contributor has increment 1

  @unwired
  Scenario: Match arms each contribute in cyclomatic metric
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a match expression at line 4 with 5 arms
    When complexity is extracted with cyclomatic metric
    Then there are 4 contributors with kind "match-arm"

  @unwired
  Scenario: Try operator contributes one contributor
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a single ? operator at line 7
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "try"
    And the contributor has line 7
    And the contributor has increment 1

  @unwired
  Scenario: Let-else contributes one contributor
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a let-else at line 5
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "let-else"
    And the contributor has increment 1

  @unwired
  Scenario: Infinite loop contributes with nesting increment
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a loop at line 4 containing an if at line 6
    When complexity is extracted with cognitive metric
    Then the contributor at line 4 has kind "loop" with increment 1
    And the contributor at line 6 has kind "if-branch" with increment 2

  @unwired
  Scenario: For-loop contributes one contributor
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a for loop at line 3
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "for-loop"
    And the contributor has line 3
    And the contributor has increment 1

  @unwired
  Scenario: While-loop contributes one contributor
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a while loop at line 3
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "while-loop"
    And the contributor has increment 1

  @unwired
  Scenario: Logical operator chain contributes one contributor per sequence
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with "a && b && c" at line 5
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "logical-operator"
    And the contributor has increment 1

  @unwired
  Scenario: Mixed logical operator chain produces contributor per sequence switch
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with "a && b || c" starting at line 5
    When complexity is extracted with cognitive metric
    Then there are 2 contributors with kind "logical-operator"

  @unwired
  Scenario: Break contributes one contributor
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a break statement at line 8 inside a loop
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "break"
    And the contributor has increment 1

  @unwired
  Scenario: Continue contributes one contributor
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with a continue statement at line 8 inside a loop
    When complexity is extracted with cognitive metric
    Then there is 1 contributor with kind "continue"
    And the contributor has increment 1

  @unwired
  Scenario: Closure does not contribute as a standalone contributor
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function containing a closure
    When complexity is extracted with cognitive metric
    Then no contributor has kind "closure"

  @unwired
  Scenario: Contributors are sorted by line number
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a function with an if at line 10 and a match at line 5
    When complexity is extracted
    Then the first contributor is at line 5
    And the second contributor is at line 10

  # ── Property invariant ─────────────────────────────────────────────────────

  @unwired
  Scenario: Contributor increments sum to complexity minus one
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given any function with complexity C and contributors list
    Then the sum of all contributor increments equals C - 1

  @unwired
  Scenario: Each contributor has increment of at least 1
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given any function with at least one contributor
    Then every contributor has increment >= 1

  # ── Terminal reporter: --breakdown flag ────────────────────────────────────

  @unwired
  Scenario: Breakdown is off by default — no sub-rows in output
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an analysis where "parse_record" exceeds threshold with 3 contributors
    When the table is formatted without --breakdown
    Then the output does not contain contributor sub-rows

  @unwired
  Scenario: --breakdown shows contributors only for exceeding functions
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an analysis where "parse_record" exceeds threshold at 15.23 with contributors:
      | kind       | line | increment |
      | match      | 12   | 1         |
      | if-branch  | 18   | 2         |
      | try        | 22   | 1         |
    And "simple_fn" is within threshold
    When the table is formatted with --breakdown
    Then sub-rows appear under "parse_record"
    And no sub-rows appear under "simple_fn"

  @unwired
  Scenario: Sub-rows display kind and increment in tree format
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an exceeding function with a contributor: kind "match", line 12, increment 1
    When the table is formatted with --breakdown
    Then the sub-row contains "line 12: match (+1)"

  @unwired
  Scenario: Nesting increment shown with (nested) suffix
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an exceeding function with a contributor: kind "if-branch", line 18, increment 2
    When the table is formatted with --breakdown
    Then the sub-row contains "if-branch (+2 (nested))"

  @unwired
  Scenario: Last contributor uses └─ tree character
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an exceeding function with exactly 3 contributors
    When the table is formatted with --breakdown
    Then the first two sub-rows start with "├─"
    And the last sub-row starts with "└─"

  @unwired
  Scenario: Sub-rows appear sorted by line number
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an exceeding function with contributors at lines 22, 12, and 18
    When the table is formatted with --breakdown
    Then sub-rows appear in order: line 12, line 18, line 22

  # ── JSON reporter: contributors always present ─────────────────────────────

  @unwired
  Scenario: JSON output always includes contributors array
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an analysis with one function having 3 contributors
    When the JSON is formatted without --breakdown
    Then each function entry contains a "contributors" array
    And the contributors array has 3 entries

  @unwired
  Scenario: JSON contributors array is empty for base-complexity functions
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an analysis with one function having complexity 1
    When the JSON is formatted
    Then the function entry's "contributors" is an empty array

  @unwired
  Scenario: JSON contributor entry contains kind, line, column, and increment
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an analysis with one function having a contributor: kind "match", line 12, column 4, increment 1
    When the JSON is formatted
    Then the contributor entry contains "kind" equal to "match"
    And the contributor entry contains "line" equal to 12
    And the contributor entry contains "column" equal to 4
    And the contributor entry contains "increment" equal to 1

  @unwired
  Scenario: JSON contributor kind uses kebab-case
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given an analysis with contributors of kind IfBranch and ForLoop
    When the JSON is formatted
    Then contributor kinds appear as "if-branch" and "for-loop"

  @unwired
  Scenario: JSON column field is null when not available
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given a contributor with no column information
    When the JSON is formatted
    Then the contributor entry contains "column" as null

  # ── Regression: existing complexity totals unchanged ──────────────────────

  @unwired
  Scenario: Adding contributors does not change existing complexity values
    # tracked: crap-rs#169 — complexity-breakdown cucumber harness not yet built
    Given any function that previously reported complexity C
    When complexity is extracted with contributors enabled
    Then the function still reports complexity C
