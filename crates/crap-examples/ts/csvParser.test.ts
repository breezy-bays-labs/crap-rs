import { describe, expect, it } from "vitest";
import { parseAll, parseRecord } from "./csvParser.js";

describe("parseRecord", () => {
  it("empty input yields a single empty field", () => {
    const { record } = parseRecord("");
    expect(record.fields).toEqual([""]);
  });

  it("parses three unquoted fields", () => {
    const { record, consumed } = parseRecord("a,b,c");
    expect(record.fields).toEqual(["a", "b", "c"]);
    expect(consumed).toBe(5);
  });

  it("newline terminates the record", () => {
    const { record, consumed } = parseRecord("a,b\nrest");
    expect(record.fields).toEqual(["a", "b"]);
    expect(consumed).toBe(4);
  });

  it("strips surrounding quotes from quoted fields", () => {
    const { record } = parseRecord('"hello",world');
    expect(record.fields).toEqual(["hello", "world"]);
  });

  it("embeds comma inside quotes", () => {
    const { record } = parseRecord('"a,b",c');
    expect(record.fields).toEqual(["a,b", "c"]);
  });

  it("throws on unterminated quote", () => {
    expect(() => parseRecord('"abc')).toThrow(/unterminated/);
  });
});

describe("parseAll", () => {
  it("returns multiple records", () => {
    const records = parseAll("a,b\nc,d\ne,f\n");
    expect(records).toHaveLength(3);
    expect(records[0].fields).toEqual(["a", "b"]);
  });

  it("handles trailing record without newline", () => {
    const records = parseAll("a,b\nc,d");
    expect(records).toHaveLength(2);
  });
});
