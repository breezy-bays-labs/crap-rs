---
paths:
  - "src/cli/**"
  - "src/main.rs"
---

# CLI

When working with CLI code:

1. **Thin shell** — CLI parses args with clap, calls `core::analyze()`, formats output. No business logic.
2. **Exit codes** — 0 on pass, 1 on threshold violations, 2 on errors.
3. **Flags mirror crap4ts** where applicable: `--src`, `--coverage`, `--threshold`, `--metric`, `--format`.
4. **Default metric is cognitive** — differs from crap4ts (cyclomatic). Document this prominently.
5. **Helpful errors** — missing coverage file, no source files found, invalid threshold should produce actionable messages.
