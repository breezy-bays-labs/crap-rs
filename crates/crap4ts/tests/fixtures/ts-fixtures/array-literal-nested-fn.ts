// `discovered` is reachable ONLY through an array-literal element: the
// initializer is an ArrayExpression whose sole element wraps the
// function. The walker's `Expression::ArrayExpression` arm forwards to
// `visit_array_elements`, which maps each element through
// `array_element_as_expression` before recursing. Deleting that arm,
// stubbing `visit_array_elements` to a no-op, or making
// `array_element_as_expression` return `None` each drop the function
// to the no-op fallthrough so it is never found. One fixture pins all
// three sites deterministically regardless of proptest generation.
const probe = [function discovered(n: number): number {
  if (n > 0) {
    return n;
  }
  return 0;
}];
