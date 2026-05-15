import type {
  BreakdownMode,
  ComplexityContributor,
  FunctionVerdict,
} from "./types.js";

export function selectContributors(
  verdict: FunctionVerdict,
  mode: BreakdownMode,
): ReadonlyArray<ComplexityContributor> {
  switch (mode) {
    case "off":
      return [];
    case "all":
      return verdict.scored.contributors;
    case "exceeding":
      return verdict.exceeds ? verdict.scored.contributors : [];
  }
}
