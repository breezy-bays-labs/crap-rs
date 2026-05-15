import type { FunctionComplexity } from "../domain/types.js";

export interface ComplexityPort {
  extract(sourceText: string, filePath: string): FunctionComplexity[];
}
