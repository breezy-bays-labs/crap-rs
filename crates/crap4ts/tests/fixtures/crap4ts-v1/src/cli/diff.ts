import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { FunctionFilter, SourceSpan } from "../domain/types.js";

// ── Exec Abstraction (for testing) ──────────────────────────────────

export interface ExecFn {
  (command: string, args: string[]): Promise<{ stdout: string; exitCode: number }>;
}

const execFileAsync = promisify(execFile);

type RawExecFile = (
  command: string,
  args: string[],
) => Promise<{ stdout: string }>;

/**
 * Default exec implementation using execFile (not exec) to avoid
 * shell injection. Arguments are passed as an array, never interpolated.
 */
export function createExecAdapter(exec: RawExecFile): ExecFn {
  return async (command: string, args: string[]) => {
    try {
      const { stdout } = await exec(command, args);
      return { stdout, exitCode: 0 };
    } catch (error: unknown) {
      return handleExecError(command, error);
    }
  };
}

const defaultExec = createExecAdapter(
  (command: string, args: string[]) => execFileAsync(command, args),
);

function handleExecError(
  command: string,
  error: unknown,
): { stdout: string; exitCode: number } {
  if (error instanceof Error && "code" in error) {
    const execError = error as { stdout?: string; code?: number | string };

    if (execError.code === "ENOENT") {
      throw new Error(`"${command}" is not installed or not in PATH`);
    }

    if (typeof execError.code === "number") {
      return {
        stdout: execError.stdout ?? "",
        exitCode: execError.code,
      };
    }
  }

  throw error;
}

// ── parseUnifiedDiff ────────────────────────────────────────────────

/** Captures file path from "diff --git a/... b/..." */
const DIFF_HEADER_RE = /^diff --git a\/.+ b\/(.+)$/;
/** Captures start line and optional count from "@@ -old +start,count @@" */
const HUNK_RE = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/;

/**
 * Parses `git diff --unified=0` output into a map of file paths to
 * line-level spans. New files (--- /dev/null) map to `null` (whole-file).
 */
export function parseUnifiedDiff(
  raw: string,
): Map<string, ReadonlyArray<SourceSpan> | null> {
  const result = new Map<string, ReadonlyArray<SourceSpan> | null>();
  if (!raw.trim()) return result;

  let currentFile: string | null = null;
  let isNewFile = false;
  let spans: SourceSpan[] = [];

  const flushFile = () => {
    if (currentFile !== null) {
      if (isNewFile) {
        result.set(currentFile, null);
      } else {
        result.set(currentFile, spans);
      }
    }
  };

  for (const line of raw.split("\n")) {
    const headerMatch = line.match(DIFF_HEADER_RE);
    if (headerMatch) {
      flushFile();
      currentFile = headerMatch[1]!.replace(/\\/g, "/");
      isNewFile = false;
      spans = [];
      continue;
    }

    if (line === "--- /dev/null") {
      isNewFile = true;
      continue;
    }

    const hunkMatch = line.match(HUNK_RE);
    if (hunkMatch && !isNewFile) {
      const start = parseInt(hunkMatch[1]!, 10);
      const count = hunkMatch[2] !== undefined ? parseInt(hunkMatch[2]!, 10) : 1;
      // count=0 means deletion-only hunk (no new lines added); skip
      if (count > 0) {
        spans.push({
          startLine: start,
          endLine: start + count,
          startColumn: 0,
          endColumn: 0,
        });
      }
    }
  }

  flushFile();
  return result;
}

// ── getChangedFiles ─────────────────────────────────────────────────

interface GetChangedFilesOptions {
  cwd?: string;
  exec?: ExecFn;
}

/**
 * Runs `git diff --unified=0 <ref>` and produces a FunctionFilter
 * with line-level spans for each changed file.
 * New files map to `null` (whole-file changed).
 */
export async function getChangedFiles(
  ref: string,
  options?: GetChangedFilesOptions,
): Promise<FunctionFilter> {
  // Validate ref: reject anything that looks like a flag to prevent argument injection
  if (ref.startsWith("-")) {
    throw new Error(
      `Invalid git ref: "${ref}". Refs must not start with "-".`,
    );
  }

  const exec = options?.exec ?? defaultExec;

  // Use -- separator to prevent git from interpreting ref as flags
  const { stdout, exitCode } = await exec("git", ["diff", "--unified=0", ref, "--"]);
  const safeRef = ref.replace(/[\x00-\x1f\x7f-\x9f]/g, "");

  if (exitCode !== 0) {
    throw new Error(
      `git diff failed with exit code ${exitCode}: ${stdout.trim()} (ref: ${safeRef})`,
    );
  }

  const changedFiles = parseUnifiedDiff(stdout);

  return {
    description: `Changed since ${safeRef}`,
    changedFiles,
  };
}
