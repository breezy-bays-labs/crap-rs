// A default-exported declaration inside a namespace must carry the
// namespace prefix like every other member — `Api.handler` /
// `Svc.Repo.find`, not the bare `handler` / `Repo.find`. Before the
// fix this arm routed through the unqualified top-level
// export-default path. Function and class defaults live in separate
// namespaces because a namespace may have only one `export default`.
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
