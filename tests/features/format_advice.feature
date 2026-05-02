Feature: --format advice (issue #76)

  The advice format emits the canonical JSON envelope with each
  over-threshold `FunctionVerdict` carrying a populated `Diagnostic`:
  AST-derived `coverage_gaps`, `complexity_drivers`, `suggested_actions`,
  and a flat `root_cause` scalar. Primary consumer is the `/cut-the-crap`
  agent skill (#77); secondary consumers are CI/SARIF and humans.

  The shape is **experimental** in v0.3.x and stabilises at v0.4.0
  (schema_version stays at 1 throughout v0.3.x; bumps to 2 at v0.4.0).

  Background:
    Given a project with a mix of over-threshold and under-threshold
      functions
    And the over-threshold set contains:
      | shape                                                            |
      | a function with low coverage and acceptable complexity           |
      | a function with high complexity and full coverage                |
      | a function with both low coverage and high complexity            |
      | a function with high complexity and zero viable split candidates |

  # ── Envelope shape ─────────────────────────────────────────────────

  Scenario: --format advice emits the canonical envelope on stdout
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then stdout is parseable JSON
    And the document carries top-level `schema_version` "1"
    And the document carries a top-level `view.shown[]` array
    And every `view.shown[].diagnostic` is either populated or absent
      (never `null`-with-fields-present)

  Scenario: --format advice exit code matches --format json
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then the exit code is 1

  # ── Diagnostic gating (R6.4 / F2) ──────────────────────────────────

  Scenario: Under-threshold functions carry no Diagnostic
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then for every `view.shown[]` entry where `verdict.exceeds == false`,
      `verdict.diagnostic` key is absent from the serialised JSON

  Scenario: Over-threshold functions always carry a Diagnostic
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then for every `view.shown[]` entry where `verdict.exceeds == true`,
      `verdict.diagnostic` is populated with all four fields:
      `coverage_gaps`, `complexity_drivers`, `suggested_actions`,
      `root_cause`

  # ── SuggestedAction taxonomy (R1.3) ────────────────────────────────

  Scenario: Low coverage emits AddTestsForLines
    Given an over-threshold function with coverage < 100% and acceptable
      complexity
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then `verdict.diagnostic.suggested_actions[]` contains one entry with
      `kind` "add_tests_for_lines"
    And that entry carries `lines: Vec<LineRange>` matching the uncovered
      ranges in the function's span
    And that entry carries an `applicability` field

  Scenario: High complexity with viable splits emits ExtractFunction
    Given an over-threshold function with full coverage and high
      complexity
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then `verdict.diagnostic.suggested_actions[]` contains one entry with
      `kind` "extract_function"
    And that entry carries a non-empty `candidates: Vec<ProposedSplit>`
    And no `add_tests_for_lines` action is emitted for this function

  Scenario: Both low coverage and high complexity emit both actions
    Given an over-threshold function with coverage < 100% and high
      complexity
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then `verdict.diagnostic.suggested_actions[]` contains both
      `add_tests_for_lines` and `extract_function` entries

  Scenario: High complexity with zero viable splits falls back to AcceptInherentComplexity
    Given an over-threshold function with full coverage and high
      complexity but no extractable subexpression
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then `verdict.diagnostic.suggested_actions[]` contains exactly one
      entry with `kind` "accept_inherent_complexity"
    And no `extract_function` action is emitted for this function

  # ── ProposedSplit shape (R1.4) ─────────────────────────────────────

  Scenario: Each ProposedSplit carries the five wire fields
    Given an over-threshold function with high complexity
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then for every `extract_function.candidates[]` entry, all of the
      following keys are present and non-null: `line_range`,
      `complexity_contribution`, `branch_path`, `kind`, `recommended`
    And `kind` is one of "deepest_nesting", "largest_subblock",
      "highest_branch_count"

  Scenario: Exactly one ProposedSplit per function has recommended:true
    Given an over-threshold function whose `extract_function.candidates`
      list is non-empty
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then exactly one entry in that function's `candidates` has
      `recommended` true
    And every other entry has `recommended` false

  Scenario Outline: De-duplication priority for same-line-range candidates
    Given the walker emits multiple split candidates that resolve to the
      same `line_range` with kinds <kinds_present>
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then the surviving candidate at that `line_range` has `kind`
      <surviving_kind>

    Examples:
      | kinds_present                                              | surviving_kind         |
      | "deepest_nesting" + "highest_branch_count"                 | "deepest_nesting"      |
      | "deepest_nesting" + "largest_subblock"                     | "deepest_nesting"      |
      | "highest_branch_count" + "largest_subblock"                | "highest_branch_count" |
      | "deepest_nesting" + "highest_branch_count" + "largest_subblock" | "deepest_nesting"  |

  # ── root_cause derivation (R1.2) ───────────────────────────────────

  Scenario Outline: root_cause is derived deterministically from the action set
    Given an over-threshold function whose `suggested_actions[]`
      contains <actions_present>
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then `verdict.diagnostic.root_cause` is <root_cause>

    Examples:
      | actions_present                                  | root_cause        |
      | "add_tests_for_lines" only                       | "low_coverage"    |
      | "extract_function" only                          | "high_complexity" |
      | "simplify_branching" only                        | "high_complexity" |
      | "accept_inherent_complexity" only                | "high_complexity" |
      | "add_tests_for_lines" + "extract_function"       | "both"            |
      | "add_tests_for_lines" + "simplify_branching"     | "both"            |

  # ── Composition with View flags (R2.1) ─────────────────────────────

  Scenario: --format advice composes with --top
    Given six exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format advice --top 3`
    Then `view.shown[]` length is 3
    And every entry in `view.shown[]` carries a populated `diagnostic`

  Scenario: --format advice composes with --sort-by coverage
    Given several exceeding functions with varying coverage
    When the operator runs `crap4rs --coverage lcov.info --format advice --sort-by coverage`
    Then `view.shown[]` is ordered by `verdict.scored.coverage_percent`
      ascending
    And every entry's `diagnostic` is populated

  Scenario: --format advice composes with --min-coverage / --max-coverage
    Given a mix of over-threshold functions across the coverage spectrum
    When the operator runs `crap4rs --coverage lcov.info --format advice --min-coverage 80`
    Then `view.shown[]` contains only entries where
      `verdict.scored.coverage_percent >= 80`
    And each surviving entry carries a populated `diagnostic`

  Scenario: --format advice composes with --no-fail
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format advice --no-fail`
    Then the exit code is 0
    But every exceeding function still carries a populated `diagnostic`
      — advice reports findings, the gate decides exit code

  # ── Stderr summary (R5.2 / S-8) ────────────────────────────────────

  Scenario: --format advice emits a one-line-per-function summary on stderr
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then stdout is parseable JSON
    And stderr contains one line per over-threshold function in the form
      `[crap=N.NN] file:line-line qualified::name [actions: …]`
    And stderr lines are ordered to match `view.shown[]`

  Scenario: stdout stays JSON-only when --format advice is set
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then stdout contains no human-readable prose, banners, or table
      borders
    And stdout is parseable as a single JSON value

  Scenario: --format json without advice emits no stderr summary
    When the operator runs `crap4rs --coverage lcov.info --format json`
    Then stderr is empty (apart from operational warnings, if any)
    And stdout's `view.shown[].diagnostic` keys are absent

  # ── Naming / determinism (R6.3) ────────────────────────────────────

  Scenario: Diagnostic carries no prose, no human names, no LLM-shaped guesses
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then no `diagnostic.suggested_actions[]` entry contains a `rationale`
      string field
    And no `proposed_splits[]` entry contains a `name` field, an
      `extracted_name` field, or any natural-language description
    And every `branch_path` is a `/`-joined chain of
      `ContributorKind` discriminants only

  Scenario: Same input produces byte-identical advice JSON
    When the operator runs `crap4rs --coverage lcov.info --format advice`
      twice with the same coverage file
    Then both stdout bytes are identical
    And both stderr summaries are identical

  # ── Naming conflict (R6.6 / A1) ────────────────────────────────────

  Scenario: --explain is NOT an alias of --format advice
    Given the project has at least one over-threshold function
    When the operator runs `crap4rs --coverage lcov.info --explain`
    Then stdout is the human-readable breakdown legend (PR #59
      semantics), not the advice JSON
    And the `--explain` flag does not populate `view.shown[].diagnostic`

  # ── Stability (R4.1 / G1) ──────────────────────────────────────────

  Scenario: schema_version stays at 1 throughout v0.3.x
    When the operator runs `crap4rs --coverage lcov.info --format advice`
    Then the document's top-level `schema_version` is "1"
    And the README "Output formats" section flags this shape as
      "experimental" until v0.4.0
