# crap-examples

Pedagogical sample crate. Four Rust modules under `src/` and four
matching TypeScript modules under `ts/` carry deliberately-chosen
shapes so the rendered CRAP scorecard spans every risk band.

`publish = false` keeps this crate out of crates.io; the workspace
LCOV gate explicitly excludes it from aggregate coverage signals.
The fixtures committed at `lcov.info` and `coverage-final.json` feed
the release-plz envelope publication job — every released crap-rs
adapter binary attaches a JSON envelope describing its analysis of
this corpus, and the repo's own dogfood smoke fetches the latest
envelope as a `--baseline` for the Delta tab. See the action's
README § "Pattern 2b — Cross-release envelope baseline" for the
consumer-facing pattern this crate exists to dogfood.

## Why these four modules

Each module isolates one term or scaling pattern of the CRAP formula:

```
CRAP(c, cov) = c² × (1 − cov)³ + c
```

| Module | Class taught | Target c | Target cov | Target CRAP |
|---|---|---|---|---|
| `event_log` | Baseline — low c × high cov anchors scale | 1-2 | 95-100% | 1-3 (Low) |
| `csv_parser` | c² at full coverage — quadratic complexity dominates | 8-12 | 95-100% | 10-15 (Acceptable) |
| `string_utils` (slugify) | (1-cov)³ non-linearity — moderate c, partial cov | ~20 | 70-85% | 20-30 (Moderate) |
| `config_merger` | Compound catastrophe — high c × low cov product | ~20 | 60-70% | 35-50 (High) |

The TypeScript counterparts at `ts/` mirror the same intent module-
for-module, with the TS adapter's default cyclomatic metric (the
Rust adapter defaults to cognitive). Per-language complexity-count
differences are expected — what stays constant across adapters is
the cross-module ranking and the four-band distribution.

The acceptance gate is "the four modules together span the Low,
Acceptable, and Moderate risk bands on a fresh run." When a
contributor edits a module, the smoke job's coverage-staleness
check warns if the committed fixtures weren't regenerated — see the
regen recipe below.

## Worked example: c × cov heatmap

The pedagogical heatmap below shows where each module's headline
function lands in the c × cov plane. The risk-band boundaries are
fixed (`Low ≤ 8 < Acceptable ≤ 15 < Moderate ≤ 25 < High`), so
moving any module within its row changes its score without changing
the heatmap structure.

```
cov ↑
100%  │  pluralize      parse_record               ·             ·
      │  truncate          (c²=121)
      │  (low band)        (acceptable band)
      │
 75%  │       ·            ·         merge_configs           ·
      │                              (compound, moderate)
      │
 ~70% │       ·            ·          slugify                ·
      │                              (c²×(1-cov)³)
      │                              (moderate band)
      │
  0%  │  (unmapped — every module's headline fn has at least one test)
      └────────────────────────────────────────────────────────────→ c
         1-4                  8-12              ~13-16
```

Reading the heatmap: the Y-axis collapses coverage gaps,
contributing the cubic term; the X-axis multiplies the complexity
squared. The compound term — high c × low coverage — drives the
score that puts `merge_configs` and `slugify` near the top of the
Moderate band; the same compound term, applied to a function with
both higher c AND lower coverage, would push it into the High band.

## Regenerating the fixtures

Both fixtures are committed verbatim. Regenerate them locally
whenever a module's source changes; the smoke job warns (does not
fail) when source-without-regen drift is detected.

### Rust (`lcov.info`)

```shell
cargo llvm-cov nextest --package crap-examples \
  --lcov --output-path crates/crap-examples/lcov.info
sed -i.bak 's|^SF:.*/crates/crap-examples/src/|SF:|' crates/crap-examples/lcov.info
rm -f crates/crap-examples/lcov.info.bak
```

The `sed` step strips the absolute prefix that `cargo llvm-cov`
emits by default. The adapters' coverage-to-function matcher joins
the LCOV's `SF:` paths onto `--src` before lookup, so the committed
fixture uses paths relative to `--src` (just the file basename in
this single-directory case). The release-plz envelope build job's
CI lint rejects any committed `lcov.info` with absolute `SF:` lines
— stripping is mandatory.

### TypeScript (`coverage-final.json`)

```shell
cd crates/crap-examples/ts
npm install
npm test -- --coverage
cd ../../..
jq --arg p "$PWD/crates/crap-examples/ts/" \
   'with_entries(.key |= ltrimstr($p) | .value.path |= ltrimstr($p))' \
   crates/crap-examples/ts/.vitest-coverage/coverage-final.json \
  > crates/crap-examples/coverage-final.json
```

The vitest config emits the Istanbul-shape coverage that `crap4ts`
consumes (`v8` provider's JSON shape is incompatible — must be
`@vitest/coverage-istanbul`). The `jq` step strips the absolute
prefix vitest writes by default; same rationale as the LCOV step
above. The committed envelope uses file basenames as keys
(matching the structure the adapter resolves under `--src ts/`).

## Reading the envelope

After regenerating the fixtures, you can preview the envelope shape
the release publishes:

```shell
cargo run --release --package crap4rs -- \
  --src ./crates/crap-examples/src \
  --coverage crates/crap-examples/lcov.info \
  --format json --no-fail \
  | jq '.result.summary'
```

The same shape gets uploaded as `crap4rs-envelope.json` to every
crap-rs release page; the dogfood smoke fetches it back as
`--baseline` to render an enabled Delta tab in the unified HTML
report.
