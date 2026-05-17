# istanbul-vitest fixtures

Istanbul `coverage-final.json` payloads in **vitest** (`@vitest/coverage-istanbul`) shape.

| File | Origin |
|---|---|
| `coverage-final.json` | Synthetic, 3-file vitest-flat shape with emitter metadata. |
| `coverage-with-null-columns.json` | Derived from the W3.1 crap4ts@1.x corpus — the original `column: null` regression corpus. |
| `coverage-real-vitest4.json` | **Captured-real** from `@vitest/coverage-istanbul@4`. |

Paths use the `{SRC_ROOT}` placeholder; smoke tests substitute a
canonical tempdir root.

## `coverage-real-vitest4.json` — captured state

- **Captured on**: 2026-05-17
- **Source**: `tests/fixtures/ts-fixtures/producer-sample.ts` as `src/sample.ts`
- **Toolchain**:
  - node: `v25.9.0`
  - vitest: `4.1.6`
  - @vitest/coverage-istanbul: `4.1.6`
  - typescript: `5.9.3`

### Observed shape — why the 4.x capture matters

The 3.x line emitted `"column": null` on span ends. The 4.x major bump
did **not** make columns concrete: `coverage-real-vitest4.json` still
carries `"column": null` on every span end and empty `{"start":{},
"end":{}}` objects inside `branchMap.locations[]`. The parser models
neither (only `start.line` is read), so the original deserialization
bail does not occur. This fixture is the live regression lock that 4.x
remains tolerated. Inline-arrow `fnMap` names are synthesized
`(anonymous_N)`.

## Capture procedure

1. Scaffold (`package.json` devDeps `vitest@^4.0.0`,
   `@vitest/coverage-istanbul@^4.0.0`, `typescript@^5.4.5`;
   `vitest.config.ts` with `coverage.provider: "istanbul"`,
   `reporter: ["json"]`, `root: "src"`):

   ```bash
   mkdir -p vitest/src && cp producer-sample.ts vitest/src/sample.ts
   # add a sample.test.ts importing vitest's test/expect
   cd vitest && npm install
   ```

2. Run coverage:

   ```bash
   npx vitest run --coverage
   # writes src/coverage/coverage-final.json
   ```

3. Substitute the absolute `.../src` prefix with `{SRC_ROOT}` and copy
   in as `coverage-real-vitest4.json`.
