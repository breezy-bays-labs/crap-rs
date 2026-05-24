Feature: GitHub Actions inline annotations reporter (issue #276)

  The `github-annotations` reporter emits GitHub Actions workflow-command
  lines (`::warning file=…,line=…,title=…::message`) so CRAP findings
  render inline on the PR "Files Changed" tab — universal, free, no
  GHAS / Code Scanning dependency.

  Like the SARIF reporter, this is a *gate translation*: results derive
  from `view.full.functions.iter().filter(|v| v.exceeds)`, not the
  shaped `view.shown`. PR annotations must reflect the gate, not a
  presentation choice. The annotation cap (`--annotation-limit`) is a
  display safety net for the GitHub Actions per-step UI limit, not a
  shaping of the gate; the count emitted in the trailing notice always
  reflects the unshaped eligible set.

  # No `Background:` block — Rule 4 requires Backgrounds to be executable,
  # but 17/19 scenarios specify their own fixture cardinality (15, 12, 6,
  # three, eleven, fifteen, etc.) and would override any shared setup.
  # The 2 scenarios with no per-scenario `Given` inline `Given several
  # exceeding functions` themselves, matching the #167/#168 precedent.
  #
  # Step text on a single line per step: the cucumber-rs gherkin parser
  # does not support indented continuation lines on step text (verified
  # empirically), so each step keyword's full text lives on one line.

  # ── Envelope shape ─────────────────────────────────────────────────

  @wired
  Scenario: --format github-annotations emits one ::warning per exceeding function
    Given several exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then stdout contains one line starting with `::warning ` per exceeding function (up to the annotation limit)
    And every emitted line includes a `file=<path>`, `line=<number>`, `title=CRAP <score:.1>` triple before the `::` data separator
    And the message data after `::` includes the function's qualified name, CRAP score, complexity, coverage percent, and the threshold value

  @wired
  Scenario: Functions at or below threshold produce no annotation
    Given every function is below the threshold
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then stdout is empty (no `::warning`, no `::notice`, no other workflow commands)

  @wired
  Scenario: Severity is single-tier ::warning regardless of risk level
    Given exceeding functions across risk levels high, moderate, acceptable
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then every emitted line begins with `::warning ` — never `::error ` or `::notice ` (the trailing summary notice in the cap scenario is the only exception)

  # ── Ordering & cap ─────────────────────────────────────────────────

  @wired
  Scenario: Annotations are sorted by CRAP score descending
    Given five exceeding functions with distinct CRAP scores
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then the emitted lines appear in CRAP-score-descending order (the worst function's annotation first, the least-bad last)

  @unwired
  Scenario: --annotation-limit caps the emitted set
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    Given fifteen exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations --annotation-limit 10`
    Then exactly ten `::warning` lines are emitted (the ten with the highest CRAP)
    And exactly one trailing `::notice::` line is emitted whose message names the remaining count: `5 more functions exceed threshold; see scorecard for the full list`

  @unwired
  Scenario: No truncation notice when the cap is not exceeded
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    Given three exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations --annotation-limit 10`
    Then three `::warning` lines are emitted and no `::notice` line follows

  @unwired
  Scenario: Default annotation limit is 10
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    Given twelve exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations` (without an explicit `--annotation-limit`)
    Then ten `::warning` lines and one trailing `::notice::` line are emitted

  @unwired
  Scenario: --annotation-limit can be configured via crap4rs.toml
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    Given a `crap4rs.toml` with `[output] annotation_limit = 25`
    And eleven exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then all eleven `::warning` lines are emitted and no `::notice` line follows

  @unwired
  Scenario: CLI --annotation-limit overrides crap4rs.toml
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    Given a `crap4rs.toml` with `[output] annotation_limit = 25`
    And fifteen exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations --annotation-limit 5`
    Then exactly five `::warning` lines are emitted (the CLI flag wins)

  # ── Escaping per the GH Actions workflow-command spec ──────────────

  @unwired
  Scenario: Percent, carriage-return, and newline characters in the message are escaped
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    Given an exceeding function whose qualified name contains `%`, `\r`, and `\n`
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then the emitted message replaces `%` with `%25`, `\r` with `%0D`, `\n` with `%0A`
    And the `file=`, `line=`, and `title=` property values are not modified (no dynamic data lands in property fields, so delimiter escaping is unnecessary)

  @unwired
  Scenario: Rust qualified names containing :: are preserved verbatim in the message
    # tracked: crap-rs#283 — syn walker emits only single-tier qualified names (Type::method); nested-mod qualification (module::submodule::function) needs walker work
    Given an exceeding function with qualified name `module::submodule::function`
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then the emitted message includes `module::submodule::function` verbatim (colons are legal in workflow-command message data)

  # ── Path handling ──────────────────────────────────────────────────

  @wired
  Scenario: file= property is CWD-relative when the analyzed path lives under CWD
    Given the operator's CWD is the project root and an exceeding function in `src/lib.rs`
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then the annotation's `file=` value is `src/lib.rs` (relative), not the absolute path

  @unwired
  Scenario: file= falls back to the absolute path when strip_prefix fails
    # tracked: crap-rs#284 — walker normalizes all paths relative to --src; absolute-path fallback branch is unreachable via CLI today (reporter-side behavior covered by inline unit tests)
    Given an analyzed file whose absolute path does not start with CWD
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then the annotation's `file=` value is the absolute path

  # ── Gate keystone: reporter iterates the FULL analysis ─────────────

  @unwired
  Scenario: --top does NOT shrink the annotation set
    # tracked: crap-rs#276 — depends on --annotation-limit landing in Session 2; clap rejects the flag until then
    Given six exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations --top 2 --annotation-limit 10`
    Then six `::warning` lines are emitted (the `--top` view shaping is independent of the annotation cap; the cap is the GitHub UI limit, not a display knob)

  @wired
  Scenario: --only-failing is a no-op for the annotation set
    Given an analysis with both passing and exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations --only-failing`
    Then the annotation set is identical to the run without `--only-failing` (the reporter already filters to `exceeds == true`)

  @wired
  Scenario: --sort-by does NOT reorder the annotations
    Given several exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations --sort-by coverage`
    Then the emitted lines remain CRAP-score-descending (the reporter's own ordering invariant — the View's sort key does not leak through)

  # ── Multi-format composition ───────────────────────────────────────

  @unwired
  Scenario: github-annotations can be combined with another format in one invocation
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    Given several exceeding functions
    When the operator runs `crap4rs --coverage lcov.info --format markdown:scorecard.md,github-annotations`
    Then `scorecard.md` is created with the markdown reporter's output
    And stdout contains the `::warning` lines from the annotation reporter
    And the two reporters produce consistent function counts (the annotation cap may truncate but the markdown is full-fidelity)

  # ── Empty / boundary cases ─────────────────────────────────────────

  @unwired
  Scenario: Empty analysis produces empty output
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    Given no functions are discovered
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations`
    Then stdout is empty

  @unwired
  Scenario: --annotation-limit 0 is rejected at the CLI boundary
    # tracked: crap-rs#276 — github-annotations reporter not yet wired
    When the operator runs `crap4rs --coverage lcov.info --format github-annotations --annotation-limit 0`
    Then the CLI exits non-zero with a clap error explaining the value must be ≥ 1 (the per-step display cap is meaningless at zero; the user almost certainly meant to use the default)
