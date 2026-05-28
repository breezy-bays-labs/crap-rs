import { describe, expect, it } from "vitest";
import { pluralize, slugify, truncate } from "./stringUtils.js";

// slugify: only the happy path is covered. Branches for
// non-ASCII alphabetic, non-ASCII numeric, all-stripped error,
// and punctuation pass-through stay uncovered so the CRAP score
// lands in the High band.
describe("slugify", () => {
  it("lowercases ASCII", () => {
    expect(slugify("Hello World")).toBe("hello-world");
  });

  it("collapses whitespace runs", () => {
    expect(slugify("hello   world")).toBe("hello-world");
  });
});

describe("truncate", () => {
  it("returns short strings unchanged", () => {
    expect(truncate("hi", 10)).toBe("hi");
  });

  it("clips long strings with an ellipsis", () => {
    expect(truncate("hello world", 5)).toBe("hello…");
  });

  it("returns exact-length strings unchanged", () => {
    expect(truncate("hello", 5)).toBe("hello");
  });
});

describe("pluralize", () => {
  it("empty string yields empty string", () => {
    expect(pluralize("")).toBe("");
  });

  it("regular nouns append s", () => {
    expect(pluralize("cat")).toBe("cats");
  });

  it("nouns ending in s append es", () => {
    expect(pluralize("bus")).toBe("buses");
  });

  it("nouns ending in x append es", () => {
    expect(pluralize("box")).toBe("boxes");
  });

  it("nouns ending in z append es", () => {
    expect(pluralize("buzz")).toBe("buzzes");
  });

  it("nouns ending in ch append es", () => {
    expect(pluralize("church")).toBe("churches");
  });

  it("nouns ending in sh append es", () => {
    expect(pluralize("dish")).toBe("dishes");
  });
});
