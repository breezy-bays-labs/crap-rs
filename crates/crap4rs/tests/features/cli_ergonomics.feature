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

  Background:
    Given a project with an LCOV file at "lcov.info"
    And the project's analysis produces TOTAL_FUNCTIONS functions
    And VIOLATING_FUNCTIONS of those functions exceed the threshold
    And TOTAL_FUNCTIONS > 0 and VIOLATING_FUNCTIONS > 0

  # ── --top: truncate (issue #62) ────────────────────────────────────

  Scenario: --top N limits the report to the N highest-CRAP functions
    When the operator runs `crap4rs --coverage lcov.info --top 10`
    Then the table contains 10 rows
    And the table rows are the 10 highest-CRAP functions
    And the JSON envelope reports `view.limit` equal to 10
    And the JSON envelope reports `view.eligible_count` equal to TOTAL_FUNCTIONS
    And the JSON envelope reports `view.truncated` equal to true

  Scenario: --top 0 means no limit
    When the operator runs `crap4rs --coverage lcov.info --top 0`
    Then the report includes every function
    And the JSON envelope reports `view.limit` equal to null
    And the JSON envelope reports `view.truncated` equal to false

  Scenario: --top 1 surfaces only the worst function
    When the operator runs `crap4rs --coverage lcov.info --top 1`
    Then the table contains 1 row
    And that row is the highest-CRAP function in the analysis
    And the JSON envelope reports `view.truncated` equal to true

  Scenario: --top greater than the eligible count truncates nothing
    When the operator runs `crap4rs --coverage lcov.info --top 1000000`
    Then the report includes every function
    And the JSON envelope reports `view.limit` equal to 1000000
    And the JSON envelope reports `view.truncated` equal to false

  Scenario: --top hiding violations does not change the exit code
    When the operator runs `crap4rs --coverage lcov.info --top 5`
    Then the table shows 5 rows
    And the exit code is 1
    And the JSON envelope's `result.passed` is false

  Scenario Outline: --top rejects non-positive-integer values
    When the operator runs `crap4rs --coverage lcov.info <flags>`
    Then the exit code is 2
    And stderr contains "<message>"

    Examples:
      | flags        | message                                |
      | --top -3     | invalid value '-3' for '--top'         |
      | --top 3.5    | invalid value '3.5' for '--top'        |
      | --top abc    | invalid value 'abc' for '--top'        |

  # ── --min-coverage / --max-coverage: range filter (issue #63) ──────

  Scenario: --min-coverage filters out untested functions
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 1`
    Then no function in the report has coverage_percent equal to 0.0
    And the JSON envelope reports `view.filters.coverage_range` as { "min": 1.0, "max": 100.0 }

  Scenario: --max-coverage 0 surfaces only untested functions
    When the operator runs `crap4rs --coverage lcov.info --max-coverage 0`
    Then every function in the report has coverage_percent equal to 0.0
    And the JSON envelope reports `view.filters.coverage_range` as { "min": 0.0, "max": 0.0 }

  Scenario: --min-coverage 100 surfaces only fully-tested functions
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 100`
    Then every function in the report has coverage_percent equal to 100.0

  Scenario: combining --min-coverage and --max-coverage targets partial coverage
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 1 --max-coverage 90`
    Then every function in the report has coverage_percent strictly above 0 and at most 90
    And the JSON envelope reports `view.filters.coverage_range` as { "min": 1.0, "max": 90.0 }

  Scenario Outline: invalid coverage ranges produce exit 2 with a clear stderr message
    When the operator runs `crap4rs --coverage lcov.info <flags>`
    Then the exit code is 2
    And stderr contains "<message>"

    Examples:
      | flags                                       | message                              |
      | --min-coverage -5                           | --min-coverage must be in [0, 100]   |
      | --max-coverage 105                          | --max-coverage must be in [0, 100]   |
      | --min-coverage 90 --max-coverage 30         | --min-coverage must not exceed --max-coverage |

  Scenario: filter hiding violations does not change the exit code
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 99`
    Then the report contains zero functions
    And the exit code is 1
    And the JSON envelope's `result.summary.exceeding_threshold` is greater than 0

  # ── --sort-by: choose sort dimension (issue #68) ───────────────────

  Scenario: --sort-by crap is the default order (CRAP descending)
    When the operator runs `crap4rs --coverage lcov.info`
    Then the report is ordered by CRAP score descending
    And the JSON envelope reports `view.sort` equal to "crap"

  Scenario: --sort-by coverage orders by coverage percent ascending
    When the operator runs `crap4rs --coverage lcov.info --sort-by coverage`
    Then the report is ordered by coverage percent ascending
    And the JSON envelope reports `view.sort` equal to "coverage"

  Scenario: --sort-by complexity orders by complexity descending
    When the operator runs `crap4rs --coverage lcov.info --sort-by complexity`
    Then the report is ordered by complexity descending
    And the JSON envelope reports `view.sort` equal to "complexity"

  Scenario: --sort-by path orders alphabetically by file, then CRAP descending within file
    # Delegates to domain SortKey::Path semantics — see view.feature
    # for the full secondary-sort spec.
    Given the project has functions in src/a.rs (CRAPs 5 and 30), src/b.rs (CRAP 10), src/c.rs (CRAPs 1 and 50)
    When the operator runs `crap4rs --coverage lcov.info --sort-by path`
    Then the report rows appear in order: src/a.rs::CRAP 30, src/a.rs::CRAP 5, src/b.rs::CRAP 10, src/c.rs::CRAP 50, src/c.rs::CRAP 1
    And the JSON envelope reports `view.sort` equal to "path"

  Scenario: --sort-by composes with --top to surface the lowest-coverage targets
    When the operator runs `crap4rs --coverage lcov.info --sort-by coverage --top 10`
    Then the report contains the 10 functions with the lowest coverage percent
    And the rows are ordered by coverage percent ascending

  # ── --no-fail: exit-code override (issue #65) ──────────────────────

  Scenario: --no-fail returns 0 even when violations exist
    When the operator runs `crap4rs --coverage lcov.info --no-fail`
    Then the report shows every violating function
    And the exit code is 0
    And the JSON envelope's `result.passed` is false

  Scenario: --no-fail is a no-op when there are no violations
    Given the project has zero functions exceeding the threshold
    When the operator runs `crap4rs --coverage lcov.info --no-fail`
    Then the exit code is 0

  Scenario: --quiet alone preserves CI exit-1 behavior
    When the operator runs `crap4rs --coverage lcov.info --quiet`
    Then the report is suppressed
    And the exit code is 1

  Scenario: --quiet --no-fail composes to silent success
    When the operator runs `crap4rs --coverage lcov.info --quiet --no-fail`
    Then the report is suppressed
    And the exit code is 0

  Scenario: --quiet also suppresses JSON output
    When the operator runs `crap4rs --coverage lcov.info --quiet --format json`
    Then stdout is empty

  # ── --only-failing relocated, summary-semantics fix ────────────────

  Scenario: --only-failing produces a self-consistent summary
    When the operator runs `crap4rs --coverage lcov.info --only-failing`
    Then the report contains only functions that exceed the threshold
    And the JSON envelope's `result.summary.total_functions` equals TOTAL_FUNCTIONS
    And the JSON envelope's `result.summary.exceeding_threshold` equals VIOLATING_FUNCTIONS
    And the JSON envelope's `result.summary.average_crap` reflects all TOTAL_FUNCTIONS functions
    And the JSON envelope's `result.summary.median_crap` reflects all TOTAL_FUNCTIONS functions
    And the JSON envelope's `result.summary.max_crap` reflects all TOTAL_FUNCTIONS functions
    And the JSON envelope's `result.summary.distribution` reflects all TOTAL_FUNCTIONS functions
    And the JSON envelope reports `view.filters.only_failing` equal to true

  Scenario: --only-failing on a passing project produces an empty report and exit 0
    Given the project has zero functions exceeding the threshold
    When the operator runs `crap4rs --coverage lcov.info --only-failing`
    Then the report contains zero functions
    And the exit code is 0

  Scenario: --only-failing composes with --sort-by coverage for the worst-tested-violations investigation
    When the operator runs `crap4rs --coverage lcov.info --only-failing --sort-by coverage`
    Then every function in the report exceeds the threshold
    And the rows are ordered by coverage percent ascending

  Scenario: --only-failing composes with --min-coverage as AND
    When the operator runs `crap4rs --coverage lcov.info --only-failing --min-coverage 50`
    Then every function in the report exceeds the threshold AND has coverage_percent at least 50

  # ── JSON envelope shape ───────────────────────────────────────────

  Scenario: view block is always present, even on default invocation
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

  Scenario: default invocation has shown_summary equal to result.summary
    When the operator runs `crap4rs --coverage lcov.info --format json`
    Then `view.shown_summary` is equal in every field to `result.summary`

  Scenario: view.shown contains complete FunctionVerdict objects, not indices
    When the operator runs `crap4rs --coverage lcov.info --format json --top 3`
    Then `view.shown` is an array of length 3
    And each entry in `view.shown` contains `scored`, `threshold`, and `exceeds`
    And each entry's `scored` object contains `identity`, `complexity`, `coverage_percent`, `crap`
    And the entries are full objects, not array indices into `result.functions`

  Scenario: view.eligible_count distinguishes filter-narrowing from truncation
    When the operator runs `crap4rs --coverage lcov.info --format json --min-coverage 1 --max-coverage 90 --top 10`
    Then `view.eligible_count` equals the count of functions with coverage_percent in [1, 90]
    And `view.shown.length` equals 10
    And `view.truncated` is true

  Scenario: result block is invariant under any view spec
    When the operator runs `crap4rs --coverage lcov.info --format json` (the baseline)
    And the operator runs `crap4rs --coverage lcov.info --format json --top 5 --sort-by coverage --only-failing` (the shaped view)
    Then the `result` block in both invocations contains the same functions
    And the `result.summary` is identical between the two invocations
    And the `result.passed` is identical between the two invocations

  Scenario: view block declares values according to applied flags
    When the operator runs `crap4rs --coverage lcov.info --format json --top 10 --min-coverage 5 --max-coverage 80 --sort-by complexity`
    Then `view.filters.coverage_range` is { "min": 5.0, "max": 80.0 }
    And `view.sort` is "complexity"
    And `view.limit` is 10

  Scenario: schema_version is 2 with the additive view block
    # Bumped 1 → 2 in 0.4.0 by #107 (ComplexityContributor.column 0-based → 1-based).
    When the operator runs `crap4rs --coverage lcov.info --format json`
    Then the JSON envelope's `schema_version` is the integer 2
    And the JSON envelope key declaration order is `schema_version, tool_version, language, timestamp, metric, threshold, diff_ref, result, view`

  Scenario: agent consumer reads the JSON envelope to plan refactor scope
    # Story E from the breadboard. Validates that the envelope contains
    # everything an agent needs without inferring or re-deriving state.
    When the operator runs `crap4rs --coverage lcov.info --format json --only-failing --sort-by crap --top 50`
    Then the JSON envelope contains both `result` and `view`
    And `result.summary.exceeding_threshold` reflects the global picture
    And `view.eligible_count` reflects the post-filter scope before truncation
    And `view.shown` contains up to 50 entries, each with full identity, complexity, coverage_percent, crap, and contributors
    And `view.eligible_count` may differ from `result.summary.exceeding_threshold` when the operator has applied filters beyond `--only-failing`

  # ── Display invariant: the optional "View" line ───────────────────

  Scenario: default invocation shows only the Analysis summary line
    When the operator runs `crap4rs --coverage lcov.info`
    Then the table shows an "Analysis: VIOLATING_FUNCTIONS/TOTAL_FUNCTIONS over threshold" line
    And the table does not show a "View:" line

  Scenario: --sort-by alone does not trigger the View line because sorting reorders without reducing rows
    When the operator runs `crap4rs --coverage lcov.info --sort-by coverage`
    Then the table does not show a "View:" line

  Scenario: --top triggers the View line because rows are truncated
    When the operator runs `crap4rs --coverage lcov.info --top 10`
    Then the table shows an "Analysis:" line referencing the full TOTAL_FUNCTIONS functions
    And the table shows a "View:" line referencing the 10 shown functions

  Scenario: a coverage filter triggers the View line because functions are excluded
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 1 --max-coverage 90`
    Then the table shows a "View:" line including "filtered from <eligible_count>"

  Scenario: --only-failing triggers the View line because violations are isolated
    When the operator runs `crap4rs --coverage lcov.info --only-failing`
    Then the table shows a "View:" line referencing the VIOLATING_FUNCTIONS violating functions

  # ── Composed investigation example (Story B) ───────────────────────

  Scenario: investigator's flag-set produces a shaped report and exits 0
    When the operator runs `crap4rs --coverage lcov.info --min-coverage 1 --max-coverage 90 --sort-by coverage --top 10 --no-fail`
    Then the report contains 10 functions
    And every function has coverage_percent in [1, 90]
    And the rows are ordered by coverage percent ascending
    And the JSON envelope reports `view.eligible_count` as the count of partially-covered functions
    And the JSON envelope reports `view.truncated` as true
    And the exit code is 0

  # ── First-run discoverability (V6) ────────────────────────────────

  Scenario: --help shows a basic first-run example
    When the operator runs `crap4rs --help`
    Then the help text includes the example "crap4rs --coverage lcov.info --top 20"

  Scenario: --help shows an investigation example
    When the operator runs `crap4rs --help`
    Then the help text includes an example using --min-coverage, --max-coverage, --sort-by, --top, and --no-fail together

  Scenario: --only-failing appears under filter flags in --help
    # User-observable consequence of relocating --only-failing from
    # OutputArgs to FilterArgs (V1b).
    When the operator runs `crap4rs --help`
    Then the help text groups --only-failing alongside --min-coverage, --max-coverage, and --top under filter flags

  Scenario: first-run example from --help produces a tractable report
    When the operator runs the basic `--help` example `crap4rs --coverage lcov.info --top 20`
    Then the table contains at most 20 rows
    And the table shows a "View:" line indicating truncation from TOTAL_FUNCTIONS
    And the table fits within 25 rows of output

  # ── Exit-code matrix summary ───────────────────────────────────────

  Scenario: default invocation on a violating project exits 1
    When the operator runs `crap4rs --coverage lcov.info`
    Then the exit code is 1

  Scenario Outline: exit-code matrix for flag combinations
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

  @summary
  Scenario: --summary on a passing run emits a single PASS line
    Given a synthetic project where every function is within threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 25 --summary`
    Then stdout contains exactly one line
    And stdout matches "^PASS: \d+ functions \| 0 above threshold \(25\) \| worst: \d+\.\d \| avg: \d+\.\d$"
    And the exit code is 0

  @summary
  Scenario: --summary on a failing run emits a single FAIL line
    Given a synthetic project where at least one function exceeds threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --summary`
    Then stdout contains exactly one line
    And stdout matches "^FAIL: \d+ functions \| \d+ above threshold \(5\) \| worst: \d+\.\d \| avg: \d+\.\d$"
    And the exit code is 1

  @summary
  Scenario: --summary with --no-fail keeps emitting FAIL but exits 0
    Given a synthetic project where at least one function exceeds threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --summary --no-fail`
    Then stdout contains exactly one line
    And stdout starts with "FAIL:"
    And the exit code is 0

  @summary
  Scenario: --summary with --quiet suppresses output (quiet wins)
    Given a synthetic project where at least one function exceeds threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --summary --quiet`
    Then stdout is empty
    And the exit code is 1

  @summary
  Scenario: --summary short-circuits --format json (summary line wins)
    Given a synthetic project where every function is within threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 25 --summary --format json`
    Then stdout contains exactly one line
    And stdout starts with "PASS:"
    And stdout does not contain "schema_version"

  @summary
  Scenario: --summary renders fractional threshold with decimals
    Given a synthetic project where every function is within threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 25.5 --summary`
    Then stdout contains exactly one line
    And stdout contains "above threshold (25.5)"

  @summary
  Scenario: --summary help text includes the format template
    When the operator runs `crap4rs --help`
    Then stdout contains "--summary"
    And stdout contains "single-line analysis verdict"
