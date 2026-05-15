import { selectContributors } from "../domain/contributors.js";
import type { AnalysisResult, BreakdownMode } from "../domain/types.js";

export function prepareForJsonOutput(
  result: AnalysisResult,
  breakdown: BreakdownMode,
): AnalysisResult {
  return {
    ...result,
    functions: result.functions.map((v) => {
      const include = breakdown !== "off" && (breakdown === "all" || v.exceeds);
      const { contributors: _, ...scoredRest } = v.scored;
      return {
        ...v,
        scored: include
          ? { ...scoredRest, contributors: selectContributors(v, breakdown) }
          : scoredRest as typeof v.scored,
      };
    }),
  };
}
