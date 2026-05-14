export function classify(x: number): string {
  return x < 0 ? "neg" : x === 0 ? "zero" : "pos";
}
