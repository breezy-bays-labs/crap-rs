import type { FunctionFilter, FunctionIdentity } from "./types.js";
import { spansOverlap } from "./matching.js";

/**
 * Returns true if a function should be included based on the filter.
 *
 * - File not in filter map → exclude
 * - File mapped to null → include (whole-file changed)
 * - File mapped to spans → include if any span overlaps function's span
 */
export function shouldInclude(
  filter: FunctionFilter,
  identity: FunctionIdentity,
): boolean {
  const entry = filter.changedFiles.get(identity.filePath);
  if (entry === undefined) return false;
  if (entry === null) return true;
  return entry.some((changedSpan) => spansOverlap(identity.span, changedSpan));
}
