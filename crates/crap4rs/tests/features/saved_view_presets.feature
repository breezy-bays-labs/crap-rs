Feature: --view saved presets (Bundle D, issue #80)

  CI users and investigators repeat the same flag combinations across
  invocations. Saved view presets in `crap.toml` let projects bake
  those combinations under named keys. `crap4rs --view ci` resolves the
  preset, folds its values into the CLI before validation, and proceeds
  through the existing pipeline. The gate keystone is preserved — a
  preset cannot change `result.passed`; only the displayed view is
  shapeable.

  This file pins the CLI-process contracts the running binary uniquely
  captures: a preset on disk is discovered, resolved, applied, and
  reflected in the `view.spec` envelope; a CLI flag overrides the
  preset's value through the pipeline; resolution and config-load errors
  surface as exit 2; and a preset never moves the gate. The preset
  *merge logic* (every-field application, per-field CLI override, the
  bool OR-merge, the unknown-preset / no-config message text) is owned
  by `crap-core`'s `cli::view_args` unit suite, and the TOML
  parsing/validation (multiple-preset independence, coverage-range
  rejection) by `adapters::config` units — so those cases live there,
  not here (see `AGENTS.md` § BDD hygiene). Step defs in
  `tests/saved_view_presets_cucumber.rs`. Absorbs the (binary-shelling,
  zero-lib-coverage) `saved_view_presets_integration.rs`.

  Background:
    Given a project with `crap.toml` containing:
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

  # ── Resolution: preset on disk → view.spec envelope ────────────────

  @wired
  Scenario: --view ci folds every preset field into the view and minimal_view elides view.shown
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --no-gitignore --no-fail --view ci --format json`
    Then the exit code is 0
    And the JSON value at "view.spec.limit" is 20
    And the JSON value at "view.spec.sort" is "coverage"
    And the JSON value at "view.spec.group_by" is "file"
    And the JSON value at "view.spec.filters.only_failing" is true
    And the JSON value at "view.spec.filters.coverage_range.min" is 0
    And the JSON value at "view.spec.filters.coverage_range.max" is 90
    And the JSON path "view.shown" is absent

  # ── Override priority: CLI wins through the pipeline ────────────────

  @wired
  Scenario: A CLI flag overrides the preset's value through the pipeline
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --no-gitignore --no-fail --view ci --top 5 --format json`
    Then the JSON value at "view.spec.limit" is 5
    And the JSON value at "view.spec.filters.only_failing" is true

  # ── Validation: resolution + config-load errors exit 2 ─────────────

  @wired
  Scenario: Unknown preset name exits 2 and lists the available presets
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --no-gitignore --no-fail --view nonsense`
    Then the exit code is 2
    And stderr contains "unknown view preset"
    And stderr contains "ci"
    And stderr contains "investigate"

  @wired
  Scenario: An out-of-range preset field exits 2 at config load
    Given the config file instead contains:
      """
      [views.bad]
      max_coverage = 105
      """
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --no-gitignore --no-fail --view bad`
    Then the exit code is 2
    And stderr contains "out of range"
    And stderr contains "bad"

  # ── Gate keystone: a preset never moves the gate ───────────────────

  @wired
  Scenario: A preset cannot change the gate on a failing analysis
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 5 --no-gitignore --view ci --format json`
    Then the exit code is 1
    And the JSON value at "result.passed" is false

  # ── Discoverability ────────────────────────────────────────────────

  @wired
  Scenario: --help advertises --view
    When the operator runs `crap4rs --help`
    Then stdout contains "--view"
    And stdout contains "saved view preset"
