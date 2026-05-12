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

  @unwired
  Scenario: --view ci applies every preset field to the view
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --view ci --format json`
    Then `view.spec.limit` is `20`
    And `view.spec.filters.coverage_range` covers `[0, 90]`
    And `view.spec.sort` is `"coverage"`
    And `view.spec.filters.only_failing` is true
    And `view.spec.group_by` is `"file"`
    And `view.shown` is absent (minimal_view applied)

  # ── Override priority ──────────────────────────────────────────────

  @unwired
  Scenario: --top on the CLI overrides the preset's top
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --view ci --top 5 --format json`
    Then `view.spec.limit` is `5`
    And `view.spec.filters.only_failing` is true (other preset fields preserved)

  @unwired
  Scenario: --no-fail OR-merges with the preset
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
    Given the analysis has threshold violations
    When the operator runs `crap4rs --coverage lcov.info --view ci --no-fail`
    Then the process exits 0 (CLI --no-fail wins)

  @unwired
  Scenario: Multiple presets coexist independently
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --view investigate --format json`
    Then `view.spec.sort` is `"complexity"`
    And `view.spec.limit` is `10`
    And `view.spec.filters.only_failing` is false (preset `investigate` does not assert it)

  # ── Validation errors ──────────────────────────────────────────────

  @unwired
  Scenario: Unknown preset name exits 2 with available list
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
    When the operator runs `crap4rs --coverage lcov.info --view nonsense`
    Then the process exits 2
    And stderr contains "unknown view preset"
    And stderr contains "ci"
    And stderr contains "investigate"

  @unwired
  Scenario: --view with no crap4rs.toml exits 2 with hint
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
    Given no `crap4rs.toml` exists
    When the operator runs `crap4rs --coverage lcov.info --view ci`
    Then the process exits 2
    And stderr contains "unknown view preset"
    And stderr contains "crap4rs.toml"

  @unwired
  Scenario: Invalid preset field fails fast at config load
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
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

  @unwired
  Scenario: Preset does not change exit code on a failing analysis
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
    Given the unfiltered analysis would exit 1 (violations exist)
    And the preset `ci` has `no_fail = false`
    When the operator runs `crap4rs --coverage lcov.info --view ci`
    Then the process exits 1
    And `result.passed` is false

  # ── Discoverability ────────────────────────────────────────────────

  @unwired
  Scenario: --help advertises --view
    # tracked: crap-rs#169 — saved-view-presets cucumber harness not yet built
    When the operator runs `crap4rs --help`
    Then the help text mentions "--view"
    And the help text mentions "saved view preset"
