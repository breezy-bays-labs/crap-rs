// crap-rs#224: TS expression wrappers and `yield` are walked so a
// decision point inside the wrapped expression still scores. Each
// function below wraps one ternary; the wrapper must recurse into its
// payload for the ternary to be counted. Covers the `visit_expression`
// arms for YieldExpression, TSTypeAssertion, TSNonNullExpression,
// TSInstantiationExpression, ImportExpression, TSSatisfiesExpression.
export function* yieldExpr(flag: boolean): Generator<number> {
  yield flag ? 1 : 2;
}

export function typeAssertion(flag: boolean): number {
  return <number>(flag ? 1 : 2);
}

export function nonNull(flag: boolean): number {
  return (flag ? 1 : 2)!;
}

export function instantiation(flag: boolean): unknown {
  return make(flag ? 1 : 2).build<string>;
}

export async function dynImport(flag: boolean): Promise<unknown> {
  return import(flag ? "./a" : "./b");
}

export function satisfiesExpr(flag: boolean): number {
  return (flag ? 1 : 2) satisfies number;
}
