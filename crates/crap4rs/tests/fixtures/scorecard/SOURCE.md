# Scorecard-row producer schema

`schema.json` is the locked JSON Schema fragment defining the
`Row::CrapDelta` variant that `crap4rs --format scorecard-row` and
`crap4ts --format scorecard-row` emit. crap-rs is the source of truth
for this fragment — any aggregator or PR-comment renderer that consumes
crap-rs row output validates against this file (or a copy of it).

| Field | Value |
|---|---|
| Owner | This repository (`breezy-bays-labs/crap-rs`) |
| Schema version | `1` (producer-side; bumps on additive or breaking changes to a `Row` variant crap-rs emits) |
| Last updated | 2026-05-24 |

## Scope

The fragment defines only what crap-rs adapters PRODUCE:

- The `Scorecard` envelope's required keys (`schema_version`, `rows`)
- The `Row::CrapDelta` variant
- The `Status` enum referenced by `CrapDelta`

Aggregator-side variants (coverage deltas, mutation survivors, BDD
skip counts, dependency-graph deltas, GitHub Checks references, etc.)
are deliberately not enumerated here — those belong to whichever tool
produces them. An aggregator that composes crap-rs rows with rows from
other tools extends this schema downstream by adding their own
variants to `Row.oneOf`.

## When to bump `schema_version`

Bump when **any** of the following land in `schema.json`:

- A new `Row` variant emitted by a crap-rs adapter.
- A new or renamed required field on `CrapDelta`.
- A change in the `Red ⇒ failure_detail_md` `if/then` enforcement.
- A change in the producer-side status policy that downstream
  validators need to know about.

The pin is asserted by
`tests/scorecard_row_integration::schema_pins_version_1` so the bump
ceremony is explicit (update the schema, update the version, update
the pin test, all in one PR).

## Used by

- `crates/crap4rs/tests/scorecard_row_integration.rs` — JSON Schema
  round-trip test validating emitted `--format scorecard-row` stdout
  against the `Row::CrapDelta` member of `definitions/Row`'s
  `oneOf`.
