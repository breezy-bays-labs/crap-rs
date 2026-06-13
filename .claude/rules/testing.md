---
paths:
  - "tests/**"
  - "**/*test*"
---

# Testing — crap4rs overlay

crap4rs's thin overlay on the org **Testing Strategy**
(`~/Github/ops/standards/testing-strategy.md`; meta-principles are always
injected via `~/.claude/rules/testing-framework.md`). The canonical doc says
*which level and why*; this file says *which tool*. Apply the **Boundary
Rule**: test each behavior once, at the lowest level that fully captures it;
a `.feature` scenario earns its place only if it is a product-level
**contract** (a CLI flag's observable effect, an output format's shape, a
gate's pass/fail, config discovery/precedence, an end-to-end result).
Parser/matching/formula edge cases are implementation details — they belong
in unit/property/fuzz, not the `.feature` corpus.

## crap4rs tools by level

| Level | Tool | Notes |
|-------|------|-------|
| Unit | `cargo nextest` | domain + adapter `#[test]` |
| Property | `proptest` | CRAP formula, LCOV parser, line-range matching, walker invariants. Regression files in `proptest-regressions/` are **committed**, never gitignored |
| **Fuzz (Q4)** | `cargo-fuzz` / `bolero` — **planned** | the LCOV/Istanbul parsers ingest untrusted input; no fuzz target yet |
| Integration | `cargo nextest` shelling the built binary against real fixtures | real `.rs` for complexity, real LCOV for coverage; no adapter mocking |
| Acceptance (BDD) | `cucumber-rs` | `tests/features/*.feature` + `*_cucumber.rs`; hygiene enforced by `bdd-tracked-lint.py` — see AGENTS.md "BDD hygiene" |
| E2E / smoke | the composite scorecard action; Playwright DOM validation of the unified HTML | CI-only |
| Coverage | `cargo llvm-cov` | |
| Risk (CRAP) | `crap4rs` self-gate — strict-8 `scorecard-production` | dogfoods its own analyzer |
| Mutation | `cargo-mutants` | dual-file surface (`view.rs` per-PR, walker per-merge) + `mutants-skip-lint.py` guard-the-guard |
| Fitness functions | `clippy -D warnings`, the four-layer `ast-purity` job, `deny.toml`, dependency direction | |
| Performance | — *(aspirational)* | no benchmarks yet |

## crap4rs rules

1. **TDD** — tests before implementation for domain + adapter code.
2. **Domain purity** — `domain/`/`ports/` import no external crates and do no I/O.
3. **Fixtures over mocks** — real `.rs` + real LCOV; only mock I/O boundaries.
4. **Golden-file** — `insta` for wire/report snapshots; `cargo insta accept` only after verifying the drift is span/coverage-only.
5. **Assert behavior, not implementation** — `qualified_name`/`complexity` values, not AST-traversal internals.
6. **Self-referential** — crap4rs analyzes its own source as an integration test.
7. **Boundary Rule** — keep `.feature` for product contracts; push engine internals down to unit/property/fuzz.
