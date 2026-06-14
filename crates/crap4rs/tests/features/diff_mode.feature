Feature: Diff mode

  The --diff <ref> flag scopes CRAP analysis to only functions in
  files changed since the given git ref. This enables CI PR gating
  where developers only see CRAP scores for code they touched.

  This file pins the CLI-acceptance contracts a running binary uniquely
  captures against a real git repo: the function-selection a --diff run
  surfaces, the empty-diff exit code, the `diff_ref` envelope field, the
  validation exit codes, filter composition, and rename handling. The
  lower-level diff MECHANICS — unified-diff + hunk parsing, deletion-only
  skipping, new-file detection, bad-ref handling, path normalization —
  are owned by crap-core's `adapters::diff` unit tests; the `.rs`
  extension filter by `core` / `walker`; and `diff_ref` serialization by
  `reporters::json`. Step defs live in `tests/diff_cucumber.rs`.

  # ── Function selection ────────────────────────────────────────────
  # core::compute_diff_regions (changed line-ranges → which functions)
  # has no crap-core unit test of its own; these two are its coverage.

  @wired
  Scenario: Only changed functions appear in the report
    Given a git repo whose latest commit changed only function foo
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff HEAD~1 --threshold 30 --format json`
    Then the report includes function "foo"
    And the report excludes function "bar"

  @wired
  Scenario: Hunk-level precision excludes an untouched function in a changed file
    Given a git repo whose latest commit changed only function alpha, leaving beta untouched
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff HEAD~1 --threshold 30 --format json`
    Then the report includes function "alpha"
    And the report excludes function "beta"

  # ── Empty diff ────────────────────────────────────────────────────

  @wired
  Scenario: An empty diff produces an empty report and exit 0
    Given a git repo with no changes since HEAD
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff HEAD --threshold 30 --format json`
    Then the exit code is 0
    And the report contains 0 functions
    And the result reports passed as true

  # ── JSON envelope ─────────────────────────────────────────────────

  @wired
  Scenario: The JSON envelope records diff_ref when --diff is used
    Given a git repo whose latest commit changed only function foo
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff HEAD~1 --threshold 30 --format json`
    Then the JSON envelope at "diff_ref" is "HEAD~1"

  @wired
  Scenario: The JSON envelope has a null diff_ref when --diff is not used
    Given a project that is not a git repository
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 30 --format json`
    Then the JSON envelope at "diff_ref" is null

  # ── Filter composition (AND) ──────────────────────────────────────

  @wired
  Scenario: --diff composes with --exclude as AND
    Given a git repo with changes in src/lib.rs and src/tests/test_lib.rs
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff HEAD~1 --exclude tests/** --threshold 30 --format json`
    Then the report includes function "kept"
    And the report excludes function "excluded"

  @wired
  Scenario: --diff composes with --only-failing as AND
    # --diff scopes the analysis (result.functions = changed: both); --only-failing
    # shapes the view, so view.shown is the changed AND failing intersection.
    Given a git repo with changed functions, one passing and one exceeding threshold
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff HEAD~1 --only-failing --threshold 5 --format json`
    Then the view includes function "complex"
    And the view excludes function "simple"

  # ── Rename ────────────────────────────────────────────────────────

  @wired
  Scenario: A renamed file surfaces its functions under the new path
    Given a git repo where src/old.rs was renamed to src/new.rs with changes
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff HEAD~1 --threshold 30 --format json`
    Then the report includes function "moved"

  # ── Validation errors ─────────────────────────────────────────────

  @wired
  Scenario: --diff outside a git repository exits 2
    Given a project that is not a git repository
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff main --threshold 30`
    Then the exit code is 2
    And stderr contains "not inside a git work tree"

  @wired
  Scenario: --diff with an invalid ref exits 2
    Given a git repo
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff nonexistent-ref-xyz --threshold 30`
    Then the exit code is 2
    And stderr contains "bad revision"

  @wired
  Scenario: --diff with a dash-prefixed ref is rejected (never reaches git)
    Given a git repo
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --diff --malicious-flag --threshold 30`
    Then the exit code is 2
    And stderr contains "unexpected argument"
