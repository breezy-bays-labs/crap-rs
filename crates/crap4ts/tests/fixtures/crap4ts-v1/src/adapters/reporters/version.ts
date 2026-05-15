import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export interface VersionFs {
  exists(path: string): boolean;
  read(path: string): string;
}

const defaultVersionFs: VersionFs = {
  exists: existsSync,
  read: (path: string) => readFileSync(path, "utf-8"),
};

class PackageJsonNotFoundError extends Error {
  constructor() {
    super("package.json not found");
    this.name = "PackageJsonNotFoundError";
  }
}

/**
 * Read the package version from the nearest package.json.
 * Falls back to "0.0.0" when the file cannot be located (e.g. bundled builds).
 */
export function readPackageVersion(): string {
  const startDir = fileURLToPath(new URL(".", import.meta.url));
  return readPackageVersionFrom(startDir);
}

export function readPackageVersionFrom(
  startDir: string,
  fs: VersionFs = defaultVersionFs,
): string {
  let packageJsonPath: string;

  try {
    packageJsonPath = findPackageJson(startDir, fs);
  } catch (error: unknown) {
    if (error instanceof PackageJsonNotFoundError) {
      return "0.0.0";
    }
    throw error;
  }

  try {
    const pkg = JSON.parse(fs.read(packageJsonPath)) as { version?: string };
    return pkg.version ?? "0.0.0";
  } catch (error: unknown) {
    if (isErrnoException(error) && error.code === "ENOENT") {
      return "0.0.0";
    }
    throw error;
  }
}

function isErrnoException(error: unknown): error is NodeJS.ErrnoException {
  return typeof error === "object" && error !== null && "code" in error;
}

function findPackageJson(startDir: string, fs: VersionFs): string {
  let currentDir = startDir;

  for (let i = 0; i < 5; i++) {
    const candidate = join(currentDir, "package.json");
    if (fs.exists(candidate)) {
      return candidate;
    }
    currentDir = dirname(currentDir);
  }

  throw new PackageJsonNotFoundError();
}
