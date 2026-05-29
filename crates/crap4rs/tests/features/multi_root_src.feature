Feature: Multi-root source analysis — one scorecard across several source roots

  A project split across multiple source roots (e.g. a Cargo workspace
  with several crates) can be analyzed in a single invocation by passing
  `--src` more than once. The functions discovered under every root are
  unioned into one scorecard, joined against a single shared coverage
  file.

  # Vocabulary used in scenarios:
  #   "the operator"      — the human or script invoking crap4rs
  #   "the JSON envelope" — terminal stdout when --format json
  #   "the run identity base" — the path every function's `file_path`
  #                         is relativized to. Single-root runs use the
  #                         src root (back-compat); multi-root runs use
  #                         the git toplevel so paths stay globally
  #                         unique across roots.
  #
  # Design contract (see shaping.md L2/L3a + adr-multi-root-identity-base):
  #   The identity base is decided ONCE from the root count and applied
  #   to BOTH function identity and coverage-SF normalization. A run is
  #   either single-root/src-relative OR multi-root/toplevel-relative —
  #   the two regimes never mix within one run.
  #
  # Self-CRAP regression invariant: the multi-root code path must not
  # introduce any crap-core/crap4rs/crap4ts function with CRAP above 15;
  # the production scorecard gate enforces this.

  # ── union semantics ────────────────────────────────────────────────

  @unwired
  Scenario: Multiple --src roots union their functions into one report
    # tracked: crap-rs#169 — behavior covered by proptest + multi_root_integration.rs; cucumber-harness wiring deferred to the BDD umbrella
    When the operator runs `crap4rs --coverage lcov.info --src crate-a/src --src crate-b/src --format json`
    Then the JSON envelope includes every function discovered under `crate-a/src`
    And the JSON envelope includes every function discovered under `crate-b/src`
    And the summary `total_functions` equals the count across both roots

  @unwired
  Scenario: Union is order-independent and deduplicates overlapping roots
    # tracked: crap-rs#169 — behavior covered by proptest + multi_root_integration.rs; cucumber-harness wiring deferred to the BDD umbrella
    When the operator analyzes roots `[A, B]` and separately `[B, A]`
    Then both runs report the identical function set
    And a function discovered under two overlapping roots appears exactly once

  # ── identity base (the α' contract) ────────────────────────────────

  @unwired
  Scenario: A single --src root yields src-relative identity (byte-identical back-compat)
    # tracked: crap-rs#169 — behavior covered by proptest + multi_root_integration.rs; cucumber-harness wiring deferred to the BDD umbrella
    When the operator runs `crap4rs --coverage lcov.info --src crate-a/src --format json`
    Then each function `file_path` is relative to `crate-a/src` (e.g. `lib.rs`)
    And the JSON envelope is byte-identical to the pre-multi-root single-root output

  @unwired
  Scenario: Multiple --src roots yield git-toplevel-relative identity
    # tracked: crap-rs#169 — behavior covered by proptest + multi_root_integration.rs; cucumber-harness wiring deferred to the BDD umbrella
    When the operator runs `crap4rs --coverage lcov.info --src crate-a/src --src crate-b/src --format json`
    Then each function `file_path` is relative to the git toplevel (e.g. `crate-a/src/lib.rs`)
    And two same-named files in different roots have distinct `file_path` keys

  # ── coverage join under multi-root (no collision, no bleed) ─────────

  @unwired
  Scenario: Same-named files in different roots do not bleed coverage
    # tracked: crap-rs#169 — behavior covered by proptest + multi_root_integration.rs; cucumber-harness wiring deferred to the BDD umbrella
    Given `crate-a/src/adapters/mod.rs` and `crate-b/src/adapters/mod.rs` both exist
    And the shared coverage file covers both with different hit counts
    When the operator analyzes both roots in one run
    Then each `adapters/mod.rs` function is joined to its own root's coverage
    And neither file's coverage is attributed to the other

  @unwired
  Scenario: Multi-root with an unresolvable git toplevel is a hard error
    # tracked: crap-rs#169 — behavior covered by proptest + multi_root_integration.rs; cucumber-harness wiring deferred to the BDD umbrella
    Given the operator runs outside any git working tree
    When the operator passes more than one --src root
    Then crap4rs exits with an actionable error naming the unresolvable toplevel
    And it does NOT silently fall back to a basename strip

  # ── scorecard action surfaces ──────────────────────────────────────

  @unwired
  Scenario: comment-preamble prepends caller markdown to the sticky comment
    # tracked: crap-rs#169 — action-integration scenarios wire at the CI layer (BDD umbrella), not the cli harness
    Given the scorecard action is invoked with a non-empty `comment-preamble`
    When the action composes the sticky comment
    Then the preamble markdown appears above the rendered scorecard body
    And an empty `comment-preamble` produces a byte-identical comment to today

  @unwired
  Scenario: Production and examples scorecards post distinct, non-colliding comments
    # tracked: crap-rs#169 — action-integration scenarios wire at the CI layer (BDD umbrella), not the cli harness
    Given the production scorecard uses `comment-header: crap-scorecard-production`
    And the examples scorecard uses `comment-header: crap-scorecard-quickstart-smoke`
    When both run on the same pull request
    Then two separate sticky comments are posted
    And each carries its own preamble labeling production vs teaching sample
