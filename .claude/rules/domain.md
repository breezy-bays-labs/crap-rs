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
