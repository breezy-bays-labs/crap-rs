export function pickField(obj: { nested?: { value: number } } | null): number | undefined {
  return obj?.nested?.value;
}

export function callMethod(obj: { compute?: () => number }): number | undefined {
  return obj?.compute?.();
}
