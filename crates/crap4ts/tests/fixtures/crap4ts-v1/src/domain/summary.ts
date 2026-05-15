import type {
  AnalysisSummary,
  CrapScore,
  FunctionIdentity,
  FunctionVerdict,
  RiskDistribution,
} from "./types.js";
import { RiskLevel } from "./types.js";

/**
 * Compute aggregate statistics over an array of function verdicts.
 *
 * Pure function: FunctionVerdict[] → AnalysisSummary
 */
export function computeSummary(
  verdicts: ReadonlyArray<FunctionVerdict>,
): AnalysisSummary {
  const totalFunctions = verdicts.length;

  if (totalFunctions === 0) {
    return {
      totalFunctions: 0,
      totalFiles: 0,
      exceedingThreshold: 0,
      exceedingPercent: 0,
      averageCrap: 0,
      medianCrap: 0,
      maxCrap: { value: 0, riskLevel: RiskLevel.Low },
      worstFunction: null,
      distribution: emptyDistribution(),
      crapLoad: 0,
    };
  }

  const totalFiles = new Set(
    verdicts.map((v) => v.scored.identity.filePath),
  ).size;

  const exceeding = verdicts.filter((v) => v.exceeds);
  const exceedingThreshold = exceeding.length;
  const exceedingPercent = Math.round((exceedingThreshold / totalFunctions) * 100);

  const crapValues = verdicts.map((v) => v.scored.crap.value);
  const averageCrap = roundTo2(
    crapValues.reduce((sum, v) => sum + v, 0) / totalFunctions,
  );

  const medianCrap = computeMedian(crapValues);

  const { maxCrap, worstFunction } = findWorst(verdicts);

  const distribution = computeDistribution(verdicts);

  const crapLoad = exceeding.reduce(
    (sum, v) => sum + (v.scored.crap.value - v.threshold),
    0,
  );

  return {
    totalFunctions,
    totalFiles,
    exceedingThreshold,
    exceedingPercent,
    averageCrap,
    medianCrap,
    maxCrap,
    worstFunction,
    distribution,
    crapLoad,
  };
}

function emptyDistribution(): RiskDistribution {
  return {
    [RiskLevel.Low]: 0,
    [RiskLevel.Acceptable]: 0,
    [RiskLevel.Moderate]: 0,
    [RiskLevel.High]: 0,
  };
}

function computeMedian(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  if (sorted.length % 2 === 1) {
    return sorted[mid]!;
  }
  return (sorted[mid - 1]! + sorted[mid]!) / 2;
}

function findWorst(
  verdicts: ReadonlyArray<FunctionVerdict>,
): { maxCrap: CrapScore; worstFunction: FunctionIdentity } {
  let worst = verdicts[0]!;
  for (const v of verdicts) {
    if (v.scored.crap.value > worst.scored.crap.value) {
      worst = v;
    }
  }
  return {
    maxCrap: worst.scored.crap,
    worstFunction: worst.scored.identity,
  };
}

function computeDistribution(
  verdicts: ReadonlyArray<FunctionVerdict>,
): RiskDistribution {
  const dist = emptyDistribution() as Record<RiskLevel, number>;
  for (const v of verdicts) {
    dist[v.scored.crap.riskLevel]++;
  }
  return dist as RiskDistribution;
}

function roundTo2(value: number): number {
  return Math.round((value + Number.EPSILON) * 100) / 100;
}
