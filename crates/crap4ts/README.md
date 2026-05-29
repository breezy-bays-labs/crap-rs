# crap4ts

[![crates.io](https://img.shields.io/crates/v/crap4ts.svg)](https://crates.io/crates/crap4ts)
[![npm](https://img.shields.io/npm/v/crap4ts.svg)](https://www.npmjs.com/package/crap4ts)
[![docs.rs](https://img.shields.io/docsrs/crap4ts)](https://docs.rs/crap4ts)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

**Rust-powered CRAP (Change Risk Anti-Patterns) score analyzer for TypeScript and JavaScript.** Find the functions in your codebase that are both complex *and* under-tested.

`crap4ts` combines [oxc](https://oxc.rs/)-driven AST complexity with [Istanbul](https://istanbul.js.org/) JSON coverage. One pass, one CI gate, one number per function.

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

High complexity + low coverage = high CRAP = high change risk.

> **JavaScript / Node consumers**: install [`crap4ts` from npm](https://www.npmjs.com/package/crap4ts) — that package ships pre-built Node addons for every supported platform. Its [npm README](https://github.com/breezy-bays-labs/crap-rs/blob/main/packages/crap4ts/README.md) covers Node-side usage.
>
> **This crates.io page** documents the Rust crate: the standalone CLI binary and the `cdylib` artifact that backs the npm package.

## Install (CLI)

```bash
cargo install crap4ts
```

Pre-built CLI binaries for `crap4ts` are tracked for a future release (the napi `cdylib` for the npm package ships pre-built today; the standalone CLI artifact does not yet). Use `cargo install` for now, or install the [npm package](https://www.npmjs.com/package/crap4ts) if you're consuming from Node.

## 30-second tour

```bash
# Generate Istanbul JSON coverage (e.g. via Vitest + istanbul provider)
vitest run --coverage --coverage.reporter=json

# Analyze
crap4ts --coverage coverage/coverage-final.json --src src

# Gate CI strictly
crap4ts --coverage coverage/coverage-final.json --src src --strict
```

## Why cyclomatic complexity (only)

`crap4ts` 2.x ships **cyclomatic complexity** as the only supported metric. Two reasons:

1. **Classic CRAP semantics.** Cyclomatic decision-point count is the original CRAP metric and aligns with how virtually every TypeScript / JavaScript quality tool (ESLint's `complexity` rule, SonarJS, Code Climate, etc.) reports complexity. CI gates and reviewer expectations transfer cleanly.
2. **AST signal density differs from Rust.** TypeScript code doesn't lean as heavily on `match`-style branching as idiomatic Rust does, so the cognitive-vs-cyclomatic divergence is much smaller. Cyclomatic ships first; cognitive may follow in a later release if the ecosystem demand justifies the additional walker logic.

Passing `--metric cognitive` errors out cleanly with `MetricNotSupported`.

The companion Rust analyzer [`crap4rs`](https://crates.io/crates/crap4rs) defaults to cognitive complexity for the inverse reason — Rust idioms benefit from it. The shared CRAP formula, risk tiers, and envelope shape are identical across both adapters; only the complexity number entering the formula differs.

## Threshold presets

`crap4ts` ships three preset gates calibrated against four risk tiers:

| Preset | Threshold | Gates at | Use for |
|---|---|---|---|
| `--strict` | CRAP ≤ 8 | Low → Acceptable | safety-critical, high-quality libraries |
| *(default)* | CRAP ≤ 15 | Acceptable → Moderate | typical app / library code |
| `--lenient` | CRAP ≤ 25 | Moderate → High | legacy / transitional codebases |

The risk tiers themselves:

| CRAP score | Risk |
|---|---|
| ≤ 8 | Low |
| ≤ 15 | Acceptable |
| ≤ 25 | Moderate |
| > 25 | High |

The presets correspond to "gate at the next risk tier up." Override with `--threshold <N>` for a custom value, or define presets per-codebase in `crap.toml` (the canonical config name, written by `crap4ts init`). The legacy name `crap4ts.toml` is still discovered as a deprecated alias when no `crap.toml` is present.

## What it looks like

### Table (TTY default)

```
crap4ts v2.0.0-rc.2 — CRAP Score Analysis

+-----------------------------+-------------------------+----+-------+-------+----------+
| File                        | Function                | CC | Cov%  | CRAP  | Risk     |
+=========================================================================================+
| src/walker.ts               | walkExpression          | 14 |  72.0 | 17.06 | moderate |
| src/cli.ts                  | resolveConfig           | 11 |  88.5 | 11.16 | acceptable |
| src/reporters/markdown.ts   | renderTopOffenders      |  9 | 100.0 |  9.00 | acceptable |
| src/coverage/istanbul.ts    | parseStatementMap       |  8 |  91.2 |  8.05 | acceptable |
+-----------------------------+-------------------------+----+-------+-------+----------+

Functions: 142 · Above threshold: 1 · Worst CRAP: 17.06 · Distribution: 98 low · 32 acceptable · 11 moderate · 1 high
```

### Markdown (`--format markdown`) — PR-comment ready

```markdown
# crap4ts v2.0.0-rc.2 — CRAP Score Analysis

**Result:** FAIL · **Functions:** 142 · **Above threshold (15):** 1

| Metric     | Worst | Average | Median |
|------------|------:|--------:|-------:|
| CRAP       | 17.06 |    2.41 |   1.00 |
| Complexity |    14 |     2.3 |    1.0 |
| Coverage   |  0.0% |   89.4% |  98.5% |

**Risk distribution:** low 98 · acceptable 32 · moderate 11 · high 1
```

### GitHub annotations (`--format github-annotations`) — inline PR review

Drops findings as inline warnings on the PR Files Changed tab, no GHAS / Code Scanning license needed:

```
::warning file=src/walker.ts,line=128,title=CRAP 17.1::Function `walkExpression` has CRAP 17.06 (complexity=14, coverage=72.0%) which exceeds threshold 15.0
```

### JSON (`--format json`) — programmatic consumption

```json
{
  "schema_version": 2,
  "run_meta": { "tool": "crap4ts", "version": "2.0.0-rc.2", "metric": "cyclomatic" },
  "result": {
    "summary": {
      "total_functions": 142,
      "exceeding_threshold": 1,
      "distribution": { "low": 98, "acceptable": 32, "moderate": 11, "high": 1 },
      "max_crap": { "value": 17.06, "risk_level": "moderate" }
    },
    "functions": [
      {
        "scored": {
          "identity": { "file_path": "src/walker.ts", "qualified_name": "walkExpression", "span": { "start_line": 128, "end_line": 184 } },
          "complexity": 14,
          "complexity_metric": "cyclomatic",
          "coverage_percent": 72.0,
          "crap": { "value": 17.06, "risk_level": "moderate" }
        },
        "threshold": 15.0,
        "exceeds": true
      }
    ]
  }
}
```

The wire envelope is byte-identical to `crap4rs`'s output — same fields, same risk-tier strings, same delta-gate semantics. A multi-language monorepo can drive a single CI gate across both ecosystems.

### Other formats

- `--format csv` — spreadsheet ingestion
- `--format sarif` — Code Scanning upload (requires GHAS) for surface-as-security-finding flows
- `--format scorecard` — single-row CI gate output for cross-PR delta tracking
- **HTML report** — interactive, sortable, with per-function contributor drill-down. *Coming in a future release; file an issue for early-access interest.*

Multiple formats compose in one pass: `--format json:envelope.json,markdown:report.md` writes both files from a single analysis.

## Delta gates — fail PRs that introduce new high-CRAP functions

```bash
# Generate baseline on main
crap4ts --coverage main-coverage/coverage-final.json --src src --format json > baseline.json

# On PR — fail only on NEW threshold violations
crap4ts --coverage pr-coverage/coverage-final.json --src src --baseline baseline.json --delta-gate
```

A function above-threshold on main doesn't fail the PR; only functions newly introduced or newly elevated do. Lets teams ratchet quality forward without blocking every PR on pre-existing debt.

## What this is (architecture)

`crap4ts` is the TypeScript / JavaScript adapter in the [`crap-rs`](https://github.com/breezy-bays-labs/crap-rs) workspace. It compiles to two artifacts from one crate:

- A standalone Rust CLI binary, distributed via crates.io (this page) and `cargo binstall`
- A [napi-rs](https://napi.rs/) `cdylib` Node addon, distributed via [npm](https://www.npmjs.com/package/crap4ts)

Both share the same walker (`oxc` for complexity), the same Istanbul JSON coverage parser, and the same [`crap-core`](https://crates.io/crates/crap-core) scoring/reporter pipeline as the Rust adapter [`crap4rs`](https://crates.io/crates/crap4rs).

## Library use (Rust)

```toml
[dependencies]
crap4ts = "2"
```

Most users want the CLI or the npm package; the library crate is intended for downstream tooling that needs programmatic access to TypeScript walking + scoring without spawning a subprocess.

## Stability

`crap4ts` is in the `2.0.0-rc.x` release-candidate series ahead of the GA `2.0.0` cut. The CLI surface, configuration shape, and scorecard envelope are locked across the rc series; rc bumps fix bugs and tighten the walker. See the [changelog](https://github.com/breezy-bays-labs/crap-rs/blob/main/packages/crap4ts/CHANGELOG.md) for the per-version history.

## See also

- **Repository**: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- **npm package**: [`crap4ts` on npm](https://www.npmjs.com/package/crap4ts) — for JS/Node consumers
- **Rust analyzer**: [`crap4rs`](https://crates.io/crates/crap4rs)
- **Shared core library**: [`crap-core`](https://crates.io/crates/crap-core)
- **Issues**: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
