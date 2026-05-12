Feature: Diff mode

  The --diff <ref> flag scopes CRAP analysis to only functions in
  files changed since the given git ref. This enables CI PR gating
  where developers only see CRAP scores for code they touched.

  # ── Core Behavior ─────────────────────────────────────────────────

  @unwired
  Scenario: Only changed functions appear in output
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with a baseline commit containing functions foo and bar
    And a second commit that modifies only foo
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then the output includes function foo
    And the output does not include function bar

  @unwired
  Scenario: New file includes all functions
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with a baseline commit
    And a second commit that adds a new file with functions baz and qux
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then the output includes function baz
    And the output includes function qux

  @unwired
  Scenario: Hunk-level precision — untouched function in changed file excluded
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with a file containing functions alpha (lines 1-10) and beta (lines 20-30)
    And a commit that changes only lines 5-8
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then the output includes function alpha
    And the output does not include function beta

  # ── Score Invariant ───────────────────────────────────────────────

  @unwired
  Scenario: Diff mode produces identical scores to full analysis
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with a baseline commit and subsequent changes
    And a full analysis has been recorded for the changed functions
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then each function's CRAP score matches the full analysis score exactly

  # ── Empty Diff ────────────────────────────────────────────────────

  @unwired
  Scenario: Empty diff produces empty result and exit 0
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo where HEAD matches the baseline ref
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then the output contains zero functions
    And the exit code is 0
    And the result shows passed as true

  # ── Filter Composition ────────────────────────────────────────────

  @unwired
  Scenario: --diff composes with --exclude as AND
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with changes in src/lib.rs and tests/test_lib.rs
    When I run crap4rs --diff <baseline> --exclude "tests/**" --coverage lcov.info
    Then the output includes functions from src/lib.rs
    And the output does not include functions from tests/test_lib.rs

  @unwired
  Scenario: --diff composes with --only-failing as AND
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with changed functions, some below and some above threshold
    When I run crap4rs --diff <baseline> --only-failing --coverage lcov.info
    Then only functions that are both changed AND exceed the threshold appear

  # ── JSON Envelope ─────────────────────────────────────────────────

  @unwired
  Scenario: JSON output includes diff_ref when --diff is used
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with changes
    When I run crap4rs --diff main --format json --coverage lcov.info
    Then the JSON envelope contains "diff_ref" with value "main"

  @unwired
  Scenario: JSON output has null diff_ref when --diff is not used
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    When I run crap4rs --format json --coverage lcov.info
    Then the JSON envelope contains "diff_ref" with value null

  # ── Error Handling ────────────────────────────────────────────────

  @unwired
  Scenario: Error when not inside a git repository
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given I am not inside a git repository
    When I run crap4rs --diff main --coverage lcov.info
    Then the exit code is 2
    And stderr contains "not inside a git work tree"

  @unwired
  Scenario: Error when ref is invalid
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo
    When I run crap4rs --diff nonexistent-ref-xyz --coverage lcov.info
    Then the exit code is 2
    And stderr contains an error about the invalid ref

  @unwired
  Scenario: Ref starting with dash is rejected
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    When I run crap4rs --diff --malicious-flag --coverage lcov.info
    Then the exit code is 2
    And stderr contains "invalid" or the ref is not passed to git

  # ── Edge Cases ────────────────────────────────────────────────────

  @unwired
  Scenario: Non-Rust file changes are ignored
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with changes in both src/lib.rs and README.md
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then the output includes functions from src/lib.rs
    And no functions appear from README.md

  @unwired
  Scenario: Renamed file includes functions from new path
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo where src/old.rs was renamed to src/new.rs with modifications
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then the output includes functions from src/new.rs with the new path

  @unwired
  Scenario: Deletion-only changes do not surface functions
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo where the only change is deleting lines from a function
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then the output does not include the function with only deleted lines

  @unwired
  Scenario: Path normalization matches between git diff and complexity data
    # tracked: crap-rs#169 — diff-mode cucumber harness not yet built
    Given a git repo with changes in a nested file src/sub/mod.rs
    When I run crap4rs --diff <baseline> --coverage lcov.info
    Then functions from src/sub/mod.rs appear correctly in the output
    And their file paths match between the diff filter and the CRAP report
