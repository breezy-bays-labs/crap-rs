import { resolve } from "node:path";
import { computeCrap } from "../domain/crap.js";
import {
  createThresholdConfig,
  resolveThreshold,
} from "../domain/threshold.js";
import { computeSummary } from "../domain/summary.js";
import { shouldInclude } from "../domain/filtering.js";
import type {
  AnalyzeOptions,
  AnalysisResult,
  FunctionVerdict,
  UnmatchedFunction,
  Warning,
  FunctionComplexity,
  FunctionCoverage,
  FunctionFilter,
  MatchResult,
  ThresholdConfig,
  ScoredFunction,
} from "../domain/types.js";
import type { AnalyzeDeps } from "./deps.js";
import {
  normalizeGlobPattern,
  resolveIncludePatterns,
  resolveInputPath,
} from "./path-utils.js";
export type { AnalyzeDeps };

// ── Main Entry Point ──────────────────────────────────────────────

export async function analyze(
  options?: AnalyzeOptions,
  deps?: AnalyzeDeps,
): Promise<AnalysisResult> {
  const opts = resolveOptions(options);
  const resolvedDeps = deps ?? (await loadDefaults(opts.cwd));

  // 1. Build threshold config
  const thresholdConfig = buildThresholdConfig(opts);

  // 2. Find source files
  const sourceFiles = await resolvedDeps.findFiles(
    opts.include,
    { cwd: opts.cwd, exclude: opts.exclude },
  );

  if (sourceFiles.length === 0) {
    return emptyResult(thresholdConfig);
  }

  // 3. Read source files (used for both complexity extraction and accurate coverage mapping)
  const sourceContents = new Map<string, string>();
  const allComplexities: FunctionComplexity[] = [];
  for (const filePath of sourceFiles) {
    const absolutePath = resolve(opts.cwd, filePath);
    const source = await resolvedDeps.readFile(absolutePath);
    sourceContents.set(filePath, source);
    const fileComplexities = resolvedDeps.complexityPort.extract(
      source,
      filePath,
    );
    allComplexities.push(...fileComplexities);
  }

  // 4. Apply function filter (if provided)
  const complexitiesToMatch = opts.filter
    ? allComplexities.filter((c) => shouldInclude(opts.filter!, c.identity))
    : allComplexities;

  // 5. Read coverage data (pass source content for accurate line mapping)
  const { coverage: coverageData, warnings: coverageWarnings } =
    await loadCoverageData(resolvedDeps, opts.coveragePath, sourceContents);

  // 6. Flatten all coverage data
  const allCoverages = flattenCoverages(coverageData);

  // 7. Match complexity with coverage
  const matchResult = resolvedDeps.matcher(complexitiesToMatch, allCoverages);

  // 8. Score matched pairs and collect unmatched
  const matchedVerdicts = scoreMatchedPairs(
    matchResult.matched,
    opts.coverageMetric,
    thresholdConfig,
    resolvedDeps.globMatcher,
  );
  const unmatchedVerdicts = scoreUnmatchedComplexities(
    matchResult.unmatchedComplexity,
    thresholdConfig,
    resolvedDeps.globMatcher,
  );
  const allVerdicts = [...matchedVerdicts, ...unmatchedVerdicts];
  const allUnmatched = collectUnmatched(matchResult);

  // 9. Collect warnings
  const warnings = collectWarnings(coverageWarnings, allUnmatched, opts.filter, complexitiesToMatch.length, allComplexities.length);

  // 10. Compute summary and pass/fail
  const summary = computeSummary(allVerdicts);
  const passed = allVerdicts.every((v) => !v.exceeds);

  return { functions: allVerdicts, unmatched: allUnmatched, warnings, summary, thresholdConfig, passed };
}

// ── Internal Helpers ──────────────────────────────────────────────

const DEFAULT_EXCLUDE = [
  "**/node_modules/**",
  "**/dist/**",
  "**/*.d.ts",
  "**/*.test.ts",
  "**/*.spec.ts",
];

interface ResolvedOptions {
  cwd: string;
  coveragePath: string | undefined;
  threshold: number | undefined;
  thresholds: Record<string, number> | undefined;
  coverageMetric: "line" | "branch";
  include: string[];
  exclude: string[];
  filter: FunctionFilter | undefined;
}

function resolveOptions(options?: AnalyzeOptions): ResolvedOptions {
  const opts = options ?? {};
  const cwd = opts.cwd ?? process.cwd();
  return {
    cwd,
    coveragePath: resolveInputPath(opts.coverage, cwd),
    threshold: opts.threshold,
    thresholds: opts.thresholds,
    coverageMetric: opts.coverageMetric ?? "line",
    include: resolveIncludePatterns(cwd, opts.src, opts.include),
    exclude: (opts.exclude ?? DEFAULT_EXCLUDE).map((pattern) =>
      normalizeGlobPattern(pattern, cwd),
    ),
    filter: opts.filter,
  };
}

function buildThresholdConfig(opts: ResolvedOptions): ThresholdConfig {
  const overrides = opts.thresholds
    ? Object.entries(opts.thresholds).map(([glob, threshold]) => ({
        glob,
        threshold,
      }))
    : undefined;

  return createThresholdConfig({
    preset: opts.threshold,
    overrides,
  });
}

async function loadCoverageData(
  deps: AnalyzeDeps,
  coveragePath: string | undefined,
  sources?: ReadonlyMap<string, string>,
): Promise<{ coverage: ReadonlyMap<string, ReadonlyArray<FunctionCoverage>>; warnings: Warning[] }> {
  if (!coveragePath) {
    return { coverage: new Map(), warnings: [] };
  }

  const rawData = await deps.readJson(coveragePath);
  const result = deps.coveragePort.parse(rawData, sources);
  return { coverage: result.coverage, warnings: [...result.warnings] };
}

export function flattenCoverages(
  coverageData: ReadonlyMap<string, ReadonlyArray<FunctionCoverage>>,
): FunctionCoverage[] {
  return Array.from(coverageData.values()).flat();
}

export function extractCoveragePercent(
  coverage: FunctionCoverage,
  metric: "line" | "branch",
): number {
  if (metric === "branch") {
    return coverage.branchCoverage !== null
      ? coverage.branchCoverage.percent
      : 100; // A function with no branches has trivially 100% branch coverage
  }
  return coverage.lineCoverage.percent;
}

function scoreMatchedPairs(
  matched: MatchResult["matched"],
  coverageMetric: "line" | "branch",
  thresholdConfig: ThresholdConfig,
  globMatcher: (path: string, pattern: string) => boolean,
): FunctionVerdict[] {
  return matched.map(({ complexity, coverage }) => {
    const coveragePercent = extractCoveragePercent(coverage, coverageMetric);
    const crap = computeCrap(complexity.cyclomaticComplexity, coveragePercent);
    const threshold = resolveThreshold(thresholdConfig, complexity.identity.filePath, globMatcher);
    const scored: ScoredFunction = {
      identity: complexity.identity,
      cyclomaticComplexity: complexity.cyclomaticComplexity,
      coveragePercent,
      crap,
      contributors: complexity.contributors,
    };
    return { scored, threshold, exceeds: crap.value > threshold };
  });
}

function scoreUnmatchedComplexities(
  unmatched: MatchResult["unmatchedComplexity"],
  thresholdConfig: ThresholdConfig,
  globMatcher: (path: string, pattern: string) => boolean,
): FunctionVerdict[] {
  return unmatched.map((complexity) => {
    const crap = computeCrap(complexity.cyclomaticComplexity, 0);
    const threshold = resolveThreshold(
      thresholdConfig,
      complexity.identity.filePath,
      globMatcher,
    );

    return {
      scored: {
        identity: complexity.identity,
        cyclomaticComplexity: complexity.cyclomaticComplexity,
        coveragePercent: 0,
        crap,
        contributors: complexity.contributors,
      },
      threshold,
      exceeds: crap.value > threshold,
    };
  });
}

function collectUnmatched(matchResult: MatchResult): UnmatchedFunction[] {
  const noCoverage: UnmatchedFunction[] = matchResult.unmatchedComplexity.map((complexity) => ({
    kind: "no-coverage" as const,
    complexity,
    worstCaseCrap: computeCrap(complexity.cyclomaticComplexity, 0),
  }));
  const noAst: UnmatchedFunction[] = matchResult.unmatchedCoverage.map((coverage) => ({
    kind: "no-ast" as const,
    coverage,
  }));
  return [...noCoverage, ...noAst];
}

function collectWarnings(
  coverageWarnings: Warning[],
  unmatched: ReadonlyArray<UnmatchedFunction>,
  filter: FunctionFilter | undefined,
  filteredCount: number,
  totalCount: number,
): Warning[] {
  const warnings: Warning[] = [...coverageWarnings];

  if (filter && filteredCount === 0 && totalCount > 0) {
    warnings.push({
      code: "filter-excluded-all",
      message: `Filter "${filter.description}" excluded all ${totalCount} functions. No functions overlap with changed lines.`,
    });
  }

  for (const u of unmatched) {
    warnings.push(unmatchedToWarning(u));
  }

  return warnings;
}

function unmatchedToWarning(u: UnmatchedFunction): Warning {
  if (u.kind === "no-coverage") {
    return {
      code: "unmatched-no-coverage",
      message: `No coverage data found for function "${u.complexity.identity.qualifiedName}"`,
      file: u.complexity.identity.filePath,
      function: u.complexity.identity.qualifiedName,
    };
  }
  return {
    code: "unmatched-no-ast",
    message: `No AST match found for coverage entry "${u.coverage.name}"`,
    file: u.coverage.filePath,
    function: u.coverage.name,
  };
}

function emptyResult(thresholdConfig: ThresholdConfig): AnalysisResult {
  return {
    functions: [],
    unmatched: [],
    warnings: [],
    summary: computeSummary([]),
    thresholdConfig,
    passed: true,
  };
}

async function loadDefaults(cwd?: string): Promise<AnalyzeDeps> {
  const { createDefaultDeps } = await import("./defaults.js");
  return createDefaultDeps(cwd);
}
