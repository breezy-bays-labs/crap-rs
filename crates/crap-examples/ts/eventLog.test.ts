import { describe, expect, it } from "vitest";
import { EventLog, makeEvent } from "./eventLog.js";

describe("EventLog", () => {
  it("starts empty", () => {
    const log = new EventLog();
    expect(log.isEmpty()).toBe(true);
    expect(log.length()).toBe(0);
    expect(log.last()).toBeUndefined();
  });

  it("appends increase length", () => {
    const log = new EventLog();
    log.append(makeEvent("start", "boot", 0));
    log.append(makeEvent("step", "init", 100));
    expect(log.length()).toBe(2);
    expect(log.isEmpty()).toBe(false);
  });

  it("last returns most recent event", () => {
    const log = new EventLog();
    log.append(makeEvent("a", "first", 1));
    log.append(makeEvent("b", "second", 2));
    expect(log.last()?.kind).toBe("b");
    expect(log.last()?.message).toBe("second");
  });

  it("clear resets the log", () => {
    const log = new EventLog();
    log.append(makeEvent("x", "y", 0));
    log.clear();
    expect(log.isEmpty()).toBe(true);
  });

  it("toArray returns a copy of the events", () => {
    const log = new EventLog();
    log.append(makeEvent("a", "1", 1));
    const arr = log.toArray();
    expect(arr).toHaveLength(1);
    expect(arr[0].kind).toBe("a");
  });

  it("makeEvent populates fields", () => {
    const event = makeEvent("kind", "message", 42);
    expect(event.kind).toBe("kind");
    expect(event.message).toBe("message");
    expect(event.timestampMs).toBe(42);
  });
});
