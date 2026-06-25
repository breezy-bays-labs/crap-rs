# Understanding the CRAP score

CRAP (Change Risk Anti-Patterns) scores a single function by combining its complexity with its test coverage. High complexity is tolerable when coverage is high; the same complexity with no coverage is dangerous to change. CRAP makes that tradeoff a number.

## The formula

```
CRAP(c, cov) = c² × (1 − cov)³ + c
```

- `c` is the function's complexity (cognitive or cyclomatic — see below). Always an integer ≥ 1.
- `cov` is the line-coverage **fraction** in `[0, 1]` in this published form.

Internally `compute_crap` takes coverage as a **percent** in `[0, 100]`, clamps it to that range, then divides by 100 before applying the cube. Keep the units straight: the formula above uses a fraction; the implementation signature uses a percent. Coverage outside the range is clamped (150% scores as 100%, −10% scores as 0%), never rejected. The result is rounded to two decimal places.

## Two identities

The cube term is what makes coverage dominate. Two endpoints fall out directly:

| Coverage | Score | Why |
|----------|-------|-----|
| 100% | `c` | `(1 − 1)³ = 0`, so only the trailing `+ c` survives. A fully covered function scores its raw complexity. |
| 0% | `c² + c` | `(1 − 0)³ = 1`, so the full `c²` term is added. An untested function's score grows quadratically with complexity. |

The score is always ≥ 1, monotonically increases with complexity, and monotonically decreases with coverage.

## Worked values

| Complexity | Coverage | CRAP | Calculation |
|------------|----------|------|-------------|
| 1 | 100% | 1.00 | `1 × 0 + 1` |
| 1 | 0% | 2.00 | `1 × 1 + 1` |
| 10 | 100% | 10.00 | `100 × 0 + 10` |
| 10 | 0% | 110.00 | `100 × 1 + 10` |
| 5 | 50% | 8.13 | `25 × 0.5³ + 5 = 25 × 0.125 + 5` |
| 15 | 90% | 15.23 | `225 × 0.1³ + 15 = 225 × 0.001 + 15` |

The 5@50% case shows the leverage of coverage: halving the tested lines on a complexity-5 function adds only ~3 to the score, but the same function untested would score 30.

## Cognitive vs cyclomatic complexity

CRAP is agnostic to which complexity metric feeds it — the formula is identical. crap4rs supports both:

| Metric | Counts | Default |
|--------|--------|---------|
| Cognitive | Nesting depth + structural complexity. Each construct adds `1 + nesting_depth`. | crap4rs default |
| Cyclomatic | Decision points (branches). Each construct adds 1, flat. | Classic CRAP metric |

Cognitive is the crap4rs default because it suits match-heavy Rust. A flat `match` with N arms is one decision a reader processes at once, not N independent branches: cognitive scores it as **1**, cyclomatic as **N**. Nesting `match` inside a loop inside an `if` is what cognitive penalizes — and that is what actually makes Rust hard to change. crap4ts (the TypeScript adapter) is cyclomatic-only.

Because the two metrics produce different-magnitude scores for the same code, never compare a cognitive CRAP score against a cyclomatic one as if they were the same scale.

## What coverage means

Coverage is **line coverage**, measured by a positional heuristic — not by function name. The complexity walker emits each function as a `(file, start_line, end_line)` span; the LCOV parser emits `(file, line, hits)` records. A function's coverage is the fraction of instrumented lines that fall **within its span** (inclusive on both ends) and were hit at least once.

This is positional matching, not name matching. There is no symbol demangling and no `FN`/`FNDA` parsing. A function whose span contains no instrumented lines is treated as 100% covered (nothing to miss). Coverage is computed strictly per file — lines in one file never count toward a function in another.

Branch coverage, when present, is collected the same way (branch points within the span) and surfaced informationally. The CRAP gate is **line-only**; branch coverage never feeds the score.

For where the spans and coverage records come from, see [introduction](introduction.md). For coverage-file generation, see [quick start](quick-start.md).

## Risk bands

Every score is classified into one of four bands by `classify_risk`:

| Band | Score range |
|------|-------------|
| Low | ≤ 8 |
| Acceptable | ≤ 15 |
| Moderate | ≤ 25 |
| High | > 25 |

The bands are score-based, fixed, and metric-agnostic. They describe a function's intrinsic risk regardless of any threshold you set.

## Bands and the gate are two separate axes

The cutoffs 8 / 15 / 25 appear twice in this tool, and they mean different things:

- **Risk bands** (Low / Acceptable / Moderate / High) are a *classification*, derived purely from the score via `classify_risk`. They never change.
- **The threshold gate** is a *pass/fail line*. A function "exceeds" when its score is above the active threshold. The default gate is 15; the strict preset is 8 and the lenient preset is 25.

These two axes share the numbers 8 / 15 / 25 today by calibration convention — the presets are set at the band boundaries — but they are conceptually independent. A function can sit in the Moderate band (score 20) and still pass a lenient gate (threshold 25). Never read "risk level: Moderate" as "exceeds threshold," or vice versa. The thresholds themselves are a calibration convention, not empirically derived values.

For how to set the gate, see [CLI reference](cli-reference.md) and [configuration](configuration.md).
