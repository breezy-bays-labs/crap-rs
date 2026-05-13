// W1.2 Switch fixture: one switch with a single case. Per
// `cyclomatic_walker.feature` outline row "switch (x) { case 1: ... }",
// this should score cyclomatic=2 with exactly one `case-branch`
// contributor (the `default:` arm is NOT counted — it's not a decision
// point, it's the fallthrough).
export function describe(x: number): string {
  switch (x) {
    case 1:
      return "one";
    default:
      return "other";
  }
}
