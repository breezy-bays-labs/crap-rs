// Single-function happy-path fixture for W1.1 + W1.2 smoke tests.
// Shared between Istanbul parser and oxc walker harnesses per CQO ADVISORY-8.
export function add(a: number, b: number): number {
  return a + b;
}
