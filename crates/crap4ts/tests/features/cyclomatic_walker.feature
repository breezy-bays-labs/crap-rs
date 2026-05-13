Feature: Cyclomatic complexity for TypeScript via the oxc walker
  As a TypeScript developer running crap4ts on my codebase
  I want cyclomatic complexity scores that match my intuition about decision points
  So that I can identify functions that need refactoring or more tests

  Background:
    # Cyclomatic complexity counts decision points. Per ADR D5, crap4ts defaults
    # to --metric cyclomatic with threshold 16. Per ADR D8, risk-cutoffs are
    # metric-invariant (5/8/30). Decision points map to ContributorKind
    # variants already in crap-core's domain (no new variants for TS).

  @unwired
  Scenario: A function with no decision points scores complexity 1
    # tracked: crap-rs#173 — W1.2 baseline; harness lands in W3.3
    Given a TypeScript source file containing:
      """
      export function greet(name: string): string {
        return `hello, ${name}`;
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `greet` with cyclomatic complexity 1
    And no contributors are emitted for `greet`

  @unwired
  Scenario: An if/else branch increments complexity by 1
    # tracked: crap-rs#173 — W1.2 IfBranch contributor; harness lands in W3.3
    Given a TypeScript source file containing:
      """
      export function classify(x: number): string {
        if (x < 0) return "neg";
        return "non-neg";
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `classify` with cyclomatic complexity 2
    And the contributors include one `if-branch` at line 2

  @unwired
  Scenario Outline: Each universal decision-point construct contributes 1
    # tracked: crap-rs#173 — W1.2 universal decision points; harness lands in W3.3
    Given a TypeScript source file containing the construct `<construct>`
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the function's cyclomatic complexity is `<base + 1>`
    And the contributors include exactly one `<kind>` entry

    Examples:
      | construct                              | base + 1 | kind             |
      | `if (cond) { … }`                      | 2        | if-branch        |
      | `for (let i = 0; i < n; i++) { … }`    | 2        | for-loop         |
      | `for (const x of xs) { … }`            | 2        | for-loop         |
      | `for (const k in obj) { … }`           | 2        | for-loop         |
      | `while (cond) { … }`                   | 2        | while-loop       |
      | `do { … } while (cond)`                | 2        | do-while-loop    |
      | `switch (x) { case 1: … }`             | 2        | case-branch      |
      | `a && b`                               | 2        | logical-operator |
      | `a \|\| b`                             | 2        | logical-operator |

  @unwired
  Scenario Outline: TypeScript-specific decision points add to cyclomatic
    # tracked: crap-rs#173 — W2.1 TS-specific decision points; harness lands in W3.3
    Given a TypeScript source file containing the construct `<construct>`
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the function's cyclomatic complexity is `<base + 1>`
    And the contributors include exactly one `<kind>` entry

    Examples:
      | construct                              | base + 1 | kind             |
      | `cond ? a : b`                         | 2        | ternary          |
      | `obj?.field`                           | 2        | optional-chain   |
      | `obj?.method()`                        | 2        | optional-chain   |
      | `a ?? fallback`                        | 2        | logical-operator |
      | `try { … } catch (e) { … }`            | 2        | catch            |

  @unwired
  Scenario: JSX conditional rendering decomposes through logical-operator
    # tracked: crap-rs#173 — W2.1 JSX patterns reuse existing logical-operator variant
    Given a TypeScript JSX source file containing:
      """
      function Greeting({ visible, name }: { visible: boolean, name: string }) {
        return <div>{visible && <span>hello, {name}</span>}</div>;
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the function's cyclomatic complexity is 2
    And the JSX conditional is counted via the existing `logical-operator` contributor
    And the contributors list contains exactly one entry of kind `logical-operator`

  @unwired
  Scenario: Nested functions are tracked as their own complexity sites
    # tracked: crap-rs#173 — W1.2 function discovery semantics; harness lands in W3.3
    Given a TypeScript source file containing:
      """
      export function outer(xs: number[]) {
        const inner = (x: number) => x > 0 ? x : -x;
        return xs.map(inner);
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `outer` with cyclomatic complexity 1
    And the report includes function `inner` with cyclomatic complexity 2
    And `inner`'s contributors include one `ternary` entry

  @unwired
  Scenario: A chained ternary contributes one decision point per `?`
    # tracked: crap-rs#173 — W2.1 chained-construct stacking grounds ADR (a); harness lands in W3.3
    Given a TypeScript source file containing:
      """
      export function classify(x: number): string {
        return x < 0 ? "neg" : x === 0 ? "zero" : "pos";
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `classify` with cyclomatic complexity 3
    And the contributors include exactly two `ternary` entries

  @unwired
  Scenario: A chained logical-operator expression contributes one per operator
    # tracked: crap-rs#173 — W2.1 chained-construct stacking grounds ADR (a); harness lands in W3.3
    Given a TypeScript source file containing:
      """
      export function allTruthy(a: any, b: any, c: any, d: any): boolean {
        return a && b && c && d;
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `allTruthy` with cyclomatic complexity 4
    And the contributors include exactly three `logical-operator` entries

  @unwired
  Scenario: Nested if-statements contribute one per if-branch (no flattening)
    # tracked: crap-rs#173 — W1.2 nested-if stacking grounds ADR (a); harness lands in W3.3
    Given a TypeScript source file containing:
      """
      export function deep(a: number, b: number): string {
        if (a > 0) {
          if (b > 0) return "both positive";
        }
        return "default";
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `deep` with cyclomatic complexity 3
    And the contributors include exactly two `if-branch` entries

  @unwired
  Scenario: Compound construct `if (a && b)` counts the `if` AND the `&&` (no skipping)
    # tracked: crap-rs#173 — W1.2 compound-construct stacking; harness lands in W3.3
    # Per CQO BDD audit — outline tests isolation only; this scenario catches the
    # failure mode where the walker visits the `if` but skips into-`&&`-descent.
    Given a TypeScript source file containing:
      """
      export function both(a: boolean, b: boolean): string {
        if (a && b) return "both";
        return "not-both";
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `both` with cyclomatic complexity 3
    And the contributors include exactly one `if-branch` entry
    And the contributors include exactly one `logical-operator` entry

  @unwired
  Scenario: Risk classification is metric-invariant per ADR D8
    # tracked: crap-rs#173 — W1.2 risk bucket parity with crap4rs; harness lands in W3.3
    Given a TypeScript function with cyclomatic complexity 4 and coverage 50%
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the function's CRAP score is 6.0
    And the function's risk classification is `acceptable`
