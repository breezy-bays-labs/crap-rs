/**
 * Baseline anchor: low complexity, high coverage → CRAP near 1.
 *
 * Append-only event log with structured records. Each function does
 * one straight-line thing: no branching, no nested conditionals.
 * Cyclomatic complexity stays at 1 across the surface, and every
 * branch is exercised by the tests below, so the CRAP score lands in
 * the Low band. This module anchors the bottom of the pedagogical
 * heatmap.
 */

export interface Event {
  kind: string;
  message: string;
  timestampMs: number;
}

export class EventLog {
  private events: Event[] = [];

  append(event: Event): void {
    this.events.push(event);
  }

  length(): number {
    return this.events.length;
  }

  isEmpty(): boolean {
    return this.events.length === 0;
  }

  last(): Event | undefined {
    return this.events[this.events.length - 1];
  }

  clear(): void {
    this.events = [];
  }

  toArray(): Event[] {
    return [...this.events];
  }
}

export function makeEvent(
  kind: string,
  message: string,
  timestampMs: number,
): Event {
  return { kind, message, timestampMs };
}
