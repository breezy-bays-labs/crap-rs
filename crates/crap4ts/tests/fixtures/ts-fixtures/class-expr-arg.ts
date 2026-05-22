// crap-rs#224: a class expression in expression position (here a call
// argument) is walked via the `visit_expression` ClassExpression arm,
// so its methods become their own complexity sites.
export function classExprArg(flag: boolean): void {
  register(
    class Widget {
      render(): number {
        return flag ? 1 : 2;
      }
    },
  );
}
