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
  # Every scenario below is @wired: each sets up a concrete tempdir
  # fixture via a `Given a synthetic project ...` step and invokes the
  # binary, so counts are fixture-derived rather than placeholder
  # symbols. The cucumber harness runs them all (it filters on @wired).

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

  # The coverage-range FILTER SEMANTICS (which functions pass, boundary
  # inclusivity at 0/50/90/100, NaN exclusion) and the AND-composition with
  # other filters are owned by the domain::view::apply filter unit tests +
  # the prop_filters_and_compose property test in crap-core. These scenarios
  # pin only the CLI-level contract view.rs cannot reach: the flags resolve
  # into the envelope's coverage_range with min/max defaulted from the
  # absent bound, validate_view_args rejects out-of-range/inverted bounds,
  # and a filter that hides violations never relaxes the gate.

  @wired
  Scenario Outline: coverage-range flags resolve into the envelope filter
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json <flags>`
    Then the JSON envelope at "view.spec.filters.coverage_range.min" is <min>
    And the JSON envelope at "view.spec.filters.coverage_range.max" is <max>

    Examples:
      | flags                               | min | max   |
      | --min-coverage 1                    | 1.0 | 100.0 |
      | --max-coverage 0                    | 0.0 | 0.0   |
      | --min-coverage 1 --max-coverage 90  | 1.0 | 90.0  |

  @wired
  Scenario Outline: invalid coverage ranges exit 2 with a clear message
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src <flags>`
    Then the exit code is 2
    And stderr contains "<message>"

    Examples:
      | flags                               | message                                       |
      | --min-coverage -5                   | --min-coverage must be in [0, 100]            |
      | --max-coverage 105                  | --max-coverage must be in [0, 100]            |
      | --min-coverage 90 --max-coverage 30 | --min-coverage must not exceed --max-coverage |

  @wired
  Scenario: a coverage filter hiding violations does not change the exit code
    # The feature's headline promise: shaping the view never relaxes the
    # gate. --min-coverage 99 drops the three uncovered failing functions
    # from the view (eligible falls to the three fully-covered passing
    # rows), yet the process still exits 1 because the gate reflects the
    # full unfiltered analysis.
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json --min-coverage 99`
    Then the exit code is 1
    And the JSON envelope at "view.eligible_count" is 3
    And the JSON envelope at "result.summary.exceeding_threshold" is 3

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

  # --no-fail (exit-code override) and --quiet (output suppression) are
  # CLI-process behaviors with no domain representation — the only place
  # they are observable is the live process's exit code and stdout. Their
  # core override + suppression are already pinned by the wired --summary
  # scenarios below (--summary --no-fail exits 0; --summary --quiet is empty
  # and preserves exit 1). These two pin what those cannot: that --no-fail
  # leaves the JSON result block truthful (a consumer still reads
  # result.passed=false), and that --quiet and --no-fail compose to silent
  # CI success.

  @wired
  Scenario: --no-fail forces exit 0 but keeps result.passed false
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json --no-fail`
    Then the exit code is 0
    And the JSON envelope at "result.passed" is false

  @wired
  Scenario: --quiet and --no-fail compose to silent success
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --quiet --no-fail`
    Then stdout is empty
    And the exit code is 0

  # ── --only-failing: filter to violations, summary stays global ─────
  #
  # The summary-semantics promise: under --only-failing, result.summary.*
  # still reflects the FULL unfiltered analysis (total_functions,
  # exceeding_threshold, averages, distribution) while view.shown carries
  # only the violations. The filter+sort COMPOSITION and per-row ordering
  # are owned by domain::view::apply — prop_filters_and_compose lifts the
  # AND-composition and the order-of-ops unit tests pin coverage-ascending
  # — so the two former composition scenarios (--only-failing --sort-by
  # coverage, --only-failing --min-coverage AND) live there, not here.

  @wired
  Scenario: --only-failing keeps result.summary global while view.shown holds only violations
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json --only-failing`
    Then the exit code is 1
    And the JSON envelope at "result.summary.total_functions" is 6
    And the JSON envelope at "result.summary.exceeding_threshold" is 3
    And the JSON envelope at "view.spec.filters.only_failing" is true
    And the JSON envelope at "view.shown" has 3 entries

  @wired
  Scenario: --only-failing on a passing project produces an empty view and exit 0
    Given a synthetic project where every function is within threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --format json --only-failing`
    Then the exit code is 0
    And the JSON envelope at "result.summary.total_functions" is 3
    And the JSON envelope at "view.shown" has 0 entries

  # ── JSON envelope: the view block ─────────────────────────────────
  #
  # The envelope SHAPE is owned at lower levels: json_reporter.feature
  # pins schema_version, the result.functions[] entry shape (scored,
  # threshold, exceeds), and the result.summary aggregates; the
  # wire_envelope_crap4rs snapshot pins the exact byte layout including
  # key declaration order and the view block. The per-flag echo of
  # spec.limit / spec.sort / spec.filters.coverage_range is wired by the
  # --top / --sort-by / coverage-range scenarios above. The result block's
  # immutability under ANY spec is the
  # prop_result_block_invariant_under_any_spec proptest in crap-core.
  # What remains uniquely at the CLI-acceptance level is the promise a
  # JSON consumer relies on: a default invocation always carries a view
  # block whose spec is neutral.

  @wired
  Scenario: a default invocation carries a neutral view block
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json`
    Then the JSON envelope at "view.spec.filters.only_failing" is false
    And the JSON envelope at "view.spec.filters.coverage_range" is null
    And the JSON envelope at "view.spec.sort" is "crap"
    And the JSON envelope at "view.spec.limit" is null
    And the JSON envelope at "view.eligible_count" is 6
    And the JSON envelope at "view.truncated" is false
    And the JSON envelope at "view.shown" has 6 entries

  # ── Display: the optional "View:" subtitle line (table mode) ───────
  #
  # The table always prints a "Summary:" block (the gate truth). It prints
  # a second "View:" line ONLY when the shaped view materially differs
  # from the analysis — rows filtered out or truncated. Sort-only and
  # default invocations leave it absent. The predicate
  # (should_render_view_line) and the exact shaping descriptors are owned
  # by crap-core; these scenarios pin the CLI-level emergent contract: the
  # line appears iff rows were reduced, and names the active shaping. The
  # six-function fixture makes the counts concrete (three trivial+covered,
  # three branchy+uncovered).

  @wired
  Scenario: a default invocation prints the Summary block and no View line
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5`
    Then stdout contains "Summary: 6 functions"
    And stdout does not contain "View:"

  @wired
  Scenario: --sort-by alone does not print a View line (reorders without reducing rows)
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --sort-by coverage`
    Then stdout does not contain "View:"

  @wired
  Scenario: --top prints a View line because rows are truncated
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --top 3`
    Then stdout contains "View: showing 3 of 6 functions (top 3)"

  @wired
  Scenario: a coverage filter prints a View line because functions are excluded
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --max-coverage 90`
    Then stdout contains "View: showing 3 of 6 functions (coverage 0–90%)"

  @wired
  Scenario: --only-failing prints a View line because violations are isolated
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --only-failing`
    Then stdout contains "View: showing 3 of 6 functions (failing only)"

  # ── Composed investigation example (Story B) ───────────────────────

  @wired
  Scenario: an investigator's full flag-set composes into a shaped report that does not fail CI
    # The headline composition: filter (--only-failing) + sort (--sort-by
    # coverage) + truncate (--top 5) + gate override (--no-fail) in one
    # invocation. The ordering and per-row predicates are owned by the
    # domain::view::apply order-of-operations tests; this pins the CLI-level
    # emergent contract — the quartet composes, --no-fail forces exit 0, and
    # the gate stays truthful (result.passed reflects the full analysis, so a
    # JSON consumer still sees the would-have-failed signal).
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --format json --only-failing --sort-by coverage --top 5 --no-fail`
    Then the exit code is 0
    And the JSON envelope at "result.passed" is false
    And the JSON envelope at "view.shown" has 3 entries

  # ── First-run discoverability: --help content ─────────────────────
  #
  # Absorbs cli_help_content_integration.rs (the about / long_about /
  # EXAMPLES strings threaded from main.rs through AdapterMeta into clap).
  # The former "first-run example produces a tractable report" scenario is
  # dropped: its truncation behavior is the --top View-line contract above
  # and its example text is asserted here.

  @wired
  Scenario: short -h help shows the one-line tool description
    When the operator runs `crap4rs -h`
    Then stdout contains "CRAP score analyzer for Rust"

  @wired
  Scenario: long --help shows the extended description and the first-run example
    When the operator runs `crap4rs --help`
    Then stdout contains "Change Risk Anti-Patterns"
    And stdout contains "cognitive complexity"
    And stdout contains "EXAMPLES:"
    And stdout contains "crap4rs --coverage lcov.info --top 20"

  @wired
  Scenario: --help shows an investigation example combining the shaping flags
    # The literal example line wraps to the terminal width in clap's
    # after-help block, so this asserts whitespace-normalized.
    When the operator runs `crap4rs --help`
    Then the help text shows the example "--min-coverage 1 --max-coverage 90 --sort-by coverage --top 10 --no-fail"

  @wired
  Scenario: --only-failing is grouped under the Filtering heading in --help
    # User-observable consequence of relocating --only-failing from the
    # Output group to Filtering.
    When the operator runs `crap4rs --help`
    Then the help text lists "--only-failing" under the "Filtering" heading
    And the help text lists "--min-coverage" under the "Filtering" heading
    And the help text lists "--max-coverage" under the "Filtering" heading
    And the help text lists "--top" under the "Filtering" heading

  # ── Exit-code matrix ──────────────────────────────────────────────
  #
  # The headline gate promise — shaping the view never changes the CI
  # exit code — is pinned at each flag's keystone: --top (truncate),
  # --min/--max-coverage (filter), and --no-fail / --quiet (override) each
  # have @wired exit-code scenarios above, and the validation exits (2)
  # ride their clap-rejection outlines. The former consolidated matrix
  # outline re-tested those same rows, so it is removed (test each
  # behavior once). What is NOT otherwise covered is the bare gate: a
  # default invocation on a violating project exits 1.

  @wired
  Scenario: a default invocation on a violating project exits 1
    Given a synthetic project with six functions spanning the CRAP range
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5`
    Then the exit code is 1

  # ── --summary: one-line CLI output (issue #131) ────────────────────
  #
  # crap4ts parity. Format:
  #   `<STATUS>: <N> functions | <M> above threshold (<T>) | worst: <W> | avg: <A>`
  # Status from `result.passed`, threshold formatted integer-when-whole,
  # worst/avg one decimal place. Like the other @wired scenarios in this
  # feature, these run against the `cli_ergonomics_cucumber` harness, which
  # sets up a synthetic LCOV+src layout per scenario.

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
