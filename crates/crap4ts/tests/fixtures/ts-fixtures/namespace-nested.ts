// Block-nested namespaces: `namespace A { namespace B { … } }`. The
// inner `namespace B` is a statement inside A's block (a different
// recursion path than the dotted-continuation form), yet the qualified
// name must be the same `A.B.g`. A function declared directly in the
// outer block stays `A.outer`. Shallow qualification: `inner` nested
// inside `g` keeps its bare name (mirrors a function nested in a class
// method).
namespace A {
  export function outer(): void {}

  namespace B {
    export function g(n: number): number {
      if (n > 0) {
        function inner(): void {}
        inner();
        return n;
      }
      return 0;
    }
  }
}
