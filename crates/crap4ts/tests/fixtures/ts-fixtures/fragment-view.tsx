// crap-rs#224: a JSX fragment's children are walked via the
// `visit_expression` JSXFragment arm, so a `{cond && <x/>}` conditional
// inside `<>...</>` contributes its LogicalOperator decision point.
export function FragmentView(show: boolean) {
  return <>{show && <span />}</>;
}
