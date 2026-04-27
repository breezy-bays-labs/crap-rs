Feature: SARIF reporter (issue #70)

  The SARIF reporter formats CRAP analysis results as SARIF v2.1.0 JSON
  for GitHub Code Scanning. It is a *gate translation*, not a display:
  results derive from the unshapeable analysis (`view.full.functions`),
  not the filtered/sorted/truncated `view.shown`. PR annotations must
  reflect truth, not a presentation choice.

  Background:
    Given a project with several functions whose CRAP scores cross the
    threshold

  # ── Envelope shape ─────────────────────────────────────────────────

  Scenario: --format sarif emits SARIF v2.1.0 JSON
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then stdout is parseable JSON
    And the document has top-level "$schema" "https://json.schemastore.org/sarif-2.1.0.json"
    And the document has top-level "version" "2.1.0"
    And `runs[0].tool.driver.name` is "crap4rs"
    And `runs[0].tool.driver.version` matches the binary version
    And `runs[0].tool.driver.rules[0].id` is "crap/threshold-exceeded"

  Scenario: One result per exceeding function
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then `runs[0].results[]` length equals the number of functions where
      `exceeds == true` in the full analysis
    And every result has a `ruleId`, `level`, `message.text`, a single
      `locations[0].physicalLocation`, and `partialFingerprints.functionIdentity`

  Scenario: Empty results when no function exceeds the threshold
    Given every function is below the threshold
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then `runs[0].results[]` is the empty array
    And the rule definition is still present on `runs[0].tool.driver.rules`

  # ── Severity mapping ───────────────────────────────────────────────

  Scenario Outline: Risk level maps to SARIF severity
    Given an exceeding function with risk level <risk>
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then the result for that function has `level` <sarif_level>

    Examples:
      | risk       | sarif_level |
      | high       | "error"     |
      | moderate   | "warning"   |
      | acceptable | "note"      |
      | low        | "note"      |

  # ── Gate keystone: SARIF iterates the FULL analysis ────────────────

  Scenario: --top does NOT truncate SARIF results
    Given six exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format sarif --top 2`
    Then `runs[0].results[]` length is 6
    And the View shaping (limit / truncated) does not appear in the SARIF
      output — SARIF has no `view` block, only the unshapeable gate

  Scenario: --only-failing does NOT shrink SARIF results
    Given the analysis contains both passing and exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format sarif --only-failing`
    Then `runs[0].results[]` length equals the number of exceeding
      functions, identical to the run without `--only-failing`

  Scenario: --sort-by does NOT reorder SARIF results
    Given several exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format sarif --sort-by coverage`
    Then `runs[0].results[]` is in the order produced by iterating
      `view.full.functions`, not the View's display sort

  # ── Exit code semantics ────────────────────────────────────────────

  Scenario: Exit code is unchanged by --format sarif
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then the exit code is 1
    And stdout is non-empty SARIF JSON

  Scenario: --no-fail still emits the truth in SARIF
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format sarif --no-fail`
    Then the exit code is 0
    But `runs[0].results[]` still lists every exceeding function — SARIF
      reports findings, the gate decides exit code

  # ── Location format (GitHub-compatible) ────────────────────────────

  Scenario: Artifact URI is the repo-relative file path
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then every `result.locations[0].physicalLocation.artifactLocation.uri`
      is the same repo-relative path the table reporter uses
    And the path does not begin with "file://" or "/"

  Scenario: Region carries the full line range from the analysis
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then every `result.locations[0].physicalLocation.region.startLine`
      equals the function's `span.start_line` (1-based)
    And every region's `endLine` equals the function's `span.end_line`
      (inclusive)

  # ── Fingerprints (GitHub dedup) ────────────────────────────────────

  Scenario: partialFingerprints stable across runs and rebases
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then every result's `partialFingerprints.functionIdentity` is the
      string "{file_path}:{qualified_name}"
    And running the same command twice produces byte-identical SARIF
      (no timestamp, no run-scoped IDs)
