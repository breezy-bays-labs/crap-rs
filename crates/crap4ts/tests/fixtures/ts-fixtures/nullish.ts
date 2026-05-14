export function withDefault(value: string | null | undefined, fallback: string): string {
  return value ?? fallback;
}
