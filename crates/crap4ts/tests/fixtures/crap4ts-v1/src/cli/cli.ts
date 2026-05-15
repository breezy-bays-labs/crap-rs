#!/usr/bin/env node
import { Command } from "commander";
import { writeFileSync, existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { analyze } from "../core/analyze.js";
import {
  findConfigFile,
  loadConfigFile,
  resolveConfig,
  type ResolvedConfig,
} from "./config.js";
import {
  discoverCoverage,
  discoverSourceRoot,
  formatCoverageNotFoundError,
} from "./discover.js";
import { getChangedFiles } from "./diff.js";
import { prepareForJsonOutput } from "../core/prepare-output.js";
import {
  applyFilters,
  CliOptionError,
  coerceArrayOption,
  createReporter,
  formatSummaryLine,
  parseBreakdownCliFlag,
  resolveThresholdFlag,
  validateMutualExclusions,
} from "./runtime.js";
import type { AnalysisResult } from "../domain/types.js";

// ── Exit Codes ─────────────────────────────────────────────────────

const EXIT_OK = 0;
const EXIT_THRESHOLD = 1;
const EXIT_CONFIG_ERROR = 2;
const EXIT_PARSE_ERROR = 3;

// ── Version ───────────────────────────────────────────────────────

function readVersion(): string {
  const __dirname = fileURLToPath(new URL(".", import.meta.url));
  const pkgPath = join(__dirname, "..", "package.json");
  try {
    const pkg = JSON.parse(readFileSync(pkgPath, "utf-8")) as { version: string };
    return pkg.version;
  } catch {
    return "0.0.0";
  }
}

// ── CLI Definition ─────────────────────────────────────────────────

const program = new Command();

program
  .name("crap4ts")
  .description(
    "CRAP score analyzer for TypeScript — find complex, under-tested functions",
  )
  .version(readVersion())
  .option("-c, --coverage <path>", "path to coverage JSON")
  .option("-s, --src <paths...>", "source directories")
  .option("-t, --threshold <n>", "CRAP threshold", parseFloat)
  .option("--strict", "use strict threshold (8)")
  .option("--lenient", "use lenient threshold (30)")
  .option("--coverage-metric <type>", "coverage metric: line or branch")
  .option("-f, --format <type>", "output format: table, json, markdown")
  .option("-n, --top <n>", "show N worst functions", parseInt)
  .option("--sort <field>", "sort by: crap, complexity, coverage, name")
  .option(
    "--changed-since <ref>",
    "only analyze files changed since git ref",
  )
  .option("--diff <ref>", "alias for --changed-since")
  .option("--include <glob...>", "file include globs")
  .option("--exclude <glob...>", "file exclude globs")
  .option("--summary", "show summary line only")
  .option("-q, --quiet", "exit code only, no output")
  .option("-v, --verbose", "show discovery and warnings")
  .option("--no-color", "disable colors")
  .option("--config <path>", "explicit config file path")
  .option(
    "--breakdown [mode]",
    "show CC contributors (JSON only): all or exceeding (default)",
  );

// ── init subcommand ────────────────────────────────────────────────

program
  .command("init")
  .description("scaffold a crap4ts.config.ts with sensible defaults")
  .action(() => {
    const configPath = join(process.cwd(), "crap4ts.config.ts");

    if (existsSync(configPath)) {
      console.error(
        `Config file already exists: ${configPath}\nRemove it first if you want to reinitialize.`,
      );
      process.exit(EXIT_CONFIG_ERROR);
    }

    const template = `import { defineConfig } from "crap4ts";

export default defineConfig({
  threshold: 16,
  coverageMetric: "line",
  exclude: ["**/*.test.*", "**/*.spec.*", "**/*.d.ts"],
  // format: "table",
  // src: ["src"],
  // breakdown: "off",
  // sort: "crap",
  // top: 10,
  // summary: false,
});
`;

    writeFileSync(configPath, template, "utf-8");
    console.log(`Created ${configPath}`);
    process.exit(EXIT_OK);
  });

// ── Main action ────────────────────────────────────────────────────

program.action(async (opts: Record<string, unknown>) => {
  try {
    // 1. Validate mutual exclusions
    validateMutualExclusions(opts);

    const cwd = process.cwd();
    const verbose = Boolean(opts["verbose"]);

    // 2. Resolve threshold from --strict, --lenient, or --threshold
    const threshold = resolveThresholdFlag(opts);

    // 3. Load config file
    const configFilePath = opts["config"]
      ? resolve(String(opts["config"]))
      : findConfigFile(cwd);

    let fileConfig = {};
    if (configFilePath) {
      if (verbose) {
        console.error(`Loading config from ${configFilePath}`);
      }
      try {
        fileConfig = await loadConfigFile(configFilePath);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error(`Error loading config: ${msg}`);
        process.exit(EXIT_CONFIG_ERROR);
      }
    } else if (opts["config"]) {
      console.error(
        `Config file not found: ${String(opts["config"])}`,
      );
      process.exit(EXIT_CONFIG_ERROR);
    }

    // 4. Merge config layers (defaults < file < env < CLI)
    const coverageMetricRaw = opts["coverageMetric"] as
      | string
      | undefined;
    let coverageMetric: "line" | "branch" | undefined;
    if (coverageMetricRaw === "line" || coverageMetricRaw === "branch") {
      coverageMetric = coverageMetricRaw;
    } else if (coverageMetricRaw !== undefined) {
      console.error(
        `Invalid --coverage-metric: "${coverageMetricRaw}". Must be "line" or "branch".`,
      );
      process.exit(EXIT_CONFIG_ERROR);
    }

    const resolved: ResolvedConfig = resolveConfig({
      fileConfig,
      env: process.env as Record<string, string | undefined>,
      cliFlags: {
        threshold,
        coverage: opts["coverage"] as string | undefined,
        format: opts["format"] as ResolvedConfig["format"],
        noColor: opts["color"] === false ? true : undefined, // only set when --no-color is explicit
        coverageMetric,
        include: opts["include"] as string[] | undefined,
        exclude: opts["exclude"] as string[] | undefined,
        src: opts["src"] as string[] | undefined,
        breakdown: parseBreakdownCliFlag(opts["breakdown"]),
        sort: opts["sort"] as ResolvedConfig["sort"],
        top: opts["top"] as number | undefined,
        summary: opts["summary"] === true ? true : undefined,
      },
    });
    const include = coerceArrayOption(resolved.include);
    const exclude = coerceArrayOption(resolved.exclude);

    // 5. Auto-discover coverage if not explicitly provided
    let coveragePath = resolved.coverage;
    if (!coveragePath) {
      const discovered = discoverCoverage(cwd);
      if (discovered) {
        coveragePath = discovered.path;
        if (verbose) {
          console.error(
            `Auto-discovered coverage: ${discovered.path} (${discovered.format})`,
          );
        }
      } else {
        console.error(formatCoverageNotFoundError(cwd));
        process.exit(EXIT_PARSE_ERROR);
      }
    }

    // 6. Auto-discover source root if not explicitly provided
    let srcPaths: string[] | undefined;
    if (resolved.src) {
      srcPaths = Array.isArray(resolved.src)
        ? resolved.src
        : [resolved.src];
    } else {
      const sourceRoot = discoverSourceRoot(cwd);
      if (verbose) {
        console.error(`Auto-discovered source root: ${sourceRoot}`);
      }
      srcPaths = [sourceRoot];
    }

    // 7. Handle --changed-since / --diff
    const changedSinceRef =
      (opts["changedSince"] as string | undefined) ??
      (opts["diff"] as string | undefined);

    // 8. Call analyze()
    let result: AnalysisResult;
    try {
      result = await analyze({
        src: srcPaths,
        coverage: coveragePath,
        threshold: resolved.threshold,
        thresholds: resolved.thresholds,
        coverageMetric: resolved.coverageMetric ?? "line",
        include,
        exclude,
        filter: changedSinceRef
          ? await getChangedFiles(changedSinceRef, { cwd })
          : undefined,
        cwd,
      });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`Analysis failed: ${msg}`);
      process.exit(EXIT_PARSE_ERROR);
    }

    // 8b. Resolve breakdown mode
    const breakdown = resolved.breakdown ?? "off";
    const format = resolved.format ?? "table";
    if (breakdown !== "off" && format !== "json") {
      console.error(
        `Warning: --breakdown is only supported with JSON format (-f json). Ignoring for "${format}" output.`,
      );
    }

    // 9. Output (unless --quiet)
    const quiet = Boolean(opts["quiet"]);
    if (!quiet) {
      const reporter = createReporter(resolved);
      let output: string;

      const summaryOnly = resolved.summary ?? false;
      if (summaryOnly) {
        output = formatSummaryLine(result);
      } else {
        // Apply --sort and --top before formatting
        const sortField = resolved.sort;
        const topN = resolved.top;
        const filtered = applyFilters(result, sortField, topN);
        // Pre-map contributors for JSON output (reporters are pure serializers)
        const reportable = format === "json"
          ? prepareForJsonOutput(filtered, breakdown)
          : filtered;
        output = reporter.format(reportable);
      }

      console.log(output);
    }

    // 9b. Show warnings on stderr when --verbose
    if (verbose && result.warnings.length > 0) {
      console.error("");
      console.error(`Warnings (${result.warnings.length}):`);
      for (const w of result.warnings) {
        console.error(`  [${w.code}] ${w.message}`);
      }
    }

    // 10. Exit with appropriate code
    process.exit(result.passed ? EXIT_OK : EXIT_THRESHOLD);
  } catch (err) {
    if (err instanceof CliOptionError) {
      console.error(err.message);
      process.exit(EXIT_CONFIG_ERROR);
    }
    const msg = err instanceof Error ? err.message : String(err);
    console.error(`Unexpected error: ${msg}`);
    process.exit(EXIT_CONFIG_ERROR);
  }
});

// ── Run ────────────────────────────────────────────────────────────

program.parse();
