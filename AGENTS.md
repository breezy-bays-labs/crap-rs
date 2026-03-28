# crap4rs Agent Notes

- This repo uses a shared Cargo target directory via `.cargo/config.toml`; let worktrees inherit it normally.
- Preserve any worktree the user identifies as active.

## Repo Context

- Architecture: hexagonal (ports & adapters) — `domain/` → `ports/` → `adapters/` → `core/` → `cli/`. Never import "inward."
- Testing: `cargo nextest run` for tests, `cargo clippy -- -D warnings` for lints, `cargo fmt --check` for formatting. Quick verify: all three chained.
- Property tests use `proptest` — regression files in `proptest-regressions/` are committed to git, never gitignored.
- Safety: do not push directly to `main`, always branch + PR. Do not modify `.github/workflows/*` unless the task clearly requires CI changes.
- The `domain/` and `ports/` layers must stay language-agnostic (no `syn`, no LCOV types) — they are designed for future extraction into a shared `crap-core` library.
