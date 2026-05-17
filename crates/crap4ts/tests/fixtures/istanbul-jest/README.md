# istanbul-jest fixtures

Istanbul `coverage-final.json` payloads in **jest** shape.

| File | Origin |
|---|---|
| `coverage-final.json` | Synthetic (hand-authored), 5-file jest-flat shape — the W1.1 happy-path corpus. |
| `coverage-with-branches.json` | Synthetic, branch-coverage corpus (`b` + `branchMap`). |
| `coverage-real-jest29.json` | **Captured-real** from jest 29 via `ts-jest`. |

Paths use the `{SRC_ROOT}` placeholder; smoke tests substitute a
canonical tempdir root (see `tests/istanbul_smoke.rs`).

## `coverage-real-jest29.json` — captured state

- **Captured on**: 2026-05-17
- **Source**: `tests/fixtures/ts-fixtures/producer-sample.ts` (one file:
  statements + an `if` + a ternary + an inline anonymous arrow +
  an uncovered function), copied in as `src/sample.ts`
- **Toolchain**:
  - node: `v25.9.0`
  - jest: `29.7.0`
  - ts-jest: `29.4.9`
  - typescript: `5.9.3`

### Observed shape

- `fnMap` names for the inline arrow are synthesized `(anonymous_N)`
  (babel-plugin-istanbul), **never `null`**.
- `branchMap` `type` is concrete (`if`, `cond-expr`); no null columns.

## Capture procedure

Reproducible from a throwaway scaffold. Only the machine-absolute
source dir is rewritten to `{SRC_ROOT}`; the payload is otherwise
byte-faithful.

1. Scaffold (`package.json` devDeps `jest@^29.7.0`, `ts-jest@^29.1.2`,
   `typescript@^5.4.5`, `@types/jest@^29.5.12`; `jest.config.js` with
   `preset: "ts-jest"`, `rootDir: "src"`,
   `collectCoverageFrom: ["**/*.ts", "!**/*.test.ts"]`):

   ```bash
   mkdir -p jest/src && cp producer-sample.ts jest/src/sample.ts
   # add a sample.test.ts that imports + exercises classify(5) and double(3)
   cd jest && npm install
   ```

2. Run coverage:

   ```bash
   npx jest --coverage --coverageReporters=json --ci
   # writes src/coverage/coverage-final.json (rootDir-relative)
   ```

3. Substitute the absolute `.../src` prefix with `{SRC_ROOT}` and copy
   into this directory as `coverage-real-jest29.json`.
