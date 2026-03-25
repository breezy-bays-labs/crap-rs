---
paths:
  - "tests/**"
  - "**/*test*"
---

# Testing

When working with tests:

1. **TDD** — write tests before implementation for domain and adapter code.
2. **Test runner** — `cargo nextest run` preferred, `cargo test` as fallback.
3. **Fixtures** — test fixtures live in `tests/fixtures/`. Use real `.rs` files for complexity and real LCOV files for coverage.
4. **Golden-file tests** — use `insta` for snapshot testing of analysis output (JSON format).
5. **Property tests** — use `proptest` for CRAP formula invariants. Reference values from crap4ts.
6. **No mocking adapters in integration tests** — use the real adapter with fixture files. Only mock I/O boundaries.
7. **Assert behavior, not implementation** — test `qualified_name` and `complexity` values, not internal AST traversal details.
8. **Self-referential test** — crap4rs should be able to analyze its own source code as an integration test.
