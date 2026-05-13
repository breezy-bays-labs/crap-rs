// W1.2 WhileLoop + DoWhileLoop fixture: one of each in its own
// top-level function. Each scores cyclomatic=2 with exactly one
// `while-loop` or `do-while-loop` contributor respectively.
export function countDown(n: number): number {
  let count = 0;
  while (n > 0) {
    n -= 1;
    count += 1;
  }
  return count;
}

export function countUpAtLeastOnce(n: number): number {
  let count = 0;
  do {
    count += 1;
    n -= 1;
  } while (n > 0);
  return count;
}
