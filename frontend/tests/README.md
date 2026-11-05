# Widget checks

Run the helper and Svelte rendering tests with `npm run test:widgets`.

For browser checks, start the frontend with `npm run dev -- --host 127.0.0.1 --port 4173`, then run `npm run test:widgets:browser`. Pass another server URL directly to `node tests/widget-browser.mjs` if needed.

The browser script uses repository-local Chromium. Set `WIDGET_BROWSER_EXECUTABLE` to another locally installed Chromium executable if the bundled revision or platform differs. Screenshots and browser temporary files go into `node_modules/.cache/widget-browser-tests`.

API responses are intercepted fixtures: no backend session, database or provider credentials are required. Checks cover every Fundamentals and Quant variant, keyboard inspection, company sorting, earnings without a company snapshot, light/dark palettes, mobile layout, confidence and volume configuration saves, retained results after a failed refresh, and recovery from a removed dataset. The script also verifies that the dashboard makes only GET requests to Fundamentals.

## Mobile sweep and desktop baseline

Both scripts share `tests/mobile-helpers.mjs`: route discovery (every `src/routes/**/+page.svelte`, `[id]` replaced by `fixture-id`; the count is asserted so a new route is noticed), the intercepted shell API fixture, the browser launcher and the reachability audit. Point `WIDGET_BROWSER_EXECUTABLE` at a Chromium headless shell if the repository-local one is missing. Add `--browser=webkit` to run either script in WebKit (install it once with `npx playwright install webkit`); WebKit results go into a `webkit/` subfolder. WebKit refuses port 4190 (restricted-port list), so the baseline script relays both servers through local ports when `--browser=webkit` is set; the sweep needs no relay at 4173.

`node tests/mobile-sweep-browser.mjs [base URL]` (default `http://127.0.0.1:4173`) loads every route at 320x568, 390x844, 600x844, 667x375 (landscape), 768x1024 and 1024x768 (tablet). It fails on page-level horizontal overflow at any of them, on overflow at 390 px with the root font size at 200%, on a route that does not render and on the per-route interaction checks. It also lists, without failing:

- controls clipped or pushed off screen at 200% text;
- primary action reachability: every visible named button, link, input, select, textarea or tab inside the module content is scrolled into view like `scrollIntoViewIfNeeded`, then its center must be in the viewport and `elementFromPoint` must hit it (not a sticky bar, the floating feature shortcut or the assistant button). A control only an `overflow: hidden` box could scroll to is reported as clipped. Controls entirely left or right of the viewport (closed drawers) are listed apart as off-canvas.

Results: `node_modules/.cache/mobile-sweep/report.json`, `reachability.json` and screenshots.

`node tests/mobile-desktop-baseline-browser.mjs [working tree URL] [--baseline=URL]` (defaults `http://127.0.0.1:4173` and `http://127.0.0.1:4190`) renders every route at 1440x900 and 1100x800, light and dark, on both servers and compares the full-page PNGs byte for byte. It exits non-zero on any difference or failed capture. The report and each differing pair (`<route>-<viewport>-<theme>-baseline.png` / `-work.png`, with pixel count, max delta and bounding box in the report) go into `node_modules/.cache/mobile-desktop-baseline/`, which is emptied at each run. `--only=/journal,/histviz` limits the routes.

Determinism: fixed clock, seeded `Math.random` and counter-based `crypto` ids, `reducedMotion: 'reduce'`, animations disabled and caret hidden in screenshots, every request outside the server origin aborted, a fresh page per capture with the theme persisted before first paint. One source of noise remains in Chromium: in the dark theme a page load rasterizes a few anti-aliased icon and text edges in one of a small set of variants (DOM, computed styles and boxes identical; GPU and compositor flags do not remove it). A differing pair is therefore captured again up to `--resamples=6` times per side and passes only when both sides produce the exact same PNG at least once. Check the harness itself by comparing the baseline with itself: `node tests/mobile-desktop-baseline-browser.mjs http://127.0.0.1:4190`.
