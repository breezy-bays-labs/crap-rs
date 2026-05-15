import type { CrapScore } from "./types.js";
import { RiskLevel, InvalidComplexityError, InvalidCoverageError } from "./types.js";

export function computeCrap(
  cyclomaticComplexity: number,
  coveragePercent: number,
): CrapScore {
  if (!Number.isFinite(cyclomaticComplexity) || cyclomaticComplexity < 1) {
    throw new InvalidComplexityError(cyclomaticComplexity);
  }
  if (!Number.isFinite(coveragePercent)) {
    throw new InvalidCoverageError(coveragePercent);
  }

  const clamped = Math.max(0, Math.min(100, coveragePercent));
  const uncovered = 1 - clamped / 100;
  const value = roundTo2(
    cyclomaticComplexity ** 2 * uncovered ** 3 + cyclomaticComplexity
  );

  return { value, riskLevel: classifyRisk(value) };
}

function roundTo2(value: number): number {
  return Math.round((value + Number.EPSILON) * 100) / 100;
}

export function classifyRisk(score: number): RiskLevel {
  if (score <= 5) return RiskLevel.Low;
  if (score <= 8) return RiskLevel.Acceptable;
  if (score <= 30) return RiskLevel.Moderate;
  return RiskLevel.High;
}
