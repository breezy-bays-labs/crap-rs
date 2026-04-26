Feature: --group-by file (Bundle C, issue #64)

  At scale (hundreds of functions across dozens of files), users need to
  know *which files* are problematic before drilling into individual
  functions. The `--group-by file` flag aggregates the displayed view
  by source file: each row becomes a file with rolled-up counts and
  CRAP statistics. The gate keystone is preserved — exit code still
  derives from the unshapeable underlying analysis.

  Background:
    Given an analysis with 6 functions across 3 files:
      - src/blob.rs has 3 functions, 2 exceeding threshold
      - src/index.rs has 2 functions, 1 exceeding threshold
      - src/util.rs has 1 function, 0 exceeding threshold
    And the threshold is 8

  # ── Default invocation (no --group-by) ─────────────────────────────

  Scenario: Default invocation produces no grouped block
    When the operator runs `crap4rs --coverage lcov.info --format json`
    Then `view.grouped` is null
    And `view.spec.group_by` is null
    And `view.shown` is the full per-function row list

  # ── --group-by file populates the grouped block ────────────────────

  Scenario: --group-by file emits a grouped block in JSON
    When the operator runs `crap4rs --coverage lcov.info --group-by file --format json`
    Then `view.grouped.key` is "file"
    And `view.grouped.files.length` is 3
    And `view.grouped.eligible_count` is 3
    And `view.grouped.truncated` is false
    And `view.shown.length` is 6
    And each file in `view.grouped.files` has `file_path`, `function_count`, `exceeding_count`, `average_crap`, `median_crap`, `max_crap`, `worst_function`, `distribution`, `average_coverage`, and `max_complexity`

  # ── --top truncates files when grouped ─────────────────────────────

  Scenario: --top N truncates to top N files when grouped
    When the operator runs `crap4rs --coverage lcov.info --group-by file --top 1 --format json`
    Then `view.grouped.files.length` is 1
    And `view.grouped.truncated` is true
    And `view.grouped.eligible_count` is 3
    And `view.shown.length` is 6
    And `view.truncated` is false

  # ── --sort-by keys at file level under grouping ────────────────────

  Scenario: --sort-by coverage --group-by file sorts files by avg coverage ascending
    When the operator runs `crap4rs --coverage lcov.info --group-by file --sort-by coverage --format json`
    Then `view.grouped.files` is sorted by `average_coverage` ascending

  Scenario: --sort-by complexity --group-by file sorts files by max complexity descending
    When the operator runs `crap4rs --coverage lcov.info --group-by file --sort-by complexity --format json`
    Then `view.grouped.files` is sorted by `max_complexity` descending

  Scenario: --sort-by path --group-by file sorts files alphabetically
    When the operator runs `crap4rs --coverage lcov.info --group-by file --sort-by path --format json`
    Then `view.grouped.files` is sorted by `file_path` ascending

  # ── --only-failing composes with --group-by file ───────────────────

  Scenario: --only-failing --group-by file filters to files with at least one failing function
    When the operator runs `crap4rs --coverage lcov.info --only-failing --group-by file --format json`
    Then every file in `view.grouped.files` has `exceeding_count` >= 1
    And `view.grouped.files.length` is 2

  # ── CSV schema shifts under --group-by file ────────────────────────

  Scenario: --format csv --group-by file emits per-file header
    When the operator runs `crap4rs --coverage lcov.info --group-by file --format csv`
    Then the first line is "file,function_count,exceeding_count,average_crap,max_crap,worst_function,distribution_low,distribution_acceptable,distribution_moderate,distribution_high"
    And subsequent lines are per-file rows

  # ── --minimal-view composes with --group-by file ───────────────────

  Scenario: --minimal-view --group-by file strips view.shown but keeps view.grouped
    When the operator runs `crap4rs --coverage lcov.info --minimal-view --group-by file --format json`
    Then `view.shown` is absent from the JSON envelope
    And `view.grouped` is present and populated

  # ── Gate keystone: --group-by file does NOT change exit code ───────

  Scenario: --group-by file does not change exit code on a failing analysis
    Given the unfiltered analysis would exit 1 (violations exist)
    When the operator runs `crap4rs --coverage lcov.info --group-by file`
    Then the process exits 1
    And `result.passed` is false

  Scenario: --group-by file --top truncating files leaves the gate alone
    Given the unfiltered analysis would exit 1
    When the operator runs `crap4rs --coverage lcov.info --group-by file --top 1`
    Then the process exits 1
    And `result.passed` is false

  # ── Help text discoverability (issue #64 acceptance criteria) ──────

  Scenario: --help documents the --top and --sort-by semantic shift
    When the operator runs `crap4rs --help`
    Then the `--group-by` description mentions that `--top N` truncates files
    And mentions that `--sort-by` keys at the file level
