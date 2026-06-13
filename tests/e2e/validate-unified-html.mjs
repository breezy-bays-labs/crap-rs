// End-to-end DOM validation of the crap-rs unified multi-language HTML
// report (crap-rs#328). The quick-start smoke already grep-asserts that
// the interactive MARKUP is present (`<nav class="tabs">` count, disabled
// Delta attributes). This goes one level deeper: it loads the rendered
// report in a real headless browser and asserts the JAVASCRIPT actually
// works — clicking a Language button swaps the visible panel, clicking a
// View tab swaps the visible tab-panel, and the URL hash reflects the
// two-axis `#<lang>:<view>` state. A grep can't catch a broken event
// handler or a regressed hash router; this can. (Origin: PR #327 review
// found the rendered artifact had ZERO interactive nav markup despite the
// feature being "implemented" — the Given-based BDD scenarios passed
// anyway. This is the mechanical end-to-end net for that class of bug.)
//
// Usage: node validate-unified-html.mjs <path-to-unified-html>
// Exits 0 on success, 1 (with ::error:: annotations) on any failure.

import { chromium } from "playwright";
import { pathToFileURL } from "node:url";
import { existsSync, statSync } from "node:fs";

const htmlPath = process.argv[2];
if (!htmlPath || !existsSync(htmlPath) || statSync(htmlPath).size === 0) {
  console.error(
    `::error::validate-unified-html: missing or empty HTML path: ${htmlPath || "<unset>"}`,
  );
  process.exit(1);
}

const failures = [];
const check = (cond, msg) => {
  if (cond) {
    console.log(`  ok: ${msg}`);
  } else {
    failures.push(msg);
    console.error(`::error::unified HTML interactivity: ${msg}`);
  }
};

const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  // Surface page console errors (a thrown handler would otherwise be silent).
  page.on("pageerror", (err) =>
    failures.push(`page threw: ${err.message}`),
  );
  await page.goto(pathToFileURL(htmlPath).href);

  // The inline boot script sets the default hash via history.replaceState
  // on load; wait for it so we never race the router.
  await page
    .waitForFunction(() => location.hash === "#combined:current", null, {
      timeout: 5000,
    })
    .catch(() => {
      /* asserted explicitly below for a clear message */
    });

  // 1) At least 3 View-axis navs: Combined + one per language (rust, ts).
  const navCount = await page.locator("nav.tabs").count();
  check(navCount >= 3, `>=3 nav.tabs present (Combined + per-language); got ${navCount}`);

  // 2) Default two-axis state on first load: #combined:current, with the
  //    Combined language panel active.
  check(
    page.url().endsWith("#combined:current"),
    `default hash is #combined:current; got ${page.url().split("#")[1] ? "#" + page.url().split("#")[1] : "<none>"}`,
  );
  check(
    (await page.locator('.lang-panel[data-lang="combined"][data-active]').count()) === 1,
    "Combined language panel is active on load",
  );

  // Interactions are driven with dispatchEvent('click'), which fires the
  // exact `click` event the report's handlers listen for. We deliberately
  // avoid Playwright's actionable .click(): on a synthetic file:// page its
  // visibility/stability heuristics are flaky on these CSS-transitioned
  // tab buttons, and they would ALSO refuse to click a `disabled` Delta
  // tab — but asserting the disabled tab is a no-op is exactly what we
  // need. Visibility (the "markup present but hidden" regression class
  // from PR #327) is asserted explicitly via isVisible() instead.

  // 3) View axis: the active panel's Delta tab. Two valid worlds:
  //    - baseline present → Delta enabled → click switches the visible
  //      tab-panel to delta and the hash view becomes :delta.
  //    - no baseline      → Delta disabled → it carries the explanatory
  //      tooltip and its handler guard makes a click a no-op (stays current).
  const activeDelta = page.locator(
    '.lang-panel[data-active] .tabs .tab[data-tab="delta"]',
  );
  // Resolve the count ONCE and guard every following operation on it. A
  // bare `.first().getAttribute()`/`.dispatchEvent()` on an empty locator
  // would auto-wait the full 30s timeout and throw an opaque TimeoutError
  // instead of failing fast with the assertion below (gemini catch).
  const deltaCount = await activeDelta.count();
  check(deltaCount >= 1, "active panel exposes a Delta View tab");
  if (deltaCount >= 1) {
    const firstDelta = activeDelta.first();
    check(await firstDelta.isVisible(), "Delta View tab is actually visible (not hidden markup)");
    const deltaDisabled = (await firstDelta.getAttribute("disabled")) !== null;
    if (!deltaDisabled) {
      await firstDelta.dispatchEvent("click");
      check(
        (await page
          .locator('.lang-panel[data-active] .tab-panel[data-tab="delta"][data-active]')
          .count()) === 1,
        "clicking the enabled Delta tab activates the Delta tab-panel",
      );
      check(
        page.url().includes(":delta"),
        `hash view axis reflects :delta after click; got ${page.url().split("#")[1] || "<none>"}`,
      );
      // Reset to a clean View before the Language assertion.
      await page
        .locator('.lang-panel[data-active] .tabs .tab[data-tab="current"]')
        .dispatchEvent("click");
    } else {
      const title = (await firstDelta.getAttribute("title")) || "";
      check(
        title.includes("no baselines provided"),
        `disabled Delta tab carries the no-baseline tooltip; got "${title}"`,
      );
      await firstDelta.dispatchEvent("click");
      check(
        (await page
          .locator('.lang-panel[data-active] .tab-panel[data-tab="current"][data-active]')
          .count()) === 1,
        "clicking a disabled Delta tab is a no-op (stays on Current)",
      );
    }
  }

  // 4) Language axis: the first non-Combined Language button. Clicking it
  //    activates its panel, deactivates Combined, and updates the hash
  //    language axis.
  const otherLangKey = await page.evaluate(() => {
    const btns = Array.from(
      document.querySelectorAll("[data-multi-lang] .lang-nav [data-lang]"),
    ).map((b) => b.dataset.lang);
    return btns.find((k) => k && k !== "combined") || null;
  });
  check(otherLangKey !== null, "report exposes at least one non-Combined Language button");
  if (otherLangKey) {
    const langBtn = page.locator(
      `[data-multi-lang] .lang-nav [data-lang="${otherLangKey}"]`,
    );
    check(await langBtn.isVisible(), `${otherLangKey} Language button is actually visible`);
    await langBtn.dispatchEvent("click");
    check(
      (await page.locator(`.lang-panel[data-lang="${otherLangKey}"][data-active]`).count()) === 1,
      `clicking the ${otherLangKey} Language button activates its panel`,
    );
    check(
      (await page.locator('.lang-panel[data-lang="combined"][data-active]').count()) === 0,
      "Combined panel deactivates after switching Language",
    );
    check(
      page.url().includes(`#${otherLangKey}:`),
      `hash language axis reflects #${otherLangKey}: after click; got ${page.url().split("#")[1] || "<none>"}`,
    );
  }
} finally {
  await browser.close();
}

if (failures.length > 0) {
  console.error(`\nunified HTML interactive validation FAILED (${failures.length} issue(s))`);
  process.exit(1);
}
console.log("\nunified HTML interactive validation passed");
