# CLAUDE.md — crap4rs

## Architecture

Hexagonal (ports & adapters) with strict dependency direction:

```
domain/ → ports/ → adapters/ → core/ → cli/
```

- **domain/** — Pure logic (CRAP formula, matching, thresholds, types). No I/O, no external crates.
- **ports/** — Trait definitions for complexity extraction and coverage parsing.
- **adapters/** — Implementations of port traits (syn complexity walker, LCOV parser, reporters).
- **core/** — Wiring layer: composes adapters through ports, exposes `analyze()` API.
- **cli/** — Thin shell over core (clap). Argument parsing, output formatting.

## Dependency Direction

Never import "inward":
- `domain` imports nothing external
- `ports` use domain types only
- `adapters` implement port traits, may use external crates (syn, serde)
- `core` wires adapters through ports
- `cli` calls core

## Key Design Decisions

### Line-Range Matching (not name matching)

crap4rs matches complexity with coverage using **line ranges**, not function names. This is a deliberate divergence from crap4ts's span-overlap matching:

- Syn extracts `(file, fn_name, start_line, end_line, complexity)` per function
- LCOV `DA:` lines give `(file, line_number, hit_count)` per line
- Coverage for a function = DA lines within its line range
- No demangling, no name matching, no span-overlap logic needed

This works because Rust functions have deterministic line ranges from the AST, and LCOV DA data is per-line.

### Complexity Metrics

Supports both cognitive (default) and cyclomatic complexity via `--metric` flag:
- **Cognitive** (default for Rust): penalizes nesting, better for match-heavy idiomatic Rust
- **Cyclomatic**: counts decision points, matches original CRAP paper

The `ComplexityPort` trait is metric-agnostic — it returns a numeric value per function.

### LCOV Only

MVP parses `cargo-llvm-cov --lcov` output exclusively. Only `SF:` and `DA:` records are used. `FN:`/`FNDA:` records are ignored (mangled Rust symbols, redundant with line-range strategy).

## Development Rules

- **TDD** — Write tests before implementation for all domain and adapter code.
- **Domain purity** — `src/domain/` must never import external crates or perform I/O.
- **Dependency direction** — Never import "inward".
- **Property tests** — CRAP formula must have property tests validated against crap4ts reference values.

## Commands

| Task | Command |
|------|---------|
| Build | `cargo build` |
| Test | `cargo nextest run` (or `cargo test`) |
| Coverage | `cargo llvm-cov --lcov --output-path lcov.info` |
| Lint | `cargo clippy -- -D warnings` |
| Format | `cargo fmt` |
| Quick verify | `cargo fmt --check && cargo clippy -- -D warnings && cargo test` |

## Commit Convention

Conventional commits with architectural scope:

```
feat(domain):  feat(ports):  feat(adapters):  feat(core):  feat(cli):
fix(domain):   test:         ci:              docs:        chore:
```

## Reference

- **crap4ts** (TypeScript equivalent): `~/Github/crap4ts/`
- **Pipeline note**: `~/Github/ops/pipelines/crap4rs/crap4rs-20260324-council-spike.md`
- **Testing standard**: `~/Github/ops/standards/testing.md`

## Compact Instructions

During context compaction, preserve:
- Architecture (ports-and-adapters, dependency direction)
- Line-range matching strategy (not name matching)
- Metric design (cognitive default, cyclomatic via flag)
- Which sessions/milestones are complete vs remaining
- Reference values from crap4ts for property tests

Discard:
- Full file contents from reads older than 5 tool calls
- Search results not acted on
- Detailed spike output (findings are captured in pipeline note)
