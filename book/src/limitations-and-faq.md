# Limitations & FAQ

Every analyzer makes choices. This chapter collects the ones that shape what a crap4rs score does and does not mean, in one place. Each limit links back to the chapter that owns the detail.

## Limitations

### Matching is positional, not by name

crap4rs joins complexity data to coverage data by file path plus line range, never by function name. The syn walker emits `(file, start_line, end_line)` per function; LCOV `DA:` records give `(file, line, hits)`. A function's coverage is the fraction of its lines that the coverage data marks executed. This is a positional heuristic: it carries no symbol demangling, no `FN:`/`FNDA:` parsing, and no name resolution. A macro that expands across a function's declared span, or coverage data generated against a different revision of the file, can shift the join. See [Understanding CRAP](understanding-crap.md) for the matching model.

### Rust coverage is LCOV-only

The Rust adapter reads `lcov.info` and parses `SF:` and `DA:` records only. `FN:` and `FNDA:` records are ignored — they carry mangled symbols and duplicate information the line-range join already derives. Generate input with `cargo llvm-cov --lcov`. See [Installation](installation.md) for the coverage-generation command and [Multi-language](multi-language.md) for the TypeScript path (Istanbul `coverage-final.json`).

### Code your coverage run never executes scores `c² + c`

A function whose file is absent from the coverage data is treated as 0% covered, so its CRAP score is `c² + c` (complexity squared plus complexity). This is the pessimistic default: it never hides risk. It also means code that a coverage run structurally cannot reach scores high by design, not by defect:

- Integration-only code, HTTP handlers, and binary entry points that `cargo llvm-cov --lib` does not exercise.
- Code reached only through BDD/cucumber harnesses run out of process.
- `#[cfg(feature = "…")]` blocks excluded from the default-feature coverage build.

These are expected high scores, not bugs. When more than half the analyzed files have every analyzed function at 0% line coverage, crap4rs prints a warning to stderr naming the likely cause — uninstrumented integration code — and points at `--exclude`. The gate still runs; the warning is advisory.

To change the policy for files missing from coverage, set the missing-coverage policy:

| Policy | Coverage assumed | Resulting CRAP | Use when |
|--------|------------------|----------------|----------|
| `pessimistic` (default) | 0% | `c² + c` | You want missing coverage to count as risk. |
| `optimistic` | 100% | `c` | You run a scoped test slice that legitimately omits files. |
| `skip` | — (function omitted) | none | The function should not appear at all. |

The default is a choice — the safe one — not a statement about the code. See [Configuration](configuration.md) for the policy key and [CLI reference](cli-reference.md) for the flag.

### Branch coverage is informational; the gate is line-only

When the coverage data carries branch records, crap4rs surfaces a branch-coverage percentage per function. The CRAP score and the `--threshold` gate are computed against line coverage only. Branch data is reported, never gated. Promoting it to a gate would be a separate decision. See [Output formats](output-formats.md) for where the branch column appears.

### Thresholds are a calibration convention

The default gate is 15; the strict preset is 8 and the lenient preset is 25. These numbers are a calibration convention, not an empirically derived constant. They are also distinct from the score-based risk bands (Low ≤ 8, Acceptable ≤ 15, Moderate ≤ 25, High above), which classify every function regardless of the gate. The two axes share the same numbers today but answer different questions: the band is a fixed score classification, the gate is the pass/fail line you set. See [Understanding CRAP](understanding-crap.md) for the bands and [Configuration](configuration.md) for the presets.

### Walker scope: closures fold in, nested functions stand alone

The complexity walker handles two nesting cases distinctly:

- **Closures** count toward their enclosing function. A closure adds nothing by existing, but branching inside its body is charged one extra nesting level, so deeper logic costs more under the cognitive metric.
- **Nested `fn` items** are recorded as their own separate functions, named by their module path rather than the enclosing function. A `fn helper` declared inside `outer::do_work` appears as `outer::helper`, scored independently.

The default complexity metric for crap4rs is cognitive; cyclomatic is available via flag. See [CLI reference](cli-reference.md) for the metric flag and [Understanding CRAP](understanding-crap.md) for how each metric counts.

## FAQ

### Why does a fully tested function still show a CRAP score above 1?

A CRAP score equals the complexity when coverage is 100%. A function with cognitive complexity 12 and full coverage scores 12 — the formula collapses to the complexity alone. The score above 1 reflects complexity, not a coverage gap. Decompose the function to lower it. See [Understanding CRAP](understanding-crap.md).

### Why is my handler crate all red?

`cargo llvm-cov --lib` does not instrument integration-only code, so those files are absent from coverage and score `c² + c` under the pessimistic default. Either generate coverage that exercises them, set the missing-coverage policy to `optimistic` for a scoped slice, or `--exclude` the uncoverable paths. See [Configuration](configuration.md).

### Can I compare a Rust score to a TypeScript score directly?

No. Cross-language CRAP scores are not directly comparable. The combined view ranks functions by risk band first, then by the ratio of CRAP to its threshold within the band — not by raw score. See [Multi-language](multi-language.md).

### Coverage is a fraction in the formula but I pass a percent — which is it?

The published formula uses coverage as a fraction in `[0, 1]`. The internal computation takes a percentage in `[0, 100]` and clamps out-of-range input. You generate coverage as LCOV either way; the unit difference lives inside the formula. See [Understanding CRAP](understanding-crap.md).

### The `advice` format output looks rough — is that final?

The `advice` format is experimental. Its remediation phrasing may change. The numeric scores it reports are the same scores every other format reports. See [Output formats](output-formats.md).

### Do I have to pass `--coverage`?

clap does not mark `--coverage` as required, but a run needs it: crap4rs computes coverage-aware scores and has nothing to join against without it. In the composite action, `--src` defaults to nothing so configuration wins. See [CLI reference](cli-reference.md) and [CI integration](ci-integration.md).
