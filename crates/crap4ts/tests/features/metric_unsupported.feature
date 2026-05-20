Feature: --metric cognitive returns a helpful error in crap4ts@2.0.0
  As a TypeScript developer who tried `--metric cognitive` on crap4ts
  I want a clear, actionable error message
  So that I know to switch to `--metric cyclomatic` (the default) without re-reading the manual

  @wired
  Scenario: --metric cognitive on crap4ts exits non-zero with direct guidance
    Given a TypeScript source tree under `src/`
    And a valid Istanbul `coverage-final.json`
    When the operator runs `crap4ts --coverage coverage-final.json --src src --metric cognitive`
    Then `crap4ts` exits with status 2
    And the user-facing error reads exactly:
      """
      crap4ts: complexity metric `cognitive` is not yet supported. Use `--metric cyclomatic` (the default for crap4ts) or track support at https://github.com/breezy-bays-labs/crap-rs.
      """

  @wired
  Scenario: --metric cyclomatic on crap4ts is the default and works
    Given a TypeScript source tree under `src/`
    And a valid Istanbul `coverage-final.json`
    When the operator runs `crap4ts --coverage coverage-final.json --src src --metric cyclomatic`
    Then `crap4ts` produces a complete CRAP scorecard
    And no MetricNotSupported error is emitted

  @unwired
  Scenario: crap4rs's --metric cognitive default continues to work (sanity)
    # tracked: crap-rs#229 — this crap4rs-shelling scenario stays deferred: a crap4ts cucumber
    # harness shelling the crap4rs bin re-triggers the crap-rs#224 mutants-baseline class
    # (CARGO_BIN_EXE_crap4rs unset under `cargo mutants --package crap4ts`). Contract stays
    # pinned by the mutants-skipped metric_unsupported_smoke.rs::crap4rs_no_flag_default_cognitive_still_works.
    Given a Rust source tree under `src/`
    And a valid LCOV `lcov.info`
    When the operator runs `crap4rs --coverage lcov.info --src src --metric cognitive`
    Then `crap4rs` produces a complete CRAP scorecard
    And no MetricNotSupported error is emitted

  @wired
  Scenario: The MetricNotSupported error uses metric Display format, not Debug
    When the binary renders the MetricNotSupported error for metric `Cognitive`
    Then the rendered metric name in the user message is `cognitive` (lowercase, matching CLI input)
    And the rendered metric name is NOT `Cognitive` (PascalCase, Debug format)

  @wired
  Scenario: --metric an-unknown-value exits with clap's standard error (not MetricNotSupported)
    When the operator runs `crap4ts --coverage cov.json --src src --metric halstead`
    Then `crap4ts` exits with status 2
    And the error originates from clap's argument validation, NOT from MetricNotSupported
    And the error names the valid `--metric` values
