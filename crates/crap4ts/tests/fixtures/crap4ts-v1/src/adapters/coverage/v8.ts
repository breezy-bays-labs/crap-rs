import type { CoveragePort, CoverageParseResult } from "../../ports/coverage-port.js";
import type {
  FunctionCoverage,
  CoverageRatio,
  SourceSpan,
  Warning,
} from "../../domain/types.js";

// ── V8 Coverage JSON types ────────────────────────────────────────

interface V8Range {
  readonly startOffset: number;
  readonly endOffset: number;
  readonly count: number;
}

interface V8FunctionCoverage {
  readonly functionName: string;
  readonly ranges: readonly V8Range[];
  readonly isBlockCoverage: boolean;
}

interface V8ScriptCoverage {
  readonly scriptId: string;
  readonly url: string;
  readonly functions: readonly V8FunctionCoverage[];
}

// ── Constants ─────────────────────────────────────────────────────

/** Approximate characters per line for Tier 3 fallback. */
const APPROX_CHARS_PER_LINE = 40;

// ── Line Offset Table (Tier 2) ───────────────────────────────────

/**
 * Build a table of cumulative byte offsets for each line boundary.
 * table[i] = byte offset of the start of line (i+1).
 * table[0] = 0 (line 1 starts at byte 0).
 */
export function buildLineOffsetTable(source: string): number[] {
  const table: number[] = [0];
  for (let i = 0; i < source.length; i++) {
    if (source[i] === "\n") {
      table.push(i + 1);
    }
  }
  return table;
}

/**
 * Convert a byte offset to a 1-based line number using binary search
 * on a line offset table.
 */
export function byteOffsetToLineFromTable(
  offset: number,
  table: readonly number[],
): number {
  let lo = 0;
  let hi = table.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >>> 1;
    if (table[mid]! <= offset) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }
  return lo + 1; // 1-based
}

// ── Tier 3 Fallback ──────────────────────────────────────────────

function byteOffsetToLineApprox(offset: number): number {
  return Math.max(1, Math.ceil(offset / APPROX_CHARS_PER_LINE));
}

// ── Helpers ───────────────────────────────────────────────────────

function stripFileProtocol(url: string): string {
  if (url.startsWith("file:///")) {
    const stripped = url.slice("file:///".length);
    // On Unix, paths start with / (e.g. file:///home/...) → restore leading /
    // On Windows, paths start with drive letter (e.g. file:///C:/...) → no leading /
    if (/^[a-zA-Z]:/.test(stripped)) {
      return stripped;
    }
    return "/" + stripped;
  }
  return url;
}

function findLongestCommonPrefix(paths: readonly string[]): string {
  if (paths.length === 0) return "";
  if (paths.length === 1) {
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

  const prefix = first.slice(0, i);
  const lastSlash = prefix.lastIndexOf("/");
  return lastSlash >= 0 ? prefix.slice(0, lastSlash + 1) : "";
}

function normalizePath(absolutePath: string, prefix: string): string {
  let relative = absolutePath;
  if (prefix && absolutePath.startsWith(prefix)) {
    relative = absolutePath.slice(prefix.length);
  }
  return relative.replace(/\\/g, "/").replace(/^\/+/, "");
}

function computeByteCoverage(ranges: readonly V8Range[]): CoverageRatio {
  const outer = ranges[0]!;
  const totalBytes = outer.endOffset - outer.startOffset;

  if (totalBytes === 0) {
    return { covered: 0, total: 0, percent: 100 };
  }

  // Sum bytes from ranges with count === 0
  let uncoveredBytes = 0;
  for (const range of ranges) {
    if (range.count === 0) {
      uncoveredBytes += range.endOffset - range.startOffset;
    }
  }

  const coveredBytes = totalBytes - uncoveredBytes;
  const percent =
    Math.round((coveredBytes / totalBytes) * 10000) / 100;

  return { covered: coveredBytes, total: totalBytes, percent };
}

function computeSpan(
  outerRange: V8Range,
  table: readonly number[] | null,
): SourceSpan {
  if (table) {
    const startLine = byteOffsetToLineFromTable(outerRange.startOffset, table);
    const endLine = Math.max(
      startLine + 1,
      byteOffsetToLineFromTable(outerRange.endOffset, table) + 1,
    );
    return { startLine, startColumn: 0, endLine, endColumn: 0 };
  }

  // Tier 3: approximation
  const startLine = byteOffsetToLineApprox(outerRange.startOffset);
  const endLine = Math.max(
    startLine + 1,
    Math.ceil(outerRange.endOffset / APPROX_CHARS_PER_LINE) + 1,
  );
  return { startLine, startColumn: 0, endLine, endColumn: 0 };
}

// ── Adapter ───────────────────────────────────────────────────────

export class V8CoverageAdapter implements CoveragePort {
  private readonly cwd: string | undefined;

  constructor(cwd?: string) {
    this.cwd = cwd;
  }

  parse(
    data: unknown,
    sources?: ReadonlyMap<string, string>,
  ): CoverageParseResult {
    const scripts = this.validateAndExtractScripts(data);

    // Resolve absolute paths from URLs
    const absolutePaths = scripts.map((s) => stripFileProtocol(s.url));

    // Determine prefix to strip
    const prefix = this.cwd
      ? this.cwd.replace(/\\/g, "/").replace(/\/?$/, "/")
      : findLongestCommonPrefix(absolutePaths);

    const result = new Map<string, FunctionCoverage[]>();
    const warnings: Warning[] = [];
    const warnedFiles = new Set<string>();

    for (let i = 0; i < scripts.length; i++) {
      const script = scripts[i]!;
      const relativePath = normalizePath(absolutePaths[i]!, prefix);
      const { functions, warning } = this.processScript(script, relativePath, sources, warnedFiles);
      result.set(relativePath, functions);
      if (warning) warnings.push(warning);
    }

    return { coverage: result, warnings };
  }

  private validateAndExtractScripts(data: unknown): V8ScriptCoverage[] {
    if (data === null || typeof data !== "object" || Array.isArray(data)) {
      throw new Error(
        "Invalid V8 coverage data: expected an object with a 'result' array",
      );
    }

    const v8Data = data as Record<string, unknown>;

    if (!Array.isArray(v8Data.result)) {
      throw new Error(
        "Invalid V8 coverage data: expected an object with a 'result' array",
      );
    }

    return (v8Data.result as V8ScriptCoverage[]).filter(
      (s) => s && typeof s === "object" && typeof s.url === "string" && Array.isArray(s.functions),
    );
  }

  private processScript(
    script: V8ScriptCoverage,
    relativePath: string,
    sources: ReadonlyMap<string, string> | undefined,
    warnedFiles: Set<string>,
  ): { functions: FunctionCoverage[]; warning: Warning | null } {
    const sourceContent = sources?.get(relativePath) ?? null;
    const lineTable = sourceContent
      ? buildLineOffsetTable(sourceContent)
      : null;

    const needsApproximationWarning =
      !lineTable &&
      !warnedFiles.has(relativePath) &&
      script.functions.some((fn) => fn.functionName && fn.ranges.length > 0);

    let warning: Warning | null = null;
    if (needsApproximationWarning) {
      warning = {
        code: "approximate-span",
        message: `Source content unavailable for "${relativePath}" — using approximate byte-to-line conversion`,
        file: relativePath,
      };
      warnedFiles.add(relativePath);
    }

    const functions: FunctionCoverage[] = [];
    for (const fn of script.functions) {
      if (!fn.functionName || fn.ranges.length === 0) continue;

      const outerRange = fn.ranges[0]!;
      functions.push({
        filePath: relativePath,
        name: fn.functionName,
        span: computeSpan(outerRange, lineTable),
        lineCoverage: computeByteCoverage(fn.ranges),
        branchCoverage: null,
      });
    }

    return { functions, warning };
  }
}
