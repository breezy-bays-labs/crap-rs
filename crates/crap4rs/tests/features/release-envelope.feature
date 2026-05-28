Feature: Wire-envelope publication to release assets

  Released crap-rs adapter binaries attach a JSON envelope asset
  (`crap4rs-envelope.json`, `crap4ts-envelope.json`) to every
  release page produced in a release-plz run. The dogfood smoke
  fetches the latest envelope as a `--baseline` source so the
  composite scorecard action's Delta tab renders enabled with
  populated cross-release deltas.

  Both envelopes describe the analyzer's view of a fixed
  pedagogical sample at `crates/crap-examples/` — they are
  baselines for tracking crap-rs's own tool drift across
  releases, NOT a substitute for consumers' main-vs-PR
  baselines (see action README Pattern 2b).

  # ── Asset publication ──────────────────────────────────────────────

  @unwired
  Scenario: release-plz tag publishes both envelope assets
    # tracked: crap-rs#329 — no library-level surface; verified mechanically by post-merge `gh release view --json assets`
    Given release-plz publishes a new tagged release for any crap-rs package
    When the upload jobs complete
    Then every release page in that run carries `crap4rs-envelope.json`
    And every release page in that run carries `crap4ts-envelope.json`

  @unwired
  Scenario: Published envelope shape matches the canary contract
    # tracked: crap-rs#329 — verified mechanically by the release-time `jq -e '.schema_version, .language, .result.summary'` step
    Given the build-envelope job produced `crap4rs-envelope.json`
    When jq queries its top-level keys
    Then `.schema_version` is `1`
    And `.language` is `"rust"`
    And `.result.summary` exists

  # ── Dogfood smoke baseline fetch ───────────────────────────────────

  @unwired
  Scenario: Quick-start smoke fetches envelope from latest release
    # tracked: crap-rs#329 — verified mechanically by the smoke artifact's enabled Delta tabs after the first envelope-bearing release
    Given a previous crap-rs release page carries the envelope assets
    When the quick-start smoke fetches them via `gh release download`
    And invokes the scorecard action with `run-mode: both`
    Then the rendered HTML artifact carries an enabled Delta tab in the Combined panel
    And the rendered HTML artifact carries an enabled Delta tab in the Rust panel
    And the rendered HTML artifact carries an enabled Delta tab in the TypeScript panel

  @unwired
  Scenario: Quick-start smoke falls back when no envelope exists
    # tracked: crap-rs#329 — bootstrap window degradation; verified mechanically by the smoke job's success during the first 1-3 days post-merge
    Given no crap-rs release page yet carries envelope assets
    When the quick-start smoke attempts `gh release download`
    Then the fetch step's outcome is `failure`
    And the smoke continues with `run-mode: full`
    And the rendered HTML artifact carries a disabled Delta tab in every panel

  # ── Coverage staleness forcing function ────────────────────────────

  @unwired
  Scenario: Coverage staleness check emits warning
    # tracked: crap-rs#329 — verified mechanically by inspecting smoke job logs after a `crap-examples/src` edit without coverage regen
    Given a contributor edits `crates/crap-examples/src/event_log.rs`
    And does NOT regenerate `crates/crap-examples/lcov.info`
    When the quick-start smoke runs its coverage staleness check
    Then a `::warning::` annotation is emitted naming the regeneration command
    And the smoke job does NOT fail

  # ── Templated example consistency ──────────────────────────────────

  @unwired
  Scenario: Templated example reads cleanly with run-mode full
    # tracked: crap-rs#329 — verified by reading the templated example file and copy-pasting verbatim into a test workflow

  # NOTE: Scenario 6 is structurally testable via `yq` assertion in CI (CPO HIGH-3
  # finding). The yq-based structural lint is planned for the same PR as π #314;
  # this `.feature` scenario documents the *intent* while CI mechanically enforces
  # it. See crap-rs#329 for the broader BDD-tracker context.
    Given the templated example at `.github/workflows/examples/crap-scorecard.yml`
    When a consumer copies it verbatim
    Then `run-mode` is `full`
    And `baseline:` is absent
    And the action does not error on "both requires baseline"
