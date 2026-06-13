# End-to-end DOM validation — unified HTML report

`validate-unified-html.mjs` loads the rendered **unified multi-language
HTML scorecard** in a headless Chromium (via Playwright) and asserts that
its JavaScript interactivity actually works — not just that the markup is
present. It is the mechanical end-to-end net for the class of bug PR #327
surfaced: the rendered artifact once shipped with *zero* interactive nav
markup despite the feature being "implemented", and the `Given`-based BDD
scenarios passed anyway because nothing ever loaded the page.

This complements — does not replace — the cheaper grep-level check
(`Gate (i)` in `.github/workflows/quick-start-smoke.yml`), which asserts
the interactive *markup* is present. This goes one level deeper and
asserts the *behavior*.

## What it asserts

Against the report's two-axis (Language × View) switcher:

- ≥ 3 `nav.tabs` View navs (Combined + one per language).
- Default load resolves to `#combined:current` with the Combined panel active.
- The active panel's Delta View tab is genuinely **visible** (catches the
  "markup present but hidden" regression).
- **Enabled-Delta** (a baseline was supplied): clicking the Delta tab
  activates the Delta tab-panel and the hash view axis becomes `:delta`.
- **Disabled-Delta** (no baseline): the tab carries the
  `no baselines provided …` tooltip, and a click is a no-op (the view
  stays Current — the handler's `disabled` guard holds).
- Clicking a non-Combined Language button activates its panel, deactivates
  Combined, and updates the hash language axis (`#<lang>:…`).

Both Delta worlds are exercised because the smoke runs in `run-mode:
both` (enabled Delta) when the baseline envelope fetch succeeds and
`run-mode: full` (disabled Delta) on the bootstrap-window fallback.

## How interactions are driven

Clicks use `locator.dispatchEvent('click')` rather than Playwright's
actionable `.click()`. The report's handlers listen for the `click`
event, so dispatching it tests exactly the handler logic; and on a
synthetic `file://` page Playwright's visibility/stability heuristics are
flaky on CSS-transitioned tab buttons (and would refuse to click a
`disabled` Delta tab — yet asserting the disabled tab is a no-op is the
whole point). Visibility, the "hidden markup" regression class, is
asserted explicitly with `isVisible()`.

## Running locally

```bash
cd tests/e2e
npm ci
npx playwright install chromium
# Render a unified HTML first (or extract one of the html_multi snapshots):
node validate-unified-html.mjs /path/to/crap-scorecard-unified.html
```

Exit 0 on success; exit 1 with `::error::` annotations on any failure.

## CI

`Gate (j)` in `quick-start-smoke.yml` runs this against
`steps.crap.outputs.unified-html-path` — the same on-disk file `Gate (i)`
checks — so it self-verifies on every PR with no artifact re-download.
