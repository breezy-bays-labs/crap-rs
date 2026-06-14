Feature: --group-by file (Bundle C, issue #64)

  At scale (hundreds of functions across dozens of files), users need to
  know *which files* are problematic before drilling into individual
  functions. The `--group-by file` flag aggregates the displayed view
  by source file: each row becomes a file with rolled-up counts and
  CRAP statistics. The gate keystone is preserved — exit code still
  derives from the unshapeable underlying analysis.

  This file pins the CLI-process contracts: the `view.grouped` envelope
  shape, the `--group-by` / `--top` / `--only-failing` / `--minimal-view`
  flag wiring, the CSV per-file header, the gate keystone, and `--help`
  discoverability. The file-level sort / truncate / filter SEMANTICS are
  owned by `domain::view`'s 17 `group_by_file_*` unit tests (so the
  `--sort-by` ordering cases live there, not here). Step defs in
  `tests/group_by_file_cucumber.rs`.

  Background:
    Given a project with 6 functions across 3 files (blob.rs 3 functions 2 exceeding, index.rs 2 functions 1 exceeding, util.rs 1 function 0 exceeding)

  # ── Default invocation (no --group-by) ─────────────────────────────

  @wired
  Scenario: Default invocation produces no grouped block
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 8 --no-fail --format json`
    Then the JSON envelope at "view.grouped" is null
    And the JSON envelope at "view.spec.group_by" is null
    And the JSON envelope at "view.shown" has 6 entries

  # ── --group-by file populates the grouped block ────────────────────

  @wired
  Scenario: --group-by file emits a grouped block in JSON
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 8 --no-fail --group-by file --format json`
    Then the JSON envelope at "view.grouped.key" is "file"
    And the JSON envelope at "view.grouped.files" has 3 entries
    And the JSON envelope at "view.grouped.eligible_count" is 3
    And the JSON envelope at "view.grouped.truncated" is false
    And the JSON envelope at "view.shown" has 6 entries
    And each grouped file carries the FileSummary fields

  # ── --top truncates files when grouped ─────────────────────────────

  @wired
  Scenario: --top N truncates to top N files when grouped (functions untouched)
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 8 --no-fail --group-by file --top 1 --format json`
    Then the JSON envelope at "view.grouped.files" has 1 entry
    And the JSON envelope at "view.grouped.truncated" is true
    And the JSON envelope at "view.grouped.eligible_count" is 3
    And the JSON envelope at "view.shown" has 6 entries
    And the JSON envelope at "view.truncated" is false

  # ── --only-failing composes with --group-by file ───────────────────

  @wired
  Scenario: --only-failing --group-by file keeps only files with a failing function
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 8 --no-fail --only-failing --group-by file --format json`
    Then the JSON envelope at "view.grouped.files" has 2 entries
    And every grouped file has at least one exceeding function

  # ── CSV schema shifts under --group-by file ────────────────────────

  @wired
  Scenario: --format csv --group-by file emits the per-file header
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 8 --no-fail --group-by file --format csv`
    Then the first stdout line is "file,function_count,exceeding_count,average_crap,max_crap,worst_function,distribution_low,distribution_acceptable,distribution_moderate,distribution_high"

  # ── --minimal-view composes with --group-by file ───────────────────

  @wired
  Scenario: --minimal-view --group-by file strips view.shown but keeps view.grouped
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 8 --no-fail --minimal-view --group-by file --format json`
    Then the JSON envelope has no "view.shown" path
    And the JSON envelope at "view.grouped.files" has 3 entries

  # ── Gate keystone: --group-by file does NOT change exit code ───────

  @wired
  Scenario: --group-by file does not change exit code on a failing analysis
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 8 --group-by file --format json`
    Then the exit code is 1
    And the JSON envelope at "result.passed" is false

  @wired
  Scenario: --group-by file --top truncating files leaves the gate alone
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 8 --group-by file --top 1 --format json`
    Then the exit code is 1
    And the JSON envelope at "result.passed" is false

  # ── Help text discoverability (issue #64 acceptance criteria) ──────

  @wired
  Scenario: --help documents the --top and --sort-by semantic shift
    When the operator runs `crap4rs --help`
    Then stdout contains "--group-by"
    And stdout contains "top N **files**"
    And stdout contains "keys at the file level"
