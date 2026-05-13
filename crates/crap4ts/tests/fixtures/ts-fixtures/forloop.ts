// W1.2 ForLoop fixture: each of the three for-statement flavours in
// its own top-level function. Each function should score cyclomatic=2
// with exactly one `for-loop` contributor — the walker treats
// ForStatement, ForOfStatement, and ForInStatement uniformly.
export function sumIndices(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    total += i;
  }
  return total;
}

export function sumValues(xs: number[]): number {
  let total = 0;
  for (const x of xs) {
    total += x;
  }
  return total;
}

export function sumKeys(obj: Record<string, number>): number {
  let total = 0;
  for (const k in obj) {
    total += obj[k];
  }
  return total;
}
