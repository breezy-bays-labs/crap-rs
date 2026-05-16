// #205 (class side) — computed class PropertyDefinition key embedding
// an IIFE arrow function. `[(() => "x")()]` is a computed key; the
// arrow must be discovered as its own FunctionComplexity, separate
// from the class's regular method `regular`.
class Widget {
  [(() => {
    return "dynamicKey";
  })()]: number = 0;

  regular(n: number): number {
    if (n > 0) {
      return n;
    }
    return 0;
  }
}
