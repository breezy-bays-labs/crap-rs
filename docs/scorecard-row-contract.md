# Scorecard row contract

> Reference for `crap4rs --format scorecard-row` and `crap4ts --format scorecard-row`.
> Audience: CI / PR-comment aggregator authors, contributors maintaining the producer
> reporter in crap-core.

## What this is

A **scorecard row** is one structured JSON object describing a single
quality-gate's verdict on a pull request. crap-rs adapters (crap4rs for
Rust, crap4ts for TypeScript) act as *producers* — each emits exactly
one `Row::CrapDelta` JSON object per run, on stdout (or via the
composite action's `outputs.row-json`). Aggregators (any downstream CI
or PR-comment tooling) compose those rows into a single scorecard
envelope.

```
       crap4rs --format scorecard-row    ─┐
       crap4ts --format scorecard-row    ─┼──►  any aggregator (CI workflow,  ──►  composed PR comment
       (other producers e.g. coverage,   ─┤        PR-comment bot, dashboard)        / dashboard / artifact
       mutation, BDD — owned by their    ─┘        — validates each row against
       respective tools)                            this contract, composes the
                                                    Scorecard envelope
```

The contract is owned by **this repository** — the producer side
defines the wire shape, and `crates/crap4rs/tests/fixtures/scorecard/schema.json`
is the source of truth. Aggregators that consume crap-rs rows
validate against that fragment (or a vendored copy of it). crap-rs
deliberately does NOT define the aggregator-side row variants that
other tools produce — those belong to whichever producer ships them.

## Wire contract

Each producer emits one `Row::CrapDelta` JSON object — no envelope, no
wrapper, no markdown noise. The locked schema fragment lives at
[`crates/crap4rs/tests/fixtures/scorecard/schema.json`](../crates/crap4rs/tests/fixtures/scorecard/schema.json)
(`schema_version = 1`; see [`SOURCE.md`](../crates/crap4rs/tests/fixtures/scorecard/SOURCE.md)
for the bump ceremony). The shared producer routes through
crap-core's `cli::format_as_scorecard_row` → `domain::summary::project_crap_delta_row`
→ `adapters::reporters::format_scorecard_row` pipeline, so both
adapters emit byte-identical row shapes (locked by
`crates/crap-core/tests/scorecard_row_parity.rs`).

### Field reference

| Field | Type | Description |
|---|---|---|
| `type` | `"CrapDelta"` | Variant tag (PascalCase, schema-required) |
| `id` | `"crap_delta"` | Stable row identifier — aggregators key on this for anchor links |
| `label` | `string` | Display label, e.g. `"CRAP Δ"` |
| `anchor` | `string` | URL-fragment anchor for the rendered row, e.g. `"crap-delta"` |
| `status` | `"Red"` \| `"Yellow"` \| `"Green"` | Producer-minted verdict — see [Status policy](#status-policy) |
| `threshold` | `u32` | The CRAP threshold the count is over (default `15` post-#272 alignment; tunable via `crap-rs.toml` or `--threshold`) |
| `delta_count` | `i32` | Signed delta in over-threshold function count vs. baseline (positive = new violations landed; negative = violations decreased; equals current over-threshold count when no baseline) |
| `delta_text` | `string` | Producer-rendered display string, e.g. `"5 → 7 (+2)"`. Aggregators surface verbatim — never reparse. |
| `failure_detail_md` | `string?` | Markdown describing the new violations. **Required when `status == "Red"`** (schema-enforced via `if/then`); omitted otherwise. |

`schema_version` lives on the **outer envelope** (the `Scorecard`
object an aggregator builds), not on each row. Rows themselves have no
version field — the schema's `oneOf` discriminator is `type`, and
additive changes (new row variants, new optional fields) bump
`schema_version` on the envelope.

### Examples

**Green** — no new violations, no modified-function regressions:

```json
{
  "type": "CrapDelta",
  "id": "crap_delta",
  "label": "CRAP Δ",
  "anchor": "crap-delta",
  "status": "Green",
  "threshold": 15,
  "delta_count": -1,
  "delta_text": "5 → 4 (-1)"
}
```

**Yellow** — modified-function CRAP regressed but no new violations crossed threshold:

```json
{
  "type": "CrapDelta",
  "id": "crap_delta",
  "label": "CRAP Δ",
  "anchor": "crap-delta",
  "status": "Yellow",
  "threshold": 15,
  "delta_count": 0,
  "delta_text": "5 → 5 (regressions on existing functions)"
}
```

**Red** — at least one new function landed above threshold; `failure_detail_md` is mandatory:

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
  "failure_detail_md": "**New CRAP threshold violations (>15):**\n- `auth::login::handle_post` — `src/auth/login.rs:142` — CRAP 18.4\n- `backup::run_backup` — `src/backup.rs:88` — CRAP 21.1"
}
```

### What's deliberately absent

- **`tool` / `tool_version`.** Producer-attribution is not on the wire.
  If multi-producer disambiguation becomes a real ask (e.g. crap4rs +
  crap4ts in one repo emitting overlapping rows for distinct
  language-scoped source trees), the field belongs on the
  shared row body so all variants share it. Until then, aggregators
  that need attribution can carry it out-of-band (e.g. workflow job
  name, or per-job sticky-comment headers).
- **Absolute current/baseline counts as structured fields.** Only
  `delta_count` (signed) is exposed. Absolute counts live in
  `delta_text` for human read; downstream tooling reads `delta_count`
  for machine-readable signal.
- **Per-function regression detail in the row body.** Yellow's
  per-function regression context lives in prose inside `delta_text`
  (`"(regressions on existing functions)"`). Structured per-function
  detail lives in the adapter's `--format json` envelope
  (`delta.shown[]`) for tooling that needs it — this row contract
  stays lean.

## Status policy

crap-rs **mints status itself** on the producer side. The aggregator
does not reinterpret it.

### Rules

| Status | Condition |
|---|---|
| **Red** | At least one function exists in the working tree, above threshold, that did not exist (or was below threshold) in the baseline. The `failure_detail_md` field enumerates them with `path:line` and CRAP score. |
| **Yellow** | No new threshold violations, **and** at least one modified function's CRAP score increased vs. baseline. Existing-but-stable violations don't trip Yellow on their own — re-running on unchanged code is always Green. |
| **Green** | Otherwise. |

### Why producer-side

The adapter owns the analysis: it knows the CRAP scores, the
per-function deltas, the threshold, and what counts as a "new
violation" vs. a pre-existing one. Asking the aggregator to re-derive
that from a raw producer-row would mean restating the rules in two
places. Producer-side status mint keeps the contract lean (one type,
not "producer-row" → "Row::CrapDelta") and matches the JSON envelope's
own `result.passed` semantics — both gates flow from the same
domain projection.

### Operator-tunability boundary

The producer/aggregator split has a clean ownership boundary:

| Knob | Owner | Where tuned |
|---|---|---|
| CRAP threshold (the gate) | Producer (crap-rs adapter) | `crap-rs.toml` `threshold = N`, or `--threshold N` |
| Ignored paths / glob excludes | Producer | `crap-rs.toml` `exclude = [...]` |
| Complexity metric (cognitive vs cyclomatic) | Producer | `crap-rs.toml` `metric = "..."`, or `--metric` |
| Red/Yellow/Green status rules | Producer (this contract) | Hardcoded — not operator-tunable |
| Per-row gate composition (which gates block merge) | Aggregator | Aggregator's own config |
| Sticky-comment routing, threshold fallback, gate skipping | Aggregator | Aggregator's own config |

If a future repo asks for aggregator-side CRAP threshold tuning,
that's a non-breaking evolution at the wire level — the row shape
doesn't change, only who fills which fields does. The current design
keeps that future open at zero cost.

## Producer pattern (for non-CRAP rows)

This document defines the crap-rs producer-side contract. Other
quality-gate producers (coverage, mutation, BDD, dependency graph)
follow the same general pattern but ship their own variants in their
own schemas — aggregators compose by accepting heterogeneous `Row`
variants from multiple producer schemas. crap-rs deliberately does
NOT enumerate those variants here.

Each producer is a small CLI that:

1. Runs the underlying analysis.
2. Diffs against a baseline (typically the merge target's last
   successful run).
3. Emits exactly one `Row` JSON object on stdout (or a workflow
   output).
4. Mints status from its own rules and produces a Red-required detail
   field when applicable.

The composite action at `.github/actions/scorecard` exposes
`outputs.row-json` so any aggregator workflow can ingest the
structured row alongside the existing `markdown` output.

## Schema versioning

The schema fragment lives at
[`crates/crap4rs/tests/fixtures/scorecard/schema.json`](../crates/crap4rs/tests/fixtures/scorecard/schema.json).
Current version: `1`. Bump when:

- A new `Row` variant lands that a crap-rs adapter emits.
- A new or renamed required field on `CrapDelta` lands.
- The `Red ⇒ failure_detail_md` enforcement changes.
- The producer-side status policy changes in ways downstream
  validators need to know about.

The pin is asserted by `tests/scorecard_row_integration::schema_pins_version_1`
so a bump requires updating the schema, the version field, and the pin
test in one PR.

## Cross-adapter parity

Both crap4rs and crap4ts route `--format scorecard-row` through the
same crap-core pipeline, so the wire shape is structurally identical
across both adapters. The parity test at
`crates/crap-core/tests/scorecard_row_parity.rs` exercises both
binaries against representative fixtures and asserts byte-identical
key sets in both the Green and Red branches plus value-shape
invariants. See AGENTS.md "Composite scorecard action" for the full
contract and enforcement story.

## Cross-references

- **Schema fragment:** [`crates/crap4rs/tests/fixtures/scorecard/schema.json`](../crates/crap4rs/tests/fixtures/scorecard/schema.json)
- **Bump ceremony:** [`crates/crap4rs/tests/fixtures/scorecard/SOURCE.md`](../crates/crap4rs/tests/fixtures/scorecard/SOURCE.md)
- **Parity test:** [`crates/crap-core/tests/scorecard_row_parity.rs`](../crates/crap-core/tests/scorecard_row_parity.rs)
- **Integration test:** [`crates/crap4rs/tests/scorecard_row_integration.rs`](../crates/crap4rs/tests/scorecard_row_integration.rs)
- **Composite action:** [`.github/actions/scorecard/action.yml`](../.github/actions/scorecard/action.yml) — exposes `outputs.row-json`
- **README entry point:** [`README.md` § Scorecard row](../README.md#scorecard-row---format-scorecard-row)
