import { z } from "zod";

const configSchema = z.object({
  threshold: z.number().positive().optional(),
  coverageMetric: z.enum(["line", "branch"]).optional(),
  include: z.array(z.string()).optional(),
  exclude: z.array(z.string()).optional(),
  thresholds: z.record(z.string(), z.number().positive()).optional(),
  format: z.enum(["table", "json", "markdown"]).optional(),
  src: z.union([z.string(), z.array(z.string())]).optional(),
  breakdown: z.enum(["off", "exceeding", "all"]).optional(),
  sort: z.enum(["crap", "complexity", "coverage", "name"]).optional(),
  top: z.number().int().positive().optional(),
  summary: z.boolean().optional(),
});

export type Crap4tsConfig = z.infer<typeof configSchema>;

export function defineConfig(config: Crap4tsConfig): Crap4tsConfig {
  return configSchema.parse(config);
}
