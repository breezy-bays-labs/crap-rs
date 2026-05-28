import { describe, expect, it } from "vitest";
import { mergeConfigs } from "./configMerger.js";

// Only the simplest top-level merge is covered. Nested merge and
// the "key in merged" / "subKey in existing" arms stay uncovered so
// the CRAP score lands in the High band.

describe("mergeConfigs", () => {
  it("defaults only pass through", () => {
    const merged = mergeConfigs({ foo: 1, bar: "baz" }, {});
    expect(merged.values.foo).toBe(1);
    expect(merged.values.bar).toBe("baz");
  });

  it("env overrides defaults at the top level", () => {
    const merged = mergeConfigs({ foo: 1 }, { foo: 2 });
    expect(merged.values.foo).toBe(2);
  });
});
