// crap-rs#224: the interpolated expressions of a template literal are
// walked via the `visit_expression` TemplateLiteral arm, so a decision
// point inside a `${...}` placeholder is counted.
export function templateInterp(flag: boolean): string {
  return `result: ${flag ? "a" : "b"}`;
}
