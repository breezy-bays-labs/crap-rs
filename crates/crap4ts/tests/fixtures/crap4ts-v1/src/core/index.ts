export { analyze } from "./analyze.js";
export type { AnalyzeDeps } from "./deps.js";
export { analyzeFile } from "./analyze-file.js";
export type { AnalyzeFileOptions, AnalyzeFileResult } from "./analyze-file.js";
export { defineConfig } from "./define-config.js";
export type { Crap4tsConfig } from "./define-config.js";
export { createDefaultDeps } from "./defaults.js";

export type {
  AnalyzeOptions,
  AnalysisResult,
  AnalysisSummary,
  FunctionVerdict,
  ScoredFunction,
  CrapScore,
  FunctionIdentity,
  FunctionFilter,
  SourceSpan,
  ThresholdConfig,
  ThresholdOverride,
  ThresholdPreset,
  CoverageRatio,
  FunctionComplexity,
  FunctionCoverage,
  RiskDistribution,
  UnmatchedFunction,
  Warning,
  WarningCode,
  ContributorKind,
  ComplexityContributor,
  BreakdownMode,
  MatchResult,
  MatchFunctions,
} from "../domain/types.js";
export { RiskLevel } from "../domain/types.js";
export { selectContributors } from "../domain/contributors.js";
export { prepareForJsonOutput } from "./prepare-output.js";

export type { GlobMatcher, ThresholdOptions } from "../domain/threshold.js";
export { PRESETS, createThresholdConfig, resolveThreshold } from "../domain/threshold.js";

export type { ComplexityPort } from "../ports/complexity-port.js";
export type { CoveragePort, CoverageParseResult } from "../ports/coverage-port.js";
