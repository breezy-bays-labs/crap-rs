import type { CoveragePort, CoverageParseResult } from "../../ports/coverage-port.js";
import type {
  FunctionCoverage,
  CoverageRatio,
  SourceSpan,
} from "../../domain/types.js";

// ── Istanbul JSON types ────────────────────────────────────────────

interface IstanbulPosition {
  readonly line: number;
  readonly column: number;
}

interface IstanbulRange {
  readonly start: IstanbulPosition;
  readonly end: IstanbulPosition;
}

interface IstanbulFnEntry {
  readonly name: string;
  readonly decl: IstanbulRange;
  readonly loc: IstanbulRange;
}

interface IstanbulBranchEntry {
  readonly type: string;
  readonly loc: IstanbulRange;
  readonly locations: readonly IstanbulRange[];
}

interface IstanbulFileCoverage {
  readonly path: string;
  readonly fnMap: Record<string, IstanbulFnEntry>;
  readonly f: Record<string, number>;
  readonly statementMap: Record<string, IstanbulRange>;
  readonly s: Record<string, number>;
  readonly branchMap: Record<string, IstanbulBranchEntry>;
  readonly b: Record<string, readonly number[]>;
}

// ── Helpers ────────────────────────────────────────────────────────

function isWithinSpan(range: IstanbulRange, fnLoc: IstanbulRange): boolean {
  // A statement/branch is within the function if its start line is >= the
  // function start line and its end line is <= the function end line.
  // For same-line boundaries, also check column positions.
  if (range.start.line < fnLoc.start.line) return false;
  if (range.end.line > fnLoc.end.line) return false;
  if (
    range.start.line === fnLoc.start.line &&
    range.start.column < fnLoc.start.column
  )
    return false;
  if (
    range.end.line === fnLoc.end.line &&
    range.end.column > fnLoc.end.column
  )
    return false;
  return true;
}

function computeLineCoverage(
  statementMap: Record<string, IstanbulRange>,
  s: Record<string, number>,
  fnLoc: IstanbulRange,
): CoverageRatio {
  let total = 0;
  let covered = 0;
  for (const key of Object.keys(statementMap)) {
    const stmt = statementMap[key];
    if (stmt && isWithinSpan(stmt, fnLoc)) {
      total++;
      if ((s[key] ?? 0) > 0) {
        covered++;
      }
    }
  }
  return {
    covered,
    total,
    percent: total === 0 ? 100 : Math.round((covered / total) * 10000) / 100,
  };
}

function computeBranchCoverage(
  branchMap: Record<string, IstanbulBranchEntry>,
  b: Record<string, readonly number[]>,
  fnLoc: IstanbulRange,
): CoverageRatio | null {
  let total = 0;
  let covered = 0;
  for (const key of Object.keys(branchMap)) {
    const branch = branchMap[key];
    if (branch && isWithinSpan(branch.loc, fnLoc)) {
      const counts = b[key];
      if (counts) {
        for (const count of counts) {
          total++;
          if (count > 0) {
            covered++;
          }
        }
      }
    }
  }
  if (total === 0) return null;
  return {
    covered,
    total,
    percent: Math.round((covered / total) * 10000) / 100,
  };
}

function findLongestCommonPrefix(paths: readonly string[]): string {
  if (paths.length === 0) return "";
  if (paths.length === 1) {
    // Use the directory portion of the single path
    const lastSlash = paths[0]!.lastIndexOf("/");
    return lastSlash >= 0 ? paths[0]!.slice(0, lastSlash + 1) : "";
  }

  const sorted = [...paths].sort();
  const first = sorted[0]!;
  const last = sorted[sorted.length - 1]!;

  let i = 0;
  while (i < first.length && i < last.length && first[i] === last[i]) {
    i++;
  }

  // Trim back to last directory separator
  const prefix = first.slice(0, i);
  const lastSlash = prefix.lastIndexOf("/");
  return lastSlash >= 0 ? prefix.slice(0, lastSlash + 1) : "";
}

function normalizePath(absolutePath: string, prefix: string): string {
  let relative = absolutePath;
  if (prefix && absolutePath.startsWith(prefix)) {
    relative = absolutePath.slice(prefix.length);
  }
  // Ensure forward slashes and strip leading slash
  return relative.replace(/\\/g, "/").replace(/^\/+/, "");
}

// ── Adapter ────────────────────────────────────────────────────────

export class IstanbulCoverageAdapter implements CoveragePort {
  private readonly cwd: string | undefined;

  constructor(cwd?: string) {
    this.cwd = cwd;
  }

  parse(data: unknown, _sources?: ReadonlyMap<string, string>): CoverageParseResult {
    if (data === null || typeof data !== "object" || Array.isArray(data)) {
      throw new Error(
        "Invalid Istanbul coverage data: expected an object keyed by file paths",
      );
    }

    const record = data as Record<string, unknown>;
    const entries = Object.entries(record).filter(
      ([, v]) => v !== null && typeof v === "object" && !Array.isArray(v),
    ) as [string, IstanbulFileCoverage][];

    // Determine prefix to strip
    const prefix = this.cwd
      ? this.cwd.replace(/\\/g, "/").replace(/\/?$/, "/")
      : findLongestCommonPrefix(entries.map(([key]) => key));

    const result = new Map<string, FunctionCoverage[]>();

    for (const [, fileCov] of entries) {
      const relativePath = normalizePath(fileCov.path, prefix);
      const functions: FunctionCoverage[] = [];

      // Process fnMap entries in key order (numeric)
      const fnKeys = Object.keys(fileCov.fnMap).sort(
        (a, b) => Number(a) - Number(b),
      );

      for (const fnKey of fnKeys) {
        const fnEntry = fileCov.fnMap[fnKey];
        if (!fnEntry?.loc?.start || !fnEntry.loc.end) continue;

        const fnLoc = fnEntry.loc;

        // Convert inclusive endLine to exclusive (domain convention)
        const span: SourceSpan = {
          startLine: fnLoc.start.line,
          startColumn: fnLoc.start.column,
          endLine: fnLoc.end.line + 1,
          endColumn: fnLoc.end.column,
        };

        const lineCoverage = computeLineCoverage(
          fileCov.statementMap,
          fileCov.s,
          fnLoc,
        );

        const branchCoverage = computeBranchCoverage(
          fileCov.branchMap,
          fileCov.b,
          fnLoc,
        );

        functions.push({
          filePath: relativePath,
          name: fnEntry.name,
          span,
          lineCoverage,
          branchCoverage,
        });
      }

      result.set(relativePath, functions);
    }

    return { coverage: result, warnings: [] };
  }
}
