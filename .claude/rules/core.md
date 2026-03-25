---
paths:
  - "src/core/**"
---

# Core

When working with core (wiring) code:

1. **Composition only** — core wires adapters through port traits. No business logic here.
2. **`analyze()` is the main entry point** — takes options, returns `AnalysisResult`.
3. **Dependency injection** — accept port trait objects, use defaults when not provided.
4. **File discovery** — use `walkdir` + glob patterns. Default excludes: `target/`, `tests/`, `**/test*`.
5. **Error propagation** — use `anyhow::Result` for the orchestration layer. Domain errors are typed (`CrapError`).
