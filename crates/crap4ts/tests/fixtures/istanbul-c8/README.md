# istanbul-c8 fixtures

Istanbul `coverage-final.json` payloads from **c8** (`--reporter=json`).

| File | Origin |
|---|---|
| `coverage-real-c8.json` | **Captured-real** from c8 10 (+ tsx, `node:test`). |

This directory is wholly captured (no synthetic fixture). Paths use the
`{SRC_ROOT}` placeholder; smoke tests substitute a canonical tempdir
root.

## Discovery resolved: c8 does emit Istanbul shape

c8 is a native V8-coverage tool, but it converts V8 coverage into an
Istanbul `CoverageMap` and its `json` reporter writes a
`coverage-final.json` in the flat Istanbul shape the parser's arm 1
already consumes — `statementMap` / `fnMap` / `branchMap` / `s` / `b` /
`f` / `path`, plus one **extra per-entry `all` field** and a generic
`type: "branch"` on every `branchMap` entry (c8 does not classify
`if`/`cond-expr`/`switch`). `fnMap` names are concrete transpiled
helper names (e.g. `__name`, `__export`), not `(anonymous_N)`. None of
`all`, branch `type`, or `fnMap` is modelled, so the parse is
unaffected. The canonical flag is `--reporter=json` (verified against
c8 10.x).

## `coverage-real-c8.json` — captured state

- **Captured on**: 2026-05-17
- **Source**: `tests/fixtures/ts-fixtures/producer-sample.ts` as `src/sample.ts`
- **Toolchain**:
  - node: `v25.9.0`
  - c8: `10.1.3`
  - tsx: `4.22.1`
  - typescript: `5.9.3`

## Capture procedure

1. Scaffold (`package.json` devDeps `c8@^10.1.3`, `tsx@^4.19.2`,
   `typescript@^5.4.5`):

   ```bash
   mkdir -p c8/src && cp producer-sample.ts c8/src/sample.ts
   # add a sample.test.ts using node:test + node:assert
   cd c8 && npm install
   ```

2. Run coverage (c8 wraps the node process, sets `NODE_V8_COVERAGE`,
   converts V8 → Istanbul):

   ```bash
   npx c8 --reporter=json --src=src \
     --include='src/**/*.ts' --exclude='src/**/*.test.ts' \
     --reports-dir=coverage \
     node --import tsx --test src/sample.test.ts
   # writes coverage/coverage-final.json (Istanbul shape)
   ```

3. Substitute the absolute `.../src` prefix with `{SRC_ROOT}` and copy
   in as `coverage-real-c8.json`.
