---
name: cut-the-crap
description: Drive every over-threshold function below the CRAP threshold by consuming `crap4rs --format advice` output. Cover-then-split heuristic — write tests for low-coverage gaps first, propose named extractions for high-complexity drivers, accept inherent complexity as a last resort.
when_to_use: After running `crap4rs --strict` (or equivalent) and finding functions over threshold; when refactoring to lower CRAP debt; when the user asks "drive my CRAP score down" or "cut the crap on this codebase".
argument-hint: "[--explain-only] [--src <DIR>] [--coverage <FILE>] [--threshold <N>]"
---

# /cut-the-crap

Reference Claude Code skill that consumes `crap4rs --format advice` and drives the cover-then-split remediation loop. The crap4rs binary stays a sharp unix-style emitter (mechanical analysis, structured output); the agent loop — covering untested code first, proposing named extractions, writing a plan, optionally applying changes — lives here in the skill.

This is the user-facing realisation of the [Uncle Bob CRAP-rant loop](https://www.youtube.com/results?search_query=uncle+bob+crap+morning+bathrobe): an agent you point at a codebase that methodically drives every function below threshold, explaining each cut.

## Inputs

The skill takes optional flags:

| Flag | Purpose |
|---|---|
| `--explain-only` | Produce the named-splits plan to `tmp/cut-the-crap-plan.md`, do **not** modify code. |
| `--src <DIR>` | Source directory (default `src`). Forwarded to crap4rs. |
| `--coverage <FILE>` | LCOV path (default `lcov.info`). Forwarded to crap4rs. |
| `--threshold <N>` | CRAP threshold (default crap4rs default). Forwarded to crap4rs. |

If no `lcov.info` exists, generate one first via `cargo llvm-cov --lcov --output-path lcov.info`.

## Process

### Step 1 — Run the analyzer

```bash
crap4rs --src src --coverage lcov.info --format advice --no-fail > tmp/advice.json 2>&1
```

`--format advice` (issue #76) attaches a `Diagnostic` to every over-threshold verdict containing:

| Field | Shape | Meaning |
|---|---|---|
| `coverage_gaps` | `[{ start, end }]` | Inclusive 1-based line ranges with zero hits inside the function. |
| `complexity_drivers` | `[{ kind, line, column, increment, end_line, nesting_depth }]` | AST-derived contributors ranked by impact. |
| `suggested_actions` | tagged enum array | Each entry is one of `add_tests_for_lines`, `extract_function`, `simplify_branching`, `accept_inherent_complexity` (with `applicability` mapping rustc's `Applicability` taxonomy). |
| `root_cause` | `low_coverage` \| `high_complexity` \| `both` | Single-token classification driving the heuristic below. |

Each `extract_function` action carries `candidates: [ProposedSplit]` where a single entry has `recommended: true`.

### Step 2 — Apply the cover-then-split heuristic

Iterate over `result.functions[] | select(.exceeds == true)`. For each, branch on `diagnostic.root_cause`:

- **`low_coverage`** — only action is `add_tests_for_lines`.
  1. Read the function's source range (`identity.span.start_line..end_line`).
  2. For each `LineRange` in `coverage_gaps`, write a focused test case that exercises that line range. Re-use existing test infrastructure (don't add new test frameworks).
  3. Re-run the analyzer; confirm the verdict's CRAP value dropped or `exceeds` flipped to false.

- **`high_complexity`** — actions include `extract_function`, possibly `simplify_branching` or `accept_inherent_complexity`.
  1. Read the function's source.
  2. Pick the `recommended: true` `ProposedSplit`. Use `branch_path` (a `/`-joined chain of `ContributorKind` Display strings, outermost → innermost) as a description anchor.
  3. **Name** the extraction. The `ProposedSplit` only carries coordinates (`line_range`, `branch_path`, `complexity_contribution`, `kind`); naming is the skill's value-add. Pick a name that describes the **invariant** the extracted block enforces — not the line range.
  4. Apply the extraction (move the lines into a private function, replace with a call). Verify the test suite still passes.
  5. Re-run the analyzer; confirm the verdict's CRAP dropped.

- **`both`** — cover first, then re-evaluate.
  1. Apply the `low_coverage` recipe above.
  2. Re-run the analyzer.
  3. If the verdict still exceeds, apply the `high_complexity` recipe to the (now lower-CRAP) function.

The cover-first ordering is load-bearing: covering uncovered branches often drops CRAP enough that no extraction is needed, and extractions made *before* covering shift line ranges around, breaking the gaps stored in `diagnostic.coverage_gaps`.

### Step 3 — Plan before applying

Before modifying any file, write a structured plan to `tmp/cut-the-crap-plan.md`:

```markdown
# /cut-the-crap plan — <timestamp>

## Functions over threshold: N (worst CRAP <X>)

### `<qualified_name>` — CRAP <value>, root_cause <root_cause>
- File: `<file_path>:<start_line>-<end_line>`
- Coverage gaps: <line ranges>
- Recommended action: <ExtractFunction at lines a..b → name `validate_payment_window`>
  - Rationale: isolates the date-range guard so it can be tested independently of the dispatch logic.
- Branch path: `if-branch/match/match-arm`

### `<...>` — ...
```

The user reviews the plan; the skill then applies in plan order.

### Step 4 — `--explain-only` mode

When invoked with `--explain-only`, stop after Step 3. Produce the plan, summarise it to the user, exit. No file modifications.

### Step 5 — Idempotence

Re-running the skill on a codebase whose verdicts all pass is a no-op:

1. Run analyzer.
2. If `result.passed == true`, print "no functions over threshold" and exit cleanly.

### Step 6 — Verify

After applying changes:

1. Run the project's test suite.
2. Re-run `crap4rs --strict` (or the user's CI invocation).
3. Confirm the gate passes. If a function regressed (its CRAP went up), revert the change for that function and report it in the plan.

## Naming guidance

The crap4rs binary ships *coordinates* — it does not invent prose names. The skill's value-add is naming each extraction in terms of the invariant it preserves. Good names:

- Describe **why** the block is grouped, not what lines it spans.
- Anchor on the dominant `ContributorKind` from `complexity_drivers` if useful (e.g., a `MatchArm`-heavy block might extract as `dispatch_<noun>`).
- Avoid line numbers (`extract_lines_47_to_62`) — they rot.

Examples for a real `branch_path`:

| Branch path | Coordinates | Good name | Bad name |
|---|---|---|---|
| `if-branch/match/match-arm` | lines 47-62 | `validate_payment_window` | `do_match_block` |
| `for-loop/if-branch/try` | lines 88-104 | `accumulate_until_first_failure` | `loop_with_retry` |
| `match/match-arm` | lines 12-30 | `dispatch_kind` | `extract_match` |

## Out of scope

- Changing the crap4rs binary's behavior. The skill consumes its output; it does not edit `--format advice` itself.
- Cross-function refactors (moving code between functions, introducing types). Stick to single-function extractions.
- Multi-language analysis. crap4rs ships Rust today; this skill matches that scope.

## Reference invocation

Dogfood on crap4rs itself:

```bash
cd path/to/crap4rs              # adjust to your local clone
cargo llvm-cov --lcov --output-path lcov.info
crap4rs --src src --coverage lcov.info --strict --format advice --no-fail > tmp/crap-advice.json
# Then trigger the skill — it reads tmp/crap-advice.json (or re-invokes crap4rs).
```

The PR shipping this skill (#77) includes one such dogfood example linked from the changelog entry.

## Cross-references

- crap4rs `--format advice` schema: `src/domain/diagnostic.rs` (`Diagnostic`, `SuggestedAction`, `ProposedSplit`, `RootCause`, `Applicability`).
- Issue #76 — introduces `--format advice`.
- Issue #77 — this skill.
- Issue #115 — cucumber-rs adoption (orthogonal, but ships in the same PR).
