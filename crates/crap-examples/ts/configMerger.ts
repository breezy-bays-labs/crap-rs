/**
 * Compound catastrophe. High complexity multiplied by low coverage
 * lands in the High band — the product of both terms of the CRAP
 * formula at once. This module isolates the worst case so the
 * heatmap has a top-band anchor.
 *
 * `mergeConfigs` reconciles two plain-object sources (defaults and
 * env overrides) with nested merging and explicit precedence rules.
 * The tests only cover the simplest top-level merge; nested merging
 * stays uncovered.
 */

export type ConfigValue =
  | string
  | number
  | boolean
  | ConfigValue[]
  | { [key: string]: ConfigValue };

export interface MergedConfig {
  values: Record<string, ConfigValue>;
}

function isPlainObject(
  v: ConfigValue,
): v is { [key: string]: ConfigValue } {
  return (
    typeof v === "object" && v !== null && !Array.isArray(v)
  );
}

export function mergeConfigs(
  defaults: Record<string, ConfigValue>,
  env: Record<string, ConfigValue>,
): MergedConfig {
  const merged: Record<string, ConfigValue> = { ...defaults };

  for (const key of Object.keys(env)) {
    const value = env[key];
    const existing = merged[key];
    if (isPlainObject(existing) && isPlainObject(value)) {
      // Nested merge — overwrite each sub-key. Uncovered when tests
      // only exercise top-level merging, which is what keeps this
      // function at the top of the heatmap.
      for (const subKey of Object.keys(value)) {
        existing[subKey] = value[subKey];
      }
    } else {
      // New top-level key, OR existing key whose value isn't an
      // object on one or both sides — env overwrites defaults.
      merged[key] = value;
    }
  }

  return { values: merged };
}
