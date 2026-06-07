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

### Relocations don't count as new violations

A function that is **moved to another file, renamed, or moved between modules** — with its body otherwise unchanged — is recognized as a single `renamed` change rather than an unrelated `removed` + `added` pair. Because the relocated function carries its baseline score forward, a pure relocation contributes **zero new violations**, so large migrations and refactors sail through the delta gate even when the moved function was already over threshold. A relocation that *also* worsens the score (e.g. coverage drops as it moves) still counts — the relocation wasn't the only change.

The match is conservative: a function is paired across the move only when the match is unambiguous — the same name *and* the same structure, or (for a rename) a distinctive structure with exactly one candidate on each side. Ambiguous or trivial functions stay `added` + `removed`. Enabling rename detection can only ever *lower* the new-violation count, never raise it, so it can never newly fail a PR the old behavior would have passed — that is the migration-friendly guarantee. The honest limitation: because matching works from the analysis output rather than source text, a genuinely-unrelated function whose structure happens to exactly match a removed one (and is its only structural twin) is indistinguishable from a real rename and will pair — which can lower the count below the true figure. The guards make that rare, but it is not impossible. The `renamed` count appears in every delta report (table, markdown, JSON, CSV, HTML), and the JSON envelope's `delta` block carries both the baseline and current sides of each `renamed` row for the full from → to audit trail.

### Threshold-border jitter suppression (opt-in)

A function whose CRAP score sits right on the threshold line can flip across it on pure measurement noise — coverage rounding, or surrounding-code edits shifting per-line attribution. Set a **threshold-border epsilon** to stop that jitter from tripping the delta gate:

```bash
crap4rs --coverage pr-lcov.info --baseline baseline.json --delta-gate --threshold-epsilon 0.5
```

or in `crap.toml`:

```toml
[delta]
epsilon = 0.5
```

`epsilon` is an **absolute, unitless CRAP-point** band half-width (not a percentage of the threshold — so the same value is a tighter *relative* band at threshold 25 than at 8). A would-be new violation whose transition stays within `epsilon` of the threshold — on **both** the baseline and current side — is treated as border jitter and not counted; the count of what was suppressed surfaces as `border_jitter_suppressed` in the delta summary (JSON always; table/markdown/HTML when non-zero). The default `0.0` disables suppression entirely, so output is byte-identical to a run without the flag.

This is deliberately **narrow** — it tolerates oscillation *across the threshold line*, not delta magnitude or coverage flakiness in general. And it is a *jitter* knob, **not** a "noise-only" guarantee: like rename detection it can only ever lower the new-violation count, so a genuinely-new function that happens to land inside the band is suppressed too. For an `Added` row there is only the current reading — no prior state to jitter from — so an in-band new function is a one-sided *soft threshold bypass*. Keep `epsilon` small.

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
