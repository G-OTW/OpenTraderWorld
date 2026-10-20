# Widget checks

Run the helper and Svelte rendering tests with `npm run test:widgets`.

For browser checks, start the frontend with `npm run dev -- --host 127.0.0.1 --port 4173`, then run `npm run test:widgets:browser`. Pass another server URL directly to `node tests/widget-browser.mjs` if needed.

The browser script uses repository-local Chromium. Set `WIDGET_BROWSER_EXECUTABLE` to another locally installed Chromium executable if the bundled revision or platform differs. Screenshots and browser temporary files go into `node_modules/.cache/widget-browser-tests`.

API responses are intercepted fixtures: no backend session, database or provider credentials are required. Checks cover every Fundamentals and Quant variant, keyboard inspection, company sorting, earnings without a company snapshot, light/dark palettes, mobile layout, confidence and volume configuration saves, retained results after a failed refresh, and recovery from a removed dataset. The script also verifies that the dashboard makes only GET requests to Fundamentals.
