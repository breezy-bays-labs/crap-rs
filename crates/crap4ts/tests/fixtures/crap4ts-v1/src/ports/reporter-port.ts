import type { AnalysisResult } from "../domain/types.js";

export interface ReporterPort {
  format(result: AnalysisResult): string;
}
