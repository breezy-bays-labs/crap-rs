export type CoverageFormat = "istanbul" | "v8" | "unknown";

export function detectCoverageFormat(data: unknown): CoverageFormat {
  // V8: top-level array of script entries
  if (Array.isArray(data)) {
    return "v8";
  }

  if (typeof data !== "object" || data === null) {
    return "unknown";
  }

  const obj = data as Record<string, unknown>;

  // V8: object with `result` array
  if ("result" in obj && Array.isArray(obj.result)) {
    return "v8";
  }

  // Istanbul: object keyed by file paths, each value has fnMap/statementMap/branchMap
  const keys = Object.keys(obj);
  if (keys.length > 0) {
    const firstValue = obj[keys[0]!];
    if (
      typeof firstValue === "object" &&
      firstValue !== null &&
      "fnMap" in firstValue
    ) {
      return "istanbul";
    }
  }

  return "unknown";
}
