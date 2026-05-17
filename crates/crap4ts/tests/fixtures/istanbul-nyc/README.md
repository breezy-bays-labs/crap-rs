# istanbul-nyc fixtures

Istanbul `coverage-final.json` payloads in **nyc** shape.

| File | Origin |
|---|---|
| `coverage-final.json` | Synthetic, 3-file nyc-flat shape (absolute paths). |
| `coverage-real-nyc17.json` | **Captured-real** from nyc 17 (+ tsx, `node:test`). |

Paths use the `{SRC_ROOT}` placeholder; smoke tests substitute a
canonical tempdir root.

## `coverage-real-nyc17.json` — captured state

- **Captured on**: 2026-05-17
- **Source**: `tests/fixtures/ts-fixtures/producer-sample.ts` as `src/sample.ts`
- **Toolchain**:
  - node: `v25.9.0`
  - nyc: `17.1.0`
  - tsx: `4.22.1`
  - typescript: `5.9.3`

### Observed shape

nyc instruments TS via the `tsx` loader and source-map remapping; the
remap path emits `"column": null` on some spans. nyc also surfaces
extra `binary-expr` branch types from transpiled helpers and extra
synthesized `(anonymous_N)` fnMap entries. None of `column`, branch
`type`, or `fnMap` is modelled, so the parse is unaffected.

## Capture procedure

1. Scaffold (`package.json` devDeps `nyc@^17.1.0`, `tsx@^4.19.2`,
   `typescript@^5.4.5`; an `nyc` config block with `all: true`,
   `include: ["src/**/*.ts"]`, `exclude: ["src/**/*.test.ts"]`,
   `extension: [".ts"]`, `reporter: ["json"]`, `report-dir:
   "coverage"`):

   ```bash
   mkdir -p nyc/src && cp producer-sample.ts nyc/src/sample.ts
   # add a sample.test.ts using node:test + node:assert
   cd nyc && npm install
   ```

2. Run coverage:

   ```bash
   npx nyc node --import tsx --test src/sample.test.ts
   # writes coverage/coverage-final.json
   ```

3. Substitute the absolute `.../src` prefix with `{SRC_ROOT}` and copy
   in as `coverage-real-nyc17.json`.
