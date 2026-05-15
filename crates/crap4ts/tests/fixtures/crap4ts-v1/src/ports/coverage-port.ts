import type { FunctionCoverage, Warning } from "../domain/types.js";

export interface CoverageParseResult {
  readonly coverage: ReadonlyMap<string, ReadonlyArray<FunctionCoverage>>;
  readonly warnings: ReadonlyArray<Warning>;
}

export interface CoveragePort {
  /**
   * Parse coverage data into per-file function coverage entries.
   *
   * @param data - Raw coverage data in a format recognized by the implementing adapter.
   * @param sources - Optional source text keyed by file path. When provided,
   *   adapters that use byte-offset ranges (e.g. V8) can perform precise
   *   byte-offset-to-line mapping, eliminating approximate-span warnings.
   *   Adapters whose format already contains line-level data ignore this
   *   parameter — it is accepted for interface uniformity.
   */
  parse(
    data: unknown,
    sources?: ReadonlyMap<string, string>,
  ): CoverageParseResult;
}
