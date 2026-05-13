Feature: Arrow-function coverage accuracy
  As a developer working in modern TypeScript codebases (React, Svelte, functional patterns)
  I want crap4ts to count my arrow-function executions correctly
  So that my CRAP scores reflect what's actually exercised by my test suite

  Background:
    # Promoted from breadboard reflection note #5 to a W1.1 AC per CPO Concern 2.
    # Istanbul records coverage in two parallel tables: `s` (statement counts +
    # statementMap) and `f` (function counts + fnMap). Line-coverage-from-statements
    # may undercount arrow-function invocations because the function-invocation
    # site is in `f`/`fnMap`, not necessarily `s`/`statementMap`. Modern TS
    # codebases are arrow-function-heavy (useCallback, hooks, Svelte stores).
    # If this scenario fails on the W1.1 minimal parser, W2.3 grows to populate
    # coverage from f/fnMap rather than just s/statementMap.
    # CQO BDD audit: all 4 scenarios use concrete docstring fixtures + explicit
    # expected percentages — assertions must distinguish 100% covered from 10%
    # covered (the exact undercount failure mode this feature canary catches).

  @unwired
  Scenario: An invoked arrow function has matching coverage
    # tracked: crap-rs#173 — W1.1 arrow-function fixture sanity; harness lands in W3.3
    Given a TypeScript source file `src/arrow.ts` containing:
      """
      export const square = (x: number) => x * x;
      export const cube = (x: number) => x * x * x;
      """
    And a jest-emitted `coverage-final.json` recording 100 invocations of `square` and zero of `cube`
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report shows function `square` with line coverage 100.0
    And the report shows function `cube` with line coverage 0.0
    And the report does NOT show `square` as 0.0 (would be silent undercount)

  @unwired
  Scenario: A useCallback-style arrow has matching coverage
    # tracked: crap-rs#173 — W1.1 React-idiom arrow-heavy fixture; harness lands in W3.3
    Given a TypeScript source file `src/Button.tsx` containing:
      """
      import { useCallback } from 'react';
      export function Button({ onClick }: { onClick: () => void }) {
        const handle = useCallback(() => { onClick(); }, [onClick]);
        return <button onClick={handle}>Click</button>;
      }
      """
    And a jest-emitted `coverage-final.json` recording `Button` invoked 5 times and the `handle` arrow invoked 5 times (both at 100% line coverage)
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report shows function `Button` with line coverage 100.0
    And the report shows function `handle` (the useCallback arrow) with line coverage 100.0

  @unwired
  Scenario: An array.map(arrow) covers the inner arrow
    # tracked: crap-rs#173 — W1.1 functional-pattern arrow coverage; harness lands in W3.3
    Given a TypeScript source file `src/map.ts` containing:
      """
      export function increment(xs: number[]): number[] {
        return xs.map(x => x + 1);
      }
      """
    And a jest-emitted `coverage-final.json` recording the outer `increment` and the inner arrow each invoked at least once (both at 100% line coverage)
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the inner arrow's line coverage in the report is 100.0
    And `increment`'s CRAP score is computed using the arrow's coverage value

  @unwired
  Scenario: Mixed function-expression / arrow / declared-function bodies in one file
    # tracked: crap-rs#173 — W1.1 mixed-syntax fixture sanity; harness lands in W3.3
    Given a TypeScript source file `src/mixed.ts` containing:
      """
      export function declared() { return 1; }
      export const expression = function() { return 2; };
      export const arrow = () => 3;
      """
    And a jest-emitted `coverage-final.json` recording all three invoked at 100% line coverage
    When the operator runs `crap4ts --coverage coverage-final.json --src src`
    Then the report shows function `declared` with line coverage 100.0
    And the report shows function `expression` with line coverage 100.0
    And the report shows function `arrow` with line coverage 100.0
