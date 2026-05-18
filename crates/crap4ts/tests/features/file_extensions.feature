Feature: TypeScript file extension discovery and parsing
  As a TypeScript developer with a polyglot frontend codebase
  I want crap4ts to discover and parse my .ts, .tsx, .jsx, .mjs, and .cjs files
  So that I don't have to pre-filter the file list or write a custom discovery script

  @unwired
  Scenario Outline: Files with supported extensions are discovered and parsed
    # tracked: crap-rs#229 — cucumber harness deferred from W3.3 #191 (pre-GA)
    Given a source tree under `src/` containing a single file `example<extension>` with contents `<content>`
    And a valid Istanbul `coverage-final.json` covering that file
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then `crap4ts` exits with status 0 (no parse errors)
    And the report includes at least one function from `example<extension>`

    Examples:
      | extension | content                                                                              |
      | .ts       | `export function greet(name: string): string { return `hello ${name}`; }`            |
      | .tsx      | `export const Greet = ({name}: {name: string}) => <span>hi {name}</span>;`           |
      | .js       | `export function greet(name) { return 'hello ' + name; }`                            |
      | .jsx      | `export const Greet = ({name}) => <span>hi {name}</span>;`                           |
      | .mjs      | `export function greet(name) { return 'hello ' + name; }`                            |
      | .cjs      | `module.exports.greet = function(name) { return 'hello ' + name; };`                 |

  @unwired
  Scenario: A .d.ts file is skipped by default (declaration-only, no executable code)
    # tracked: crap-rs#229 — cucumber harness deferred from W3.3 #191 (pre-GA)
    Given a source tree under `src/` containing `types.d.ts` and `app.ts`
    And the operator's `crap4ts.toml` does NOT explicitly include `.d.ts`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes functions from `app.ts`
    And the report does NOT include entries from `types.d.ts`

  @unwired
  Scenario: A .test.ts file is included unless excluded via crap4ts.toml
    # tracked: crap-rs#229 — cucumber harness deferred from W3.3 #191 (pre-GA)
    Given a source tree under `src/` containing `app.ts` and `app.test.ts`
    And the operator's `crap4ts.toml` has no exclusion for `.test.ts`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes functions from both `app.ts` and `app.test.ts`

  @unwired
  Scenario: A .test.ts file is excluded when crap4ts.toml lists it in excludes
    # tracked: crap-rs#229 — cucumber harness deferred from W3.3 #191 (pre-GA)
    Given a source tree under `src/` containing `app.ts` and `app.test.ts`
    And the operator's `crap4ts.toml` has `excludes = ["**/*.test.ts"]`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes functions from `app.ts`
    And the report does NOT include entries from `app.test.ts`

  @unwired
  Scenario: An unrecognized extension is silently skipped
    # tracked: crap-rs#229 — cucumber harness deferred from W3.3 #191 (pre-GA)
    Given a source tree under `src/` containing `app.ts` and `notes.txt`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes functions from `app.ts`
    And the report does NOT mention `notes.txt`
    And no diagnostic is emitted about `notes.txt`

  @unwired
  Scenario: A parser failure on one file does not abort the run for others
    # tracked: crap-rs#229 — cucumber harness deferred from W3.3 #191 (pre-GA)
    # Note: verified via crap-core/src/core/mod.rs:286-310 — orchestrator catches per-file
    # ComplexityPort::extract errors and increments AnalysisDiagnostics.files_unparseable
    # before continuing. crap4ts inherits this behavior automatically.
    Given a source tree under `src/` containing `good.ts` (parses cleanly) and `broken.ts` (syntactically invalid)
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then `crap4ts` still produces a scorecard for functions in `good.ts`
    And the diagnostics section reports `broken.ts` as unparseable
    And `AnalysisDiagnostics.files_unparseable` equals 1
    And the run exits with non-zero status ONLY if threshold violations gate it (not because of parse failure alone)
