import { TypeScriptEslintComplexityAdapter } from "./typescript-eslint.js";
import type { FunctionComplexity } from "../../domain/types.js";

// ── API Boundary Error ────────────────────────────────────────────

export class ComplexityExtractionError extends Error {
  readonly filePath: string;
  constructor(filePath: string, options?: ErrorOptions) {
    super(`Failed to extract complexity from ${filePath}`, options);
    this.name = "ComplexityExtractionError";
    this.filePath = filePath;
  }
}

// ── Convenience Function ──────────────────────────────────────────

const adapter = new TypeScriptEslintComplexityAdapter();

export function extractComplexity(
  sourceText: string,
  filePath: string,
): FunctionComplexity[] {
  try {
    return adapter.extract(sourceText, filePath);
  } catch (error) {
    throw new ComplexityExtractionError(filePath, { cause: error });
  }
}

// Re-export adapter for advanced usage
export { TypeScriptEslintComplexityAdapter } from "./typescript-eslint.js";
