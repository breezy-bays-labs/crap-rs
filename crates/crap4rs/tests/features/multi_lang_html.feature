Feature: Multi-language unified HTML report

  The crap-render binary composes per-language analysis envelopes
  (one per adapter — crap4rs for Rust, crap4ts for TypeScript, etc.)
  into a single HTML document with two-axis navigation: a Language
  axis (Rust / TypeScript / Combined) above the existing View axis
  (Current / Delta) from the HTML reporter delta tab.

  Combined view layers a workspace-wide ranked-CRAP table — grouped
  by risk level (per-adapter calibrated) and sorted by
  CRAP/threshold ratio within each band — over the per-adapter
  views, so reviewers can see workspace risk at a glance and drill
  down by language with one click.

  The composite scorecard action uses crap-render in multi-language
  mode (`languages: rust,typescript` or `all`); single-language mode
  preserves byte-identical output for existing consumers.

  # ── Single-language passthrough ───────────────────────────────────

  @wired
  Scenario: Single-language input renders byte-identical to the adapter binary
    Given a crap4rs JSON envelope from a representative workspace
    When crap-render is invoked with that single envelope and --format html
    Then the HTML output is byte-identical to crap4rs --format html on the same workspace
    And the output contains no `<nav class="segmented"` markup
    And the output contains no Combined panel

  # ── Multi-language unified structure ───────────────────────────────

  @wired
  Scenario: Two-language input renders a unified document with two-axis navigation
    Given a crap4rs JSON envelope and a crap4ts JSON envelope from one workspace
    When crap-render is invoked with both envelopes and --format html
    Then the HTML output contains exactly one `<nav class="segmented"` element
    And the segmented nav has buttons with data-lang "rust", "typescript", and "combined"
    And the Combined panel button is rendered active by default
    And the document footer contains an Adapters provenance grid listing both languages

  # ── Combined view: ranked-CRAP table ───────────────────────────────

  @wired
  Scenario: Combined view ranks functions by risk level desc then ratio desc
    Given a crap4rs envelope with one High-risk function at CRAP/threshold ratio 5.7
    And a crap4ts envelope with one Moderate-risk function at CRAP/threshold ratio 2.5
    When crap-render is invoked with both envelopes and --format html
    Then the Combined panel ranked table lists the Rust High-risk function before the TypeScript Moderate-risk function
    And each row carries an adapter badge identifying its source language

  # ── Schema version mismatch ────────────────────────────────────────

  @wired
  Scenario: Mismatched schema_version on input envelopes fails with an actionable error
    Given two JSON envelopes carrying different schema_version values
    When crap-render is invoked with both envelopes
    Then crap-render exits with non-zero status
    And the error message names the offending envelope path and the unsupported schema_version value

  # ── Duplicate adapter tuple ────────────────────────────────────────

  @wired
  Scenario: Two envelopes for the same adapter language fails with an actionable error
    Given two crap4rs JSON envelopes from the same workspace
    When crap-render is invoked with both envelopes
    Then crap-render exits with non-zero status
    And the error message names the duplicate tool_name

  # ── Single-language composite-action mode ──────────────────────────

  @wired
  Scenario: Single-language composite-action mode emits no unified-render artifact
    Given a workspace configured with a single language adapter
    When the composite scorecard action runs with html-report set true and one language
    Then the workflow produces exactly one HTML artifact named after the adapter
    And the unified HTML render step does not execute
