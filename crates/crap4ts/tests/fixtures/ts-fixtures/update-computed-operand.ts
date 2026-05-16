// #200 item 4 — UpdateExpression on a computed-member operand whose
// index expression embeds a nested function. `counters[(() => {
// ... })()]++` — the IIFE arrow must be discovered as its own
// FunctionComplexity (with its own `if`), separate from `bump`.
function bump(counters: Record<string, number>, flag: boolean): void {
  counters[
    (() => {
      if (flag) {
        return "a";
      }
      return "b";
    })()
  ]++;
}
