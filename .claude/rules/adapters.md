---
paths:
  - "src/adapters/**"
---

# Adapters

When working with adapter code:

1. **Implements port traits** — every adapter must implement a trait from `src/ports/`. Never add public methods beyond the port contract.
2. **Complexity adapter** — uses `syn` 2.x to parse Rust ASTs. Walks `ItemFn` and `ImplItemFn`. Supports both cognitive and cyclomatic metrics via the `ComplexityMetric` enum.
3. **Coverage adapter** — parses LCOV format only (from `cargo-llvm-cov --lcov`). Only uses `SF:` and `DA:` records. Ignores `FN:`/`FNDA:` (mangled Rust symbols, unused with line-range matching).
4. **No demangling** — function matching uses line ranges from syn, not LCOV function names. Never parse or demangle `FN:` entries.
5. **Reporter adapters** — terminal table (comfy-table) and JSON (serde_json). Reporters must not import from domain or core.
6. **Impl blocks** — the complexity walker must handle methods in `impl` blocks. Qualified names use `Type::method` format.
7. **Keep CC low** — adapter methods tend toward high complexity from AST traversal. Extract helper functions aggressively.
