Feature: CLI ergonomics — shaping the report from the command line

  The five new flags (--top, --min-coverage, --max-coverage, --sort-by,
  --no-fail) plus the relocated --only-failing flag let an operator shape
  the CRAP report for investigation without losing the gate semantics
  that drive CI exit codes.

  # Vocabulary used in scenarios:
  #   "the operator"        — the human or script invoking crap4rs
  #   "the table"           — terminal stdout (default mode, human-readable)
  #   "the JSON envelope"   — terminal stdout when --format json
  #   "the report"          — generic stdout artifact when output mode is irrelevant
  #
  # CHANGELOG-worthy behavior change (V1b):
  #   --only-failing previously emitted summary fields (average_crap,
  #   median_crap, max_crap, distribution) computed over the FILTERED
  #   set, while exceeding_threshold reflected the filtered count and
  #   total_functions also reflected the filtered count. Under this
  #   bundle, summary always reflects the full unfiltered analysis.
  #
  # Self-CRAP regression invariant (per shaping.md):
  #   The view module must not introduce any function with cognitive
  #   complexity above 15. CI gate enforces.
  #
  # Implicit context for @unwired scenarios (placeholder vocabulary):
  #   Most @unwired scenarios below reference TOTAL_FUNCTIONS and
  #   VIOLATING_FUNCTIONS as placeholder counts, and assume a project
  #   exists with an LCOV file at "lcov.info". A non-executable
  #   `Background:` block previously documented this; it was deleted
  #   in crap-rs#168 (BDD hygiene chore) because Background blocks
  #   must be executable (AGENTS.md § BDD hygiene rule 4). When these
  #   scenarios are wired (tracked at crap-rs#169), the implementer
  #   will replace the placeholders with scenario-specific Given
  #   steps that set up concrete fixture-derived counts — just like
  #   the @wired `--summary` scenarios do with their `Given a
  #   synthetic project where ...` step.

  # ── --top: truncate (issue #62) ────────────────────────────────────

  # --top's truncation SEMANTICS (which rows survive, when `truncated`
  # flips, the limit>eligible and limit==eligible boundaries, and the
  # filter→sort→truncate order of operations) are owned by the
  # domain::view::apply unit tests in crap-core. These scenarios pin only
  # the CLI-level contract that view.rs cannot reach: the flag threads
  # end-to-end into the serialized JSON envelope, --top 0 canonicalises to
  # a null limit, clap rejects non-positive integers, and — the feature's
  # headline promise — shaping the view never changes the CI exit code.

  @wired
  Scenario: --top N limits the report to the N highest-CRAP functions
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json --top 3`
    Then the JSON envelope at "view.shown" has 3 entries
    And the JSON envelope at "view.spec.limit" is 3
    And the JSON envelope at "view.eligible_count" is 6
    And the JSON envelope at "view.truncated" is true

  @wired
  Scenario: --top 0 canonicalises to no limit
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json --top 0`
    Then the JSON envelope at "view.shown" has 6 entries
    And the JSON envelope at "view.spec.limit" is null
    And the JSON envelope at "view.truncated" is false

  @wired
  Scenario: --top hiding violations does not change the exit code
    # The feature's headline promise (see this feature's preamble): shaping
    # the view never relaxes the gate. Three functions exceed threshold;
    # --top 1 hides two of those breaches from the rendered view, yet the
    # process still exits 1 because the gate reflects the full unfiltered
    # analysis, not the shaped view.
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json --top 1`
    Then the exit code is 1
    And the JSON envelope at "view.shown" has 1 entry
    And the JSON envelope at "view.truncated" is true
    And the JSON envelope at "result.summary.exceeding_threshold" is 3

  @wired
  Scenario Outline: --top rejects non-positive-integer values
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src <flags>`
    Then the exit code is 2
    And stderr contains "<message>"

    Examples:
      | flags     | message                        |
      | --top -3  | invalid value '-3' for '--top  |
      | --top 3.5 | invalid value '3.5' for '--top |
      | --top abc | invalid value 'abc' for '--top |

  # ── --min-coverage / --max-coverage: range filter (issue #63) ──────

  @unwired
  Scenario: --min-coverage filters out untested functions
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 1`
    Then no function in the report has coverage_percent equal to 0.0
    And the JSON envelope reports `view.filters.coverage_range` as { "min": 1.0, "max": 100.0 }

  @unwired
  Scenario: --max-coverage 0 surfaces only untested functions
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --max-coverage 0`
    Then every function in the report has coverage_percent equal to 0.0
    And the JSON envelope reports `view.filters.coverage_range` as { "min": 0.0, "max": 0.0 }

  @unwired
  Scenario: --min-coverage 100 surfaces only fully-tested functions
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 100`
    Then every function in the report has coverage_percent equal to 100.0

  @unwired
  Scenario: combining --min-coverage and --max-coverage targets partial coverage
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 1 --max-coverage 90`
    Then every function in the report has coverage_percent strictly above 0 and at most 90
    And the JSON envelope reports `view.filters.coverage_range` as { "min": 1.0, "max": 90.0 }

  @unwired
  Scenario Outline: invalid coverage ranges produce exit 2 with a clear stderr message
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info <flags>`
    Then the exit code is 2
    And stderr contains "<message>"

    Examples:
      | flags                                       | message                              |
      | --min-coverage -5                           | --min-coverage must be in [0, 100]   |
      | --max-coverage 105                          | --max-coverage must be in [0, 100]   |
      | --min-coverage 90 --max-coverage 30         | --min-coverage must not exceed --max-coverage |

  @unwired
  Scenario: filter hiding violations does not change the exit code
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 99`
    Then the report contains zero functions
    And the exit code is 1
    And the JSON envelope's `result.summary.exceeding_threshold` is greater than 0

  # ── --sort-by: choose sort dimension (issue #68) ───────────────────

  # --sort-by's per-dimension ORDERINGS (crap descending, coverage
  # ascending, complexity descending, path-alphabetical-then-CRAP within
  # file) and its composition with --top are owned by the
  # domain::view::apply sort + order-of-operations unit tests in crap-core.
  # These scenarios pin only the CLI-level contract view.rs cannot reach:
  # the flag threads into the JSON envelope as the lowercase ValueEnum
  # string, and clap rejects unknown dimensions.

  @wired
  Scenario Outline: --sort-by <key> echoes into the envelope as a lowercase string
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json --sort-by <key>`
    Then the JSON envelope at "view.spec.sort" is "<key>"

    Examples:
      | key        |
      | crap       |
      | coverage   |
      | complexity |
      | path       |

  @wired
  Scenario: --sort-by rejects an unknown dimension
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --sort-by nonsense`
    Then the exit code is 2
    And stderr contains "invalid value 'nonsense' for '--sort-by"

  # ── --no-fail: exit-code override (issue #65) ──────────────────────

  @unwired
  Scenario: --no-fail returns 0 even when violations exist
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --no-fail`
    Then the report shows every violating function
    And the exit code is 0
    And the JSON envelope's `result.passed` is false

  @unwired
  Scenario: --no-fail is a no-op when there are no violations
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    Given the project has zero functions exceeding the threshold
    When the operator runs `crap4rs --coverage lcov.info --no-fail`
    Then the exit code is 0

  @unwired
  Scenario: --quiet alone preserves CI exit-1 behavior
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --quiet`
    Then the report is suppressed
    And the exit code is 1

  @unwired
  Scenario: --quiet --no-fail composes to silent success
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --quiet --no-fail`
    Then the report is suppressed
    And the exit code is 0

  @unwired
  Scenario: --quiet also suppresses JSON output
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --quiet --format json`
    Then stdout is empty

  # ── --only-failing relocated, summary-semantics fix ────────────────

  @unwired
  Scenario: --only-failing produces a self-consistent summary
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --only-failing`
    Then the report contains only functions that exceed the threshold
    And the JSON envelope's `result.summary.total_functions` equals TOTAL_FUNCTIONS
    And the JSON envelope's `result.summary.exceeding_threshold` equals VIOLATING_FUNCTIONS
    And the JSON envelope's `result.summary.average_crap` reflects all TOTAL_FUNCTIONS functions
    And the JSON envelope's `result.summary.median_crap` reflects all TOTAL_FUNCTIONS functions
    And the JSON envelope's `result.summary.max_crap` reflects all TOTAL_FUNCTIONS functions
    And the JSON envelope's `result.summary.distribution` reflects all TOTAL_FUNCTIONS functions
    And the JSON envelope reports `view.filters.only_failing` equal to true

  @unwired
  Scenario: --only-failing on a passing project produces an empty report and exit 0
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    Given the project has zero functions exceeding the threshold
    When the operator runs `crap4rs --coverage lcov.info --only-failing`
    Then the report contains zero functions
    And the exit code is 0

  @unwired
  Scenario: --only-failing composes with --sort-by coverage for the worst-tested-violations investigation
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --only-failing --sort-by coverage`
    Then every function in the report exceeds the threshold
    And the rows are ordered by coverage percent ascending

  @unwired
  Scenario: --only-failing composes with --min-coverage as AND
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --only-failing --min-coverage 50`
    Then every function in the report exceeds the threshold AND has coverage_percent at least 50

  # ── JSON envelope shape ───────────────────────────────────────────

  @unwired
  Scenario: view block is always present, even on default invocation
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --format json`
    Then the JSON envelope contains a `view` block
    And `view.filters.only_failing` is false
    And `view.filters.coverage_range` is null
    And `view.sort` is "crap"
    And `view.limit` is null
    And `view.eligible_count` equals `result.summary.total_functions`
    And `view.truncated` is false
    And `view.shown` is an array
    And `view.shown_summary` contains every field of `AnalysisSummary` — `total_functions`, `total_files`, `exceeding_threshold`, `average_crap`, `median_crap`, `max_crap`, `worst_function`, and `distribution` — so a future field accidentally dropped from the serialized payload fails the assertion

  @unwired
  Scenario: default invocation has shown_summary equal to result.summary
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --format json`
    Then `view.shown_summary` is equal in every field to `result.summary`

  @unwired
  Scenario: view.shown contains complete FunctionVerdict objects, not indices
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --format json --top 3`
    Then `view.shown` is an array of length 3
    And each entry in `view.shown` contains `scored`, `threshold`, and `exceeds`
    And each entry's `scored` object contains `identity`, `complexity`, `coverage_percent`, `crap`
    And the entries are full objects, not array indices into `result.functions`

  @unwired
  Scenario: view.eligible_count distinguishes filter-narrowing from truncation
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --format json --min-coverage 1 --max-coverage 90 --top 10`
    Then `view.eligible_count` equals the count of functions with coverage_percent in [1, 90]
    And `view.shown.length` equals 10
    And `view.truncated` is true

  @unwired
  Scenario: result block is invariant under any view spec
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --format json` (the baseline)
    And the operator runs `crap4rs --coverage lcov.info --format json --top 5 --sort-by coverage --only-failing` (the shaped view)
    Then the `result` block in both invocations contains the same functions
    And the `result.summary` is identical between the two invocations
    And the `result.passed` is identical between the two invocations

  @unwired
  Scenario: view block declares values according to applied flags
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --format json --top 10 --min-coverage 5 --max-coverage 80 --sort-by complexity`
    Then `view.filters.coverage_range` is { "min": 5.0, "max": 80.0 }
    And `view.sort` is "complexity"
    And `view.limit` is 10

  @unwired
  Scenario: schema_version is 2 with the additive view block
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    # Bumped 1 → 2 in 0.4.0 by #107 (ComplexityContributor.column 0-based → 1-based).
    When the operator runs `crap4rs --coverage lcov.info --format json`
    Then the JSON envelope's `schema_version` is the integer 2
    And the JSON envelope key declaration order is `schema_version, tool_version, language, timestamp, metric, threshold, diff_ref, result, view`

  @unwired
  Scenario: agent consumer reads the JSON envelope to plan refactor scope
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    # Story E from the breadboard. Validates that the envelope contains
    # everything an agent needs without inferring or re-deriving state.
    When the operator runs `crap4rs --coverage lcov.info --format json --only-failing --sort-by crap --top 50`
    Then the JSON envelope contains both `result` and `view`
    And `result.summary.exceeding_threshold` reflects the global picture
    And `view.eligible_count` reflects the post-filter scope before truncation
    And `view.shown` contains up to 50 entries, each with full identity, complexity, coverage_percent, crap, and contributors
    And `view.eligible_count` may differ from `result.summary.exceeding_threshold` when the operator has applied filters beyond `--only-failing`

  # ── Display invariant: the optional "View" line ───────────────────

  @unwired
  Scenario: default invocation shows only the Analysis summary line
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info`
    Then the table shows an "Analysis: VIOLATING_FUNCTIONS/TOTAL_FUNCTIONS over threshold" line
    And the table does not show a "View:" line

  @unwired
  Scenario: --sort-by alone does not trigger the View line because sorting reorders without reducing rows
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --sort-by coverage`
    Then the table does not show a "View:" line

  @unwired
  Scenario: --top triggers the View line because rows are truncated
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --top 10`
    Then the table shows an "Analysis:" line referencing the full TOTAL_FUNCTIONS functions
    And the table shows a "View:" line referencing the 10 shown functions

  @unwired
  Scenario: a coverage filter triggers the View line because functions are excluded
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 1 --max-coverage 90`
    Then the table shows a "View:" line including "filtered from <eligible_count>"

  @unwired
  Scenario: --only-failing triggers the View line because violations are isolated
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --only-failing`
    Then the table shows a "View:" line referencing the VIOLATING_FUNCTIONS violating functions

  # ── Composed investigation example (Story B) ───────────────────────

  @unwired
  Scenario: investigator's flag-set produces a shaped report and exits 0
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 1 --max-coverage 90 --sort-by coverage --top 10 --no-fail`
    Then the report contains 10 functions
    And every function has coverage_percent in [1, 90]
    And the rows are ordered by coverage percent ascending
    And the JSON envelope reports `view.eligible_count` as the count of partially-covered functions
    And the JSON envelope reports `view.truncated` as true
    And the exit code is 0

  # ── First-run discoverability (V6) ────────────────────────────────

  @unwired
  Scenario: --help shows a basic first-run example
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --help`
    Then the help text includes the example "crap4rs --coverage lcov.info --top 20"

  @unwired
  Scenario: --help shows an investigation example
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --help`
    Then the help text includes an example using --min-coverage, --max-coverage, --sort-by, --top, and --no-fail together

  @unwired
  Scenario: --only-failing appears under filter flags in --help
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    # User-observable consequence of relocating --only-failing from
    # OutputArgs to FilterArgs (V1b).
    When the operator runs `crap4rs --help`
    Then the help text groups --only-failing alongside --min-coverage, --max-coverage, and --top under filter flags

  @unwired
  Scenario: first-run example from --help produces a tractable report
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs the basic `--help` example `crap4rs --coverage lcov.info --top 20`
    Then the table contains at most 20 rows
    And the table shows a "View:" line indicating truncation from TOTAL_FUNCTIONS
    And the table fits within 25 rows of output

  # ── Exit-code matrix summary ───────────────────────────────────────

  @unwired
  Scenario: default invocation on a violating project exits 1
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info`
    Then the exit code is 1

  @unwired
  Scenario Outline: exit-code matrix for flag combinations
    # tracked: crap-rs#169 — cli_ergonomics harness wires only the @summary group today
    When the operator runs `crap4rs --coverage lcov.info <flags>`
    Then the exit code is <code>

    Examples:
      | flags                                              | code |
      | --no-fail                                          | 0    |
      | --top 5                                            | 1    |
      | --min-coverage 99                                  | 1    |
      | --only-failing                                     | 1    |
      | --top 10 --no-fail                                 | 0    |
      | --quiet                                            | 1    |
      | --quiet --no-fail                                  | 0    |
      | --min-coverage -5                                  | 2    |
      | --max-coverage 105                                 | 2    |
      | --min-coverage 90 --max-coverage 30                | 2    |
      | --top -3                                           | 2    |

  # ── --summary: one-line CLI output (issue #131) ────────────────────
  #
  # crap4ts parity. Format:
  #   `<STATUS>: <N> functions | <M> above threshold (<T>) | worst: <W> | avg: <A>`
  # Status from `result.passed`, threshold formatted integer-when-whole,
  # worst/avg one decimal place. The tagged scenarios below are wired
  # to the `cli_ergonomics_cucumber` harness, which sets up a synthetic
  # LCOV+src layout per scenario (matches `cli_no_fail_integration.rs`).
  # The rest of this feature remains spec-only.

  @wired
  Scenario: --summary on a passing run emits a single PASS line
    Given a synthetic project where every function is within threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 25 --summary`
    Then stdout contains exactly one line
    And stdout matches "^PASS: \d+ functions \| 0 above threshold \(25\) \| worst: \d+\.\d \| avg: \d+\.\d$"
    And the exit code is 0

  @wired
  Scenario: --summary on a failing run emits a single FAIL line
    Given a synthetic project where at least one function exceeds threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --summary`
    Then stdout contains exactly one line
    And stdout matches "^FAIL: \d+ functions \| \d+ above threshold \(5\) \| worst: \d+\.\d \| avg: \d+\.\d$"
    And the exit code is 1

  @wired
  Scenario: --summary with --no-fail keeps emitting FAIL but exits 0
    Given a synthetic project where at least one function exceeds threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --summary --no-fail`
    Then stdout contains exactly one line
    And stdout starts with "FAIL:"
    And the exit code is 0

  @wired
  Scenario: --summary with --quiet suppresses output (quiet wins)
    Given a synthetic project where at least one function exceeds threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --summary --quiet`
    Then stdout is empty
    And the exit code is 1

  @wired
  Scenario: --summary short-circuits --format json (summary line wins)
    Given a synthetic project where every function is within threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 25 --summary --format json`
    Then stdout contains exactly one line
    And stdout starts with "PASS:"
    And stdout does not contain "schema_version"

  @wired
  Scenario: --summary renders fractional threshold with decimals
    Given a synthetic project where every function is within threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 25.5 --summary`
    Then stdout contains exactly one line
    And stdout contains "above threshold (25.5)"

  @wired
  Scenario: --summary help text includes the format template
    When the operator runs `crap4rs --help`
    Then stdout contains "--summary"
    And stdout contains "single-line analysis verdict"
