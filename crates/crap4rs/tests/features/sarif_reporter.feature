Feature: SARIF reporter (issue #70)

  The SARIF reporter formats CRAP analysis results as SARIF v2.1.0 JSON
  for GitHub Code Scanning. It is a *gate translation*, not a display:
  results derive from the unshapeable analysis (`view.full.functions`),
  not the filtered/sorted/truncated `view.shown`. PR annotations must
  reflect truth, not a presentation choice.

  This file pins the CLI-process contracts the running binary uniquely
  captures: the SARIF v2.1.0 envelope, the driver version stamped from
  the real binary, results derived from the full analysis (so display
  flags never truncate/shrink/reorder them), GitHub-compatible
  repo-relative locations, byte-deterministic output, and the
  cross-format guarantee that `properties.diagnostic` mirrors the
  `--format advice` wire shape. The reporter's pure mapping logic —
  RiskLevel → SARIF severity, column emit/omit branches, the
  `{file}:{qualified_name}` fingerprint format — is owned by
  `crap-core`'s `sarif.rs` unit + proptest suite; the diagnostic
  content (extract_function candidates, exactly-one-recommended) is
  owned by `domain::diagnostic`. Step defs in
  `tests/sarif_reporter_cucumber.rs`. Absorbs the (binary-shelling,
  zero-lib-coverage) `sarif_reporter_integration.rs`.

  Background:
    Given a project with several functions whose CRAP scores cross the threshold

  # ── Envelope shape ─────────────────────────────────────────────────

  @wired
  Scenario: --format sarif emits SARIF v2.1.0 JSON stamped with the binary version
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then stdout is parseable JSON
    And the document at "$schema" is "https://json.schemastore.org/sarif-2.1.0.json"
    And the document at "version" is "2.1.0"
    And the document at "runs.0.tool.driver.name" is "crap4rs"
    And the document at "runs.0.tool.driver.version" matches the binary version
    And the document at "runs.0.tool.driver.rules.0.id" is "crap/threshold-exceeded"

  @wired
  Scenario: One result per exceeding function, each fully formed
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then the document at "runs.0.results" has 3 entries
    And every result carries the mandatory SARIF result fields

  @wired
  Scenario: Empty results when no function exceeds the threshold
    Given every function is below the threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then the document at "runs.0.results" has 0 entries
    And the document at "runs.0.tool.driver.rules" has 1 entry
    And the document at "runs.0.tool.driver.rules.0.id" is "crap/threshold-exceeded"

  # ── Gate keystone: SARIF iterates the FULL analysis ────────────────

  @wired
  Scenario: --top does NOT truncate SARIF results
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif --top 2`
    Then the document at "runs.0.results" has 3 entries
    And the SARIF output is byte-identical to the same command without `--top 2`

  @wired
  Scenario: --only-failing does NOT shrink SARIF results
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif --only-failing`
    Then the document at "runs.0.results" has 3 entries
    And the SARIF output is byte-identical to the same command without `--only-failing`

  @wired
  Scenario: --sort-by does NOT reorder SARIF results
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif --sort-by coverage`
    Then the SARIF output is byte-identical to the same command without `--sort-by coverage`

  # ── Exit code semantics ────────────────────────────────────────────

  @wired
  Scenario: Exit code is unchanged by --format sarif
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then the exit code is 1
    And stdout is non-empty

  @wired
  Scenario: --no-fail still emits the truth in SARIF
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif --no-fail`
    Then the exit code is 0
    And the document at "runs.0.results" has 3 entries

  # ── Location format (GitHub-compatible) ────────────────────────────

  @wired
  Scenario: Artifact URI is a repo-relative path with no scheme
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then every result's artifact URI is repo-relative with no "file://" or leading "/"

  @wired
  Scenario: Region carries the full line range from the analysis
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then every region's startLine and endLine are 1-based with endLine at least startLine

  @wired
  Scenario: Region carries 1-based column data from real source spans (issue #105)
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then every region carries startColumn and endColumn, both at least 1, with endColumn greater than startColumn

  # ── Diagnostic enrichment (issue #76) ──────────────────────────────

  @wired
  Scenario: result.properties.diagnostic carries the four diagnostic fields
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then every result's properties.diagnostic carries the coverage_gaps, complexity_drivers, suggested_actions, and root_cause fields

  @wired
  Scenario: properties.diagnostic mirrors the --format advice wire shape
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then each result's properties.diagnostic equals the same function's diagnostic under `crap4rs --coverage lcov.info --src src --threshold 8 --no-fail --format advice --sort-by path`

  @wired
  Scenario: --format sarif does NOT emit the stderr advice summary
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then stderr is empty

  # ── Determinism (GitHub dedup relies on byte-stable fingerprints) ──

  @wired
  Scenario: SARIF is byte-identical across runs, diagnostic block included
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --format sarif`
    Then running the same command again produces byte-identical SARIF
    And every result's properties.diagnostic is present
