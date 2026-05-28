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
    if (key in merged) {
      const existing = merged[key];
      if (isPlainObject(existing) && isPlainObject(value)) {
        for (const subKey of Object.keys(value)) {
          if (subKey in existing) {
            existing[subKey] = value[subKey];
          } else {
            existing[subKey] = value[subKey];
          }
        }
      } else {
        merged[key] = value;
      }
    } else {
      merged[key] = value;
    }
  }

  return { values: merged };
}
