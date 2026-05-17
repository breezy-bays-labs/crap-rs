// Function-valued `const` bindings inside a namespace. The bare form
// (`const bare = …`) is reached through the direct namespace-statement
// path; the exported form (`export const exported = …`) is reached
// through the namespace `export` declaration path. Both must be
// discovered with the namespace prefix: `Calc.bare`, `Calc.exported`.
namespace Calc {
  const bare = (n: number): number => {
    if (n > 0) {
      return n;
    }
    return 0;
  };

  export const exported = function (n: number): number {
    if (n < 0) {
      return -n;
    }
    return n;
  };
}
