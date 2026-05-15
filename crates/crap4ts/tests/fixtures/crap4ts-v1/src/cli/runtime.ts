import { ConsoleReporter } from "../adapters/reporters/console.js";
import { JsonReporter } from "../adapters/reporters/json.js";
import { MarkdownReporter } from "../adapters/reporters/markdown.js";
import type { ReporterPort } from "../ports/reporter-port.js";
import type { ResolvedConfig } from "./config.js";
import type { AnalysisResult, BreakdownMode, FunctionVerdict } from "../domain/types.js";

export class CliOptionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CliOptionError";
  }
}

export function validateMutualExclusions(
  opts: Record<string, unknown>,
): void {
  const thresholdFlags = [
    opts["strict"] ? "--strict" : null,
    opts["lenient"] ? "--lenient" : null,
    opts["threshold"] !== undefined ? "--threshold" : null,
  ].filter(Boolean) as string[];

  if (thresholdFlags.length > 1) {
    throw new CliOptionError(
      `Conflicting options: ${thresholdFlags.join(", ")} — pick one.`,
    );
  }

  if (opts["quiet"] && opts["verbose"]) {
    throw new CliOptionError(
      "Conflicting options: --quiet, --verbose — pick one.",
    );
  }
}

export function resolveThresholdFlag(
  opts: Record<string, unknown>,
): number | undefined {
  if (opts["strict"]) return 8;
  if (opts["lenient"]) return 30;
  return opts["threshold"] as number | undefined;
}

export function parseBreakdownCliFlag(
  raw: unknown,
): BreakdownMode | undefined {
  if (raw === undefined || raw === false) return undefined;
  if (raw === true) return "exceeding";
  if (raw === "all") return "all";
  if (raw === "exceeding") return "exceeding";
  if (raw === "off") return "off";

  throw new CliOptionError(
    `Invalid --breakdown value: "${String(raw)}". Valid values: all, exceeding (or omit for exceeding).`,
  );
}

export function coerceArrayOption(
  value: string[] | undefined,
): string[] | undefined {
  if (!value || value.length === 0) return undefined;
  return value;
}

export function createReporter(config: ResolvedConfig): ReporterPort {
  const format = config.format ?? "table";

  switch (format) {
    case "json":
      return new JsonReporter();
    case "markdown":
      return new MarkdownReporter();
    case "table":
      return new ConsoleReporter({ color: !config.noColor });
    default:
      throw new CliOptionError(
        `Unknown output format: "${format}". Valid formats: table, json, markdown`,
      );
  }
}

export function applyFilters(
  result: AnalysisResult,
  sortField?: string,
  topN?: number,
): AnalysisResult {
  const hasSort = Boolean(sortField);
  const hasTopN = topN !== undefined && topN > 0;

  if (!hasSort && !hasTopN) {
    return result;
  }

  const effectiveSort = sortField ?? (hasTopN ? "crap" : undefined);
  let verdicts = effectiveSort
    ? sortVerdicts([...result.functions], effectiveSort)
    : [...result.functions];

  if (hasTopN) {
    verdicts = verdicts.slice(0, topN);
  }

  return { ...result, functions: verdicts };
}

export function sortVerdicts(
  verdicts: FunctionVerdict[],
  field: string,
): FunctionVerdict[] {
  const sorted = [...verdicts];

  switch (field) {
    case "crap":
      sorted.sort((a, b) => b.scored.crap.value - a.scored.crap.value);
      return sorted;
    case "complexity":
      sorted.sort(
        (a, b) =>
          b.scored.cyclomaticComplexity - a.scored.cyclomaticComplexity,
      );
      return sorted;
    case "coverage":
      sorted.sort(
        (a, b) => a.scored.coveragePercent - b.scored.coveragePercent,
      );
      return sorted;
    case "name":
      sorted.sort((a, b) =>
        a.scored.identity.qualifiedName.localeCompare(
          b.scored.identity.qualifiedName,
        ),
      );
      return sorted;
    default:
      throw new CliOptionError(
        `Invalid --sort value: "${field}". Valid values: crap, complexity, coverage, name.`,
      );
  }
}

export function formatSummaryLine(result: AnalysisResult): string {
  const { summary, thresholdConfig, passed } = result;
  const status = passed ? "PASS" : "FAIL";
  return `${status}: ${summary.totalFunctions} functions | ${summary.exceedingThreshold} above threshold (${thresholdConfig.defaultThreshold}) | worst: ${summary.maxCrap.value.toFixed(1)} | avg: ${summary.averageCrap.toFixed(1)}`;
}
