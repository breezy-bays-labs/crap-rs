# Scorecard row contract

> Reference for `crap4rs --format scorecard-row` and the broader cross-tool
> aggregator pattern it slots into. Audience: aggregator-workflow authors,
> per-gate emitter authors, and contributors maintaining the producer.

## What this is

A **scorecard row** is one structured JSON object describing a single
quality-gate's verdict on a pull request. It is the cross-tool wire
contract that lets per-gate emitters compose into one PR comment.

The pattern is **N producers → 1 aggregator**:

```
       crap4rs --format scorecard-row    ─┐
       cargo-mutants (planned)            ─┤
       cucumber-rs   (planned)            ─┼──►  mokumo aggregator  ──►  one sticky PR comment
       depcruise     (planned)            ─┤        (consumes Rows,
       cargo-llvm-cov (existing producer)─┘         validates schema,
                                                    composes scorecard.json)
```

Each producer emits exactly one row to stdout (or a workflow output) per
run. The aggregator validates the row against the locked JSON Schema,
accumulates rows across jobs, and renders one composed scorecard. crap4rs
is the first cross-repo producer; the contract is owned upstream in
[breezy-bays-labs/mokumo](https://github.com/breezy-bays-labs/mokumo)
under the `scorecard` crate.

The current emitter for crap4rs is `--format scorecard-row` (CLI, landed
in [PR #119](https://github.com/breezy-bays-labs/crap4rs/pull/119)) plus
the composite action's `outputs.row-json` ([PR #120](https://github.com/breezy-bays-labs/crap4rs/pull/120)).

## Wire contract

The producer emits one `Row::CrapDelta` JSON object — no envelope, no
wrapper, no markdown noise. The shape is owned by mokumo's `scorecard`
crate at [`.config/scorecard/schema.json`](https://github.com/breezy-bays-labs/mokumo/blob/main/.config/scorecard/schema.json).
crap4rs vendors a copy at [`tests/fixtures/scorecard/schema.json`](../tests/fixtures/scorecard/schema.json)
for offline-clean testing — see [Schema vendoring strategy](#schema-vendoring-strategy).

### Field reference

| Field | Type | Description |
|---|---|---|
| `type` | `"CrapDelta"` | Variant tag (PascalCase, schema-required) |
| `id` | `"crap_delta"` | Stable row identifier (`RowId` newtype upstream) |
| `label` | `string` | Display label, e.g. `"CRAP Δ"` |
| `anchor` | `string` | URL-fragment anchor for the rendered row, e.g. `"crap-delta"` |
| `status` | `"Red"` \| `"Yellow"` \| `"Green"` | Producer-minted verdict — see [Status policy](#status-policy-model-p) |
| `threshold` | `u32` | The CRAP threshold the count is over (default `15`, the metric-correct `default` preset; tunable via `crap4rs.toml` or `--threshold`) |
| `delta_count` | `i32` | Signed delta in over-threshold function count vs. baseline |
| `delta_text` | `string` | Producer-rendered display string, e.g. `"5 → 7 (+2)"`. The aggregator never reparses. |
| `failure_detail_md` | `string?` | Markdown describing the new violations. **Required when `status == "Red"`** (Layer 2 schema enforcement); omitted otherwise. |

`schema_version` lives on the **outer envelope** (the `Scorecard` object the
aggregator builds), not on each row. Rows themselves have no version field —
the schema's `oneOf` discriminator is `type`, and additive changes (new row
variants, new optional fields) bump `schema_version` on the envelope.

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

- **`tool` / `tool_version`.** Producer-attribution is not on the wire. mokumo's V3 `Row::CoverageDelta` set the precedent — adding it on one variant breaks symmetry. If multi-producer disambiguation becomes a real ask (crap4rs + crap4ts in one repo emitting overlapping rows), the field belongs on `RowCommon` so all variants share it. Tracked as a follow-up in the gap doc.
- **Absolute current/baseline counts as structured fields.** Only `delta_count` (signed) is exposed. Absolute counts live in `delta_text` for human read; downstream tooling reads `delta_count` for machine-readable signal. Mirrors V3 `CoverageDelta`'s `delta_pp`-only choice.
- **Per-function regression detail in row body.** Yellow's per-function regression context lives in prose inside `delta_text` (`"(regressions on existing functions)"`). Structured per-function detail lives in crap4rs's `--format json` envelope (`delta.shown[]`) for tooling that needs it — this contract stays lean.

## Status policy (Model P)

crap4rs **mints status itself** on the producer side. The aggregator does
not reinterpret it.

### Rules

| Status | Condition |
|---|---|
| **Red** | At least one function exists in the working tree, above threshold, that did not exist (or was below threshold) in the baseline. The `failure_detail_md` field enumerates them with `path:line` and CRAP score. |
| **Yellow** | No new threshold violations, **and** at least one modified function's CRAP score increased vs. baseline. Existing-but-stable violations don't trip Yellow on their own — re-running on unchanged code is always Green. |
| **Green** | Otherwise. |

### Why producer-side (Model P)

crap4rs owns the analysis: it knows the CRAP scores, the per-function
deltas, the threshold, and what counts as a "new violation" vs. a
pre-existing one. Asking the aggregator to re-derive that from a raw
producer-row would mean restating the rules in two places. Model P keeps
the contract lean (one type, not "producer-row" → "Row::CrapDelta"), and
matches issue
[#111's acceptance criteria](https://github.com/breezy-bays-labs/crap4rs/issues/111)
literally — *"Output conforms to scorecard-schema's `Row::CrapDelta`."*

V3 `Row::CoverageDelta` looks superficially like the opposite (the
mokumo aggregator mints status from raw `delta_pp`), but that's because
**coverage has no separate producer** — the coverage `delta_pp` is a CLI
float passed in, not a wire format from another tool. CRAP genuinely
differs: crap4rs is a separate process emitting structured output, so it
naturally mints the row.

### Operator-tunability boundary

The producer/aggregator split has a clean ownership boundary:

| Knob | Owner | Where tuned |
|---|---|---|
| CRAP threshold (the gate) | Producer (crap4rs) | `crap4rs.toml` `threshold = N`, or `--threshold N` |
| Ignored paths / glob excludes | Producer | `crap4rs.toml` `exclude = [...]` |
| Complexity metric (cognitive vs cyclomatic) | Producer | `crap4rs.toml` `metric = "..."`, or `--metric` |
| Red/Yellow/Green status rules | Producer (this contract) | Hardcoded — not operator-tunable |
| Per-row gate composition (which gates block merge) | Aggregator (mokumo) | mokumo's `quality.toml` `[gates]` |
| Sticky-comment routing, threshold fallback, gate skipping | Aggregator | mokumo's `quality.toml` |

If a future repo asks for `quality.toml`-side CRAP threshold tuning,
that's a non-breaking evolution at the wire level — the row shape
doesn't change, only who fills which fields does. We pay nothing now to
keep that future open. See [decision rationale in the gap doc](#cross-references).

## Producer pattern

Each producer is a small CLI that:

1. Runs the underlying analysis (CRAP, mutation, BDD, dep-cruise, …).
2. Diffs against a baseline (typically the merge target's last successful run).
3. Emits exactly one `Row` JSON object on stdout (or a workflow output).
4. Mints status from its own rules and produces `failure_detail_md` when Red.

| Tool | Status | Row variant | Notes |
|---|---|---|---|
| `crap4rs --format scorecard-row` | **Shipped** ([PR #119](https://github.com/breezy-bays-labs/crap4rs/pull/119)) | `CrapDelta` | This crate. Composite action surfaces it via `outputs.row-json` ([PR #120](https://github.com/breezy-bays-labs/crap4rs/pull/120)). |
| `cargo-llvm-cov` (mokumo's existing flow) | Producer-light | `CoverageDelta` | Mokumo's aggregator computes `delta_pp` directly from raw coverage; no separate emitter today. |
| `cargo-mutants` adapter | Planned | `MutationSurvivors` | Variant designed in mokumo V4 (#769); producer not yet wired. |
| `cucumber-rs` adapter | Planned | `BddSkipCount` | Same. |
| `depcruise` adapter | Planned | (variant TBD) | Pending V4 row populate. |
| `crap4ts` (TypeScript twin) | Frozen | `CrapDelta` | crap4rs#114 tracks parity. Frozen pending [crap-rs unification (ops#231)](https://github.com/breezy-bays-labs/ops/issues/231). |

The composite action at `.github/actions/scorecard` exposes
`outputs.row-json` so any aggregator workflow can ingest the structured
row alongside the existing `markdown` output. The aggregator currently
runs in mokumo under [mokumo#650](https://github.com/breezy-bays-labs/mokumo/issues/650).

## Schema vendoring strategy

The locked schema lives in mokumo at
[`.config/scorecard/schema.json`](https://github.com/breezy-bays-labs/mokumo/blob/main/.config/scorecard/schema.json).
crap4rs vendors a verbatim copy at
[`tests/fixtures/scorecard/schema.json`](../tests/fixtures/scorecard/schema.json),
pinned at mokumo commit `0cfdb0f` (PR #780, V4 row populate, schema version `2`).

### Why vendored

- **Offline-clean tests.** `cargo test` and `cargo nextest run` must succeed with no network access. Build-time fetching breaks this.
- **Reproducibility.** A bumped upstream schema cannot silently break crap4rs's CI mid-run; updates are explicit, reviewable diffs.
- **Trust boundary.** The vendored copy is what the JSON Schema round-trip test in `tests/scorecard_row_integration.rs` validates against. Drift is caught on bump, not on first downstream consumer failure.

### When to bump

Regenerate the fixture when **any** of the following are true:

- mokumo bumps `schema_version` (envelope-level — currently `2`).
- mokumo adds, renames, or removes a `Row` variant relevant to crap4rs.
- mokumo modifies the `CrapDelta` variant's required/optional fields.
- mokumo changes Layer 2 enforcement rules (e.g., the Red ⇒ `failure_detail_md` `if/then`).

### How

```bash
cd tests/fixtures/scorecard
./regen.sh   # copies from $MOKUMO_REPO/.config/scorecard/schema.json
```

The script also rewrites the "Vendored at commit" pin in `SOURCE.md`
with the latest commit SHA touching the schema. Inspect the diff before
committing — a bumped `schema_version` likely requires reporter updates
and should land in a paired PR.

## Future direction (crap-core extraction)

The producer side of this contract — the `CrapDeltaRowData` projection
(`src/domain/summary.rs`) and the wire-format reporter
(`src/adapters/reporters/scorecard_row.rs`) — is split along the planned
[crap-core extraction boundary (ops#231)](https://github.com/breezy-bays-labs/ops/issues/231):

```
crap-core (future)                     crap-rust (future)
├── domain/                            ├── adapters/
│   └── CrapDeltaRowData ◄─────────┐   │   ├── complexity/  (syn walker)
├── ports/                          │  │   ├── coverage/    (LCOV parser)
│   └── ComplexityPort/CoveragePort │  │   └── reporters/
└── core/                           │  │       └── scorecard_row.rs ──► writes Row::CrapDelta JSON
    └── analyze() entry point       │  └── (Rust-specific)
                                    │
                                    └─ projection stays language-agnostic
```

Today both layers live in `crap4rs`. When extraction happens:

- `domain/`, `ports/`, `core/` move to `crap-core` unchanged. `CrapDeltaRowData` carries no Rust-specific types — it's the language-agnostic projection.
- `adapters/` (including `reporters/scorecard_row.rs`) moves to a `crap-rust` crate. The reporter renders the same wire shape; only the input pipeline (syn-walker + LCOV) is Rust-specific.
- `crap4ts` adopts the same pattern in a TypeScript adapters crate, emitting the same `CrapDelta` variant — at which point the parity tracker [crap4rs#114](https://github.com/breezy-bays-labs/crap4rs/issues/114) unfreezes.

The contract itself doesn't move with the code. It lives upstream in
mokumo (or a future standalone `scorecard-rs` repo) regardless of which
producer crate emits it.

## Cross-references

- **Producer (CLI):** [PR #119](https://github.com/breezy-bays-labs/crap4rs/pull/119) — `--format scorecard-row` landed (closes [crap4rs#111](https://github.com/breezy-bays-labs/crap4rs/issues/111)).
- **Action surface:** [PR #120](https://github.com/breezy-bays-labs/crap4rs/pull/120) — composite-action `outputs.row-json` (closes [crap4rs#112](https://github.com/breezy-bays-labs/crap4rs/issues/112)).
- **Locked schema:** [`breezy-bays-labs/mokumo:.config/scorecard/schema.json`](https://github.com/breezy-bays-labs/mokumo/blob/main/.config/scorecard/schema.json) at commit `0cfdb0f` (mokumo PR #780, V4 row populate, schema version `2`).
- **Mokumo aggregator:** [mokumo#650](https://github.com/breezy-bays-labs/mokumo/issues/650) — consumes `outputs.row-json`; `comment-mode: sticky` → `none` cutover happens in mokumo's PR, not here.
- **Unification tracker:** [ops#231](https://github.com/breezy-bays-labs/ops/issues/231) — the `crap-core` / `crap-rust` / `crap-typescript` / `crap-cli` extraction.
- **TypeScript parity:** [crap4rs#114](https://github.com/breezy-bays-labs/crap4rs/issues/114) — frozen until ops#231 unblocks.
- **Decision rationale (Model P, V3 audit, status rules):** the rollout gap doc lives in `breezy-bays-labs/ops` at `pipelines/crap4rs/crap4rs-20260503-scorecard-row-rollout.md` (private to ops; the recommendation here reflects its conclusions).
- **README entry point:** [`README.md` § Scorecard row](../README.md#scorecard-row---format-scorecard-row).
