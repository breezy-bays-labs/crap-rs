Feature: --metric cognitive returns a helpful error in crap4ts@2.0.0
  As a TypeScript developer who tried `--metric cognitive` on crap4ts
  I want a clear, actionable error message
  So that I know to switch to `--metric cyclomatic` (the default) without re-reading the manual

  Background:
    # crap4ts@2.0.0 ships --metric cyclomatic only. Cognitive complexity for
    # TS is deferred to a follow-up issue (per shaping resolved Q2). When a
    # user requests cognitive, the walker returns a metric-named CrapError
    # variant (MetricNotSupported per CAO blocking finding), and the binary's
    # error path renders adapter-specific phrasing from AdapterMeta.
    # The user message uses Display format (not Debug) and gives direct
    # guidance, not "run --help" indirection (per CPO sharpening).

  @unwired
  Scenario: --metric cognitive on crap4ts exits non-zero with direct guidance
    # tracked: crap-rs#173 — W2.5 metric-not-supported error UX; harness lands in W3.3
    Given a TypeScript source tree under `src/`
    And a valid Istanbul `coverage-final.json`
    When the operator runs `crap4ts --coverage coverage-final.json --src src --metric cognitive`
    Then `crap4ts` exits with status 2
    And the user-facing error reads exactly:
      """
      crap4ts: complexity metric `cognitive` is not yet supported. Use `--metric cyclomatic` (the default for crap4ts) or track support at https://github.com/breezy-bays-labs/crap-rs.
      """

  @unwired
  Scenario: --metric cyclomatic on crap4ts is the default and works
    # tracked: crap-rs#173 — W2.5 default-metric happy path; harness lands in W3.3
    Given a TypeScript source tree under `src/`
    And a valid Istanbul `coverage-final.json`
    When the operator runs `crap4ts --coverage coverage-final.json --src src --metric cyclomatic`
    Then `crap4ts` produces a complete CRAP scorecard
    And no MetricNotSupported error is emitted

  @unwired
  Scenario: crap4rs's --metric cognitive default continues to work (sanity)
    # tracked: crap-rs#173 — W2.5 cross-adapter check: only crap4ts is affected; harness lands in W3.3
    Given a Rust source tree under `src/`
    And a valid LCOV `lcov.info`
    When the operator runs `crap4rs --coverage lcov.info --src src --metric cognitive`
    Then `crap4rs` produces a complete CRAP scorecard
    And no MetricNotSupported error is emitted

  @unwired
  Scenario: The MetricNotSupported error uses metric Display format, not Debug
    # tracked: crap-rs#173 — W2.5 message rendering; harness lands in W3.3
    When the binary renders the MetricNotSupported error for metric `Cognitive`
    Then the rendered metric name in the user message is `cognitive` (lowercase, matching CLI input)
    And the rendered metric name is NOT `Cognitive` (PascalCase, Debug format)

  @unwired
  Scenario: --metric an-unknown-value exits with clap's standard error (not MetricNotSupported)
    # tracked: crap-rs#173 — W2.5 invalid-metric is distinct from unsupported-metric; harness lands in W3.3
    When the operator runs `crap4ts --coverage cov.json --src src --metric halstead`
    Then `crap4ts` exits with status 2
    And the error originates from clap's argument validation, NOT from MetricNotSupported
    And the error names the valid `--metric` values
