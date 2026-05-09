---
paths:
  - "src/domain/**"
---

# Domain

When working with domain code:

1. **Purity** — no I/O, no external crates, no `std::fs`. Domain code is pure logic only.
2. **No inward imports** — domain imports nothing from ports, adapters, core, or cli.
3. **Types live here** — all shared type definitions (`FunctionComplexity`, `CrapScore`, `SourceSpan`, etc.) are defined in `domain/types.rs`.
4. **CRAP formula** — `complexity² × (1 - coverage/100)³ + complexity`. Changes to the formula must update both `crap.rs` and its tests.
5. **Inclusive spans** — `SourceSpan` uses inclusive `end_line` (differs from crap4ts which uses exclusive). This matches Rust/syn conventions.
6. **Metric-agnostic formula** — `compute_crap()` accepts a numeric complexity value. It does not know or care whether the input is cognitive or cyclomatic complexity.
7. **Property tests** — CRAP formula must have property tests validated against crap4ts reference values.
8. **Coordinates, not foreign refs** — domain types carry stable coordinates (line ranges, span endpoints, AST-derived enum chains), never references to a foreign AST library's nodes (`syn::Node`, `tree_sitter::Node`, etc.). Foreign refs leak adapters into the domain, are not stable across `cargo` re-parses, and break cross-language schema parity (TS, Python, etc. don't have `syn`). Consumers that want richer signal — e.g., to drive a refactor through rust-analyzer — derive it locally from the coordinates the domain emits, or sit in an adapter that has its own access to the AST.
