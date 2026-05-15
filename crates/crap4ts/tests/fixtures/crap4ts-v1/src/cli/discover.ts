import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  detectCoverageFormat,
  type CoverageFormat,
} from "../adapters/coverage/detect.js";

// ── Filesystem Abstraction (for testing) ────────────────────────────

export interface DiscoverFs {
  exists: (path: string) => boolean;
  readFile: (path: string) => string;
}

const defaultFs: DiscoverFs = {
  exists: existsSync,
  readFile: (path: string) => readFileSync(path, "utf-8"),
};

// ── Coverage Probe Paths (in priority order) ────────────────────────

const COVERAGE_PROBE_PATHS = [
  join("coverage", "coverage-final.json"),
  join(".nyc_output", "coverage-final.json"),
  join("coverage", "coverage-v8.json"),
] as const;

// ── discoverCoverage ────────────────────────────────────────────────

export interface DiscoveredCoverage {
  path: string;
  format: CoverageFormat;
}

/**
 * Probes common coverage output paths in order and returns the first
 * valid coverage file found with its detected format.
 *
 * Returns null if no coverage data is found.
 */
export function discoverCoverage(
  cwd: string,
  fs: DiscoverFs = defaultFs,
): DiscoveredCoverage | null {
  for (const relativePath of COVERAGE_PROBE_PATHS) {
    const fullPath = join(cwd, relativePath);

    if (!fs.exists(fullPath)) {
      continue;
    }

    try {
      const content = fs.readFile(fullPath);
      const data: unknown = JSON.parse(content);
      const format = detectCoverageFormat(data);

      if (format !== "unknown") {
        return { path: fullPath, format };
      }
    } catch {
      // File is unreadable or contains invalid JSON — skip to next candidate
      continue;
    }
  }

  return null;
}

// ── discoverSourceRoot ──────────────────────────────────────────────

/**
 * Discovers the project's source root directory using these heuristics:
 *
 * 1. Reads tsconfig.json → compilerOptions.rootDir
 * 2. Reads tsconfig.json → include[0] (extracts base directory from glob)
 * 3. Falls back to `./src` if the directory exists
 * 4. Falls back to cwd
 */
export function discoverSourceRoot(
  cwd: string,
  fs: DiscoverFs = defaultFs,
): string {
  const tsconfigPath = join(cwd, "tsconfig.json");

  if (fs.exists(tsconfigPath)) {
    try {
      const content = fs.readFile(tsconfigPath);
      const tsconfig = JSON.parse(content) as TsconfigShape;

      // Try compilerOptions.rootDir first
      const rootDir = tsconfig.compilerOptions?.rootDir;
      if (rootDir) {
        const resolved = join(cwd, rootDir);
        if (fs.exists(resolved)) {
          return resolved;
        }
      }

      // Try include[0] — extract base directory from glob pattern
      const firstInclude = tsconfig.include?.[0];
      if (firstInclude) {
        const baseDir = extractBaseDir(firstInclude);
        if (baseDir) {
          const resolved = join(cwd, baseDir);
          if (fs.exists(resolved)) {
            return resolved;
          }
        }
      }
    } catch {
      // Invalid tsconfig — fall through to heuristics
    }
  }

  // Heuristic: check for ./src
  const srcDir = join(cwd, "src");
  if (fs.exists(srcDir)) {
    return srcDir;
  }

  // Last resort: use cwd itself
  return cwd;
}

// ── formatCoverageNotFoundError ─────────────────────────────────────

/**
 * Produces a user-friendly error message listing all probed paths
 * and actionable guidance for resolving the issue.
 */
export function formatCoverageNotFoundError(cwd: string): string {
  const probedList = COVERAGE_PROBE_PATHS.map(
    (p) => `  - ${p}`,
  ).join("\n");

  return [
    `No coverage data found in ${cwd}`,
    "",
    "Searched these paths:",
    probedList,
    "",
    "To fix this:",
    "  1. Run your test suite with coverage enabled (e.g., vitest run --coverage)",
    "  2. Or specify the coverage file explicitly with --coverage <path>",
  ].join("\n");
}

// ── Internal Helpers ────────────────────────────────────────────────

interface TsconfigShape {
  compilerOptions?: { rootDir?: string };
  include?: string[];
}

/**
 * Extracts the leading non-glob directory from a pattern.
 * e.g. "src/**​/*.ts" → "src", "*.ts" → null
 */
function extractBaseDir(pattern: string): string | null {
  const segments = pattern.split("/");
  const statics: string[] = [];

  for (const segment of segments) {
    if (segment.includes("*") || segment.includes("{") || segment.includes("?")) {
      break;
    }
    statics.push(segment);
  }

  return statics.length > 0 ? statics.join("/") : null;
}
