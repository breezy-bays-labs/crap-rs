// crap-rs#224: a for-loop's init clause is walked for decision points
// in both shapes — a `let` declaration init (the VariableDeclaration
// arm of `visit_for_init`) and a bare assignment init (routed through
// `for_init_as_expression`).
export function declInit(a: boolean): number {
  let total = 0;
  for (let i = a ? 0 : 5; i < 10; i++) {
    total += i;
  }
  return total;
}

export function bareInit(a: boolean): number {
  let i: number;
  let total = 0;
  for (i = a ? 0 : 5; i < 10; i++) {
    total += i;
  }
  return total;
}
