Feature: Unified crap.toml config discovery + legacy dual-discovery

  Both adapters discover a unified `crap.toml` in the working directory.
  For back-compat, each adapter also falls back to its legacy per-adapter
  name (`crap4rs.toml` for crap4rs, `crap4ts.toml` for crap4ts) when no
  `crap.toml` is present, emitting a one-line deprecation notice that
  nudges the operator to rename.

  Discovery walks an ordered name list (`AdapterMeta.config_file_names`)
  whose first entry (`crap.toml`) is canonical — it is what `init` writes
  and what error hints name — and whose remaining entries are legacy
  fallbacks. The first *existing* file wins (existence, not parseability),
  so a present `crap.toml` always takes precedence over a co-present
  legacy file (canonical-wins-by-order). When a legacy file is shadowed by
  `crap.toml`, the operator is told it is safe to remove.

  Naming decision (crap.toml over crap-rs.toml / per-adapter) is recorded
  in the #345 decision record: `-rs` reads as "Rust" on a file both
  adapters consume; `crap.toml` is language-neutral and is the single
  source of truth for shared top-level knobs.

  # Cucumber wiring for these scenarios is deferred to the #169 BDD-wiring
  # umbrella (matching multi_root_src.feature, crap-rs#336). #345 itself
  # verifies the behavior via unit tests on `discover_config`
  # (the AC's 4 back-compat cases) plus assert_cmd integration tests for
  # the stderr deprecation/shadow notices. The crap4ts scenarios are
  # permanently unwireable in *this* crap4rs harness (`CARGO_BIN_EXE_crap4ts`
  # is set only for same-package binaries) and are verified by a
  # `crates/crap4ts/tests/` integration test — same constraint noted in
  # cli_init.feature.

  # ── Canonical discovery (the common path) ──────────────────────────

  @unwired
  Scenario: discovers the canonical crap.toml in the working directory
    # tracked: crap-rs#169 — config_discovery cucumber wiring deferred; #345 verifies via unit + assert_cmd integration
    Given a project directory with a "crap.toml" containing 'threshold = 22.0'
    When the operator runs crap4rs analysis in that directory
    Then the discovered config file is "crap.toml"
    And no deprecation notice is emitted
    And the effective threshold is 22.0

  @unwired
  Scenario: no config file present falls back to built-in defaults silently
    # tracked: crap-rs#169 — config_discovery cucumber wiring deferred; #345 verifies via unit + assert_cmd integration
    Given a project directory with no config file
    When the operator runs crap4rs analysis in that directory
    Then no config file is discovered
    And no deprecation notice is emitted
    And the effective threshold is 15.0

  # ── Legacy fallback (dual-discovery) ───────────────────────────────

  @unwired
  Scenario: falls back to the legacy crap4rs.toml when crap.toml is absent
    # tracked: crap-rs#169 — config_discovery cucumber wiring deferred; #345 verifies via unit + assert_cmd integration
    Given a project directory with a "crap4rs.toml" containing 'threshold = 9.0'
    And the project directory has no "crap.toml"
    When the operator runs crap4rs analysis in that directory
    Then the discovered config file is "crap4rs.toml"
    And the effective threshold is 9.0
    And stderr contains "crap4rs.toml"
    And stderr contains "deprecated"
    And stderr contains "crap.toml"

  # ── Precedence + shadow hygiene (both present) ─────────────────────

  @unwired
  Scenario: crap.toml takes precedence over a co-present legacy crap4rs.toml
    # tracked: crap-rs#169 — config_discovery cucumber wiring deferred; #345 verifies via unit + assert_cmd integration
    Given a project directory with a "crap.toml" containing 'threshold = 22.0'
    And the project directory also has a "crap4rs.toml" containing 'threshold = 9.0'
    When the operator runs crap4rs analysis in that directory
    Then the discovered config file is "crap.toml"
    And the effective threshold is 22.0
    And stderr contains "crap4rs.toml"
    And stderr contains "safe to remove"

  # ── Discovery is existence-based, not parseability-based ───────────
  #
  # A present-but-malformed crap.toml WINS discovery and surfaces its
  # parse error — it must NOT silently fall through to a co-present
  # legacy file. Pins the "first existing file wins" contract (A1) so a
  # typo in crap.toml can never be masked by a stale crap4rs.toml.

  @unwired
  Scenario: a malformed crap.toml errors and does not fall through to legacy
    # tracked: crap-rs#169 — config_discovery cucumber wiring deferred; #345 verifies via unit + assert_cmd integration
    Given a project directory with a "crap.toml" containing 'threshold = not_a_number'
    And the project directory also has a "crap4rs.toml" containing 'threshold = 9.0'
    When the operator runs crap4rs analysis in that directory
    Then the run fails with a parse error naming "crap.toml"
    And the effective threshold is not 9.0

  # ── Cross-adapter parity ───────────────────────────────────────────
  #
  # crap4ts discovers the same canonical crap.toml and falls back to its
  # own legacy crap4ts.toml with the same deprecation nudge. Verified by a
  # plain integration test under `crates/crap4ts/tests/` (see the header
  # comment for why this cucumber harness can't run crap4ts).

  @unwired
  Scenario: crap4ts discovers the canonical crap.toml
    # tracked: crap-rs#169 — verified via crates/crap4ts/tests integration (crap4ts unrunnable in this harness)
    Given a project directory with a "crap.toml" containing 'threshold = 22.0'
    When the operator runs crap4ts analysis in that directory
    Then the discovered config file is "crap.toml"
    And the effective threshold is 22.0

  @unwired
  Scenario: crap4ts falls back to the legacy crap4ts.toml when crap.toml is absent
    # tracked: crap-rs#169 — verified via crates/crap4ts/tests integration (crap4ts unrunnable in this harness)
    Given a project directory with a "crap4ts.toml" containing 'threshold = 9.0'
    And the project directory has no "crap.toml"
    When the operator runs crap4ts analysis in that directory
    Then the discovered config file is "crap4ts.toml"
    And the effective threshold is 9.0
    And stderr contains "crap4ts.toml"
    And stderr contains "deprecated"
    And stderr contains "crap.toml"
