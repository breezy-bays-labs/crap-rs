export function classify(n: number): string {
  if (n < 0) {
    return "negative";
  }
  return n === 0 ? "zero" : "positive";
}

export const double = (x: number): number => x * 2;

export const squares = [1, 2, 3].map((v) => v * v);

export function unused(flag: boolean): number {
  return flag ? 1 : 2;
}
