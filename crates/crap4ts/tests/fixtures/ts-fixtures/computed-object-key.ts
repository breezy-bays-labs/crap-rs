// #200 item 1 — computed object property key embedding a decision
// point. `[a && b]` is a computed key whose `&&` is a LogicalOperator
// that must charge the enclosing function `build`. A spread property
// is included to confirm spread handling is unchanged.
function build(a: boolean, b: boolean, rest: object): object {
  const obj = {
    [a && b]: 1,
    ...rest,
  };
  return obj;
}
