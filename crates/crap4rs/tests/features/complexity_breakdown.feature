Feature: Complexity breakdown

  Complexity breakdown attributes a function's complexity score to specific
  constructs (if, match, loop, ?, etc.), giving developers actionable lines
  to refactor rather than an opaque number.

  This file pins only the CLI-process contracts the running binary uniquely
  captures: the `--breakdown` flag wiring (sub-rows appear for an exceeding
  function, absent by default) and the `--explain` flag (legend; requires
  `--breakdown`; leaves the JSON envelope intact). The lower-level behavior
  is owned by crap-core unit tests — contributor EXTRACTION by
  `adapters::complexity` (85 tests, one per node kind + the sum / sorted /
  positive-increment invariants), table sub-row RENDERING (tree characters,
  `(nested)` suffix, line ordering, legend text) by `reporters::table`'s
  `test_breakdown_*` / `test_explain_*`, and the JSON contributor serde
  (kebab-case kind, null column, field shape) by `domain::types` (see
  `AGENTS.md` § BDD hygiene). Step defs live in
  `tests/complexity_breakdown_cucumber.rs`.

  # ── CLI: --breakdown flag wiring ───────────────────────────────────

  @wired
  Scenario: --breakdown renders contributor sub-rows for an exceeding function
    Given a project with one nested function that exceeds threshold and is uncovered
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 1 --breakdown --color never`
    Then stdout contains "├─ line 2: if-branch (+1)"
    And stdout contains "└─ line 3: if-branch (+2 (nested))"

  @wired
  Scenario: Breakdown is off by default — no sub-rows
    Given a project with one nested function that exceeds threshold and is uncovered
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 1 --color never`
    Then stdout does not contain "if-branch"

  # ── CLI: --explain flag ────────────────────────────────────────────

  @wired
  Scenario: --explain adds the increment legend beneath the breakdown
    Given a project with one nested function that exceeds threshold and is uncovered
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 1 --breakdown --explain --color never`
    Then stdout contains "└─ line 3: if-branch (+2 (nested))"
    And stdout contains "Legend: +1 = base structural increment."

  @wired
  Scenario: --explain requires --breakdown (clap rejects it otherwise, exit 2)
    Given a project with one nested function that exceeds threshold and is uncovered
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 1 --explain`
    Then the exit code is 2
    And stderr contains "--explain requires --breakdown"

  @wired
  Scenario: --explain leaves the JSON envelope shape intact with contributors present
    Given a project with one nested function that exceeds threshold and is uncovered
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --format json --explain`
    Then the first function in the envelope carries a contributors array
    And the JSON envelope has no top-level "legend" key
