# Quick start

Two analyzers, one shared core: `crap4rs` scores Rust against LCOV coverage, `crap4ts` scores TypeScript/JavaScript against Istanbul JSON coverage. Each follows the same four steps — generate coverage, run the analyzer, read the table, gate. Install instructions live in [installation.md](installation.md).

The flow per adapter:

1. Generate a coverage file from your test run.
2. Point the analyzer at your source root and that coverage file.
3. Read the ranked table on stdout.
4. Re-run with `--strict` once you want CI to fail on the worst functions.

## Rust, end to end

```bash
# 1. Generate LCOV coverage
cargo llvm-cov --lcov --output-path lcov.info

# 2. Run the analyzer (default complexity metric is cognitive)
crap4rs --src src/ --coverage lcov.info

# 3. Read the ranked table on stdout (printed above)

# 4. Gate: fail (exit 1) on any function above the strict cutoff
crap4rs --src src/ --coverage lcov.info --strict
```

`--coverage` is required on the analysis path; `--src` defaults to `src` when omitted. The default metric for `crap4rs` is cognitive — `match`-heavy Rust inflates cyclomatic counts without adding real risk. Pass `--metric cyclomatic` to switch.

## TypeScript/JavaScript, end to end

```bash
# 1. Generate Istanbul JSON coverage (must be the istanbul provider,
#    not v8 — see installation.md)
npm test -- --coverage

# 2. Run the analyzer (crap4ts is cyclomatic-only)
crap4ts --src src/ --coverage coverage-final.json --exclude '*.test.ts'

# 3. Read the ranked table on stdout (printed above)

# 4. Gate on the strict cutoff
crap4ts --src src/ --coverage coverage-final.json --exclude '*.test.ts' --strict
```

`crap4ts` measures cyclomatic complexity only. Coverage comes from the Istanbul JSON shape (`coverage-final.json`); the v8 provider's JSON is a different shape the adapter does not consume.

## Reading the table

The default `table` output ranks functions by CRAP score, worst first. Each row carries the function's location, complexity, line coverage percent, CRAP score, and its risk band. The score fuses complexity with the fraction of lines covered:

```
CRAP = complexity² × (1 − coverage)³ + complexity
```

The math, the units, and the risk bands are covered in [understanding-crap.md](understanding-crap.md); every output format (JSON, Markdown, SARIF, HTML, and the rest) appears in [output-formats.md](output-formats.md).

## Gating with `--strict`

Without a threshold flag, the gate fires at CRAP 15. `--strict` lowers it to 8, `--lenient` raises it to 25:

```bash
crap4rs --src src/ --coverage lcov.info --strict
```

When any function exceeds the active cutoff, the process exits `1` — wire that into CI to block the merge. The preset cutoffs, the calibration caveat, and how the threshold gate differs from the score-based risk band are covered in [understanding-crap.md](understanding-crap.md). The full flag surface is in [cli-reference.md](cli-reference.md); CI wiring is in [ci-integration.md](ci-integration.md).

## Clone and try

The repo ships a pedagogical sample at `crates/crap-examples/`: four Rust modules under `src/` and four matching TypeScript modules under `ts/`, picked so a single analysis spans every risk band. Committed coverage fixtures (`lcov.info`, `coverage-final.json`) let you run both adapters without generating coverage first.

```bash
git clone https://github.com/breezy-bays-labs/crap-rs.git
cd crap-rs

# Rust — paths in lcov.info are relative to --src
crap4rs --src ./crates/crap-examples/src \
  --coverage crates/crap-examples/lcov.info

# TypeScript
crap4ts --src ./crates/crap-examples/ts \
  --coverage crates/crap-examples/coverage-final.json \
  --exclude '*.test.ts'
```

The four modules together land in the Low, Acceptable, Moderate, and High bands on a fresh run. Per-language complexity counts differ between the adapters (cognitive for Rust, cyclomatic for TS), so the absolute scores differ — what holds constant is the cross-module ranking and the four-band spread. The corpus, the per-module intent, and the recipe for regenerating the fixtures are documented in `crates/crap-examples/README.md`. Analyzing more than one Rust crate at once with multiple `--src` roots is covered in [multi-language.md](multi-language.md).
