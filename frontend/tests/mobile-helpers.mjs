// Shared pieces for the fixture-backed route sweeps: route discovery, the shell API fixture,
// browser launch (chromium or webkit) and the in-page reachability audit.
// Importing it is optional: older scripts keep their own copies on purpose.
import { strict as assert } from 'node:assert';
import { readdir } from 'node:fs/promises';
import { createServer, connect } from 'node:net';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium, webkit } from 'playwright';

const routesRoot = fileURLToPath(new URL('../src/routes/', import.meta.url));

/** Positional base URL plus `--browser=chromium|webkit`, in any order. */
export function parseArgs(argv = process.argv.slice(2), defaultBase = 'http://127.0.0.1:4173') {
  const flags = Object.fromEntries(argv.filter((a) => a.startsWith('--')).map((a) => {
    const [k, ...v] = a.slice(2).split('=');
    return [k, v.length ? v.join('=') : true];
  }));
  const positional = argv.filter((a) => !a.startsWith('--'));
  const browserName = flags.browser ?? 'chromium';
  if (!['chromium', 'webkit'].includes(browserName)) throw new Error(`Unknown --browser=${browserName} (use chromium or webkit)`);
  return { base: positional[0] ?? defaultBase, positional, flags, browserName };
}

async function routeFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const nested = await Promise.all(entries.filter((e) => e.isDirectory()).map((e) => routeFiles(join(dir, e.name))));
  return [...entries.filter((e) => e.isFile() && e.name === '+page.svelte').map((e) => join(dir, e.name)), ...nested.flat()];
}

/** Every SvelteKit page route, `[id]` replaced by the fixture id, sorted. */
export async function routePaths() {
  const paths = (await routeFiles(routesRoot)).map((file) => {
    const path = relative(routesRoot, file).replace(/\/\+page\.svelte$/, '').replace(/^\+page\.svelte$/, '').replaceAll('[id]', 'fixture-id');
    return `/${path}`.replace(/\/$/, '') || '/';
  }).sort();
  // 60 routes in the 2026-10-05 inventory plus /auth/social (added 2026-10-08).
  assert.equal(paths.length, 61, 'Keep the sweep aligned with the route inventory');
  return paths;
}

/** File-safe name for a route, as the sweep has always used it. */
export const routeSlug = (path) => path.replaceAll('/', '_') || '_root';

/** Chromium uses the repository-local headless shell (or WIDGET_BROWSER_EXECUTABLE); webkit uses the Playwright install. */
export async function launchBrowser(browserName, tmpDir) {
  if (browserName === 'webkit') return webkit.launch({ headless: true, env: { ...process.env, TMPDIR: tmpDir } });
  const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
  return chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: tmpDir } });
}

/**
 * Local TCP relay to `url` on a free 127.0.0.1 port. WebKit refuses some ports outright
 * (4190 is on its restricted-port list, Playwright only lifts that for Chromium), so WebKit
 * runs reach both servers through one of these. Returns the relayed base URL and a close().
 */
export async function tcpRelay(url) {
  const target = new URL(url);
  const sockets = new Set();
  const server = createServer((client) => {
    const upstream = connect(Number(target.port || 80), target.hostname);
    for (const s of [client, upstream]) { sockets.add(s); s.on('close', () => sockets.delete(s)); s.on('error', () => { client.destroy(); upstream.destroy(); }); }
    client.pipe(upstream).pipe(client);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}${target.pathname.replace(/\/$/, '')}`,
    close: () => new Promise((resolve) => { for (const s of sockets) s.destroy(); server.close(resolve); })
  };
}

/** Responses for every API call the 60 routes make on first load. No backend, no credentials. */
export function shellFixture(p) {
  if (p === '/api/setup/status') return { configured: true };
  if (p === '/api/settings/me') return { username: 'fixture', role: 'admin', locale: 'en' };
  if (p === '/api/settings/modules') return { installed: ['agent', 'automator', 'backtest', 'calendar', 'community-docs', 'connectors', 'economics', 'editor', 'findb', 'fundamentals', 'goals', 'histdata', 'histviz', 'journal', 'mailbox', 'mindset', 'mportfolios', 'news', 'portfolios', 'prompt-store', 'quant', 'remindme', 'resources', 'routines', 'settings', 'subscriptions', 'taxcalc', 'time', 'todos', 'watchlists', 'wealth', 'webhooks'] };
  if (p === '/api/settings/defaults') return { default_timezone: 'UTC', display_currency: 'USD' };
  if (p === '/api/settings/versioning') return { modules: {} };
  if (p === '/api/mcp/settings') return { enabled: false };
  if (p === '/api/health') return { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
  if (p === '/api/dashboard/layout') return { activePageId: 'fixture', pages: [{ id: 'fixture', name: 'Fixture', layout: { cols: 12, items: [] } }], favoriteIds: ['fixture'] };
  if (p === '/api/tasks') return { tasks: [] };
  if (p === '/api/agent/providers') return { providers: [] };
  if (p === '/api/agent/agent') return { agent: {} };
  if (p === '/api/agent/agents') return { agents: [] };
  if (p === '/api/agent/conversations') return { conversations: [{ id: 'fixture-chat', title: 'Mobile fixture', agent_id: null }] };
  if (p === '/api/agent/conversations/fixture-chat') return { messages: [], usage: {} };
  if (p === '/api/agent/mcp-servers') return { servers: [] };
  if (p === '/api/mcp/tokens') return { tokens: [] };
  if (p === '/api/jobs') return { jobs: [] };
  if (p === '/api/calendar/events') return { events: [] };
  if (p === '/api/community-docs' || p === '/api/community-docs/favorites') return { docs: [{ slug: 'fixture', title: 'A practical guide to portfolio review on a small screen', summary: 'A compact guide with a long title and readable details.', categories: ['Guides'], synced_at: '2026-10-05', favorited: true }] };
  if (p === '/api/community-docs/fixture') return { doc: { slug: 'fixture', title: 'A practical guide to portfolio review on a small screen', summary: 'A compact guide with a long title and readable details.', categories: ['Guides'], synced_at: '2026-10-05', favorited: true, body: '<p>Review positions, currency, and the latest prices before acting.</p><pre>portfolio-review --include-current-currency --include-positions</pre>' } };
  if (p === '/api/documents') return { documents: [] };
  if (p === '/api/feed-dashboards') return { dashboards: [] };
  if (p === '/api/feed-items') return { items: [] };
  if (p === '/api/feed-sources') return { source_names: [], source_types: [] };
  if (p === '/api/feeds/quotas') return { quotas: {} };
  if (p === '/api/goals') return { goals: [] };
  if (p === '/api/histdata/jobs') return { jobs: [] };
  if (p === '/api/journal/categories') return { categories: [] };
  if (p === '/api/journal/strategies') return { strategies: [] };
  if (p === '/api/journal/templates') return { templates: [] };
  if (p === '/api/journal/tags') return { tags: [] };
  if (p === '/api/journal/fee-schedules') return { fee_schedules: [] };
  if (p === '/api/journal/trade-suggestions') return { tickers: [], exchanges: [], signals: [] };
  if (p === '/api/journal/settings') return { settings: { display_currency: 'USD' } };
  if (p === '/api/journal/breakdown') return { breakdown: {} };
  if (p === '/api/mindset/history') return { prompts: [], templates: [], categories: [], entries: [], marks: [] };
  if (p === '/api/mindset/templates') return { templates: [], categories: [] };
  if (p === '/api/mportfolios') return { portfolios: [], updated_at: null };
  if (p === '/api/mportfolios/snapshots') return { snapshots: [] };
  if (p === '/api/portfolios') return { portfolios: [] };
  if (p === '/api/prompts') return { prompts: [] };
  if (p === '/api/prompts/tags') return { tags: [] };
  if (p === '/api/reminders') return { reminders: [] };
  if (p === '/api/resources') return { resources: [] };
  if (p === '/api/resources/categories') return { categories: [] };
  if (p === '/api/subscriptions') return { subscriptions: [] };
  if (p === '/api/subscriptions/breakdown') return { breakdown: {} };
  if (p === '/api/subscriptions/settings') return { settings: { display_currency: 'USD' } };
  if (p === '/api/subscriptions/suggestions') return { platforms: [], categories: [] };
  if (p === '/api/time/projects') return { projects: [] };
  if (p === '/api/time/state') return { state: { display_currency: 'USD' } };
  if (p === '/api/todos') return { todos: [] };
  if (p === '/api/trader/board') return { routines: [], categories: [], marks: [] };
  if (p === '/api/trader/routines') return { routines: [], categories: [] };
  if (p === '/api/wealth/assets') return { assets: [] };
  if (p === '/api/wealth/breakdown') return { breakdown: { net_worth: 0 } };
  if (p === '/api/wealth/settings') return { settings: { display_currency: 'USD' } };
  if (p === '/api/wealth/templates') return { templates: [] };
  if (p === '/api/fundamentals/companies') return { companies: [] };
  if (p === '/api/fundamentals/companies/AAPL') return { company: { ticker: 'AAPL', name: 'Apple Inc.', status: 'ok', metrics: { market_cap: 3200000000000, pe: 31, dividend_yield: 0.005 }, sector: 'Technology', industry: 'Consumer Electronics', exchange: 'NASDAQ', country: 'US', currency: 'USD', description: 'Consumer technology company.', followed: false } };
  if (p === '/api/fundamentals/data/price') return { snapshot: { stale: false, data: { series: [] } } };
  if (p === '/api/fundamentals/data/etf') return { snapshot: { stale: false, data: { ticker: 'SPY', name: 'SPDR S&P 500 ETF Trust', issuer: 'State Street', aum: 600000000000, expense: 0.0009, holdings: 500, index: 'S&P 500', inception: '1993-01-22', top_holdings: [{ name: 'Apple Inc.', weight: 7 }], flows: [], sectors: [{ name: 'Technology', weight: 30 }], countries: [{ name: 'United States', weight: 100 }] } } };
  if (p === '/api/fundamentals/data/congress') return { snapshot: { stale: false, data: { trades: [{ member: 'Fixture Member', chamber: 'house', ticker: 'AAPL', type: 'purchase', amount: '$1,000–15,000', traded: '2026-09-01', disclosed: '2026-09-15' }] } } };
  if (p === '/api/fundamentals/documents') return { documents: [] };
  if (p === '/api/fundamentals/datasets') return { datasets: [] };
  if (p === '/api/fundamentals/sources') return { sources: [] };
  if (p === '/api/backtest/paper/fixture-id/dashboard') return { session: { id: 'fixture-id', status: 'stopped', settings: { kind: 'signals' } }, instruments: [], positions: [], orders: [], fills: [], trades: [], equity: [] };
  if (p.includes('/notifications')) return { notifications: [], unread: 0 };
  if (p === '/api/histdata/datasets') return { datasets: [] };
  if (p === '/api/connectors/providers') return { providers: [] };
  if (p === '/api/connectors') return { connectors: [] };
  if (p === '/api/histviz/workspaces') return { workspaces: [] };
  if (p === '/api/histviz/lists') return { lists: [] };
  if (p === '/api/histviz/alerts') return { alerts: [] };
  if (p === '/api/watchlists') return { watchlists: [] };
  if (p === '/api/backtest/runs') return { runs: [] };
  if (p === '/api/backtest/strategies') return { strategies: [] };
  if (p === '/api/backtest/optimize') return { jobs: [] };
  if (p === '/api/taxcalc/templates') return { templates: [] };
  if (p === '/api/taxcalc/profiles') return { profiles: [] };
  if (p === '/api/taxcalc/scenarios') return { scenarios: [] };
  if (p === '/api/findb/status') return { installed: true, importing: false, count: 2, version: 'fixture' };
  if (p === '/api/findb/update-check') return { installed: true, latest: 'fixture', update_available: false };
  if (p === '/api/findb/search') return { results: [], has_more: false };
  if (p === '/api/findb/facets') return { values: [] };
  if (p === '/api/findb/folders') return { folders: [{ id: 'folder-1', name: 'Long-term holdings' }] };
  if (p === '/api/findb/favorites') return { favorites: [{ id: 'favorite-1', instrument_id: 'instrument-1', symbol: 'AAPL', asset_type: 'equity', name: 'Apple Inc.', folder_id: 'folder-1', note: 'Core position' }] };
  if (p === '/api/automator/workflows') return { workflows: [], schedules: [], last_runs: [] };
  if (p === '/api/automator/workflows/fixture-id') return { workflow: { id: 'fixture-id', name: 'Mobile workflow', graph: { nodes: [], edges: [] } }, schedules: [], tokens: [] };
  if (p === '/api/automator/catalog') return { endpoints: [], modules: [] };
  if (p === '/api/fundamentals/series') return { series: [] };
  if (p === '/api/fundamentals/lookup/yield-curve') return { curve: null };
  if (p === '/api/fundamentals/data/central_banks') return { snapshot: { stale: false, data: [] } };
  return {};
}

/** Intercepts `/api/**` with the shell fixture; the feed SSE stream answers an empty event stream. */
export async function installApiFixture(context) {
  await context.route('**/api/**', async (route) => {
    const p = new URL(route.request().url()).pathname;
    if (p === '/api/feeds/stream') return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
    await route.fulfill({ json: shellFixture(p) });
  });
}

/**
 * In-page audit, run with page.evaluate(reachabilityAudit, { scope, limit }).
 * For every visible named button, link or input inside `scope`: scroll it into view the way
 * scrollIntoViewIfNeeded(true) does (center it only when it is not already fully visible),
 * then require its center inside the viewport and elementFromPoint at that center to be the
 * element, a descendant, or its own label. Only user-scrollable boxes may move: a scroll that
 * scrollIntoView applies to an overflow:hidden box is undone and reported as a clip. Controls
 * that stay entirely outside the viewport (closed drawers) go to `offCanvas`, not `offenders`.
 * Scroll positions are restored afterwards so later measurements and screenshots see the page
 * as it was.
 */
export function reachabilityAudit({ scope = '.module-context', limit = 400 } = {}) {
  const root = document.querySelector(scope) ?? document.body;
  const describe = (el) => {
    if (!el) return 'nothing';
    const cls = typeof el.className === 'string' ? el.className.trim().split(/\s+/).filter(Boolean).slice(0, 2).join('.') : '';
    return `${el.tagName.toLowerCase()}${cls ? `.${cls}` : ''}`;
  };
  const nameOf = (el) => {
    const label = el.getAttribute('aria-label');
    if (label?.trim()) return label.trim();
    const by = el.getAttribute('aria-labelledby');
    if (by) {
      const text = by.split(/\s+/).map((id) => document.getElementById(id)?.textContent ?? '').join(' ').trim();
      if (text) return text;
    }
    if (el.labels?.length) {
      const text = [...el.labels].map((l) => l.textContent).join(' ').trim();
      if (text) return text;
    }
    const text = (el.innerText ?? el.textContent ?? '').trim() || el.getAttribute('title')?.trim() || el.getAttribute('placeholder')?.trim() || el.querySelector?.('img[alt]')?.getAttribute('alt')?.trim();
    return text || '';
  };
  const visible = (el) => {
    if (el.closest('[inert], [aria-hidden="true"], [hidden]')) return false;
    const rect = el.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return false;
    const style = getComputedStyle(el);
    return style.visibility !== 'hidden' && style.display !== 'none' && Number(style.opacity) > 0;
  };
  const candidates = [...root.querySelectorAll('button, a[href], input:not([type="hidden"]), select, textarea, [role="button"], [role="tab"], [role="link"]')]
    .filter(visible)
    .map((el) => ({ el, name: nameOf(el) }))
    .filter((c) => c.name)
    .slice(0, limit);
  const saved = [document.scrollingElement, ...document.querySelectorAll('*')]
    .filter((el) => el && (el.scrollTop || el.scrollLeft))
    .map((el) => [el, el.scrollTop, el.scrollLeft]);
  const offenders = [];
  const offCanvas = [];
  const userScrollable = (a, axis) => {
    if (a === document.scrollingElement) return true;
    const o = getComputedStyle(a)[axis === 'x' ? 'overflowX' : 'overflowY'];
    return o === 'auto' || o === 'scroll';
  };
  for (const { el, name } of candidates) {
    let rect = el.getBoundingClientRect();
    // Fully visible means inside the viewport and inside every clipping ancestor, like the native check.
    let fully = rect.top >= 0 && rect.left >= 0 && rect.bottom <= innerHeight && rect.right <= innerWidth;
    for (let a = el.parentElement; fully && a && a !== document.documentElement; a = a.parentElement) {
      const st = getComputedStyle(a);
      if (st.overflowX === 'visible' && st.overflowY === 'visible') continue;
      const box = a.getBoundingClientRect();
      fully = rect.top >= box.top - 1 && rect.left >= box.left - 1 && rect.bottom <= box.bottom + 1 && rect.right <= box.right + 1;
    }
    let clippedBy = null;
    if (!fully) {
      const chain = [];
      for (let a = el.parentElement; a; a = a.parentElement) chain.push([a, a.scrollTop, a.scrollLeft]);
      el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
      // scrollIntoView also scrolls overflow:hidden boxes, which a finger cannot. Undo those.
      for (const [a, top, left] of chain) {
        if (a.scrollTop !== top && !userScrollable(a, 'y')) { a.scrollTop = top; clippedBy ??= a; }
        if (a.scrollLeft !== left && !userScrollable(a, 'x')) { a.scrollLeft = left; clippedBy ??= a; }
      }
    }
    rect = el.getBoundingClientRect();
    const x = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;
    const inside = x >= 0 && y >= 0 && x <= innerWidth && y <= innerHeight;
    const hit = inside ? document.elementFromPoint(x, y) : null;
    const ok = inside && hit && (hit === el || el.contains(hit) || [...(el.labels ?? [])].some((l) => l.contains(hit)));
    if (ok) continue;
    const record = { name: name.replace(/\s+/g, ' ').slice(0, 60), element: describe(el) };
    if (rect.right <= 0 || rect.left >= innerWidth) {
      // Entirely left or right of the viewport after scrolling: an off-canvas drawer or panel. Listed apart.
      // Content stuck above or below the viewport stays an offender: no scroll reaches it.
      offCanvas.push({ ...record, reason: `off-canvas at ${Math.round(x)},${Math.round(y)}` });
      continue;
    }
    record.reason = clippedBy
      ? `clipped by ${describe(clippedBy)} (overflow hidden), ${inside ? `hit ${describe(hit)}` : `center ${Math.round(x)},${Math.round(y)} outside ${innerWidth}x${innerHeight}`}`
      : inside ? `covered by ${describe(hit)}${hit?.closest(scope) ? '' : ' (outside main content)'}` : `center ${Math.round(x)},${Math.round(y)} outside ${innerWidth}x${innerHeight}`;
    offenders.push(record);
  }
  for (const [el, top, left] of saved) { el.scrollTop = top; el.scrollLeft = left; }
  for (const el of document.querySelectorAll('*')) {
    if (!saved.some(([s]) => s === el) && (el.scrollTop || el.scrollLeft)) { el.scrollTop = 0; el.scrollLeft = 0; }
  }
  if (!saved.some(([s]) => s === document.scrollingElement)) document.scrollingElement.scrollTop = 0;
  return { checked: candidates.length, offenders, offCanvas };
}

/**
 * Init script that removes run-to-run noise without touching the app: a seeded Math.random,
 * counter-based crypto.randomUUID / getRandomValues, and no smooth scrolling. The clock is
 * fixed separately with page.clock.setFixedTime.
 */
export function determinismInitScript() {
  let seed = 0x2f6b1d3;
  Math.random = () => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed / 4294967296;
  };
  if (globalThis.crypto) {
    let n = 0;
    const uuid = () => `00000000-0000-4000-8000-${(++n).toString(16).padStart(12, '0')}`;
    try { Object.defineProperty(crypto, 'randomUUID', { value: uuid, configurable: true }); } catch { /* read-only in this engine */ }
    try {
      Object.defineProperty(crypto, 'getRandomValues', {
        configurable: true,
        value: (arr) => { for (let i = 0; i < arr.length; i++) arr[i] = Math.floor(Math.random() * 256); return arr; }
      });
    } catch { /* read-only in this engine */ }
  }
}
