// #200 item 2 — TS namespace with a nested function declaration.
// `bar` must be discovered as its own FunctionComplexity, and its
// `if` must charge `bar` (not leak to module scope).
namespace Foo {
  export function bar(n: number): number {
    if (n > 0) {
      return n;
    }
    return 0;
  }
}

// Declaration-only ambient module — `body` is None. Must be handled
// cleanly (no panic, no synthetic function discovered).
declare module "side-effect-only";
