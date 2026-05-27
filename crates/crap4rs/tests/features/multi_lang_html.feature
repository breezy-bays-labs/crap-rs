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
    And the error message names the duplicate language

  # ── Single-language composite-action mode ──────────────────────────

  @wired
  Scenario: Single-language composite-action mode emits no unified-render artifact
    Given a workspace configured with a single language adapter
    When the composite scorecard action runs with html-report set true and one language
    Then the workflow produces exactly one HTML artifact named after the adapter
    And the unified HTML render step does not execute

  # ── View axis: Current/Delta tabs inside each language panel ───────

  @wired
  Scenario: Two-language input with baselines renders Current and Delta tabs in every panel
    Given a crap4rs envelope and a crap4ts envelope with matching baselines for each language
    When crap-render is invoked with both current envelopes plus both baseline envelopes
    Then the HTML output contains a `<nav class="tabs"` element inside the Combined panel
    And both per-language panels contain a `<nav class="tabs"` element with Current and Delta tabs
    And no panel renders a disabled Delta tab when its language has a baseline

  # ── View axis: no-baseline path keeps the affordance visible ───────

  @wired
  Scenario: No-baseline multi-language input renders View nav with disabled Delta tab in every panel
    Given a crap4rs JSON envelope and a crap4ts JSON envelope from one workspace
    When crap-render is invoked with both envelopes and --format html
    Then the HTML output contains exactly three `<nav class="tabs"` elements
    And the Combined panel Delta tab is disabled with the cross-adapter no-baselines tooltip
    And both per-language Delta tabs are disabled with their per-language no-baseline tooltip

  # ── View axis: mismatched baselines disable the missing-side Delta tab ─

  @wired
  Scenario: Mismatched baselines render a disabled Delta tab on the language without a baseline
    Given a crap4rs envelope with a matching baseline and a crap4ts envelope without a baseline
    When crap-render is invoked with both current envelopes and the Rust baseline only
    Then the TypeScript panel renders the Delta tab with the disabled attribute
    And the TypeScript Delta tab carries the no-baseline tooltip text
    And the Rust panel renders the Delta tab without the disabled attribute
    And the Combined Delta scope-banner names TypeScript as a language missing a baseline

  # ── Combined Delta cross-adapter ranking ───────────────────────────

  @wired
  Scenario: Combined Delta ranks cross-adapter regressions by risk band desc then ratio desc
    Given a Rust baseline plus a Rust current with one High-risk regression at ratio 5.7
    And a TypeScript baseline plus a TypeScript current with one Moderate-risk regression at ratio 2.5
    When crap-render is invoked with both pairs of envelopes
    Then the Combined Delta tab panel lists the Rust High-risk regression before the TypeScript Moderate-risk regression
    And each Combined Delta row carries an adapter badge identifying its source language

  # ── URL hash deep-linking ──────────────────────────────────────────

  @wired
  Scenario: URL hash routing supports two-axis state with combined:current as the default
    Given two language envelopes with baselines
    When crap-render renders the unified HTML report
    Then the rendered JS parses URL hashes of the shape `#<lang>:<view>`
    And the rendered JS falls back to `#combined:current` when the URL carries no hash
