import type {
  SourceSpan,
  FunctionComplexity,
  FunctionCoverage,
  MatchResult,
  MatchFunctions,
} from "./types.js";

// ── Span Utilities ─────────────────────────────────────────────────

/**
 * Returns true when two half-open spans overlap (share at least one line).
 * Both spans use exclusive endLine (half-open convention).
 */
export function spansOverlap(a: SourceSpan, b: SourceSpan): boolean {
  return a.startLine < b.endLine && b.startLine < a.endLine;
}

/**
 * Returns true when `outer` fully contains `inner`.
 */
export function spanContains(outer: SourceSpan, inner: SourceSpan): boolean {
  return outer.startLine <= inner.startLine && inner.endLine <= outer.endLine;
}

/**
 * Returns the fraction of `a` that overlaps with `b`, in [0, 1].
 * If `a` has zero length, returns 0.
 */
export function overlapRatio(a: SourceSpan, b: SourceSpan): number {
  const aLen = a.endLine - a.startLine;
  if (aLen <= 0) return 0;

  const overlapStart = Math.max(a.startLine, b.startLine);
  const overlapEnd = Math.min(a.endLine, b.endLine);
  const overlap = Math.max(0, overlapEnd - overlapStart);

  return overlap / aLen;
}

// ── Candidate Types & Helpers ─────────────────────────────────────

type Candidate = {
  cov: FunctionCoverage;
  ratio: number;
  contains: boolean;
  nameMatch: boolean;
};

function findCandidates(
  compSpan: SourceSpan,
  compName: string,
  fileCoverages: ReadonlyArray<FunctionCoverage>,
  usedCoverage: Set<FunctionCoverage>,
  overlapThreshold: number,
): Candidate[] {
  const candidates: Candidate[] = [];
  for (const cov of fileCoverages) {
    if (usedCoverage.has(cov)) continue;
    const ratio = overlapRatio(compSpan, cov.span);
    if (ratio < overlapThreshold) continue;
    candidates.push({
      cov,
      ratio,
      contains: spanContains(cov.span, compSpan),
      nameMatch: cov.name === compName,
    });
  }
  return candidates;
}

/**
 * Returns true when `challenger` is a strictly better match than `current`.
 * Priority: containment > overlap ratio > name match.
 */
function isBetterCandidate(challenger: Candidate, current: Candidate): boolean {
  if (challenger.contains !== current.contains) return challenger.contains;
  if (challenger.ratio !== current.ratio) return challenger.ratio > current.ratio;
  return challenger.nameMatch && !current.nameMatch;
}

function selectBestCandidate(candidates: ReadonlyArray<Candidate>): Candidate | undefined {
  if (candidates.length === 0) return undefined;

  let best = candidates[0]!;
  for (let i = 1; i < candidates.length; i++) {
    const c = candidates[i]!;
    if (isBetterCandidate(c, best)) best = c;
  }

  return best;
}

// ── Default Span Matcher ───────────────────────────────────────────

/**
 * Groups items by a key function into a Map of arrays.
 */
export function groupBy<T>(items: ReadonlyArray<T>, key: (item: T) => string): Map<string, T[]> {
  const map = new Map<string, T[]>();
  for (const item of items) {
    const k = key(item);
    let group = map.get(k);
    if (!group) {
      group = [];
      map.set(k, group);
    }
    group.push(item);
  }
  return map;
}

/**
 * Matches complexity entries to coverage entries by file and span overlap.
 *
 * Algorithm:
 * 1. Group both inputs by filePath.
 * 2. Within each file, for each FunctionComplexity:
 *    - Find FunctionCoverage candidates with overlap ratio >= 0.8
 *    - Prefer containment (coverage span fully contains complexity span)
 *    - Use name as tiebreaker when overlap is equal
 *    - Enforce 1:1 constraint (each coverage matched at most once)
 * 3. Collect unmatched entries.
 */
export const defaultSpanMatcher: MatchFunctions = (
  complexities: ReadonlyArray<FunctionComplexity>,
  coverages: ReadonlyArray<FunctionCoverage>,
): MatchResult => {
  const OVERLAP_THRESHOLD = 0.8;

  const complexityByFile = groupBy(complexities, (c) => c.identity.filePath);
  const coverageByFile = groupBy(coverages, (c) => c.filePath);

  const matched: Array<{ complexity: FunctionComplexity; coverage: FunctionCoverage }> = [];
  const unmatchedComplexity: FunctionComplexity[] = [];
  const unmatchedCoverage: FunctionCoverage[] = [];

  // Track which coverage entries have been claimed (by object reference).
  const usedCoverage = new Set<FunctionCoverage>();

  // Collect all file paths from both sides.
  const allFiles = new Set([...complexityByFile.keys(), ...coverageByFile.keys()]);

  for (const file of allFiles) {
    const fileComplexities = complexityByFile.get(file) ?? [];
    const fileCoverages = coverageByFile.get(file) ?? [];

    for (const comp of fileComplexities) {
      const candidates = findCandidates(
        comp.identity.span,
        comp.identity.qualifiedName,
        fileCoverages,
        usedCoverage,
        OVERLAP_THRESHOLD,
      );

      const best = selectBestCandidate(candidates);
      if (!best) {
        unmatchedComplexity.push(comp);
        continue;
      }

      matched.push({ complexity: comp, coverage: best.cov });
      usedCoverage.add(best.cov);
    }

    // Collect unmatched coverage for this file.
    for (const cov of fileCoverages) {
      if (!usedCoverage.has(cov)) {
        unmatchedCoverage.push(cov);
      }
    }
  }

  return { matched, unmatchedComplexity, unmatchedCoverage };
};
