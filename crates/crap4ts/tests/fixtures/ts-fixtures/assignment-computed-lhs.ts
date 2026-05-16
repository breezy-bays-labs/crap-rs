// #200 item 3 — computed-member assignment LHS embedding a nested
// function. `target[(() => { ... })()]` has an IIFE arrow in the
// index expression; the arrow must be discovered as its own
// FunctionComplexity (with its own `if`), separate from `assign`.
function assign(target: Record<string, number>, flag: boolean): void {
  target[
    (() => {
      if (flag) {
        return "yes";
      }
      return "no";
    })()
  ] = 1;
}
