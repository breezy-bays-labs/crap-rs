// Class declared inside a namespace. The class methods must carry the
// namespace prefix in front of the class qualifier: `Svc.Repo.find`,
// not `Repo.find`. A namespace-level function alongside it qualifies as
// `Svc.helper`.
namespace Svc {
  export function helper(): void {}

  export class Repo {
    find(id: number): number {
      if (id > 0) {
        return id;
      }
      return 0;
    }
  }
}
