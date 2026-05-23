Feature: Arrow-function coverage accuracy
  As a developer working in modern TypeScript codebases (React, Svelte, functional patterns)
  I want crap4ts to count my arrow-function executions correctly
  So that my CRAP scores reflect what's actually exercised by my test suite

  @wired
  Scenario: An invoked arrow function has matching coverage
    # Fixed in crap-rs#252 — `IstanbulCoverage::line_coverage_for` now
    # collapses multi-statement-per-line via MIN, so an uninvoked
    # single-line arrow's body hit count survives the declaration's
    # module-load hit (`min(1, 0) = 0`).
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

  @wired
  Scenario: A useCallback-style arrow has matching coverage
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
    And the report shows the useCallback arrow with line coverage 100.0

  @wired
  Scenario: An array.map(arrow) covers the inner arrow
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

  @wired
  Scenario: Mixed function-expression / arrow / declared-function bodies in one file
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
