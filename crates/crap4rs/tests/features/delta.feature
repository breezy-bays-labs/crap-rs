Feature: --baseline delta analysis (Bundle E, issue #81)

  When investigators or CI consumers want to know whether a code change
  raised or lowered the CRAP risk profile relative to a baseline, they
  pass a previously-emitted crap4rs JSON envelope as `--baseline <path>`.
  Crap4rs computes a delta against the current analysis and emits a
  `delta` block in every reporter format. The delta is **informational
  by default** — `--delta-gate` opts in to making the delta contribute
  to exit code. `--no-fail` overrides both the threshold gate and the
  delta gate.

  The gate keystone holds: `result.passed` always reflects the
  unfiltered current analysis; `delta.summary.passed` reflects the
  delta gate. Both are truthful in JSON even when `--no-fail` forces
  exit 0.

  This file pins only the CLI-acceptance contracts a running binary
  uniquely captures: flag-to-envelope wiring, the gate-semantics
  exit-code matrix, reporter rendering, and the additive JSON envelope
  shape. The DOMAIN behaviors — FunctionChange classification, identity
  / rename matching, new-violation counting, threshold-border epsilon
  math, and DeltaViewSpec filter/sort/truncate — are owned by crap-core's
  `domain::delta` unit and property tests (see `AGENTS.md` § BDD hygiene).
  Each scenario captures a baseline from one source snapshot, optionally
  mutates the source, and re-runs with `--baseline`; the step defs live
  in `tests/delta_cucumber.rs`.

  # ── CLI: --baseline flag resolution ────────────────────────────────

  @wired
  Scenario: --baseline loads the JSON envelope and emits a delta block
    Given a baseline of three passing functions captured at threshold 5
    And the project then adds the three exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 5 --no-fail --format json`
    Then the JSON envelope has a "delta" object
    And the JSON envelope has a "delta.summary" object
    And the JSON envelope has a "delta.spec" object
    And the JSON envelope at "delta.shown" has 6 entries

  @wired
  Scenario: Without --baseline the JSON envelope omits the delta key entirely
    Given a synthetic project with six functions and no baseline
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 1000 --format json`
    Then the JSON envelope has no top-level "delta" key

  @wired
  Scenario: Without --baseline the table reporter renders no delta section
    Given a synthetic project with six functions and no baseline
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 1000 --color never`
    Then stdout does not contain "Delta vs baseline"

  # ── CLI: gate semantics ────────────────────────────────────────────

  @wired
  Scenario: --baseline alone on a passing analysis is informational and exits 0
    Given a baseline of six functions (three exceeding) captured at threshold 1000
    And the project is left unchanged
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000`
    Then the exit code is 0

  @wired
  Scenario: --baseline alone — exit code reflects only the threshold gate, not the delta
    Given a baseline of six functions (three exceeding) captured at threshold 5
    And the project is left unchanged
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 5 --no-fail --format json`
    Then the JSON envelope at "delta.summary.new_violations" is 0
    And the JSON envelope at "delta.summary.passed" is true

  @wired
  Scenario: --delta-gate fails (exit 1) when the delta introduces new violations
    Given a baseline of three passing functions captured at threshold 5
    And the project then adds the three exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 5 --delta-gate`
    Then the exit code is 1

  @wired
  Scenario: --delta-gate without new violations exits 0
    Given a baseline of six functions (three exceeding) captured at threshold 1000
    And the project is left unchanged
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --delta-gate`
    Then the exit code is 0

  @wired
  Scenario: --no-fail overrides --delta-gate while the truth stays in JSON
    Given a baseline of three passing functions captured at threshold 5
    And the project then adds the three exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 5 --delta-gate --no-fail --format json`
    Then the exit code is 0
    And the JSON envelope at "result.passed" is false
    And the JSON envelope at "delta.summary.passed" is false
    And the JSON envelope at "delta.summary.new_violations" is 3

  @wired
  Scenario: --delta-gate requires --baseline (clap rejects it otherwise, exit 2)
    Given a synthetic project with six functions and no baseline
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --threshold 5 --delta-gate`
    Then the exit code is 2
    And stderr contains "baseline"

  # ── CLI: --threshold-epsilon flag wiring (end-to-end) ──────────────
  # The epsilon MATH (border-band suppression, conservation, the
  # one-sided / Added asymmetry) is owned by `domain::delta`'s
  # `within_band` / `change_is_new_violation` tests + the
  # `prop_border_band_conserves_new_violations` proptest. These two
  # scenarios pin only that the `--threshold-epsilon` flag reaches the
  # computation and the suppressed count surfaces in the envelope.

  @wired
  Scenario: --threshold-epsilon suppresses a border-band crossing
    Given a baseline where one covered function scores below threshold 12
    And that function then becomes fully uncovered
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 12 --threshold-epsilon 10 --delta-gate --no-fail --format json`
    Then the JSON envelope at "delta.summary.new_violations" is 0
    And the JSON envelope at "delta.summary.border_jitter_suppressed" is 1
    And the JSON envelope at "delta.summary.passed" is true

  @wired
  Scenario: The same crossing with no epsilon is a genuine new violation
    Given a baseline where one covered function scores below threshold 12
    And that function then becomes fully uncovered
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 12 --delta-gate --no-fail --format json`
    Then the JSON envelope at "delta.summary.new_violations" is 1
    And the JSON envelope at "delta.summary.border_jitter_suppressed" is 0
    And the JSON envelope at "delta.summary.passed" is false

  # ── Reporter rendering ─────────────────────────────────────────────

  @wired
  Scenario: The table reporter renders a Delta section under the analysis table
    Given a baseline of two functions captured at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --color never`
    Then stdout contains "Delta vs baseline:"
    And stdout contains "removed"
    And stdout contains "added"
    And stdout contains "modified"

  @wired
  Scenario: The markdown reporter renders a PR-comment scorecard
    Given a baseline of two functions captured at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --format markdown`
    Then stdout starts with "<!-- crap4rs:scorecard -->"
    And stdout contains "## CRAP Scorecard"
    And stdout contains "- **Delta status:**"
    And stdout contains "- **Changes:**"

  @wired
  Scenario: The CSV reporter switches to a row-per-change schema
    Given a baseline of two functions captured at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --format csv`
    Then stdout starts with "change_kind,"
    And stdout does not contain "exceeds_threshold"

  @wired
  Scenario: --minimal-view does not suppress the delta block
    Given a baseline of three passing functions captured at threshold 5
    And the project then adds the three exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 5 --no-fail --minimal-view --format json`
    Then the JSON envelope at "delta.shown" has 6 entries

  # ── JSON envelope shape ────────────────────────────────────────────

  @wired
  Scenario: The delta is an additive sibling of result under schema_version 2
    Given a baseline of three passing functions captured at threshold 5
    And the project then adds the three exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 5 --no-fail --format json`
    Then the JSON envelope at "schema_version" is 2
    And the JSON envelope has a "delta" object

  @wired
  Scenario: The delta block carries baseline provenance metadata
    Given a baseline of three passing functions captured at threshold 5
    And the project then adds the three exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 5 --no-fail --format json`
    Then the JSON envelope at "delta.baseline_tool_version" holds a non-empty string
    And the JSON envelope at "delta.baseline_timestamp" holds a non-empty string
    And the JSON envelope at "delta.baseline_ref" is null

  @unwired
  Scenario: delta block propagates baseline diagnostics
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help); needs a --verbose baseline capture
    Given the baseline JSON includes a `diagnostics` field
    When the operator runs `crap4rs … --baseline baseline.json --format json`
    Then `delta.baseline_diagnostics` reflects the baseline's diagnostics

  # ── CLI: shaping flags compose with delta ──────────────────────────

  @unwired
  Scenario: --delta-top truncates delta.shown
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    Given the delta has 10 changes
    When the operator runs `crap4rs … --baseline baseline.json --delta-top 3 --format json`
    Then `delta.shown.len()` is `3`
    And `delta.eligible_count` is `10`
    And `delta.truncated` is `true`

  @unwired
  Scenario: --delta-sort current-crap orders by current score
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    When the operator runs `crap4rs … --baseline baseline.json --delta-sort current-crap --format json`
    Then `delta.spec.sort` is `"current_crap"`
    And `delta.shown` is ordered by current.score descending

  @unwired
  Scenario: --delta-only filters to specified change kinds
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    When the operator runs `crap4rs … --baseline baseline.json --delta-only added,modified --format json`
    Then `delta.shown` contains only `Added` and `Modified` entries
    And `Removed` entries are absent

  # ── Validation errors ──────────────────────────────────────────────

  @unwired
  Scenario: --baseline with non-existent path exits 2
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    When the operator runs `crap4rs --coverage lcov.info --baseline /nonexistent.json`
    Then the process exits 2
    And stderr contains "baseline file not found"

  @unwired
  Scenario: --baseline with malformed JSON exits 2
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    Given `bad.json` is not valid JSON
    When the operator runs `crap4rs --coverage lcov.info --baseline bad.json`
    Then the process exits 2
    And stderr contains "failed to parse baseline JSON"

  @unwired
  Scenario: --baseline with mismatched schema_version exits 2
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    Given `future.json` declares an unsupported `schema_version`
    When the operator runs `crap4rs --coverage lcov.info --baseline future.json`
    Then the process exits 2
    And stderr contains "unsupported baseline schema_version"

  @unwired
  Scenario: A baseline scored under a different metric warns without changing the gate
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    Given a baseline captured under the cognitive metric
    When the operator runs `crap4rs … --metric cyclomatic --baseline baseline.json --format json`
    Then stderr warns that the baseline metric differs
    And the delta still computes (the warning is non-fatal)

  @unwired
  Scenario: --delta-only with unknown kind exits 2
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    When the operator runs `crap4rs … --baseline baseline.json --delta-only nonsense`
    Then the process exits 2
    And stderr contains "invalid value 'nonsense' for '--delta-only'"

  @unwired
  Scenario: --delta-sort with unknown key exits 2
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    When the operator runs `crap4rs … --baseline baseline.json --delta-sort nonsense`
    Then the process exits 2
    And stderr contains "invalid value 'nonsense' for '--delta-sort'"

  @unwired
  Scenario: --delta-top with negative value exits 2
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    When the operator runs `crap4rs … --baseline baseline.json --delta-top -5`
    Then the process exits 2
    And stderr contains "invalid value '-5' for '--delta-top'"

  # ── Help discoverability ───────────────────────────────────────────

  @unwired
  Scenario: --help advertises --baseline and --delta-gate
    # tracked: crap-rs#169 — wired in the delta curated-pass slice 2 (shaping + validation + help)
    When the operator runs `crap4rs --help`
    Then stdout mentions "--baseline"
    And stdout mentions "--delta-gate"
    And stdout shows a basic delta example
    And stdout shows the scorecard example with `--format markdown`
