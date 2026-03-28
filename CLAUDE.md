@AGENTS.md

# CLAUDE.md — crap4rs

CRAP (Change Risk Anti-Patterns) analyzer for Rust codebases. Library crate + thin CLI binary.

## Architecture

Hexagonal (ports & adapters), strict dependency direction:

```
domain/ → ports/ → adapters/ → core/ → cli/
```

| Layer | Purpose | External crates? |
|-------|---------|-----------------|
| `domain/` | CRAP formula, matching, thresholds, types | None — pure logic |
| `ports/` | Trait definitions (ComplexityPort, CoveragePort) | Domain types only |
| `adapters/` | syn walker, LCOV parser, reporters | syn, serde, comfy-table |
| `core/` | Wires adapters through ports, exposes `analyze()` | Ports + adapters |
| `cli/` | clap argument parsing, output formatting | clap, core |

Never import "inward." Library crate layout preserves future `crap-core` extraction.

## Design Decisions

**Line-range matching** (not name matching): syn gives `(file, start_line, end_line)` per function, LCOV `DA:` gives `(file, line, hits)`. Join on file + line range. No demangling, no FN/FNDA parsing.

**Complexity metrics**: cognitive (default, better for match-heavy Rust) and cyclomatic via `--metric` flag. `ComplexityPort` is metric-agnostic.

**LCOV only**: parse `SF:` and `DA:` records from `cargo-llvm-cov --lcov`. Ignore `FN:`/`FNDA:` (mangled symbols, redundant).

## Commands

| Task | Command |
|------|---------|
| Build | `cargo build` |
| Test | `cargo nextest run` (or `cargo test`) |
| Coverage | `cargo llvm-cov --lcov --output-path lcov.info` |
| Lint | `cargo clippy -- -D warnings` |
| Format | `cargo fmt` |
| Quick verify | `cargo fmt --check && cargo clippy -- -D warnings && cargo nextest run` |

## Development Rules

- **TDD** — tests before implementation for all domain and adapter code
- **Domain purity** — `src/domain/` must never import external crates or perform I/O
- **Dependency direction** — never import "inward"
- **Property tests required** — CRAP formula, LCOV parser, line-range matching, complexity walker (see invariants below)
- **Fixtures over mocks** — real `.rs` files for complexity, real LCOV for coverage
- **Self-referential test** — crap4rs must analyze its own source as an integration test
- **Cross-validate** — CRAP formula output must match crap4ts reference values exactly
- **Regression files committed** — `proptest-regressions/` dirs are committed to git, never gitignored. Commit regression file + fix in the same PR. Delete regression files only when the corresponding test is deleted.

## Property Test Invariants

| Function | Key Invariants |
|----------|---------------|
| `compute_crap()` | `crap(c, 100%) == c`, `crap(c, 0%) == c^2 + c`, monotonic in both dimensions, always >= 1.0 |
| LCOV parser | empty input → empty result (no panic), malformed lines skipped, DA values non-negative |
| Line-range matching | coverage always in [0, 100], file-scoped (no cross-file leakage), no off-by-one on range boundaries |
| Syn walker | complexity >= 1 for any parseable fn, flat match: cognitive=1 vs cyclomatic=N |

**crap4ts reference values** (oracle test — must match exactly):

| Complexity | Coverage | CRAP |
|------------|----------|------|
| 1 | 100% | 1.00 |
| 1 | 0% | 2.00 |
| 10 | 100% | 10.00 |
| 10 | 0% | 110.00 |
| 5 | 50% | 8.13 |
| 15 | 90% | 15.23 |

## Session Plan

| Session | Issues | Work | Notes |
|---------|--------|------|-------|
| 1 | #1, #2, #3 | LCOV parser + syn walker + matching | #1 and #2 can parallelize |
| 2 | #4, #5 | clap CLI + reporters | |
| 3 | #6 | Core `analyze()` + integration tests | Self-referential test |
| 4 | (if needed) | Dogfooding edge cases, crates.io prep | |

## Commit Convention

```
feat(domain):  feat(ports):  feat(adapters):  feat(core):  feat(cli):
fix(domain):   test:         ci:              docs:        chore:
```

## Worktree Setup

```bash
git worktree add ../crap4rs-issue-N -b feat/topic-name
```

Shared target directory in `.cargo/config.toml` — all worktrees share one `target/`.

## Future: Unified `crap` Monorepo

crap4rs is designed for extraction into a unified `crap` monorepo that supports multiple languages (Rust, TypeScript, and potentially others). The current ports-and-adapters architecture maps directly to the future structure:

```
crap/ (future monorepo)
├── crates/
│   ├── crap-core/           ← domain/ + ports/ + core/ from this repo
│   ├── crap-rust/           ← adapters/ from this repo (syn walker, LCOV parser)
│   ├── crap-typescript/     ← new: tree-sitter-typescript, Istanbul JSON parser
│   └── crap-cli/            ← cli/ from this repo, expanded for multi-language
├── bindings/napi/           ← napi-rs for npm distribution
└── packages/crap4ts/        ← npm wrapper replacing current crap4ts
```

**What this means for development now:**
- `domain/` and `ports/` must stay **language-agnostic** — no Rust-specific assumptions, no `syn` imports, no LCOV-specific types. These become `crap-core`.
- `ComplexityPort` and `CoveragePort` traits must work for any language's complexity/coverage data.
- Report generation, config parsing, threshold logic, and the CRAP formula belong in domain/core — not in adapters.
- `adapters/` is where all Rust-specific code lives (syn walker, LCOV parser). These become `crap-rust`.
- When adding new features, ask: "Is this language-specific or universal?" Universal → domain/core. Language-specific → adapters.

## Cross-References

- **crap4ts** (TS equivalent): [breezy-bays-labs/crap4ts](https://github.com/breezy-bays-labs/crap4ts)
- **Unified monorepo tracking**: breezy-bays-labs/ops#231

## Compact Instructions

Preserve: architecture, line-range matching strategy, metric design, property test invariants, session/milestone progress.
Discard: full file contents from old reads, search results not acted on, completed PR details.
