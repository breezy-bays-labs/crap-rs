// crap-rs#224: the arguments of an optional call (`obj?.run(...)`) are
// walked via `visit_chain_element`, so a nested arrow passed as an
// argument is discovered as its own function with its own score.
export function optionalCallArg(
  obj: { run?: (cb: () => void) => void },
  x: boolean,
): void {
  obj?.run(() => {
    if (x) {
      return;
    }
  });
}
