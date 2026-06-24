# Introduction

CRAP (Change Risk Anti-Patterns) is a single number per function that fuses how complex the code is with how well it is tested. Complex code with thin coverage is the code most likely to break when you change it, and CRAP makes that risk one sortable column: a function the size of a one-liner with full coverage scores low; a deeply branched function with no tests scores high. You point the tool at your source and a coverage report, and it ranks every function by how dangerous it is to touch.

## The formula

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

`coverage` is the fraction in `[0, 1]` (50% is `0.5`). At full coverage the score collapses to the bare complexity (`complexity² × 0 + complexity`); at zero coverage it is `complexity² + complexity`, the maximum penalty. The cubed uncovered term means missing coverage on complex code dominates fast. The full derivation, worked examples, and the unit distinction (the published formula takes a fraction; the implementation takes a percent) live in [understanding CRAP](understanding-crap.md).

## Two adapters, one core

The metric is language-agnostic, so the workspace splits the universal math from the language-specific extraction:

| Binary | Analyzes | Complexity source | Coverage format |
|--------|----------|-------------------|-----------------|
| `crap4rs` | Rust | `syn` AST walk | LCOV (`lcov.info`) |
| `crap4ts` | TypeScript / JavaScript | `oxc` AST walk | Istanbul JSON (`coverage-final.json`) |
| `crap-core` | — (shared library) | — | — |

`crap-core` owns the CRAP formula, the analysis types, the reporters, and the wire envelope. Both adapters link it, so a Rust project and a TypeScript project get identical CRAP semantics — the same arithmetic, the same output shapes. The adapters differ only in how they parse source and read coverage.

`crap-core` also ships a `crap-render` binary that takes one or more pre-computed envelopes and renders a combined report. Cross-language scores are not directly comparable on their own axis; the combined view ranks functions by their CRAP-to-threshold ratio and risk band rather than raw score. See [multi-language analysis](multi-language.md).

## Who each binary is for

- **`crap4rs`** — Rust authors who produce LCOV via `cargo-llvm-cov`.
- **`crap4ts`** — TypeScript and JavaScript authors who produce Istanbul JSON via their test runner.
- **`crap-render`** — anyone combining envelopes from more than one language into one report (a polyglot repo, a CI summary).

The two adapters never need each other. Pick the one matching your language.

## How this book is laid out

- [Installation](installation.md) — get the binary onto your machine.
- [Quick start](quick-start.md) — first analysis in a few commands.
- [Understanding CRAP](understanding-crap.md) — the formula, risk bands, and the threshold gate.
- [CLI reference](cli-reference.md) — every flag for the adapter binaries.
- [Configuration](configuration.md) — the config file schema and precedence.
- [Output formats](output-formats.md) — the table, JSON, HTML, and other reporters.
- [CI integration](ci-integration.md) — the scorecard composite action and gating in pipelines.
- [Multi-language analysis](multi-language.md) — `crap-render` and the combined view.
- [Limitations & FAQ](limitations-and-faq.md) — what CRAP measures, what it does not, and the honest edges.
