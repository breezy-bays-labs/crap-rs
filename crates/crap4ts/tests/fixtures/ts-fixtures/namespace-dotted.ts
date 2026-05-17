// Dotted-namespace continuation: `namespace A.B` parses as `A` whose
// module body is the nested declaration `B`. `f` must qualify with the
// full dotted path `A.B.f`, and its `if` must charge `A.B.f` (not
// module scope, not a partial `B.f`).
namespace A.B {
  export function f(n: number): number {
    if (n > 0) {
      return n;
    }
    return 0;
  }
}
