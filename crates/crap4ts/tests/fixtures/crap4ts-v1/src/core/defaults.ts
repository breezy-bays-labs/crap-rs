import { TypeScriptEslintComplexityAdapter } from "../adapters/complexity/typescript-eslint.js";
import { createAutoDetectCoveragePort } from "../adapters/coverage/facade.js";
import { defaultSpanMatcher } from "../domain/matching.js";
import picomatch from "picomatch";
import { readFile, readdir } from "node:fs/promises";
import { join, relative } from "node:path";
import type { AnalyzeDeps } from "./deps.js";

// ── File Finding ──────────────────────────────────────────────────

async function findFilesRecursive(
  patterns: string[],
  options: { cwd: string; exclude: string[] },
): Promise<string[]> {
  const matchers = patterns.map((p) => picomatch(p));
  const excludeMatchers = options.exclude.map((p) => picomatch(p));

  const files: string[] = [];

  async function walk(dir: string): Promise<void> {
    const entries = await readdir(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = join(dir, entry.name);
      const relPath = relative(options.cwd, fullPath).split("\\").join("/");

      if (excludeMatchers.some((m) => m(relPath))) {
        continue;
      }

      if (entry.isDirectory()) {
        await walk(fullPath);
      } else if (entry.isFile() && matchers.some((m) => m(relPath))) {
        files.push(relPath);
      }
    }
  }

  await walk(options.cwd);
  return files.sort();
}

// ── Default Dependencies ──────────────────────────────────────────

export function createDefaultDeps(cwd?: string): AnalyzeDeps {
  return {
    complexityPort: new TypeScriptEslintComplexityAdapter(),
    coveragePort: createAutoDetectCoveragePort(cwd),
    matcher: defaultSpanMatcher,
    globMatcher: (path: string, pattern: string) => picomatch(pattern)(path),
    readFile: async (path: string) => readFile(path, "utf-8"),
    readJson: async (path: string) => {
      const content = await readFile(path, "utf-8");
      return JSON.parse(content) as unknown;
    },
    findFiles: findFilesRecursive,
  };
}
