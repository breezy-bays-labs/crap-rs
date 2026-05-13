// W1.2 nested-function fixture: per CQO BDD audit + W1.2 advisor
// guidance, this uses two NAMED function declarations (no arrow,
// no ternary — those are W2.1 territory). The outer function's
// decision points must NOT bleed into the inner function's score
// and vice versa: each function is its own complexity site.
//
// Expected: outer scores cyclomatic=2 (one IfBranch) and inner
// scores cyclomatic=2 (one IfBranch).
export function outer(x: number): string {
  function inner(y: number): string {
    if (y > 0) {
      return "pos";
    }
    return "non-pos";
  }
  if (x > 0) {
    return inner(x);
  }
  return "outer-non-pos";
}
