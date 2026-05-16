// gemini PR #220 review (#200 item 4 follow-up) — an UpdateExpression
// on a STATIC-member operand whose member *object* is an IIFE
// embedding a nested function: `(() => { if … })().prop++`. The arrow
// must be discovered as its own FunctionComplexity (with its own
// `if`), separate from `bumpStatic`. Pre-fix the walker only recursed
// the COMPUTED-member operand and dropped the static-member object.
function bumpStatic(flag: boolean): void {
  (() => {
    const o: { prop: number } = { prop: 0 };
    if (flag) {
      o.prop = 1;
    }
    return o;
  })().prop++;
}
