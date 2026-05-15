import type { ThresholdConfig, ThresholdOverride, ThresholdPreset } from "./types.js";

// ── Presets ────────────────────────────────────────────────────────────

export const PRESETS: Readonly<Record<ThresholdPreset, number>> = {
  strict: 8,
  default: 16,
  lenient: 30,
} as const;

// ── Factory ────────────────────────────────────────────────────────────

export interface ThresholdOptions {
  readonly preset?: ThresholdPreset | number;
  readonly overrides?: readonly ThresholdOverride[];
}

export function createThresholdConfig(
  options?: ThresholdOptions,
): ThresholdConfig {
  const defaultThreshold = resolvePreset(options?.preset);
  const overrides = options?.overrides ?? [];

  for (const override of overrides) {
    if (override.threshold <= 0) {
      throw new Error(
        `Invalid override threshold: ${override.threshold}. Must be > 0.`,
      );
    }
  }

  return { defaultThreshold, overrides };
}

function resolvePreset(preset: ThresholdPreset | number | undefined): number {
  if (preset === undefined) return PRESETS.default;
  if (typeof preset === "string") return PRESETS[preset];

  if (preset <= 0) {
    throw new Error(
      `Invalid threshold: ${preset}. Must be > 0.`,
    );
  }
  return preset;
}

// ── Resolver ───────────────────────────────────────────────────────────

export type GlobMatcher = (path: string, glob: string) => boolean;

export function resolveThreshold(
  config: ThresholdConfig,
  filePath: string,
  matcher: GlobMatcher,
): number {
  for (const override of config.overrides) {
    if (matcher(filePath, override.glob)) {
      return override.threshold;
    }
  }
  return config.defaultThreshold;
}
