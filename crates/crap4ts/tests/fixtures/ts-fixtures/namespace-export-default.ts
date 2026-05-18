// INTENTIONALLY MALFORMED TypeScript. `export default` inside a
// `namespace` is a type error (TS1063 "An export assignment cannot be
// used in a namespace"), and two `export default`s in one file is
// independently invalid. `tsc`/Biome reject this; oxc — a lenient
// parser — still emits two `ExportDefaultDeclaration` AST nodes for it.
// crap4ts is a complexity analyzer, not a type checker: it analyzes
// whatever oxc emits, and real codebases contain type errors. This
// fixture pins crap4ts's behavior on that lenient-parse AST: a
// default-exported declaration reached through the namespace path must
// still carry the namespace prefix (`Api.handler` / `Svc.Repo.find`),
// not fall through to the bare top-level export-default path
// (`handler` / `Repo.find`). It is split across two namespaces only so
// the two distinct arms (function default, class default) are both
// exercised in one file.
namespace Api {
  export default function handler(n: number): number {
    if (n > 0) {
      return n;
    }
    return 0;
  }
}

namespace Svc {
  export default class Repo {
    find(id: number): number {
      if (id > 0) {
        return id;
      }
      return -1;
    }
  }
}
