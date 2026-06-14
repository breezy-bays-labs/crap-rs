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

  @wired
  Scenario: The delta block propagates the baseline's diagnostics
    Given a baseline captured with diagnostics at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --no-fail --format json`
    Then the JSON envelope has a "delta.baseline_diagnostics" object

  # ── CLI: identity & relocation through the real pipeline ────────────
  # The matcher's pairing LOGIC (synthetic verdicts → classification) is
  # owned by domain::delta's identity / rename unit tests. These two pin
  # the end-to-end wiring — real source → walker → matcher → envelope.
  # Identity is (file_path, qualified_name): a line shift must stay
  # Modified (one modified, one added, one removed — not two added + two
  # removed), and a whole-file move must pair as a single Renamed.

  @wired
  Scenario: A function whose lines shift stays Modified (identity is file + name)
    Given a baseline of two functions captured at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --no-fail --format json`
    Then the JSON envelope at "delta.summary.modified" is 1
    And the JSON envelope at "delta.summary.added" is 1
    And the JSON envelope at "delta.summary.removed" is 1

  @wired
  Scenario: A relocated function pairs as one Renamed and adds no new violation
    Given a baseline with one function in old_mod.rs captured at threshold 5
    And the function relocates to new_mod.rs
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 5 --no-fail --format json`
    Then the JSON envelope at "delta.summary.renamed" is 1
    And the JSON envelope at "delta.summary.added" is 0
    And the JSON envelope at "delta.summary.removed" is 0
    And the JSON envelope at "delta.summary.new_violations" is 0
    And the JSON envelope at "delta.summary.passed" is true

  # ── CLI: shaping flags compose with delta ──────────────────────────
  # The shaping SEMANTICS (sort order, filter predicate, truncation) are
  # owned by domain::delta::apply unit tests; these pin the flag → spec
  # wiring and the truncation / filter counts the envelope surfaces.

  @wired
  Scenario: --delta-top truncates delta.shown and records the limit
    Given a baseline of two functions captured at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --no-fail --delta-top 2 --format json`
    Then the JSON envelope at "delta.shown" has 2 entries
    And the JSON envelope at "delta.eligible_count" is 3
    And the JSON envelope at "delta.truncated" is true
    And the JSON envelope at "delta.spec.limit" is 2

  @wired
  Scenario: --delta-sort current-crap records the sort key in the spec
    Given a baseline of two functions captured at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --no-fail --delta-sort current-crap --format json`
    Then the JSON envelope at "delta.spec.sort" is "current_crap"

  @wired
  Scenario: --delta-only filters delta.shown to the named change kinds
    Given a baseline of two functions captured at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --no-fail --delta-only added,modified --format json`
    Then the JSON envelope at "delta.shown" has 2 entries
    And the JSON envelope at "delta.spec.filters.change_kinds" is ["added","modified"]

  # ── Validation errors ──────────────────────────────────────────────

  @wired
  Scenario: --baseline with a non-existent path exits 2
    Given a synthetic project with six functions and no baseline
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline does-not-exist.json --threshold 5`
    Then the exit code is 2
    And stderr contains "baseline file not found"

  @wired
  Scenario: --baseline with malformed JSON exits 2
    Given a project with a malformed baseline file present
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline bad.json --threshold 5`
    Then the exit code is 2
    And stderr contains "failed to parse baseline JSON"

  @wired
  Scenario: --baseline with an unsupported schema_version exits 2
    Given a project with a baseline declaring an unsupported schema_version
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline future.json --threshold 5`
    Then the exit code is 2
    And stderr contains "unsupported baseline schema_version"

  @wired
  Scenario: A baseline scored under a different metric warns without changing the gate
    Given a baseline of two functions captured at threshold 1000
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --metric cyclomatic --no-fail --format json`
    Then the exit code is 0
    And stderr contains "metric `cognitive`"
    And stderr contains "`cyclomatic`"
    And the JSON envelope has a "delta" object

  @wired
  Scenario: A baseline that predates the metric field does not warn
    Given a baseline of two functions captured at threshold 1000
    And the baseline metric field is then stripped
    And the project drops one function, modifies another, and adds a third
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --baseline baseline.json --threshold 1000 --metric cyclomatic --no-fail`
    Then the exit code is 0
    And stderr does not contain "metric `"

  @wired
  Scenario: --delta-only with an unknown kind exits 2
    Given a synthetic project with six functions and no baseline
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --delta-only nonsense`
    Then the exit code is 2
    And stderr contains "invalid value 'nonsense' for '--delta-only"

  @wired
  Scenario: --delta-sort with an unknown key exits 2
    Given a synthetic project with six functions and no baseline
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --delta-sort nonsense`
    Then the exit code is 2
    And stderr contains "invalid value 'nonsense' for '--delta-sort"

  @wired
  Scenario: --delta-top with a negative value exits 2
    Given a synthetic project with six functions and no baseline
    When the operator runs `crap4rs --coverage lcov.info --src src --no-gitignore --delta-top -5`
    Then the exit code is 2
    And stderr contains "invalid value '-5' for '--delta-top"

  # ── Help discoverability ───────────────────────────────────────────

  @wired
  Scenario: --help advertises --baseline, --delta-gate, and the delta examples
    When the operator runs `crap4rs --help`
    Then stdout contains "--baseline"
    And stdout contains "--delta-gate"
    And stdout contains "COMPARING TWO ANALYSES"
    And stdout contains "baseline.json"
