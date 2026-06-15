Feature: --format advice (issue #76)

  The advice format emits the canonical JSON envelope with each
  over-threshold `FunctionVerdict` carrying a populated `Diagnostic`:
  AST-derived `coverage_gaps`, `complexity_drivers`, `suggested_actions`,
  and a flat `root_cause` scalar. Primary consumer is the `/cut-the-crap`
  agent skill (#77); secondary consumers are CI/SARIF and humans.

  This file pins the CLI-process contracts the running binary uniquely
  captures: the canonical envelope on stdout, diagnostic gating (over-
  threshold verdicts carry the four-field diagnostic; under-threshold
  ones omit the key) end-to-end through the real walker + coverage +
  diagnostic engine, exit-code parity with `--format json`, the gate
  keystone (`--no-fail` reports findings but flips only the exit code),
  diagnostics surviving view shaping (`--top`), stdout/stderr stream
  separation, and byte-determinism. The diagnostic *content* — the
  SuggestedAction taxonomy, ProposedSplit shape + de-dup priority,
  `root_cause` derivation, the no-prose/no-names invariant — is owned by
  `crap-core`'s `domain::diagnostic` unit + proptest suite; the exact
  stderr line format by `adapters::reporters::advice_summary`; the
  view sort/filter shaping by `domain::view`; and `--explain` (a
  `--breakdown` sub-feature) by the complexity_breakdown harness. So
  those cases live there, not here (see `AGENTS.md` § BDD hygiene). Step
  defs in `tests/format_advice_cucumber.rs`. Absorbs the
  (binary-shelling, zero-lib-coverage) `format_advice_integration.rs`.

  Background:
    Given a project with a mix of over-threshold and under-threshold functions

  # ── Envelope shape ─────────────────────────────────────────────────

  @wired
  Scenario: --format advice emits the canonical envelope on stdout
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --no-gitignore --no-fail --format advice`
    Then stdout is parseable JSON
    And the JSON value at "schema_version" is 2
    And the JSON path "view.shown" is an array
    And stdout is JSON-only with no table borders or prose

  @wired
  Scenario: --format advice exit code matches --format json
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --no-gitignore --format advice`
    Then the exit code is 1
    And the exit code equals the same command under --format json

  # ── Diagnostic gating (R6.4 / F2) ──────────────────────────────────

  @wired
  Scenario: Over-threshold verdicts carry a four-field Diagnostic, under-threshold ones omit it
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --no-gitignore --no-fail --format advice`
    Then every over-threshold entry carries a populated diagnostic
    And every under-threshold entry omits the diagnostic key

  # ── Gate keystone (R2.1) ───────────────────────────────────────────

  @wired
  Scenario: --no-fail reports findings but only flips the exit code
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --no-gitignore --no-fail --format advice`
    Then the exit code is 0
    And every over-threshold entry carries a populated diagnostic

  # ── Composition with View shaping (R2.1) ───────────────────────────

  @wired
  Scenario: Diagnostics survive view truncation under --top
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --no-gitignore --no-fail --format advice --top 2`
    Then the JSON value at "view.shown" has 2 entries
    And every over-threshold entry carries a populated diagnostic

  # ── Stream separation (R5.2 / S-8) ─────────────────────────────────

  @wired
  Scenario: --format advice emits one stderr summary line per over-threshold function
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --no-gitignore --no-fail --format advice`
    Then stdout is parseable JSON
    And stderr carries one "[crap=" summary line per over-threshold function

  @wired
  Scenario: --format json carries no diagnostics and no stderr summary
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --no-gitignore --no-fail --format json`
    Then no view.shown entry carries a diagnostic key
    And stderr carries no "[crap=" summary line

  # ── Determinism (R4.1 / G1) ────────────────────────────────────────

  @wired
  Scenario: Same input produces byte-identical advice stdout and stderr
    When the operator runs `crap4rs --coverage lcov.info --src src --threshold 8 --no-gitignore --no-fail --format advice`
    Then running the same command again produces byte-identical stdout and stderr
