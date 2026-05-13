// W1.2 LogicalOperator fixture: one function exercising `&&`, one
// exercising `||`. Each scores cyclomatic=2 with exactly one
// `logical-operator` contributor. `??` (nullish-coalescing) is
// intentionally absent — that's W2.1's job.
export function bothTruthy(a: boolean, b: boolean): boolean {
  return a && b;
}

export function eitherTruthy(a: boolean, b: boolean): boolean {
  return a || b;
}
