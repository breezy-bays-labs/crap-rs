import { readFile } from "node:fs/promises";
import { IstanbulCoverageAdapter } from "./istanbul.js";
import { V8CoverageAdapter } from "./v8.js";
import { detectCoverageFormat } from "./detect.js";
import type { CoverageFormat } from "./detect.js";
import type { CoveragePort, CoverageParseResult } from "../../ports/coverage-port.js";

// ── API Boundary Errors ───────────────────────────────────────────

export class CoverageParseError extends Error {
  readonly filePath?: string;
  constructor(message: string, filePath?: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "CoverageParseError";
    this.filePath = filePath;
  }
}

export class UnsupportedFormatError extends Error {
  constructor(detail?: string) {
    const base =
      "Unknown coverage format. Expected Istanbul JSON (object with fnMap) or V8 format (array or object with result array).";
    super(detail ? `${base} Got: ${detail}` : base);
    this.name = "UnsupportedFormatError";
  }
}

// ── Types ─────────────────────────────────────────────────────────

type UserFormat = Exclude<CoverageFormat, "unknown">;

export interface ParseCoverageOptions {
  readonly format?: UserFormat;
  readonly sources?: ReadonlyMap<string, string>;
  readonly cwd?: string;
}

export type ParseCoverageResult = CoverageParseResult;

// ── Factory ──────────────────────────────────────────────────────

export function createAutoDetectCoveragePort(
  cwd?: string,
): CoveragePort {
  const istanbul = new IstanbulCoverageAdapter(cwd);
  const v8 = new V8CoverageAdapter(cwd);
  return {
    parse(
      data: unknown,
      sources?: ReadonlyMap<string, string>,
    ): CoverageParseResult {
      const format = detectCoverageFormat(data);
      if (format === "unknown") {
        throw new UnsupportedFormatError();
      }
      const adapter = format === "istanbul" ? istanbul : v8;
      try {
        return adapter.parse(data, sources);
      } catch (error) {
        if (error instanceof CoverageParseError) throw error;
        throw new CoverageParseError("Failed to parse coverage data", undefined, {
          cause: error,
        });
      }
    },
  };
}

// ── Sync: parseCoverage(data, options?) ──────────────────────────

export function parseCoverage(
  data: object,
  options?: ParseCoverageOptions,
): ParseCoverageResult {
  const cwd = options?.cwd;
  const istanbul = new IstanbulCoverageAdapter(cwd);
  const v8 = new V8CoverageAdapter(cwd);

  let format: UserFormat;
  try {
    format = options?.format ?? detectAndValidateFormat(data);
  } catch (error) {
    if (error instanceof UnsupportedFormatError) throw error;
    throw new CoverageParseError("Failed to detect coverage format", undefined, {
      cause: error,
    });
  }

  const adapter = format === "istanbul" ? istanbul : v8;

  try {
    return adapter.parse(data, options?.sources);
  } catch (error) {
    if (error instanceof CoverageParseError) throw error;
    throw new CoverageParseError("Failed to parse coverage data", undefined, {
      cause: error,
    });
  }
}

// ── Async: parseCoverageFile(path, options?) ─────────────────────

export async function parseCoverageFile(
  filePath: string,
  options?: ParseCoverageOptions,
): Promise<ParseCoverageResult> {
  let content: string;
  try {
    content = await readFile(filePath, "utf-8");
  } catch (error) {
    throw new CoverageParseError(
      `Failed to read coverage file: ${filePath}`,
      filePath,
      { cause: error },
    );
  }

  let data: unknown;
  try {
    data = JSON.parse(content);
  } catch (error) {
    throw new CoverageParseError(
      `Coverage file contains invalid JSON: ${filePath}`,
      filePath,
      { cause: error },
    );
  }

  if (data === null || typeof data !== "object") {
    throw new CoverageParseError(
      `Coverage file does not contain a JSON object: ${filePath}`,
      filePath,
    );
  }

  return parseCoverage(data, options);
}

// ── Internal Helpers ──────────────────────────────────────────────

function detectAndValidateFormat(data: unknown): UserFormat {
  const format = detectCoverageFormat(data);
  if (format === "unknown") {
    throw new UnsupportedFormatError();
  }
  return format;
}

// Re-export adapters for advanced usage
export { IstanbulCoverageAdapter } from "./istanbul.js";
export { V8CoverageAdapter } from "./v8.js";
export { detectCoverageFormat } from "./detect.js";
export type { CoverageFormat } from "./detect.js";
