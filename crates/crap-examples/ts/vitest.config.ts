import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["**/*.test.ts"],
    coverage: {
      // crap4ts consumes Istanbul-shape coverage-final.json — vitest's
      // default `v8` provider emits a different JSON shape that the
      // crap4ts walker can't parse, so the Istanbul provider is
      // mandatory for the pedagogical envelope.
      provider: "istanbul",
      reporter: ["json"],
      reportsDirectory: "./.vitest-coverage",
      include: ["*.ts"],
      exclude: ["**/*.test.ts", "vitest.config.ts"],
    },
  },
});
