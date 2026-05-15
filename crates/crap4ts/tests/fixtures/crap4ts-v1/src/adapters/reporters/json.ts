import type { ReporterPort } from "../../ports/reporter-port.js";
import type { AnalysisResult } from "../../domain/types.js";
import { readPackageVersion } from "./version.js";

export class JsonReporter implements ReporterPort {
  format(result: AnalysisResult): string {
    const envelope = {
      $schema: "",
      version: readPackageVersion(),
      timestamp: new Date().toISOString(),
      config: result.thresholdConfig,
      summary: result.summary,
      functions: result.functions,
      unmatched: result.unmatched,
      warnings: result.warnings,
      passed: result.passed,
    };

    return JSON.stringify(envelope, null, 2);
  }
}
