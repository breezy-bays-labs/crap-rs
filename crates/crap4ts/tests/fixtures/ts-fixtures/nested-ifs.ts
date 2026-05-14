export function deep(a: number, b: number): string {
  if (a > 0) {
    if (b > 0) return "both positive";
  }
  return "default";
}
