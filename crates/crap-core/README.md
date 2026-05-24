# crap-core

[![crates.io](https://img.shields.io/crates/v/crap-core.svg)](https://crates.io/crates/crap-core)
[![docs.rs](https://img.shields.io/docsrs/crap-core)](https://docs.rs/crap-core)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/breezy-bays-labs/crap-rs#license)

**Language-agnostic foundation for the CRAP (Change Risk Anti-Patterns) analyzer family.** Domain types, port traits, threshold and risk logic, reporters, and the shared scorecard envelope used by every CRAP adapter.

## What this is

`crap-core` is the shared backbone for analyzers that compute CRAP scores —

```
CRAP(complexity, coverage) = complexity² × (1 − coverage)³ + complexity
```

— across different source languages. It owns:

- The **CRAP formula** and four-tier **risk classification** (Low / Acceptable / Moderate / High)
- The **`ComplexityPort` and `CoveragePort`** traits that language adapters implement
- The locked **wire envelope** (`scorecard`, `delta`, `crap-delta` shapes) consumed by reporters and downstream tooling
- **All reporters** — `table`, `markdown`, `json`, `csv`, `sarif`, `scorecard`, `scorecard-row`, `github-annotations`. HTML is on the roadmap
- **Threshold presets** (`strict` / default / `lenient`), configuration parsing, and delta-gate semantics

If you're using `crap-core` directly, you're probably building a new language adapter. End users want one of the adapter crates instead:

- **[`crap4rs`](https://crates.io/crates/crap4rs)** — Rust analyzer (`syn` complexity + LCOV coverage; cognitive complexity default)
- **[`crap4ts`](https://crates.io/crates/crap4ts)** — TypeScript / JavaScript analyzer (`oxc` complexity + Istanbul JSON coverage; cyclomatic complexity)

Both link `crap-core` and produce byte-identical scorecard envelopes for the same `(complexity, coverage)` inputs — Rust and TypeScript CI gates stay consistent.

## Install

```toml
[dependencies]
crap-core = "0.4"
```

## Quick example

```rust
use crap_core::domain::crap::compute_crap;

let score = compute_crap(15, 0.90);
assert_eq!(format!("{:.2}", score), "15.23");
```

For the port traits and how adapters wire complexity + coverage data into a `Scorecard`, see the [API docs on docs.rs](https://docs.rs/crap-core).

## Threshold presets and risk tiers

`crap-core` defines four risk tiers and three preset gates calibrated against them. Every adapter inherits this taxonomy automatically.

| CRAP score | Risk tier |
|---|---|
| ≤ 8 | Low |
| ≤ 15 | Acceptable |
| ≤ 25 | Moderate |
| > 25 | High |

| Preset | Threshold | Gates at | Use for |
|---|---|---|---|
| `--strict` | CRAP ≤ 8 | Low → Acceptable | safety-critical, high-quality libraries |
| *(default)* | CRAP ≤ 15 | Acceptable → Moderate | typical app / library code |
| `--lenient` | CRAP ≤ 25 | Moderate → High | legacy / transitional codebases |

The same preset values apply for both cognitive and cyclomatic complexity inputs (adapters choose which metric they pass into the formula).

## What the output looks like

Every reporter consumes the same `AnalysisView` projection over the scored functions. Adapters never re-render shapes themselves — they hand `crap-core` the data and pick a `--format`.

### Table — TTY default

```
crap4rs v0.5.0 — CRAP Score Analysis

+------------------------------------+----------------------------------+----+-------+-------+----------+
| File                               | Function                         | CC | Cov%  | CRAP  | Risk     |
+========================================================================================================+
| adapters/reporters/table.rs        | inject_breakdown_subrows         | 13 | 100.0 | 13.00 | moderate |
| domain/summary.rs                  | compute_summary                  | 13 | 100.0 | 13.00 | moderate |
| adapters/reporters/markdown.rs     | format_markdown_delta            | 12 |  97.1 | 12.00 | moderate |
+------------------------------------+----------------------------------+----+-------+-------+----------+
```

### JSON — programmatic / cross-tool

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
          "crap": { "value": 28.42, "risk_level": "high" }
        },
        "threshold": 15.0,
        "exceeds": true
      }
    ]
  }
}
```

The envelope shape is locked across patch releases of the adapter that emits it — downstream consumers can pin against a `schema_version`.

### GitHub Actions inline annotations

```
::warning file=src/lib.rs,line=42,title=CRAP 28.4::Function `process_request` has CRAP 28.42 (complexity=11, coverage=42.3%) which exceeds threshold 15.0
```

Renders as an inline annotation on the PR Files Changed tab — no GitHub Advanced Security / Code Scanning license required.

### Markdown — PR-comment ready

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

### HTML report

Interactive, sortable HTML report with per-function contributor drill-down. **Coming in a future release** — open an issue if you want early-access interest tracked.

### Other formats

`csv`, `sarif` (Code Scanning surface), `scorecard` (single-row CI gate), `scorecard-row` (cross-adapter parity-locked CI row).

Multiple formats compose in one pass: `--format json:envelope.json,markdown:report.md` writes both from a single analysis.

## Stability

`crap-core` is at `0.x` and follows pre-1.0 semver — breaking changes can land on minor bumps; adapter crates pin against a specific minor. The wire envelope schema is locked once published — patch releases never change envelope shape; minor releases may add fields under `#[serde(default)]`.

## See also

- **Repository**: [github.com/breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs)
- **Project README**: [workspace overview](https://github.com/breezy-bays-labs/crap-rs#readme)
- **Issues**: [github.com/breezy-bays-labs/crap-rs/issues](https://github.com/breezy-bays-labs/crap-rs/issues)

## License

Dual-licensed under MIT OR Apache-2.0 at your option.
