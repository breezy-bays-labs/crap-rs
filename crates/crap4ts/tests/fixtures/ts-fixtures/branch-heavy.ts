// Branch-heavy fixture for W2.3 — exercises 3 branch kinds:
// - if/else (binary branch at line 4)
// - ternary (binary branch at line 7)
// - switch (3-arm branch at line 10)
//
// Used by `istanbul_smoke::w23_branch_coverage_*` tests and the
// matching `coverage-with-branches.json` Istanbul fixture, which
// records per-arm hit counts that the parser fans out into one
// `BranchCoverage` row per arm per line.
export function classify(n: number): string {
  if (n > 0) {                          // line 11 — if/else (2 arms)
    return "positive";
  } else {
    return "non-positive";
  }
}

export function sign(n: number): number {
  return n >= 0 ? 1 : -1;               // line 19 — ternary (2 arms)
}

export function bucket(n: number): string {
  switch (n) {                          // line 23 — switch (3 arms)
    case 0: return "zero";
    case 1: return "one";
    default: return "other";
  }
}
