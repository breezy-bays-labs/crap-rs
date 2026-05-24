Feature: Cyclomatic complexity for TypeScript via the oxc walker
  As a TypeScript developer running crap4ts on my codebase
  I want cyclomatic complexity scores that match my intuition about decision points
  So that I can identify functions that need refactoring or more tests

  @wired
  Scenario: A function with no decision points scores complexity 1
    Given a TypeScript source file containing:
      """
      export function greet(name: string): string {
        return `hello, ${name}`;
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `greet` with cyclomatic complexity 1
    And no contributors are emitted for `greet`

  @wired
  Scenario: An if/else branch increments complexity by 1
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

  @wired
  Scenario Outline: Each universal decision-point construct contributes 1
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

  @wired
  Scenario Outline: TypeScript-specific decision points add to cyclomatic
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

  @wired
  Scenario: JSX conditional rendering decomposes through logical-operator
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

  @wired
  Scenario: Nested functions are tracked as their own complexity sites
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

  @wired
  Scenario: A chained ternary contributes one decision point per `?`
    Given a TypeScript source file containing:
      """
      export function classify(x: number): string {
        return x < 0 ? "neg" : x === 0 ? "zero" : "pos";
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `classify` with cyclomatic complexity 3
    And the contributors include exactly two `ternary` entries

  @wired
  Scenario: A chained logical-operator expression contributes one per operator
    Given a TypeScript source file containing:
      """
      export function allTruthy(a: any, b: any, c: any, d: any): boolean {
        return a && b && c && d;
      }
      """
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the report includes function `allTruthy` with cyclomatic complexity 4
    And the contributors include exactly three `logical-operator` entries

  @wired
  Scenario: Nested if-statements contribute one per if-branch (no flattening)
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

  @wired
  Scenario: Compound construct `if (a && b)` counts the `if` AND the `&&` (no skipping)
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

  @wired
  Scenario: Risk classification is metric-invariant
    Given a TypeScript function with cyclomatic complexity 4 and coverage 50%
    When the operator runs `crap4ts --coverage cov.json --src .`
    Then the function's CRAP score is 6.0
    And the function's risk classification is `low`
