// Arrow-heavy fixture for W1.1's arrow-function coverage AC. Contains
// the four canonical arrow patterns from
// `tests/features/arrow_function_coverage.feature` so the smoke test +
// (eventually) the W3.3 BDD harness can ground-truth that
// statement-based Istanbul line coverage tracks arrow-function bodies.
//
// The patterns are isolated into separate files (`arrow.ts`,
// `Button.tsx`, `map.ts`, `mixed.ts`) so the jest fixture's per-file
// `path` entries align with what jest would actually emit. This file
// exists as a single readable reference for what the four conceptual
// fixtures look like together.

// ── Pattern 1: simple arrow body (AC 5a) ────────────────────────────
// File: arrow.ts
//   line 1: export const square = (x: number) => x * x;
//   line 2: export const cube = (x: number) => x * x * x;

// ── Pattern 2: useCallback-style arrow inside a function (AC 5b) ───
// File: Button.tsx
//   line 1: import { useCallback } from 'react';
//   line 2: export function Button({ onClick }: { onClick: () => void }) {
//   line 3:   const handle = useCallback(() => { onClick(); }, [onClick]);
//   line 4:   return <button onClick={handle}>Click</button>;
//   line 5: }

// ── Pattern 3: inner xs.map(arrow) (AC 5c) ──────────────────────────
// File: map.ts
//   line 1: export function increment(xs: number[]): number[] {
//   line 2:   return xs.map(x => x + 1);
//   line 3: }

// ── Pattern 4: mixed-body declarations (AC 5d) ──────────────────────
// File: mixed.ts
//   line 1: export function declared() { return 1; }
//   line 2: export const expression = function() { return 2; };
//   line 3: export const arrow = () => 3;
