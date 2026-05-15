import type { MatchFunctions } from "../domain/types.js";
import type { ComplexityPort } from "../ports/complexity-port.js";
import type { CoveragePort } from "../ports/coverage-port.js";
import type { GlobMatcher } from "../domain/threshold.js";

// ── Dependency Injection Interface ────────────────────────────────

export interface AnalyzeDeps {
  complexityPort: ComplexityPort;
  coveragePort: CoveragePort;
  matcher: MatchFunctions;
  globMatcher: GlobMatcher;
  readFile: (path: string) => Promise<string>;
  readJson: (path: string) => Promise<unknown>;
  findFiles: (
    patterns: string[],
    options: { cwd: string; exclude: string[] },
  ) => Promise<string[]>;
}
