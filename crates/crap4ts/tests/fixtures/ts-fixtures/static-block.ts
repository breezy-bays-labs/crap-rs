export class Registry {
  static items: string[] = [];

  static {
    const seed = (globalThis as { __seed?: string[] }).__seed;
    if (seed) {
      Registry.items.push(...seed);
    }
  }
}
