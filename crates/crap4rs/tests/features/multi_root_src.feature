Feature: Multi-root source analysis — one scorecard across several source roots

  A project split across multiple source roots (e.g. a Cargo workspace
  with several crates) can be analyzed in a single invocation by passing
  `--src` more than once. The functions discovered under every root are
  unioned into one scorecard, joined against a single shared coverage
  file.

  This file pins the CLI-process contracts the running binary uniquely
  captures: that `prepare_pipeline` resolves the run identity base from
  the `--src` root COUNT (one root ⇒ src-relative, byte-compatible with
  the pre-multi-root path; many roots ⇒ git-toplevel-relative so paths
  stay globally unique), and that multi-root outside a git work tree is a
  hard error rather than a silent basename strip. The union/dedup
  invariant, the IdentityBase consumption, and the coverage-join
  no-bleed contract are owned IN-PROCESS by `multi_root_integration.rs`
  (it constructs `core::identity::IdentityBase` directly and calls
  `analyze()` at the library boundary — the sole lib coverage of that
  path, so it stays) plus the union proptest; the scorecard action's
  comment-preamble / comment-header surfaces are owned at the CI layer
  (`.github/actions/scorecard/action.yml` + the dogfood smoke jobs), not
  by the binary. So those cases live there, not here (see `AGENTS.md`
  § BDD hygiene). Step defs in `tests/multi_root_cucumber.rs`.

  # ── union semantics (end-to-end via the CLI) ───────────────────────

  @wired
  Scenario: Multiple --src roots union their functions into one report
    Given a git work tree with source roots crate-a/src and crate-b/src
    When the operator runs `crap4rs --coverage lcov.info --src crate-a/src --src crate-b/src --threshold 100 --no-gitignore --no-fail --format json`
    Then view.shown has 4 functions
    And view.shown contains a function keyed "crate-a/src/lib.rs"
    And view.shown contains a function keyed "crate-b/src/lib.rs"

  # ── identity base resolved from the --src root count ────────────────

  @wired
  Scenario: A single --src root yields src-relative identity (back-compat)
    Given a git work tree with source roots crate-a/src and crate-b/src
    When the operator runs `crap4rs --coverage lcov.info --src crate-a/src --threshold 100 --no-gitignore --no-fail --format json`
    Then view.shown contains a function keyed "lib.rs"
    And every view.shown function key is src-relative

  @wired
  Scenario: Multiple --src roots yield git-toplevel-relative identity with distinct keys
    Given a git work tree with source roots crate-a/src and crate-b/src
    When the operator runs `crap4rs --coverage lcov.info --src crate-a/src --src crate-b/src --threshold 100 --no-gitignore --no-fail --format json`
    Then view.shown contains a function keyed "crate-a/src/adapters/mod.rs"
    And view.shown contains a function keyed "crate-b/src/adapters/mod.rs"

  # ── hard error: multi-root requires a resolvable git toplevel ───────

  @wired
  Scenario: Multi-root outside a git work tree is a hard error, not a basename strip
    Given a non-git directory with source roots crate-a/src and crate-b/src
    When the operator runs `crap4rs --coverage lcov.info --src crate-a/src --src crate-b/src --threshold 100 --no-gitignore --no-fail --format json`
    Then the exit code is 2
    And stderr contains "multi-root analysis requires a git work tree"
    And stderr contains "not inside a git work tree"
