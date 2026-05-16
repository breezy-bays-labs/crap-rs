// gemini PR #220 review (#200 item 3 follow-up) — a STATIC-member
// assignment LHS whose member *object* is an IIFE embedding a nested
// function: `(() => { if … })().prop = 1`. The arrow must be
// discovered as its own FunctionComplexity (with its own `if`),
// separate from `assignStatic`. Pre-fix the walker only recursed the
// COMPUTED-member case and dropped the static-member object entirely.
function assignStatic(flag: boolean): void {
  (() => {
    const o: { prop: number } = { prop: 0 };
    if (flag) {
      o.prop = 1;
    }
    return o;
  })().prop = 1;
}
