Feature: Istanbul JSON coverage parsing
  As a TypeScript developer running crap4ts with jest, vitest, or nyc
  I want crap4ts to consume my coverage-final.json out of the box
  So that I don't have to translate or pre-process my coverage output

  Background:
    # Istanbul JSON is the schema emitted by jest, vitest, and nyc.
    # Per Resolved Q10 (shaping), crap4ts@2.0.0 tolerates all three emitters;
    # unknown emitters surface a ParseDiagnostic rather than aborting.
    # The CoveragePort signature is `parse(&self, data: &str)` (slurp at
    # orchestrator boundary, parse via serde_json::from_str). A separate
    # `validate(&Path)` pre-flight checks structural sanity before parse.

  @unwired
  Scenario: A jest-emitted coverage-final.json parses cleanly
    # tracked: crap-rs#173 — W1.1 jest fixture baseline; harness lands in W3.3
    Given a jest-emitted Istanbul `coverage-final.json` covering 3 source files
    And the report's source root resolves all 3 file paths to discovered sources
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report attributes line coverage to all 3 files
    And no warnings or diagnostics are emitted for the coverage input

  @unwired
  Scenario: A vitest-emitted coverage-final.json parses with the same shape
    # tracked: crap-rs#173 — W2.4 vitest variance fixture; harness lands in W3.3
    Given a vitest-emitted Istanbul `coverage-final.json` covering 3 source files
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report attributes line coverage to all 3 files
    And no warnings or diagnostics are emitted for the coverage input

  @unwired
  Scenario: An nyc-emitted coverage-final.json parses with the same shape
    # tracked: crap-rs#173 — W2.4 nyc variance fixture; harness lands in W3.3
    Given an nyc-emitted Istanbul `coverage-final.json` covering 3 source files
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report attributes line coverage to all 3 files
    And no warnings or diagnostics are emitted for the coverage input

  @unwired
  Scenario: A coverage entry whose path cannot resolve to a source file emits PathUnresolved
    # tracked: crap-rs#173 — W2.4 path-mismatch contract per CPO #3; harness lands in W3.3
    Given an Istanbul `coverage-final.json` with one entry pointing at `/private/build/transpiled/foo.js`
    And no source file resolves to that path under `--src src`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the diagnostics section of the report contains one entry for that unresolved path
    And the diagnostic's kind is `path-unresolved`
    And the diagnostic's message mentions the unresolved path
    And the scorecard still produces line coverage for the OTHER entries (never abort first-record)

  @unwired
  Scenario: A JSON file that is not Istanbul-shaped emits SchemaUnrecognized
    # tracked: crap-rs#173 — W2.4 schema-unrecognized contract per CPO sharpening; harness lands in W3.3
    Given a `coverage.json` whose top-level shape is `{ "foo": "bar" }` (not Istanbul)
    When the operator runs `crap4ts --coverage coverage.json --src src`
    Then `crap4ts` exits with a non-zero status
    And the user-facing error message names the problem ("top-level shape not recognized as Istanbul")
    And the message hints at the expected shape `{[path]: { path, s, statementMap, … }}`
    And the JSON envelope's diagnostic record carries kind `schema-unrecognized`

  @unwired
  Scenario: A branch-record references an unknown branchId — emits BranchMismatch
    # tracked: crap-rs#173 — W2.3 + W2.4 branch coverage mismatch UX; harness lands in W3.3
    Given a `coverage-final.json` whose `b` record references branchId `42`
    And `branchMap` contains no entry for branchId `42`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the diagnostics section of the report contains one entry for that branch
    And the diagnostic's kind is `branch-mismatch`
    And the diagnostic's message redirects the user to "the coverage tool's issue tracker"

  @unwired
  Scenario: validate() pre-flight catches an empty coverage file before parse
    # tracked: crap-rs#173 — W1.1 validate() impl per port trait docstring; harness lands in W3.3
    Given a `coverage-final.json` that decodes as Istanbul JSON but every entry's `statementMap` is empty
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then `crap4ts` exits with a non-zero status before reaching the parse pass
    And the user-facing error explains "no statement coverage records"
    And the error message tells the user how to regenerate coverage (e.g., "run jest with --coverage")

  @unwired
  Scenario: A coverage entry with extra unknown fields is ignored permissively
    # tracked: crap-rs#173 — W1.1 unknown-field tolerance for jest/vitest/nyc metadata; harness lands in W3.3
    Given a jest-emitted `coverage-final.json` with `hash` and `contentHash` fields
    And no other deviation from the expected schema
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the parser produces a `ParseOutput` containing line coverage for the entries
    And no `ParseDiagnostic` records are emitted for the unknown fields

  @unwired
  Scenario: Relative paths in the coverage report are resolved against --src
    # tracked: crap-rs#173 — W2.4 relative-path normalization; harness lands in W3.3
    Given an Istanbul `coverage-final.json` whose entries use relative paths like `src/foo.ts`
    And the operator invokes `crap4ts --coverage coverage-final.json --src /home/me/project/src`
    When the parser normalizes the entry paths
    Then the paths resolve against `/home/me/project/src`
    And the normalized paths in the output are workspace-relative
