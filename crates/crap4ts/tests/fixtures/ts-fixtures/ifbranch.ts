// W1.2 IfBranch fixture: one if/else, one decision point.
export function classify(x: number): string {
  if (x < 0) {
    return "neg";
  }
  return "non-neg";
}
