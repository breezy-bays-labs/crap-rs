# Output formats

`--format SPEC` selects what the analysis pass emits. One pass, many shapes: the analyzer walks the source and joins coverage once, then renders the result through every requested reporter. The flag mechanics (parsing, defaults, fan-out grammar) live in the [CLI reference](cli-reference.md); this chapter shows one sample per reporter and states what each is for.

## The `--format SPEC[:FILE]` fan-out

Each `--format` entry is a `FORMAT` (write to stdout) or `FORMAT:FILE` (write to that path). Pass a comma-separated list to fan a single pass out to several sinks:

```bash
crap4rs --coverage lcov.info --format json:envelope.json,markdown:report.md,github-annotations
```

That run produces three artifacts from one analysis: a JSON envelope on disk, a Markdown report on disk, and GitHub Actions annotations on stdout.

### The single-stdout rule

At most one entry in a multi-format invocation may target stdout; the rest must name a file. Two stdout sinks would interleave indistinguishably. One stdout sink alongside file sinks is the shape CI workflows need — e.g. `markdown:scorecard.md,github-annotations`, where the runner intercepts the workflow commands from stdout while the Markdown lands on disk.

When `--format` is omitted, the default is `table` to stdout.

## The reporters

| Format | Sink | For |
|---|---|---|
| `table` | terminal | Human reading at the terminal; the default |
| `json` | machine | Full envelope — every function, summary, view, optional delta |
| `markdown` | PR comment | Paste into a PR comment or issue; sticky-comment marker |
| `csv` | spreadsheet | One row per function, no summary; pivot/sort externally |
| `sarif` | Code Scanning | GitHub Code Scanning via `upload-sarif` |
| `github-annotations` | Actions runner | Inline `::warning` annotations on the Files Changed tab |
| `scorecard-row` | aggregator | One `Row::CrapDelta` JSON object for scorecard composition |
| `html` | browser | Self-contained dashboard |
| `advice` | CI log / agent | Grep-friendly one-line-per-violation remediation hints (experimental) |

### `table`

The default. ANSI-colored, fixed columns, a two-line summary footer. Colorize with `--color` (auto/always/never); see the [CLI reference](cli-reference.md).

```text
crap4rs vX.Y.Z — CRAP Score Analysis

+------------------------------+--------------+----+------+-------+----------+
| File                         | Function     | CC | Cov% | CRAP  | Risk     |
+============================================================================+
| src/domain/crap.rs           | complex_fn   | 20 | 30.0 | 45.20 | high     |
|------------------------------+--------------+----+------+-------+----------|
| src/adapters/coverage/mod.rs | parse_record | 6  | 72.5 | 15.00 | moderate |
|------------------------------+--------------+----+------+-------+----------|
| src/lib.rs                   | simple_fn    | 2  | 95.0 | 3.00  | low      |
+------------------------------+--------------+----+------+-------+----------+

Summary: 3 functions | 2 above threshold (8) | worst: 45.2 | FAIL
         avg: 21.1 | median: 15.0 | low: 1 | acceptable: 0 | moderate: 1 | high: 1
```

The `Risk` column is the score-based risk band; the `above threshold` count is the gate. These are [distinct axes](understanding-crap.md) that share the same numbers by default. `CC` is the complexity count under the active metric — cognitive for crap4rs, cyclomatic for crap4ts (see [understanding CRAP](understanding-crap.md)).

### `json`

The full envelope. `result` is the gate (every function, the summary, `passed`); `view` is the shaped display (`shown` rows, `spec`, `eligible_count`, `truncated`). When `--baseline` is set, a `delta` block joins the two. Pipe to `jq` for filtering. The `coverage` field reads as a percent here; the published formula uses a fraction — see [understanding CRAP](understanding-crap.md).

```json
{
  "language": "rust",
  "metric": "cognitive",
  "schema_version": 2,
  "threshold": 8.0,
  "result": {
    "functions": [
      {
        "exceeds": false,
        "threshold": 8.0,
        "scored": {
          "complexity": 5,
          "complexity_metric": "cognitive",
          "coverage_percent": 80.0,
          "crap": { "value": 5.16, "risk_level": "acceptable" },
          "identity": {
            "file_path": "src/domain/crap.rs",
            "qualified_name": "compute_crap",
            "span": { "start_line": 1, "end_line": 10, "start_column": 0, "end_column": 0 }
          },
          "contributors": []
        }
      }
    ],
    "passed": true,
    "summary": { "total_functions": 1, "exceeding_threshold": 0, "average_crap": 5.16, "median_crap": 5.16 }
  },
  "view": {
    "eligible_count": 1,
    "truncated": false,
    "shown": [ "…same shape as result.functions…" ],
    "spec": { "sort": "crap", "limit": null, "group_by": null }
  }
}
```

`result` is unaffected by view-shaping flags (`--top`, `--sort-by`, `--only-failing`) — the gate stays over the full unfiltered set. Only `view` reflects shaping. Use this envelope as the `--baseline` for a later delta run.

### `markdown`

GitHub-flavored Markdown for PR comments and issues. A compact summary block plus a top-N table (failures if any, otherwise worst-by-CRAP). The leading HTML comment is a sticky-comment marker a bot keys on to update one comment in place.

```markdown
<!-- crap4rs:scorecard -->

# crap4rs vX.Y.Z — CRAP Score Analysis

## Summary

**Result:** FAIL · **Functions:** 3 · **Above threshold (8):** 2

| Metric     | Worst | Average | Median |
|------------|------:|--------:|-------:|
| CRAP       | 45.20 |   21.07 |  15.00 |
| Complexity |    20 |     9.3 |    6.0 |
| Coverage   | 30.0% |   65.8% |  72.5% |

**Risk distribution:** low 1 · acceptable 0 · moderate 1 · high 1

## Failures (2 above threshold 8)

| File | Function | CC | Cov% | CRAP | Risk |
|------|----------|----|------|------|------|
| src/domain/crap.rs | complex_fn | 20 | 30.0 | 45.20 | high |
| src/adapters/coverage/mod.rs | parse_record | 6 | 72.5 | 15.00 | moderate |
```

`--md-top N` bounds the table; `--md-full-table` appends the full row-per-function table. See the [CLI reference](cli-reference.md).

### `csv`

RFC 4180, one row per function shown by the view, no summary header. Data-only — pivot or sort it in a spreadsheet. The `complexity_metric` column carries the analysis-wide metric on every row.

```csv
file,function,start_line,end_line,complexity,complexity_metric,coverage_percent,crap_score,risk_level,exceeds_threshold
src/domain/crap.rs,complex_fn,1,10,20,cognitive,30.0,45.20,high,true
src/adapters/coverage/mod.rs,parse_record,1,10,6,cognitive,72.5,15.00,moderate,true
src/lib.rs,simple_fn,1,10,2,cognitive,95.0,3.00,low,false
```

With `--baseline`, the schema mode-switches to row-per-change (a `change_kind` column plus side-by-side baseline/current scores). Pin your expected columns on whether `--baseline` was passed.

### `sarif`

SARIF v2.1.0 for GitHub Code Scanning. One `result` per over-threshold function, derived from the gate (view-shaping does not alter it). The risk band sets the result `level`: High maps to `error`, Moderate to `warning`, and Low or Acceptable to `note`. Upload with the `upload-sarif` action; see [CI integration](ci-integration.md).

```json
{
  "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
  "version": "2.1.0",
  "runs": [
    {
      "tool": { "driver": {
        "name": "crap4rs",
        "rules": [ { "id": "crap/threshold-exceeded", "name": "ThresholdExceeded" } ]
      } },
      "results": [
        {
          "ruleId": "crap/threshold-exceeded",
          "level": "error",
          "message": { "text": "Function `complex_fn` has CRAP 45.20 (complexity=20, coverage=30.0%) which exceeds threshold 8.0" },
          "locations": [ { "physicalLocation": {
            "artifactLocation": { "uri": "src/domain/crap.rs" },
            "region": { "startLine": 1, "endLine": 10 }
          } } ],
          "partialFingerprints": { "functionIdentity": "src/domain/crap.rs:complex_fn" }
        }
      ]
    }
  ]
}
```

### `github-annotations`

`::warning` workflow-command lines, one per over-threshold function, sorted CRAP descending. The Actions runner renders each inline on the PR Files Changed tab — universal, free, no Code Scanning dependency. Derived from the gate, not the view.

```text
::warning file=src/domain/crap.rs,line=1,title=CRAP 45.2::Function `complex_fn` has CRAP 45.20 (complexity=20, coverage=30.0%) which exceeds threshold 8.0
::warning file=src/adapters/coverage/mod.rs,line=1,title=CRAP 15.0::Function `parse_record` has CRAP 15.00 (complexity=6, coverage=72.5%) which exceeds threshold 8.0
```

GitHub silently drops annotations past a per-step cap (10 warning / 10 error / 10 notice per step). When the over-threshold set exceeds `--annotation-limit` (default 10), the reporter emits the top-N by CRAP and appends a trailing `::notice::N more functions exceed threshold; see scorecard for the full list` line. See the [CLI reference](cli-reference.md) for the flag.

### `scorecard-row`

One `Row::CrapDelta` JSON object — no envelope, no markdown — for a downstream scorecard aggregator to compose into a multi-gate PR comment. `status` is minted producer-side (Red / Yellow / Green); `failure_detail_md` is present only on Red. The full wire shape, status policy, and schema versioning are in [`docs/scorecard-row-contract.md`](https://github.com/breezy-bays-labs/crap-rs/blob/main/docs/scorecard-row-contract.md).

```json
{
  "type": "CrapDelta",
  "id": "crap_delta",
  "label": "CRAP Δ",
  "anchor": "crap-delta",
  "status": "Red",
  "threshold": 15,
  "delta_count": 2,
  "delta_text": "5 → 7 (+2)",
  "failure_detail_md": "**New CRAP threshold violations (>15):**\n- `foo` — `a.rs:1` — CRAP 22.0 (newly added)\n"
}
```

`crap4rs` and `crap4ts` route this format through the same shared pipeline, so the row shape is byte-identical across both. Consume it via the composite action's `outputs.row-json`; see [CI integration](ci-integration.md).

### `html`

A self-contained HTML dashboard — inline CSS, inline script, no external assets, mobile-responsive. Renders a verdict-stamped header, KPI tiles, a risk-distribution bar, the worst offenders, and a `<details>` card per file with a function-level table. The bundled script handles a theme toggle, a file-list filter, and a `/` keyboard shortcut.

```bash
crap4rs --coverage lcov.info --format html:report.html
```

When `--baseline` is set, a delta tab is added alongside the current view. Unifying multiple languages into one HTML document goes through the `crap-render` binary, covered in [multi-language](multi-language.md).

### `advice` (experimental)

A grep-friendly stream, one line per over-threshold function, in view order:

```text
[crap=45.20] src/cli/mod.rs:600-720 complex_handler [actions: add_tests_for_lines,extract_function]
[crap=15.00] src/adapters/coverage/mod.rs:80-145 parse_record [actions: simplify_branching,accept_inherent_complexity]
[crap=8.50] src/util.rs:30-42 opaque_helper [actions: none]
```

The shape is `[crap=N.NN] file:start-end qualified::name [actions: <kinds>]`. Lines are intentionally not aligned across rows so `awk` and `grep` parse the stream without padding. This format is experimental — the `actions` vocabulary may change.
