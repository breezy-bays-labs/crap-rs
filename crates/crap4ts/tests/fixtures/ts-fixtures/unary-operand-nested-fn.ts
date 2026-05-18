// `discovered` is reachable ONLY through the `typeof` operand: the
// initializer is a UnaryExpression whose argument wraps the function.
// The walker's `Expression::UnaryExpression` arm recurses into that
// argument; deleting it drops the unary node to the no-op `_ => {}`
// fallthrough and the nested function is never found. This fixture
// pins that arm deterministically so a single targeted test kills the
// mutant regardless of proptest generation.
const probe = typeof (function discovered(n: number): number {
  if (n > 0) {
    return n;
  }
  return 0;
});
