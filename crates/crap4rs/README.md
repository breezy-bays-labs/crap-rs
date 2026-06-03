# crap4rs

[![crates.io](https://img.shields.io/crates/v/crap4rs.svg)](https://crates.io/crates/crap4rs)
[![docs.rs](https://img.shields.io/docsrs/crap4rs)](https://docs.rs/crap4rs)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

**CRAP (Change Risk Anti-Patterns) score analyzer for Rust.** Find the functions in your codebase that are both complex *and* under-tested — the ones most likely to break when changed.

`crap4rs` combines [`syn`](https://crates.io/crates/syn)-driven AST complexity with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) LCOV output. One pass, one CI gate, one number per function.

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

High complexity + low coverage = high CRAP = high change risk.

## Install

```bash
# Pre-built binaries (recommended)
cargo binstall crap4rs

# From source
cargo install crap4rs
```

## 30-second tour

```bash
cargo llvm-cov --lcov --output-path lcov.info     # generate coverage
crap4rs --coverage lcov.info                       # analyze
crap4rs --coverage lcov.info --strict              # gate CI strictly
crap4rs --coverage lcov.info --format markdown     # PR-comment friendly
```

## Why cognitive complexity (default)

`crap4rs` defaults to **cognitive complexity**, not cyclomatic. Rust code leans heavily on `match`, nested control flow, and combinator chains — and cyclomatic complexity counts every `match` arm as +1, which inflates the score on perfectly readable Rust idioms.

Cognitive complexity scores how *hard a function is to follow*, weighting by nesting depth and structural break-up rather than raw branch count. A 12-arm `match` over an enum scores 1 (one decision); a deeply nested `if let Some(_) = ... { if let Ok(_) = ... { ... } }` scores higher because the nesting hurts readers.

Cyclomatic is still available — `--metric cyclomatic` — for parity with classic CRAP tooling or comparing against other complexity reporters.

## Threshold presets

`crap4rs` ships three preset gates calibrated against four risk tiers:

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

The presets correspond to "gate at the next risk tier up." Override with `--threshold <N>` for a custom value, or define presets per-codebase in `crap.toml` (the canonical config name, written by `crap4rs init`). The legacy name `crap4rs.toml` is still discovered as a deprecated alias when no `crap.toml` is present.

### Files missing from coverage

When a source file is absent from the coverage report — a `#[cfg(feature = "…")]` module left out of the coverage build, an untested file, or a scoped per-package run — its functions are scored at 0% coverage by default, which inflates their CRAP (a complexity-`c` function jumps to `c² + c`). Choose how that case is handled with `--missing-coverage-policy` (or a `missing_coverage_policy` key in `crap.toml`):

| Value | Behavior |
|---|---|
| `pessimistic` (default) | Score at 0% coverage — never hides risk. |
| `optimistic` | Score at 100% coverage (CRAP = complexity) — for a scoped local test slice that legitimately omits some files. |
| `skip` | Omit those functions from the report entirely. |

The chosen policy is recorded in the JSON envelope (unless `pessimistic`), so a `--baseline` delta run warns when its policy differs from the baseline's.

## What it looks like

### Table (TTY default)

```
crap4rs v0.5.0 — CRAP Score Analysis

+------------------------------------+----------------------------------+----+-------+-------+----------+
| File                               | Function                         | CC | Cov%  | CRAP  | Risk     |
+========================================================================================================+
| adapters/reporters/table.rs        | inject_breakdown_subrows         | 13 | 100.0 | 13.00 | moderate |
| domain/summary.rs                  | compute_summary                  | 13 | 100.0 | 13.00 | moderate |
| adapters/reporters/markdown.rs     | format_markdown_delta            | 12 |  97.1 | 12.00 | moderate |
| core/mod.rs                        | ensure_source_files_found        |  7 |  60.9 |  9.94 | moderate |
| domain/matching.rs                 | compute_branch_coverage          | 10 | 100.0 | 10.00 | moderate |
+------------------------------------+----------------------------------+----+-------+-------+----------+

Functions: 988 · Above threshold: 0 · Worst CRAP: 13.00 · Distribution: 951 low · 22 acceptable · 15 moderate · 0 high
```

### Markdown (`--format markdown`) — PR-comment ready

```markdown
# crap4rs v0.5.0 — CRAP Score Analysis

**Result:** PASS · **Functions:** 988 · **Above threshold (15):** 0

| Metric     | Worst | Average | Median |
|------------|------:|--------:|-------:|
| CRAP       | 13.00 |    1.62 |   1.00 |
| Complexity |    13 |     1.6 |    1.0 |
| Coverage   |  0.0% |   98.3% | 100.0% |

**Risk distribution:** low 951 · acceptable 22 · moderate 15 · high 0
```

### GitHub annotations (`--format github-annotations`) — inline PR review

Drops findings as inline warnings on the PR Files Changed tab, no GHAS / Code Scanning license needed:

```
::warning file=src/lib.rs,line=42,title=CRAP 28.4::Function `process_request` has CRAP 28.42 (complexity=11, coverage=42.3%) which exceeds threshold 15.0
```

### JSON (`--format json`) — programmatic consumption

```json
{
  "schema_version": 2,
  "run_meta": { "tool": "crap4rs", "version": "0.5.0", "metric": "cognitive" },
  "result": {
    "summary": {
      "total_functions": 988,
      "exceeding_threshold": 0,
      "distribution": { "low": 951, "acceptable": 22, "moderate": 15, "high": 0 },
      "max_crap": { "value": 13.0, "risk_level": "moderate" }
    },
    "functions": [
      {
        "scored": {
          "identity": { "file_path": "src/lib.rs", "qualified_name": "process_request", "span": { "start_line": 42, "end_line": 87 } },
          "complexity": 11,
          "complexity_metric": "cognitive",
          "coverage_percent": 42.3,
          "crap": { "value": 28.42, "risk_level": "high" },
          "contributors": [ /* per-decision-point breakdown */ ]
        },
        "threshold": 15.0,
        "exceeds": true
      }
    ]
  }
}
```

### Other formats

- `--format csv` — spreadsheet ingestion
- `--format sarif` — Code Scanning upload (requires GHAS) for surface-as-security-finding flows
- `--format scorecard` — single-row CI gate output for cross-PR delta tracking
- **HTML report** — interactive, sortable, with per-function contributor drill-down. *Coming in a future release; file an issue for early-access interest.*

Multiple formats compose in one pass: `--format json:envelope.json,markdown:report.md` writes both files from a single analysis.

## Delta gates — fail PRs that introduce new high-CRAP functions

```bash
# Generate baseline on main
crap4rs --coverage main-lcov.info --format json > baseline.json

# On PR — fail only on NEW threshold violations
crap4rs --coverage pr-lcov.info --baseline baseline.json --delta-gate
```

A function above-threshold on main doesn't fail the PR; only functions newly introduced or newly elevated do. Lets teams ratchet quality forward without blocking every PR on pre-existing debt.

## Library use

```toml
[dependencies]
crap4rs = "0.5"
```

`crap4rs` re-exports `crap-core`'s public API and adds the Rust-specific adapters (syn walker, LCOV parser). If you only need the scoring/envelope/reporter logic without Rust-specific I/O, depend on [`crap-core`](https://crates.io/crates/crap-core) directly.

## Stability

`crap4rs` is at `0.x` and follows pre-1.0 semver. The scorecard wire envelope is locked once published — patch releases never change envelope shape; minor releases may add fields under `#[serde(default)]`.

## See also

- **Repository**: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- **TypeScript / JavaScript analyzer**: [`crap4ts`](https://crates.io/crates/crap4ts) (crates.io) · [npm package](https://www.npmjs.com/package/crap4ts)
- **Shared core library**: [`crap-core`](https://crates.io/crates/crap-core)
- **Issues**: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
