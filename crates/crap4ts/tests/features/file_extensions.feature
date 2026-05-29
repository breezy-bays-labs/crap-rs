Feature: TypeScript file extension discovery and parsing
  As a TypeScript developer with a polyglot frontend codebase
  I want crap4ts to discover and parse my .ts, .tsx, .jsx, .mjs, and .cjs files
  So that I don't have to pre-filter the file list or write a custom discovery script

  @wired
  Scenario Outline: Files with supported extensions are discovered and parsed
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

  @wired
  Scenario: A .d.ts file is skipped by default (declaration-only, no executable code)
    # crap-rs#253 — `**/*.d.ts` is in `AdapterMeta::forced_excludes` for crap4ts,
    # so the source-discovery walker drops declaration files before they reach
    # the AST walker. There is no opt-out: declaration files contribute zero
    # useful CRAP signal (ambient types only, no executable code).
    Given a source tree under `src/` containing `types.d.ts` and `app.ts`
    And the operator's `crap.toml` does NOT explicitly include `.d.ts`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes functions from `app.ts`
    And the report does NOT include entries from `types.d.ts`

  @wired
  Scenario: A .test.ts file is included unless excluded via crap.toml
    Given a source tree under `src/` containing `app.ts` and `app.test.ts`
    And the operator's `crap.toml` has no exclusion for `.test.ts`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes functions from both `app.ts` and `app.test.ts`

  @wired
  Scenario: A .test.ts file is excluded when crap.toml lists it in excludes
    Given a source tree under `src/` containing `app.ts` and `app.test.ts`
    And the operator's `crap.toml` has `exclude = ["**/*.test.ts"]`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes functions from `app.ts`
    And the report does NOT include entries from `app.test.ts`

  @wired
  Scenario: An unrecognized extension is silently skipped
    Given a source tree under `src/` containing `app.ts` and `notes.txt`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report includes functions from `app.ts`
    And the report does NOT mention `notes.txt`
    And no diagnostic is emitted about `notes.txt`

  @wired
  Scenario: A parser failure on one file does not abort the run for others
    # Note: verified via crap-core/src/core/mod.rs — the orchestrator catches per-file
    # ComplexityPort::extract errors and increments AnalysisDiagnostics.files_unparseable
    # before continuing. crap4ts inherits this behavior automatically.
    Given a source tree under `src/` containing `good.ts` (parses cleanly) and `broken.ts` (syntactically invalid)
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then `crap4ts` still produces a scorecard for functions in `good.ts`
    And the diagnostics section reports `broken.ts` as unparseable
    And `AnalysisDiagnostics.files_unparseable` equals 1
    And the run exits with non-zero status ONLY if threshold violations gate it (not because of parse failure alone)
