import { realpathSync } from "node:fs";
import { basename, dirname, isAbsolute, relative, resolve } from "node:path";

const GLOB_META_RE = /[*?[\]{}]/;

function normalizeSlashes(value: string): string {
  return value.replace(/\\/g, "/");
}

function stripLeadingDotSlash(value: string): string {
  return value.replace(/^\.\//, "");
}

export function normalizeProjectPath(
  inputPath: string,
  cwd: string,
): string {
  const relativePath = isAbsolute(inputPath)
    ? relative(canonicalizePath(cwd), canonicalizePath(inputPath))
    : inputPath;

  return stripLeadingDotSlash(normalizeSlashes(relativePath));
}

export function resolveInputPath(
  inputPath: string | undefined,
  cwd: string,
): string | undefined {
  if (!inputPath) return undefined;
  const absolutePath = isAbsolute(inputPath)
    ? inputPath
    : resolve(cwd, inputPath);
  return canonicalizePath(absolutePath);
}

export function normalizeGlobPattern(
  pattern: string,
  cwd: string,
): string {
  return normalizeProjectPath(pattern, cwd);
}

export function resolveIncludePatterns(
  cwd: string,
  src?: string | string[],
  include?: string[],
): string[] {
  if (include) {
    return include.map((pattern) => normalizeGlobPattern(pattern, cwd));
  }

  if (src) {
    const dirs = Array.isArray(src) ? src : [src];
    return dirs.flatMap((dir) => {
      const normalized = normalizeProjectPath(dir, cwd).replace(/\/+$/, "");
      if (normalized === "" || normalized === ".") {
        return ["**/*.ts", "**/*.tsx"];
      }
      return [`${normalized}/**/*.ts`, `${normalized}/**/*.tsx`];
    });
  }

  return ["**/*.ts", "**/*.tsx"];
}

function canonicalizePath(inputPath: string): string {
  const absolutePath = resolve(inputPath);

  if (GLOB_META_RE.test(inputPath)) {
    return canonicalizeFromExistingAncestor(absolutePath);
  }

  try {
    return realpathSync.native(absolutePath);
  } catch (error: unknown) {
    if (isErrnoException(error) && error.code === "ENOENT") {
      return canonicalizeFromExistingAncestor(absolutePath);
    }
    throw error;
  }
}

function isErrnoException(error: unknown): error is NodeJS.ErrnoException {
  return typeof error === "object" && error !== null && "code" in error;
}

function canonicalizeFromExistingAncestor(absolutePath: string): string {
  const suffix: string[] = [];
  let currentPath = absolutePath;

  while (true) {
    try {
      const canonicalBase = realpathSync.native(currentPath);
      return suffix.length === 0
        ? canonicalBase
        : resolve(canonicalBase, ...suffix);
    } catch (error: unknown) {
      if (!isErrnoException(error) || error.code !== "ENOENT") {
        throw error;
      }

      const parentPath = dirname(currentPath);
      if (parentPath === currentPath) {
        return absolutePath;
      }

      suffix.unshift(basename(currentPath));
      currentPath = parentPath;
    }
  }
}
