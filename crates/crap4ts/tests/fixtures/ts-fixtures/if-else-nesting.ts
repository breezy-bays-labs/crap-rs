// crap-rs#224: nesting_depth recording across an if / else-if / else
// ladder. `visit_if` dispatches an `else if` (alternate is itself an
// IfStatement) at the SAME depth as the chain head, but a plain `else`
// body one level deeper. A regression in either dispatch surfaces as a
// wrong `nesting_depth` on a contributor.
export function classifyDepth(a: number, b: number): number {
  if (a > 10) {
    return 1;
  } else if (b > 10) {
    return 2;
  } else {
    if (a < b) {
      return 3;
    }
    return 4;
  }
}
