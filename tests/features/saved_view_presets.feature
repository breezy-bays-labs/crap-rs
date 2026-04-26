Feature: --view saved presets (Bundle D, issue #80)

  CI users and investigators repeat the same flag combinations across
  invocations. Saved view presets in `crap4rs.toml` let projects bake
  those combinations under named keys. `crap4rs --view ci` resolves the
  preset, folds its values into the CLI before validation, and proceeds
  through the existing pipeline. The gate keystone is preserved — a
  preset cannot change `result.passed`; only the displayed view is
  shapeable.

  Background:
    Given a project with `crap4rs.toml` containing:
      """
      [views.ci]
      top = 20
      min_coverage = 0
      max_coverage = 90
      sort = "coverage"
      only_failing = true
      no_fail = false
      group_by = "file"
      minimal_view = true

      [views.investigate]
      sort = "complexity"
      top = 10
      """

  # ── Resolution ─────────────────────────────────────────────────────

  Scenario: --view ci applies every preset field to the view
    When the operator runs `crap4rs --coverage lcov.info --view ci --format json`
    Then `view.spec.limit` is `20`
    And `view.spec.filters.coverage_range` covers `[0, 90]`
    And `view.spec.sort` is `"coverage"`
    And `view.spec.filters.only_failing` is true
    And `view.spec.group_by` is `"file"`
    And `view.shown` is absent (minimal_view applied)

  # ── Override priority ──────────────────────────────────────────────

  Scenario: --top on the CLI overrides the preset's top
    When the operator runs `crap4rs --coverage lcov.info --view ci --top 5 --format json`
    Then `view.spec.limit` is `5`
    And `view.spec.filters.only_failing` is true (other preset fields preserved)

  Scenario: --no-fail OR-merges with the preset
    Given the analysis has threshold violations
    When the operator runs `crap4rs --coverage lcov.info --view ci --no-fail`
    Then the process exits 0 (CLI --no-fail wins)

  Scenario: Multiple presets coexist independently
    When the operator runs `crap4rs --coverage lcov.info --view investigate --format json`
    Then `view.spec.sort` is `"complexity"`
    And `view.spec.limit` is `10`
    And `view.spec.filters.only_failing` is false (preset `investigate` does not assert it)

  # ── Validation errors ──────────────────────────────────────────────

  Scenario: Unknown preset name exits 2 with available list
    When the operator runs `crap4rs --coverage lcov.info --view nonsense`
    Then the process exits 2
    And stderr contains "unknown view preset"
    And stderr contains "ci"
    And stderr contains "investigate"

  Scenario: --view with no crap4rs.toml exits 2 with hint
    Given no `crap4rs.toml` exists
    When the operator runs `crap4rs --coverage lcov.info --view ci`
    Then the process exits 2
    And stderr contains "unknown view preset"
    And stderr contains "crap4rs.toml"

  Scenario: Invalid preset field fails fast at config load
    Given `crap4rs.toml` contains:
      """
      [views.bad]
      max_coverage = 105
      """
    When the operator runs `crap4rs --coverage lcov.info --view bad`
    Then the process exits 2
    And stderr contains "out of range"
    And stderr contains "bad"

  # ── Gate keystone ──────────────────────────────────────────────────

  Scenario: Preset does not change exit code on a failing analysis
    Given the unfiltered analysis would exit 1 (violations exist)
    And the preset `ci` has `no_fail = false`
    When the operator runs `crap4rs --coverage lcov.info --view ci`
    Then the process exits 1
    And `result.passed` is false

  # ── Discoverability ────────────────────────────────────────────────

  Scenario: --help advertises --view
    When the operator runs `crap4rs --help`
    Then the help text mentions "--view"
    And the help text mentions "saved view preset"
