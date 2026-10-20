// Fixture-backed viewport sweep for every SvelteKit page route.
// Run with the frontend dev server: node tests/mobile-sweep-browser.mjs [base URL].
import { strict as assert } from 'node:assert';
import { mkdir, readdir, writeFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../src/routes/', import.meta.url));
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-sweep/', import.meta.url));
const base = process.argv[2] ?? 'http://127.0.0.1:4173';
await mkdir(artifacts, { recursive: true });

async function routeFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const nested = await Promise.all(entries.filter((e) => e.isDirectory()).map((e) => routeFiles(join(dir, e.name))));
  return [...entries.filter((e) => e.isFile() && e.name === '+page.svelte').map((e) => join(dir, e.name)), ...nested.flat()];
}
const paths = (await routeFiles(root)).map((file) => {
  const path = relative(root, file).replace(/\/\+page\.svelte$/, '').replace(/^\+page\.svelte$/, '').replaceAll('[id]', 'fixture-id');
  return `/${path}`.replace(/\/$/, '') || '/';
}).sort();
assert.equal(paths.length, 60, 'Keep the sweep aligned with the 60-route inventory');

const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });
const context = await browser.newContext({ viewport: { width: 390, height: 844 }, locale: 'en-US' });
await context.route('**/api/**', async (route) => {
  const p = new URL(route.request().url()).pathname;
  let result = {};
  if (p === '/api/setup/status') result = { configured: true };
  else if (p === '/api/settings/me') result = { username: 'fixture', role: 'admin', locale: 'en' };
  else if (p === '/api/settings/modules') result = { installed: ['agent', 'automator', 'backtest', 'calendar', 'community-docs', 'connectors', 'economics', 'editor', 'findb', 'fundamentals', 'goals', 'histdata', 'histviz', 'journal', 'mailbox', 'mindset', 'mportfolios', 'news', 'portfolios', 'prompt-store', 'quant', 'remindme', 'resources', 'routines', 'settings', 'subscriptions', 'taxcalc', 'time', 'todos', 'watchlists', 'wealth', 'webhooks'] };
  else if (p === '/api/settings/defaults') result = { default_timezone: 'UTC', display_currency: 'USD' };
  else if (p === '/api/settings/versioning') result = { modules: {} };
  else if (p === '/api/mcp/settings') result = { enabled: false };
  else if (p === '/api/health') result = { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
  else if (p === '/api/dashboard/layout') result = { activePageId: 'fixture', pages: [{ id: 'fixture', name: 'Fixture', layout: { cols: 12, items: [] } }], favoriteIds: ['fixture'] };
  else if (p === '/api/tasks') result = { tasks: [] };
  else if (p === '/api/agent/providers') result = { providers: [] };
  else if (p === '/api/agent/agent') result = { agent: {} };
  else if (p === '/api/agent/agents') result = { agents: [] };
  else if (p === '/api/agent/conversations') result = { conversations: [{ id: 'fixture-chat', title: 'Mobile fixture', agent_id: null }] };
  else if (p === '/api/agent/conversations/fixture-chat') result = { messages: [], usage: {} };
  else if (p === '/api/agent/mcp-servers') result = { servers: [] };
  else if (p === '/api/mcp/tokens') result = { tokens: [] };
  else if (p === '/api/jobs') result = { jobs: [] };
  else if (p === '/api/calendar/events') result = { events: [] };
  else if (p === '/api/community-docs' || p === '/api/community-docs/favorites') result = { docs: [{ slug: 'fixture', title: 'A practical guide to portfolio review on a small screen', summary: 'A compact guide with a long title and readable details.', categories: ['Guides'], synced_at: '2026-10-05', favorited: true }] };
  else if (p === '/api/community-docs/fixture') result = { doc: { slug: 'fixture', title: 'A practical guide to portfolio review on a small screen', summary: 'A compact guide with a long title and readable details.', categories: ['Guides'], synced_at: '2026-10-05', favorited: true, body: '<p>Review positions, currency, and the latest prices before acting.</p><pre>portfolio-review --include-current-currency --include-positions</pre>' } };
  else if (p === '/api/documents') result = { documents: [] };
  else if (p === '/api/feed-dashboards') result = { dashboards: [] };
  else if (p === '/api/feed-items') result = { items: [] };
  else if (p === '/api/feed-sources') result = { source_names: [], source_types: [] };
  else if (p === '/api/feeds/quotas') result = { quotas: {} };
  else if (p === '/api/goals') result = { goals: [] };
  else if (p === '/api/histdata/jobs') result = { jobs: [] };
  else if (p === '/api/journal/categories') result = { categories: [] };
  else if (p === '/api/journal/strategies') result = { strategies: [] };
  else if (p === '/api/journal/templates') result = { templates: [] };
  else if (p === '/api/journal/tags') result = { tags: [] };
  else if (p === '/api/journal/fee-schedules') result = { fee_schedules: [] };
  else if (p === '/api/journal/trade-suggestions') result = { tickers: [], exchanges: [], signals: [] };
  else if (p === '/api/journal/settings') result = { settings: { display_currency: 'USD' } };
  else if (p === '/api/journal/breakdown') result = { breakdown: {} };
  else if (p === '/api/mindset/history') result = { prompts: [], templates: [], categories: [], entries: [], marks: [] };
  else if (p === '/api/mindset/templates') result = { templates: [], categories: [] };
  else if (p === '/api/mportfolios') result = { portfolios: [], updated_at: null };
  else if (p === '/api/mportfolios/snapshots') result = { snapshots: [] };
  else if (p === '/api/portfolios') result = { portfolios: [] };
  else if (p === '/api/prompts') result = { prompts: [] };
  else if (p === '/api/prompts/tags') result = { tags: [] };
  else if (p === '/api/reminders') result = { reminders: [] };
  else if (p === '/api/resources') result = { resources: [] };
  else if (p === '/api/resources/categories') result = { categories: [] };
  else if (p === '/api/subscriptions') result = { subscriptions: [] };
  else if (p === '/api/subscriptions/breakdown') result = { breakdown: {} };
  else if (p === '/api/subscriptions/settings') result = { settings: { display_currency: 'USD' } };
  else if (p === '/api/subscriptions/suggestions') result = { platforms: [], categories: [] };
  else if (p === '/api/time/projects') result = { projects: [] };
  else if (p === '/api/time/state') result = { state: { display_currency: 'USD' } };
  else if (p === '/api/todos') result = { todos: [] };
  else if (p === '/api/trader/board') result = { routines: [], categories: [], marks: [] };
  else if (p === '/api/trader/routines') result = { routines: [], categories: [] };
  else if (p === '/api/wealth/assets') result = { assets: [] };
  else if (p === '/api/wealth/breakdown') result = { breakdown: { net_worth: 0 } };
  else if (p === '/api/wealth/settings') result = { settings: { display_currency: 'USD' } };
  else if (p === '/api/wealth/templates') result = { templates: [] };
  else if (p === '/api/fundamentals/companies') result = { companies: [] };
  else if (p === '/api/fundamentals/companies/AAPL') result = { company: { ticker: 'AAPL', name: 'Apple Inc.', status: 'ok', metrics: { market_cap: 3200000000000, pe: 31, dividend_yield: 0.005 }, sector: 'Technology', industry: 'Consumer Electronics', exchange: 'NASDAQ', country: 'US', currency: 'USD', description: 'Consumer technology company.', followed: false } };
  else if (p === '/api/fundamentals/data/price') result = { snapshot: { stale: false, data: { series: [] } } };
  else if (p === '/api/fundamentals/data/etf') result = { snapshot: { stale: false, data: { ticker: 'SPY', name: 'SPDR S&P 500 ETF Trust', issuer: 'State Street', aum: 600000000000, expense: 0.0009, holdings: 500, index: 'S&P 500', inception: '1993-01-22', top_holdings: [{ name: 'Apple Inc.', weight: 7 }], flows: [], sectors: [{ name: 'Technology', weight: 30 }], countries: [{ name: 'United States', weight: 100 }] } } };
  else if (p === '/api/fundamentals/data/congress') result = { snapshot: { stale: false, data: { trades: [{ member: 'Fixture Member', chamber: 'house', ticker: 'AAPL', type: 'purchase', amount: '$1,000–15,000', traded: '2026-09-01', disclosed: '2026-09-15' }] } } };
  else if (p === '/api/fundamentals/documents') result = { documents: [] };
  else if (p === '/api/fundamentals/datasets') result = { datasets: [] };
  else if (p === '/api/fundamentals/sources') result = { sources: [] };
  else if (p === '/api/backtest/paper/fixture-id/dashboard') result = { session: { id: 'fixture-id', status: 'stopped', settings: { kind: 'signals' } }, instruments: [], positions: [], orders: [], fills: [], trades: [], equity: [] };
  else if (p.includes('/notifications')) result = { notifications: [], unread: 0 };
  else if (p === '/api/histdata/datasets') result = { datasets: [] };
  else if (p === '/api/connectors/providers') result = { providers: [] };
  else if (p === '/api/connectors') result = { connectors: [] };
  else if (p === '/api/histviz/workspaces') result = { workspaces: [] };
  else if (p === '/api/histviz/lists') result = { lists: [] };
  else if (p === '/api/histviz/alerts') result = { alerts: [] };
  else if (p === '/api/watchlists') result = { watchlists: [] };
  else if (p === '/api/backtest/runs') result = { runs: [] };
  else if (p === '/api/backtest/strategies') result = { strategies: [] };
  else if (p === '/api/backtest/optimize') result = { jobs: [] };
  else if (p === '/api/taxcalc/templates') result = { templates: [] };
  else if (p === '/api/taxcalc/profiles') result = { profiles: [] };
  else if (p === '/api/taxcalc/scenarios') result = { scenarios: [] };
  else if (p === '/api/findb/status') result = { installed: true, importing: false, count: 2, version: 'fixture' };
  else if (p === '/api/findb/update-check') result = { installed: true, latest: 'fixture', update_available: false };
  else if (p === '/api/findb/search') result = { results: [], has_more: false };
  else if (p === '/api/findb/facets') result = { values: [] };
  else if (p === '/api/findb/folders') result = { folders: [{ id: 'folder-1', name: 'Long-term holdings' }] };
  else if (p === '/api/findb/favorites') result = { favorites: [{ id: 'favorite-1', instrument_id: 'instrument-1', symbol: 'AAPL', asset_type: 'equity', name: 'Apple Inc.', folder_id: 'folder-1', note: 'Core position' }] };
  else if (p === '/api/automator/workflows') result = { workflows: [], schedules: [], last_runs: [] };
  else if (p === '/api/automator/workflows/fixture-id') result = { workflow: { id: 'fixture-id', name: 'Mobile workflow', graph: { nodes: [], edges: [] } }, schedules: [], tokens: [] };
  else if (p === '/api/automator/catalog') result = { endpoints: [], modules: [] };
  else if (p === '/api/fundamentals/series') result = { series: [] };
  else if (p === '/api/fundamentals/lookup/yield-curve') result = { curve: null };
  else if (p === '/api/fundamentals/data/central_banks') result = { snapshot: { stale: false, data: [] } };
  else if (p === '/api/feeds/stream') return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
  await route.fulfill({ json: result });
});

const report = [];
for (const path of paths) {
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  const errors = [];
  const listener = (e) => errors.push(e.stack || e.message);
  const apiPaths = new Set();
  const requestListener = (request) => {
    const pathname = new URL(request.url()).pathname;
    if (pathname.startsWith('/api/')) apiPaths.add(pathname);
  };
  page.on('pageerror', listener);
  page.on('request', requestListener);
  const entry = { route: path, widths: {}, errors };
  try {
    const response = await page.goto(`${base}${path}`, { waitUntil: 'domcontentloaded', timeout: 15000 });
    entry.status = response?.status();
    try {
      await page.waitForFunction(() => document.body.innerText.trim().length > 20, undefined, { timeout: 6000 });
    } catch {
      entry.notRendered = true;
    }
    entry.url = page.url();
    entry.bodyLength = await page.evaluate(() => document.body.innerText.trim().length);
    for (const width of [320, 390, 600]) {
      await page.setViewportSize({ width, height: width === 320 ? 568 : 844 });
      await page.waitForTimeout(60);
      const measure = await page.evaluate(() => ({
        viewport: document.documentElement.clientWidth,
        scroll: document.documentElement.scrollWidth,
        overflowers: [...document.querySelectorAll('body *')].filter((el) => {
          const rect = el.getBoundingClientRect();
          const style = getComputedStyle(el);
          return rect.right > document.documentElement.clientWidth + 1 && style.position !== 'fixed' && style.visibility !== 'hidden' && rect.width > 0;
        }).slice(0, 6).map((el) => `${el.tagName}.${typeof el.className === 'string' ? el.className : ''}`)
      }));
      entry.widths[width] = measure;
      if (width === 320) {
        if (path === '/agent') {
          const headerAndComposer = await page.evaluate(() => {
            const title = document.querySelector('.chat-head h1');
            const textarea = document.querySelector('.composer textarea');
            return {
              titleFits: title && title.scrollWidth <= title.clientWidth + 1 && title.scrollHeight <= title.clientHeight + 1,
              composerFits: textarea && textarea.scrollHeight <= textarea.clientHeight + 1
            };
          });
          assert.equal(headerAndComposer.titleFits, true, 'Agent conversation title must remain readable at 320 px');
          assert.equal(headerAndComposer.composerFits, true, 'Agent composer placeholder must not be cut at 320 px');
        }
        if (path === '/calendar') {
          const actionsFit = await page.evaluate(() => [...document.querySelectorAll('.head button, .cal-wrap .toolbar button')].every((button) => {
            const rect = button.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && rect.width >= 44 && rect.height >= 44;
          }));
          assert.equal(actionsFit, true, 'Calendar header and toolbar actions must fit and remain tappable at 320 px');
          const monthFits = await page.locator('.fc-host').evaluate((calendar) => calendar.scrollWidth <= calendar.clientWidth + 1);
          assert.equal(monthFits, true, 'Calendar month grid must show all seven days at 320 px');
        }
        if (path === '/mindset/history') {
          const rangesFit = await page.locator('.limits').evaluate((limits) => [...limits.querySelectorAll('button')].every((button) => {
            const rect = button.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && rect.width >= 44 && rect.height >= 44;
          }));
          assert.equal(rangesFit, true, 'All Mindset history ranges must fit at 320 px');
        }
        if (path === '/backtest/paper/fixture-id') {
          const tabsFit = await page.evaluate(() => [...document.querySelectorAll('.view-tabs button, .book .tabs button')].every((button) => {
            const rect = button.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && button.scrollWidth <= button.clientWidth + 1;
          }));
          assert.equal(tabsFit, true, 'Paper Backtest tabs must remain fully visible at 320 px');
        }
        if (path === '/backtest/optimize/fixture-id') {
          const tabsFit = await page.locator('.tabs').evaluate((tabs) => [...tabs.querySelectorAll('button')].every((button) => {
            const rect = button.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth && button.scrollWidth <= button.clientWidth + 1;
          }));
          assert.equal(tabsFit, true, 'Optimizer tabs must remain fully visible at 320 px');
        }
        if (path === '/todos') {
          const summaryReadable = await page.locator('.head .sub').evaluate((summary) => summary.scrollWidth <= summary.clientWidth + 1 && summary.getBoundingClientRect().width >= 100);
          assert.equal(summaryReadable, true, 'ToDo summary must not collapse into a narrow column at 320 px');
        }
        await page.screenshot({ path: `${artifacts}/${path.replaceAll('/', '_') || '_root'}-320.png`, fullPage: true });
      }
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: `${artifacts}/${path.replaceAll('/', '_') || '_root'}.png`, fullPage: true, timeout: 10000 });
    const visibleTypeError = await page.evaluate(() => document.body.innerText.match(/Cannot read properties of (?:undefined|null)|TypeError:|\.map is not a function/i)?.[0] ?? null);
    assert.equal(visibleTypeError, null, `${path} must render without a visible fixture type error`);
    if (path === '/community-docs') {
      await page.locator('.cat-card').first().click();
      await page.locator('.doc-card').first().click();
      await page.locator('.article').waitFor();
      await page.setViewportSize({ width: 320, height: 568 });
      const readerWidth = await page.evaluate(() => ({ viewport: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
      assert.ok(readerWidth.scroll <= readerWidth.viewport + 1, 'Community Docs reader must fit the phone viewport');
      await page.locator('.article .content pre').scrollIntoViewIfNeeded();
      await page.locator('.page').evaluate((element) => { element.scrollTop = element.scrollHeight; });
      const codeReachable = await page.locator('.article .content pre').evaluate((element) => {
        const bounds = element.getBoundingClientRect();
        return bounds.top >= 0 && bounds.bottom <= innerHeight - 60 && bounds.width <= document.documentElement.clientWidth;
      });
      assert.equal(codeReachable, true, 'Community Docs reader must scroll to the final content');
      await page.screenshot({ path: `${artifacts}/community-docs-reader-320.png`, fullPage: true });
      await page.setViewportSize({ width: 390, height: 844 });
      entry.interactions = 'category to document reader';
    } else if (path === '/findb') {
      await page.getByRole('button', { name: /Favorites/ }).first().click();
      await page.locator('.mobile-folders select').selectOption('folder-1');
      await page.locator('.fav-detail').getByText('AAPL').waitFor();
      await page.screenshot({ path: `${artifacts}/findb-favorites.png`, fullPage: true });
      entry.interactions = 'folder selector and favorite record';
    } else if (path === '/histviz') {
      const chartButton = page.locator('.mobile-pane-nav button').nth(1);
      await chartButton.click();
      assert.equal(await page.locator('.grid-wrap').isVisible(), true);
      await page.screenshot({ path: `${artifacts}/histviz-chart.png`, fullPage: true });
      entry.interactions = 'instrument list to chart pane';
    } else if (path === '/backtest') {
      await page.locator('.mobile-sections button').nth(1).click();
      assert.equal(await page.locator('.dock').isVisible(), true);
      await page.screenshot({ path: `${artifacts}/backtest-library.png`, fullPage: true });
      await page.locator('.mobile-sections button').first().click();
      assert.equal(await page.locator('.work').isVisible(), true);
      await page.setViewportSize({ width: 320, height: 568 });
      await page.locator('.step-foot .run').scrollIntoViewIfNeeded();
      const runReachable = await page.locator('.step-foot .run').evaluate((button) => {
        const rect = button.getBoundingClientRect();
        const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
        return rect.top >= 0 && rect.bottom <= innerHeight && !!hit?.closest('.run');
      });
      assert.equal(runReachable, true, 'Backtest Run stays reachable above floating shortcuts at 320 px');
      await page.screenshot({ path: `${artifacts}/backtest-run-visible-320.png`, fullPage: true });
      await page.setViewportSize({ width: 390, height: 844 });
      entry.interactions = 'configuration to library and back';
    } else if (path === '/taxcalc') {
      await page.locator('.mobile-new-profile').click();
      await page.locator('[role="dialog"]').waitFor();
      await page.waitForTimeout(250);
      await page.screenshot({ path: `${artifacts}/taxcalc-new-profile.png`, fullPage: true });
      entry.interactions = 'new profile dialog';
    } else if (path === '/fundamentals') {
      const toggle = page.locator('.mobile-catalog-toggle');
      assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
      await toggle.click();
      assert.equal(await toggle.getAttribute('aria-expanded'), 'true');
      assert.equal(await page.locator('#macro-catalog').isVisible(), true);
      await page.screenshot({ path: `${artifacts}/fundamentals-catalog-open.png`, fullPage: true });
      await toggle.click();
      entry.interactions = 'series catalog open and close';
    } else if (path === '/automator/fixture-id') {
      await page.locator('.mobile-workspace button').nth(1).click();
      await page.locator('.palette-slot .block').first().click();
      await page.locator('[role="dialog"]').waitFor();
      await page.keyboard.press('Escape');
      await page.locator('.canvas-slot .task').first().waitFor();
      await page.locator('.mobile-workspace button').nth(1).click();
      await page.locator('.palette-slot .block').nth(1).click();
      await page.locator('[role="dialog"]').waitFor();
      await page.keyboard.press('Escape');
      assert.equal(await page.locator('.canvas-slot .task').count(), 2);
      await page.locator('.mobile-move button').filter({ hasText: 'Move up' }).last().click();
      assert.match(await page.locator('.canvas-slot .task').first().innerText(), /agent1|AI step/i);
      await page.screenshot({ path: `${artifacts}/automator-mobile-canvas.png`, fullPage: true });
      entry.interactions = 'block palette to canvas by tap, then move up';
    }
    if (['/', '/automator/fixture-id', '/backtest', '/findb', '/fundamentals', '/histviz', '/taxcalc'].includes(path)) {
      const name = path.replaceAll('/', '_') || '_root';
      await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
      await page.screenshot({ path: `${artifacts}/${name}-dark-390.png`, fullPage: true });
      await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
      await page.setViewportSize({ width: 1440, height: 900 });
      await page.screenshot({ path: `${artifacts}/${name}-desktop-1440.png`, fullPage: true });
    }
  } catch (error) {
    entry.failure = error.message;
  }
  entry.finalUrl = page.url();
  page.off('pageerror', listener);
  page.off('request', requestListener);
  entry.apiPaths = [...apiPaths].sort();
  report.push(entry);
  await page.close();
  console.log(`${path}: ${Object.entries(entry.widths).filter(([, m]) => m.scroll > m.viewport + 1).map(([w, m]) => `${w}:${m.scroll}`).join(',') || 'fits'}${entry.failure ? ` FAIL ${entry.failure.slice(0, 80)}` : ''}`);
}
await writeFile(`${artifacts}/report.json`, JSON.stringify(report, null, 2));
await browser.close();
const viewportFailures = report.filter((r) => r.failure || r.notRendered || Object.values(r.widths).some((m) => m.scroll > m.viewport + 1));
const fixtureErrors = report.filter((r) => r.errors.some((error) => !error.includes('tradingview-widget.com')));
const externalErrors = report.filter((r) => r.errors.some((error) => error.includes('tradingview-widget.com')));
console.log(`${paths.length} routes checked; ${viewportFailures.length} viewport/interaction failures; ${fixtureErrors.length} routes have fixture/runtime errors; ${externalErrors.length} routes have third-party widget errors. Screenshots and report: ${artifacts}`);
if (viewportFailures.length) process.exitCode = 1;
