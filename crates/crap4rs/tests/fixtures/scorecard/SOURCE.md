# Vendored mokumo scorecard schema

`schema.json` is a verbatim copy of
`breezy-bays-labs/mokumo:.config/scorecard/schema.json`, with one local
deviation: the `definitions/CrateBreakout.properties.crate_name.description`
example was changed from a project-specific crate name to the generic
`"my-crate"`. A re-sync via `./regen.sh` must re-apply that sanitization
(or land the same change upstream first).

| Field | Value |
|---|---|
| Source repo | `breezy-bays-labs/mokumo` |
| Source path | `.config/scorecard/schema.json` |
| Vendored at commit | `0cfdb0f` (mokumo PR #780, V4 row populate, 2026-05-03) |
| Schema version | `2` |

## Why vendored

The schema is the contract crap4rs's `--format scorecard-row` (issue #111)
emits against. Vendoring (rather than build-time fetching) keeps
`cargo test` offline-clean and reproducible.

## Regenerating

When mokumo bumps `schema_version` or adds new row variants, run:

```bash
./regen.sh
```

The script copies the schema from the local mokumo checkout
(`~/Github/mokumo/.config/scorecard/schema.json`) and updates this
file's "Vendored at commit" pin in the same edit. Inspect the diff before
committing — a bumped `schema_version` may require crap4rs reporter
updates or test fixture changes.

## Used by

- `tests/scorecard_row_integration.rs` — JSON Schema round-trip test
  validating `crap4rs --format scorecard-row` stdout against the
  CrapDelta member of `$defs/Row`'s `oneOf`.
