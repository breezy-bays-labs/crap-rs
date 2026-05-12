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

  @unwired
  Scenario: --format sarif emits SARIF v2.1.0 JSON
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then stdout is parseable JSON
    And the document has top-level "$schema" "https://json.schemastore.org/sarif-2.1.0.json"
    And the document has top-level "version" "2.1.0"
    And `runs[0].tool.driver.name` is "crap4rs"
    And `runs[0].tool.driver.version` matches the binary version
    And `runs[0].tool.driver.rules[0].id` is "crap/threshold-exceeded"

  @unwired
  Scenario: One result per exceeding function
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then `runs[0].results[]` length equals the number of functions where
      `exceeds == true` in the full analysis
    And every result has a `ruleId`, `level`, `message.text`, a single
      `locations[0].physicalLocation`, and `partialFingerprints.functionIdentity`

  @unwired
  Scenario: Empty results when no function exceeds the threshold
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given every function is below the threshold
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then `runs[0].results[]` is the empty array
    And the rule definition is still present on `runs[0].tool.driver.rules`

  # ── Severity mapping ───────────────────────────────────────────────

  @unwired
  Scenario Outline: Risk level maps to SARIF severity
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
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

  @unwired
  Scenario: --top does NOT truncate SARIF results
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given six exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format sarif --top 2`
    Then `runs[0].results[]` length is 6
    And the View shaping (limit / truncated) does not appear in the SARIF
      output — SARIF has no `view` block, only the unshapeable gate

  @unwired
  Scenario: --only-failing does NOT shrink SARIF results
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given the analysis contains both passing and exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format sarif --only-failing`
    Then `runs[0].results[]` length equals the number of exceeding
      functions, identical to the run without `--only-failing`

  @unwired
  Scenario: --sort-by does NOT reorder SARIF results
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given several exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format sarif --sort-by coverage`
    Then `runs[0].results[]` is in the order produced by iterating
      `view.full.functions`, not the View's display sort

  # ── Exit code semantics ────────────────────────────────────────────

  @unwired
  Scenario: Exit code is unchanged by --format sarif
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then the exit code is 1
    And stdout is non-empty SARIF JSON

  @unwired
  Scenario: --no-fail still emits the truth in SARIF
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format sarif --no-fail`
    Then the exit code is 0
    But `runs[0].results[]` still lists every exceeding function — SARIF
      reports findings, the gate decides exit code

  # ── Location format (GitHub-compatible) ────────────────────────────

  @unwired
  Scenario: Artifact URI is the repo-relative file path
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then every `result.locations[0].physicalLocation.artifactLocation.uri`
      is the same repo-relative path the table reporter uses
    And the path does not begin with "file://" or "/"

  @unwired
  Scenario: Region carries the full line range from the analysis
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then every `result.locations[0].physicalLocation.region.startLine`
      equals the function's `span.start_line` (1-based)
    And every region's `endLine` equals the function's `span.end_line`
      (inclusive)

  @unwired
  Scenario: Region carries column data when known (issue #105)
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given the complexity adapter populates 1-based start/end columns
      from `proc_macro2::Span`
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then every region includes a `startColumn` and `endColumn`, both >= 1
    And GitHub Code Scanning underlines the exact function range in the
      PR diff instead of highlighting the entire line

  @unwired
  Scenario: Region omits column keys when columns are unknown
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given a span produced by an adapter that has no column data (e.g.,
      diff hunks parsed line-only)
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then the region for that result does NOT contain `startColumn` or
      `endColumn` keys
    And consumers see line-only precision instead of a fabricated column

  # ── Fingerprints (GitHub dedup) ────────────────────────────────────

  @unwired
  Scenario: partialFingerprints stable across runs and rebases
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then every result's `partialFingerprints.functionIdentity` is the
      string "{file_path}:{qualified_name}"
    And running the same command twice produces byte-identical SARIF
      (no timestamp, no run-scoped IDs)

  # ── Diagnostic enrichment (issue #76) ──────────────────────────────

  @unwired
  Scenario: result.properties.diagnostic populated when Diagnostic is computed
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then every `runs[0].results[].properties.diagnostic` carries the
      same `coverage_gaps`, `complexity_drivers`, `suggested_actions`,
      and `root_cause` fields the JSON envelope's
      `view.shown[].diagnostic` would carry under
      `crap4rs --format advice`

  @unwired
  Scenario: properties.diagnostic mirrors the advice wire shape
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given an exceeding function whose `Diagnostic` contains an
      `extract_function` action with two `candidates[]`
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then the SARIF result for that function has
      `properties.diagnostic.suggested_actions[]` with `kind`
      "extract_function" and a non-empty `candidates[]`
    And exactly one `candidates[].recommended` is true

  @unwired
  Scenario: --format sarif does NOT emit the stderr advice summary
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given exceeding functions exist
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
    Then stderr contains no per-function summary lines
      (the stderr summary fires only on `--format advice`)

  @unwired
  Scenario: SARIF determinism preserved with diagnostic enrichment
    # tracked: crap-rs#169 — sarif-reporter cucumber harness not yet built
    Given the same coverage file across two runs
    When the operator runs `crap4rs --coverage lcov.info --format sarif`
      twice
    Then both `.sarif` outputs are byte-identical, including the
      `properties.diagnostic` block on every result
