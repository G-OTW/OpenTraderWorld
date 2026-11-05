// Mobile check for the chart routes: /histviz, /economics and /quant. Run with the frontend
// dev server: node tests/mobile-charts-browser.mjs <base url> [baseline url]
// The API is intercepted with fixtures (no backend, no credentials). The optional baseline
// url is a clean build of HEAD: desktop screenshots of both servers must be identical.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const baseline = process.argv[3] ?? 'http://127.0.0.1:4190';
const only = process.env.CHARTS_ONLY ? process.env.CHARTS_ONLY.split(',') : null;
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-charts/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });

const PHONES = [
  { width: 320, height: 568 },
  { width: 390, height: 844 },
  { width: 600, height: 800 },
  { width: 667, height: 375 },
  { width: 768, height: 1024 }
];
const DESKTOPS = [
  { width: 1440, height: 900 },
  { width: 1100, height: 800 }
];
const CLOCK = new Date('2026-10-15T12:00:00Z');
const DAY = 86400000;
const iso = (d) => new Date(CLOCK.getTime() + d * DAY).toISOString();

// ── Fixtures ─────────────────────────────────────────────────────────────────

function barsFor(ticker, n = 160) {
  const shape = {
    AAPL: { start: 182.4, amp: 6, drift: 0.12 },
    'BTC-USD': { start: 112345.67, amp: 4200, drift: 85 },
    'CLZ6-CLF7': { start: -0.42, amp: 0.35, drift: -0.002 },
    'BRK.B': { start: 412.15, amp: 9, drift: 0.2 }
  }[ticker] ?? { start: 100, amp: 5, drift: 0.1 };
  const ts = Array.from({ length: n }, (_, i) => iso(-n + i));
  const c = ts.map((_, i) => +(shape.start + Math.sin(i / 9) * shape.amp + i * shape.drift).toFixed(4));
  const span = Math.max(Math.abs(shape.amp) / 6, 0.01);
  return {
    count: n, ts,
    o: c.map((v, i) => +(v + (i % 2 ? span / 2 : -span / 2)).toFixed(4)),
    h: c.map((v) => +(v + span).toFixed(4)),
    l: c.map((v) => +(v - span * 1.2).toFixed(4)),
    c,
    v: c.map((_, i) => 1250000 + i * 7350),
    history_start: true, stream: false
  };
}

const TICKERS = ['AAPL', 'BTC-USD', 'CLZ6-CLF7', 'BRK.B'];
const coordsOf = (ticker) => ({ provider: 'fixture', asset_type: ticker === 'BTC-USD' ? 'crypto' : 'equity', ticker, timeframe: '1d' });
const workspaceRow = () => ({
  id: 'ws-multi',
  name: 'Macro desk: energy spreads, crypto and US large caps',
  grid_rows: 2,
  grid_cols: 2,
  panes: TICKERS.map((ticker, i) => ({
    id: `chart-${i + 1}`,
    coords: coordsOf(ticker),
    type: 'candlestick',
    brick: 0,
    instances: i === 0 ? [{ id: 1, type: 'sma', params: { period: 20 }, visible: true }, { id: 2, type: 'rsi', params: { period: 14 }, visible: true }] : [],
    link: '',
    connector_id: null,
    settings: null,
    paneHeights: {}
  })),
  settings: {
    v: 2,
    row_sizes: [0.5, 0.5],
    col_sizes: [0.5, 0.5],
    active: 'chart-1',
    drawings: {
      'fixture|equity|AAPL': [{ id: 'dfix1', tool: 'hline', a: { x: Date.parse(iso(-40)), y: 186.5 }, b: null, style: { color: '', width: 0 } }]
    }
  }
});

const datasets = [
  { id: 'ds-aapl', provider: 'fixture', asset_type: 'equity', ticker: 'AAPL', timeframe: '1d', bars: 2520, range_from: iso(-3650), range_to: iso(-1) },
  { id: 'ds-btc', provider: 'fixture', asset_type: 'crypto', ticker: 'BTC-USD', timeframe: '1d', bars: 3650, range_from: iso(-3650), range_to: iso(-1) },
  { id: 'ds-spread', provider: 'fixture', asset_type: 'future', ticker: 'CLZ6-CLF7 calendar spread continuous back adjusted', timeframe: '1d', bars: 1250, range_from: iso(-1800), range_to: iso(-1) },
  { id: 'ds-brk', provider: 'fixture', asset_type: 'equity', ticker: 'BRK.B', timeframe: '1d', bars: 2520, range_from: iso(-3650), range_to: iso(-1) }
];

const singleResult = (confidence) => ({
  ticker: 'CLZ6-CLF7 calendar spread continuous back adjusted', timeframe: '1d',
  result: {
    confidence, periods_per_year: 252, periods: 1249, hv_annual: 0.4123, max_drawdown: 0.6271,
    var_hist: 0.03812, var_param: 0.04011, cvar: 0.05923,
    drawdown_curve: Array.from({ length: 60 }, (_, i) => ({ ts: iso(-600 + i * 10), dd: -Math.abs(Math.sin(i / 7)) * 0.6 })),
    histogram: { edges: Array.from({ length: 21 }, (_, i) => -0.1 + i * 0.01), counts: Array.from({ length: 20 }, (_, i) => Math.round(200 * Math.exp(-((i - 10) ** 2) / 12))) }
  },
  tail: { confidence, var_normal: 0.0401, var_cornish_fisher: 0.0466, tail_ratio: 1.183, gpd: { shape: 0.21, threshold: 0.031, exceedances: 63, levels: [{ confidence: 0.99, var: 0.071, es: 0.0943 }, { confidence: 0.995, var: 0.0862, es: 0.1121 }] } },
  episodes: [{ depth: 0.6271, peak_at: iso(-900), trough_at: iso(-610), recovered_at: null, to_trough: 290, to_recover: null, under_water: 899 }, { depth: 0.2133, peak_at: iso(-1500), trough_at: iso(-1400), recovered_at: iso(-1200), to_trough: 100, to_recover: 200, under_water: 300 }]
});

const portfolioResult = {
  labels: ['AAPL', 'BTC-USD', 'CLZ6-CLF7 calendar spread continuous back adjusted'],
  periods: 1249, measured_at: '1d',
  correlation: { labels: ['AAPL', 'BTC-USD', 'CLZ6-CLF7 calendar spread continuous back adjusted'], matrix: [[1, 0.31, -0.42], [0.31, 1, -0.08], [-0.42, -0.08, 1]] },
  frontier: {
    cloud: Array.from({ length: 60 }, (_, i) => ({ vol: 0.12 + (i % 20) * 0.02, ret: -0.05 + Math.sqrt(i) * 0.03, sharpe: -0.2 + i / 40 })),
    assets: [{ label: 'AAPL', vol: 0.28, ret: 0.18 }, { label: 'BTC-USD', vol: 0.71, ret: 0.52 }, { label: 'CLZ6-CLF7 calendar spread continuous back adjusted', vol: 0.41, ret: -0.12 }],
    max_sharpe: { vol: 0.24, ret: 0.17, sharpe: 0.71, weights: [0.52, 0.31, 0.17] },
    min_vol: { vol: 0.12, ret: 0.03, sharpe: 0.25, weights: [0.61, 0.04, 0.35] }
  },
  risk_parity: { labels: ['AAPL', 'BTC-USD', 'CLZ6-CLF7 calendar spread continuous back adjusted'], weights: [0.46, 0.18, 0.36], risk_contribution: [0.3333, 0.3333, 0.3334] }
};

const runs = [{ id: 'run-1', name: 'Mean reversion on the crude calendar spread with a long name', ticker: 'CLZ6-CLF7', timeframe: '1d', stats: { trades: 1284 } }];
const mcResult = {
  name: 'Mean reversion on the crude calendar spread with a long name', ticker: 'CLZ6-CLF7', timeframe: '1d',
  result: {
    iterations: 5000, source_trades: 1284, horizon: 1284, start_capital: 100000, ruin_level: 50000,
    risk_of_ruin: 0.0712, prob_loss: 0.3810, median_return: -0.0412,
    max_drawdown: { p50: 0.2841, p95: 0.5312 },
    final_equity: { p5: 61234.5, p50: 95880.25, p95: 1452300.75 },
    fan: Array.from({ length: 40 }, (_, i) => ({ step: i * 32, p5: 100000 - i * 900, p25: 100000 - i * 300, p50: 100000 - i * 100, p75: 100000 + i * 800, p95: 100000 + i * 2400 })),
    actual_curve: Array.from({ length: 1285 }, (_, i) => 100000 + Math.sin(i / 40) * 8000 - i * 3),
    final_histogram: { edges: Array.from({ length: 21 }, (_, i) => 40000 + i * 8000), counts: Array.from({ length: 20 }, (_, i) => Math.round(400 * Math.exp(-((i - 8) ** 2) / 18))) },
    drawdown_histogram: { edges: Array.from({ length: 21 }, (_, i) => i * 0.03), counts: Array.from({ length: 20 }, (_, i) => Math.round(300 * Math.exp(-((i - 9) ** 2) / 10))) }
  }
};

function shellResult(p) {
  if (p === '/api/setup/status') return { configured: true };
  if (p === '/api/settings/me') return { username: 'fixture', role: 'admin', locale: 'en' };
  if (p === '/api/settings/modules') return { installed: ['histviz', 'histdata', 'watchlists', 'economics', 'remindme', 'quant', 'backtest'] };
  if (p === '/api/settings/defaults') return { default_timezone: 'UTC', display_currency: 'USD' };
  if (p === '/api/settings/versioning') return { modules: {} };
  if (p === '/api/mcp/settings') return { enabled: false };
  if (p === '/api/health') return { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
  if (p === '/api/dashboard/layout') return { pages: [] };
  if (p === '/api/tasks') return { tasks: [] };
  if (p.includes('/notifications')) return { notifications: [], unread: 0 };
  if (p === '/api/connectors/providers') return { providers: [] };
  if (p === '/api/connectors') return { connectors: [] };
  if (p === '/api/watchlists') return { watchlists: [] };
  return undefined;
}

/** One page with the API fixture for `route` in `state` (populated, empty or error). */
async function openPage(url, route, state, viewport, { desktopCompare = false, store = {} } = {}) {
  const context = await browser.newContext({ viewport, locale: 'en-US', timezoneId: 'UTC', hasTouch: viewport.width < 1100 });
  const captured = [];
  // The TradingView loader is replaced by a stub that inserts a local frame where the real
  // one would put its widget: deterministic, offline, and the same on both servers.
  await context.route('https://s3.tradingview.com/**', (r) => {
    if (route === 'economics' && state === 'error') return r.fulfill({ status: 500, body: '' });
    const rows = state === 'populated'
      ? Array.from({ length: 30 }, (_, i) => `<tr><td>2026-10-${String(15 + (i % 10)).padStart(2, '0')} 14:30</td><td>US</td><td>Nonfarm payrolls and a long event title that wraps on a phone ${i}</td><td>-12,345.6K</td></tr>`).join('')
      : '<tr><td>No events in this range.</td></tr>';
    const doc = `<!doctype html><meta name=viewport content="width=device-width"><body style="margin:0;font:13px sans-serif"><table style="width:100%">${rows}</table>`;
    const js = `(function(){var s=document.currentScript;var f=document.createElement('iframe');f.title='Economic calendar fixture';f.srcdoc=${JSON.stringify(doc)};f.style.border='0';s.parentNode.appendChild(f);})();`;
    return r.fulfill({ status: 200, contentType: 'text/javascript', body: js });
  });
  await context.route('**/api/**', async (r) => {
    const req = r.request();
    const p = new URL(req.url()).pathname;
    const method = req.method();
    if (method !== 'GET') captured.push({ method, path: p, body: req.postDataJSON?.() ?? null });
    const shell = shellResult(p);
    if (shell !== undefined) return r.fulfill({ json: shell });
    const fail = () => r.fulfill({ status: 500, json: { error: 'Fixture server error: the data service is unavailable' } });
    if (route === 'histviz') {
      if (p === '/api/histviz/workspaces' && method === 'GET') {
        if (state === 'error') return fail();
        return r.fulfill({ json: { workspaces: state === 'empty' ? [] : [store.row ?? workspaceRow()] } });
      }
      if (p.startsWith('/api/histviz/workspaces/') && method === 'PUT') {
        // Kept as the server would, so a reload reopens what was saved.
        const body = req.postDataJSON();
        store.row = { id: 'ws-multi', name: body.name, grid_rows: body.rows, grid_cols: body.cols, panes: body.panes, settings: body.settings };
        return r.fulfill({ json: store.row });
      }
      if (p === '/api/histviz/series/batch') {
        if (state === 'error') return fail();
        const items = req.postDataJSON()?.items ?? [];
        return r.fulfill({ json: { results: items.map((it) => barsFor(it.ticker)) } });
      }
      if (p === '/api/histviz/series') {
        if (state === 'error') return fail();
        return r.fulfill({ json: barsFor(req.postDataJSON()?.ticker) });
      }
      if (p === '/api/histdata/datasets') return r.fulfill({ json: { datasets: state === 'populated' ? datasets : [] } });
      if (p === '/api/histviz/lists') return r.fulfill({ json: { lists: [] } });
      if (p === '/api/histviz/alerts') return r.fulfill({ json: { alerts: [] } });
      if (p === '/api/histviz/chart-settings' || p === '/api/histviz/layout') return r.fulfill({ json: {} });
    }
    if (route === 'economics' && p === '/api/reminders' && method === 'POST') return r.fulfill({ json: { reminder: { id: 'rem-1', ...(req.postDataJSON() ?? {}) } } });
    if (route === 'quant') {
      if (p === '/api/histdata/datasets') {
        if (state === 'error') return fail();
        return r.fulfill({ json: { datasets: state === 'empty' ? [] : datasets } });
      }
      if (p === '/api/backtest/runs') {
        if (state === 'error') return fail();
        return r.fulfill({ json: { runs: state === 'empty' ? [] : runs } });
      }
      if (state === 'error' && p.startsWith('/api/quant/')) return fail();
      if (p === '/api/quant/single') return r.fulfill({ json: singleResult(req.postDataJSON()?.confidence ?? 0.95) });
      if (p === '/api/quant/portfolio') return r.fulfill({ json: portfolioResult });
      if (/\/api\/backtest\/runs\/[^/]+\/montecarlo$/.test(p)) return r.fulfill({ json: mcResult });
    }
    return r.fulfill({ json: {} });
  });
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.stack || e.message));
  await page.clock.setFixedTime(CLOCK);
  if (desktopCompare) await page.emulateMedia({ reducedMotion: 'reduce' });
  let ok = false;
  for (let attempt = 0; attempt < 2 && !ok; attempt++) {
    try {
      await page.goto(`${url}/${route}`, { waitUntil: 'domcontentloaded', timeout: 20000 });
      await page.waitForFunction(() => document.body.innerText.trim().length > 20, undefined, { timeout: 10000 });
      ok = true;
    } catch (e) {
      if (attempt) throw e;
    }
  }
  return { context, page, errors, captured };
}

async function noPageOverflow(page, label) {
  const m = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    scroll: document.documentElement.scrollWidth,
    culprits: [...document.querySelectorAll('body *')].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.right > document.documentElement.clientWidth + 1 && r.width > 0 && getComputedStyle(el).position !== 'fixed';
    }).slice(0, 6).map((el) => `${el.tagName.toLowerCase()}.${typeof el.className === 'string' ? el.className.trim().replace(/\s+/g, '.') : ''}`)
  }));
  assert.ok(m.scroll <= m.viewport + 1, `${label}: page overflows (${m.scroll} > ${m.viewport}) ${m.culprits.join(', ')}`);
}

/** The control can be scrolled into view, its center is on screen and a tap there hits it. */
async function reachable(locator, label) {
  await locator.scrollIntoViewIfNeeded();
  let hit = await probeHit(locator);
  // Minimal scrolling can leave a control under the floating shell buttons at the viewport
  // edge; a user scrolls it to the middle, so does this check.
  if (!hit.hits) {
    await locator.evaluate((el) => el.scrollIntoView({ block: 'center', inline: 'center' }));
    hit = await probeHit(locator);
  }
  assert.ok(hit.inside, `${label}: center off screen ${JSON.stringify(hit.rect)}`);
  assert.ok(hit.hits, `${label}: covered by ${hit.at} ${JSON.stringify(hit.rect)}`);
}

function probeHit(locator) {
  return locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const inside = x >= 0 && y >= 0 && x <= innerWidth && y <= innerHeight;
    const top = document.elementFromPoint(x, y);
    return { inside, hits: !!top && (top === el || el.contains(top)), at: top ? `${top.tagName.toLowerCase()}.${top.className}` : null, rect: [r.left, r.top, r.width, r.height].map(Math.round) };
  });
}

const results = [];
const pass = (line) => {
  results.push(line);
  console.log(`PASS: ${line}`);
};


/** Key paths of a JSON value, so two payloads can be compared by shape. */
function shape(value, prefix = '') {
  if (Array.isArray(value)) return value.length ? shape(value[0], `${prefix}[]`) : [`${prefix}[]`];
  if (value && typeof value === 'object') {
    const keys = Object.keys(value).sort();
    // Drawing boards are keyed by instrument: compare the boards, not the symbols.
    if (prefix.endsWith('.drawings')) return keys.length ? shape(value[keys[0]], `${prefix}.*`) : [`${prefix}.*`];
    return keys.flatMap((k) => shape(value[k], prefix ? `${prefix}.${k}` : k));
  }
  return [`${prefix}:${value === null ? 'null' : typeof value}`];
}
const uniq = (list) => [...new Set(list)].sort();

/** A touch stroke through the CDP input pipeline, so touch-action and scrolling apply. */
async function touchDrag(page, from, to, steps = 8) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: from.x, y: from.y }] });
  for (let i = 1; i <= steps; i++) {
    const x = from.x + ((to.x - from.x) * i) / steps;
    const y = from.y + ((to.y - from.y) * i) / steps;
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y }] });
  }
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await cdp.detach();
}

const tag = (vp) => `${vp.width}x${vp.height}`;
const shot = (page, name) => page.screenshot({ path: `${artifacts}/${name}.png`, fullPage: true });

// ── Histviz ──────────────────────────────────────────────────────────────────

const drawCount = (page) => page.locator('.pane-dock .icon-btn[aria-label="Objects on this chart"] .badge').innerText().then((s) => Number(s.trim() || 0));

async function histvizPhone(state, vp) {
  const { context, page, errors } = await openPage(base, 'histviz', state, vp);
  const label = `histviz ${state} ${tag(vp)}`;
  try {
    const phone = vp.width < 768;
    if (state === 'populated') await page.locator('.chart-host .nav').first().waitFor({ state: 'attached', timeout: 15000 });
    else await page.locator('.top .tbtn.save').waitFor();
    await page.waitForTimeout(400);
    await noPageOverflow(page, label);
    // Workspace bar: picker, save, quick backtest, book, grid shape, fullscreen.
    for (const sel of ['.ws-btn', '.top .tbtn.save', '.top-scroll .tbtn.wide:not(.save)', '.book-wrap .tbtn', '.shape-wrap .tbtn', '.top > .tbtn']) {
      await reachable(page.locator(sel).first(), `${label} ${sel}`);
    }
    if (phone) {
      // One pane at a time, an explicit switcher with the list toggle and one button per chart.
      const nav = page.locator('.mobile-pane-nav button');
      const n = await nav.count();
      assert.equal(n, state === 'populated' ? 5 : 2, `${label}: pane switcher buttons`);
      for (let i = 0; i < n; i++) await reachable(nav.nth(i), `${label} pane switch ${i}`);
      await nav.nth(1).click();
      assert.equal(await nav.nth(1).getAttribute('aria-pressed'), 'true', `${label}: switcher marks the shown chart`);
      assert.equal(await page.locator('.cell:not(.mobile-inactive)').count(), 1, `${label}: one pane shown`);
    }
    // The active chart's controls: symbol, timeframe, plot style, studies, tools, pane buttons.
    const bar = page.locator('.pane-dock .bar');
    const buttons = bar.locator('button:visible');
    const count = await buttons.count();
    assert.ok(count >= 12, `${label}: pane bar buttons (${count})`);
    for (let i = 0; i < count; i++) await reachable(buttons.nth(i), `${label} pane bar button ${i}`);
    if (state === 'populated') {
      const tf = await bar.getByRole('button', { name: 'Timeframe' }).evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
      assert.ok(tf, `${label}: timeframe label cut`);
      // Drawing tools, each reachable (the rail scrolls on its own when short).
      const tools = page.locator('.draw-dock .tool');
      for (let i = 0; i < (await tools.count()); i++) {
        if (await tools.nth(i).isVisible()) await reachable(tools.nth(i), `${label} draw tool ${i}`);
      }
      if (phone) {
        const chart = page.locator('.cell:not(.mobile-inactive) .chart').first();
        assert.equal(await chart.evaluate((el) => getComputedStyle(el).touchAction), 'pan-y', `${label}: chart keeps page scroll`);
        for (const name of ['Zoom in', 'Zoom out', 'Move left', 'Move right']) {
          await reachable(page.locator('.cell:not(.mobile-inactive) .chart-host .nav').getByRole('button', { name, exact: true }), `${label} ${name}`);
        }
        // Readout by tap: the OHLC line follows the tapped bar.
        await chart.scrollIntoViewIfNeeded();
        const box = await chart.boundingBox();
        const ohlc = page.locator('.cell:not(.mobile-inactive) .head .ohlc');
        await page.touchscreen.tap(box.x + box.width * 0.35, box.y + box.height * 0.45);
        await page.waitForTimeout(150);
        assert.ok(await ohlc.isVisible(), `${label}: OHLC readout hidden`);
        const first = await ohlc.innerText();
        await page.touchscreen.tap(box.x + box.width * 0.75, box.y + box.height * 0.45);
        await page.waitForTimeout(150);
        assert.notEqual(await ohlc.innerText(), first, `${label}: readout did not follow the tap`);
        assert.ok(await ohlc.evaluate((el) => { const r = el.getBoundingClientRect(); const c = el.closest('.chart-host').getBoundingClientRect(); return r.right <= c.right + 1 && r.left >= c.left - 1; }), `${label}: readout clipped`);
      }
    }
    await shot(page, `histviz-${state}-${tag(vp)}`);
    assert.deepEqual(errors, [], `${label}: runtime errors`);
  } catch (e) {
    await shot(page, `histviz-${state}-${tag(vp)}-failure`).catch(() => {});
    throw e;
  } finally {
    await context.close();
  }
}

/** Draw a trend line on the AAPL chart and save: by touch on a phone, by mouse on desktop. */
async function histvizDrawAndSave(vp, store) {
  const phone = vp.width < 768;
  const { context, page, errors, captured } = await openPage(base, 'histviz', 'populated', vp, { store });
  try {
    await page.locator('.chart-host .nav').first().waitFor({ state: 'attached', timeout: 15000 });
    // AAPL is the active chart of the fixture; a phone shows it through the switcher.
    if (phone) await page.locator('.mobile-pane-nav button', { hasText: 'AAPL' }).click();
    await page.waitForTimeout(300);
    assert.equal(await drawCount(page), 1, 'fixture drawing on AAPL');
    await page.locator('.draw-dock').getByRole('button', { name: 'Trend line', exact: true }).click();
    const chart = page.locator('.cell').first().locator('.chart');
    await chart.scrollIntoViewIfNeeded();
    const box = await chart.boundingBox();
    const from = { x: box.x + box.width * 0.3, y: box.y + box.height * 0.3 };
    const to = { x: box.x + box.width * 0.6, y: box.y + box.height * 0.55 };
    if (phone) await touchDrag(page, from, to);
    else {
      await page.mouse.move(from.x, from.y);
      await page.mouse.down();
      await page.mouse.move(to.x, to.y, { steps: 8 });
      await page.mouse.up();
    }
    await page.waitForTimeout(200);
    assert.equal(await drawCount(page), 2, `trend line drawn by ${phone ? 'touch' : 'mouse'}`);
    if (phone) {
      // A drawn shape moves by touch too: drag the fixture's horizontal line up.
      const hline = page.locator('.cell:not(.mobile-inactive) .draw .shape line.hit').first();
      const hb = await hline.boundingBox();
      const at = { x: hb.x + hb.width * 0.8, y: hb.y + hb.height / 2 };
      await touchDrag(page, at, { x: at.x, y: at.y - 40 });
      await page.waitForTimeout(200);
    }
    const save = page.locator('.top .tbtn.save');
    await reachable(save, `save ${tag(vp)}`);
    await save.click();
    await page.waitForFunction(() => document.querySelector('.top .tbtn.save')?.disabled === true);
    const put = captured.find((c) => c.method === 'PUT' && c.path === '/api/histviz/workspaces/ws-multi');
    assert.ok(put, 'workspace PUT sent');
    assert.deepEqual(errors, [], 'draw and save runtime errors');
    return put.body;
  } finally {
    await context.close();
  }
}

async function histvizReopen(vp, store) {
  const { context, page, errors } = await openPage(base, 'histviz', 'populated', vp, { store });
  try {
    await page.locator('.chart-host .nav').first().waitFor({ state: 'attached', timeout: 15000 });
    await page.locator('.mobile-pane-nav button', { hasText: 'AAPL' }).click();
    await page.waitForTimeout(300);
    assert.equal(await drawCount(page), 2, 'reopened workspace keeps both drawings');
    assert.equal((await page.locator('.shape-lbl').innerText()).trim(), '2×2', 'reopened workspace keeps its 2x2 grid');
    assert.equal(await page.locator('.top .tbtn.save').isDisabled(), true, 'reopened workspace is clean');
    assert.deepEqual(errors, [], 'reopen runtime errors');
  } finally {
    await context.close();
  }
}

async function histvizSuite() {
  for (const state of ['populated', 'empty', 'error']) {
    for (const vp of PHONES) await histvizPhone(state, vp);
    pass(`histviz ${state}: no page overflow, workspace bar, pane switcher, pane bar, drawing tools and chart controls reachable at ${PHONES.map(tag).join(', ')}`);
  }
  pass('histviz: tap moves the crosshair and the OHLC readout follows it; chart touch-action stays pan-y');
  const mobileStore = {};
  const mobile = await histvizDrawAndSave({ width: 390, height: 844 }, mobileStore);
  const desktop = await histvizDrawAndSave({ width: 1440, height: 900 }, {});
  assert.deepEqual(uniq(shape(mobile)), uniq(shape(desktop)), 'mobile and desktop save payloads differ in shape');
  assert.equal(mobile.panes.length, 4);
  assert.equal(mobile.settings.drawings['fixture|equity|AAPL'].length, 2);
  const moved = mobile.settings.drawings['fixture|equity|AAPL'].find((d) => d.id === 'dfix1');
  assert.ok(moved.a.y > 186.5, `horizontal line moved up by touch (y ${moved.a.y})`);
  pass('histviz 390: trend line drawn and a horizontal line moved by touch drag, saved with Save; PUT payload has the same shape as the desktop mouse flow');
  await histvizReopen({ width: 390, height: 844 }, mobileStore);
  pass('histviz 390: reopening the saved workspace keeps the 2x2 grid and both drawings');
}

// ── Economics ────────────────────────────────────────────────────────────────

async function economicsPhone(state, vp) {
  const { context, page, errors } = await openPage(base, 'economics', state, vp);
  const label = `economics ${state} ${tag(vp)}`;
  try {
    await page.locator('.econ-page .tv').waitFor();
    if (state !== 'error') await page.locator('.tv iframe').waitFor({ state: 'attached' });
    await page.waitForTimeout(300);
    await noPageOverflow(page, label);
    await reachable(page.locator('.econ-page .head .rem-icon'), `${label} reminder`);
    // The calendar frame fits the width and its whole height can be scrolled into view.
    const fit = await page.locator('.econ-page .tv').evaluate((el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, height: r.height, vw: document.documentElement.clientWidth };
    });
    assert.ok(fit.left >= 0 && fit.right <= fit.vw + 1, `${label}: calendar wider than the screen`);
    assert.ok(fit.height >= 290, `${label}: calendar too short (${fit.height})`);
    if (state !== 'error') {
      const frame = page.locator('.tv iframe');
      const fb = await frame.evaluate((el) => { const r = el.getBoundingClientRect(); const t = el.closest('.tv').getBoundingClientRect(); return { w: r.width, h: r.height, tw: t.width, th: t.height }; });
      assert.ok(Math.abs(fb.w - fb.tw) <= 2 && fb.h <= fb.th + 1 && fb.h >= fb.th - 4, `${label}: frame does not fill its box ${JSON.stringify(fb)}`);
      // Bottom edge reachable: scrolled into view, it sits above the viewport bottom.
      await page.locator('.econ-page .tv').evaluate((el) => el.scrollIntoView({ block: 'end' }));
      const bottom = await page.locator('.econ-page .tv').evaluate((el) => el.getBoundingClientRect().bottom);
      assert.ok(bottom <= vp.height + 1, `${label}: calendar bottom cut (${bottom} > ${vp.height})`);
      if (state === 'populated') assert.match(await page.frameLocator('.tv iframe').locator('body').innerText(), /Nonfarm payrolls/);
    }
    await shot(page, `economics-${state}-${tag(vp)}`);
    assert.deepEqual(errors, [], `${label}: runtime errors`);
  } finally {
    await context.close();
  }
}

async function economicsReminder(vp) {
  const { context, page, errors, captured } = await openPage(base, 'economics', 'populated', vp);
  try {
    await page.locator('.econ-page .head .rem-icon').click();
    const dialog = page.getByRole('dialog');
    await dialog.locator('input').first().fill('Check the payrolls release');
    const submit = dialog.locator('button[type="submit"]');
    await reachable(submit, `reminder submit ${tag(vp)}`);
    await submit.click();
    await dialog.waitFor({ state: 'hidden' });
    const post = captured.find((c) => c.method === 'POST' && c.path === '/api/reminders');
    assert.ok(post, 'reminder POST sent');
    assert.equal(post.body.name, 'Check the payrolls release');
    assert.deepEqual(errors, [], 'reminder runtime errors');
    return post.body;
  } finally {
    await context.close();
  }
}

async function economicsSuite() {
  for (const state of ['populated', 'empty', 'error']) {
    for (const vp of PHONES) await economicsPhone(state, vp);
    pass(`economics ${state}: no page overflow, reminder reachable, calendar frame fits the width and its bottom scrolls into view at ${PHONES.map(tag).join(', ')}`);
  }
  const mobile = await economicsReminder({ width: 390, height: 844 });
  const desktop = await economicsReminder({ width: 1440, height: 900 });
  assert.deepEqual(uniq(shape(mobile)), uniq(shape(desktop)), 'reminder payloads differ in shape');
  pass('economics 390: reminder created by tap; POST payload has the same shape as on desktop');
}

// ── Quant ────────────────────────────────────────────────────────────────────

/** A tap on an ECharts canvas shows its tooltip: the readout without hover. `points` are
 *  fractions of the chart box (or a function of the box), tried in turn until one shows it. */
async function tapReadout(page, host, label, points = [[0.55, 0.45]]) {
  await host.evaluate((el) => el.scrollIntoView({ block: 'center', inline: 'nearest' }));
  await page.waitForTimeout(500); // a smooth scroll settles before the box is read
  const box = await host.boundingBox();
  const list = typeof points === 'function' ? points(box) : points.map(([fx, fy]) => [box.x + box.width * fx, box.y + box.height * fy]);
  for (const [x, y] of list) {
    await page.touchscreen.tap(x, y);
    await page.waitForTimeout(250);
    const text = await host.evaluate((el) => [...el.querySelectorAll('div')].filter((d) => getComputedStyle(d).display !== 'none' && getComputedStyle(d).visibility !== 'hidden' && d.style.zIndex && d.innerText.trim()).map((d) => d.innerText.trim()).join(' '));
    if (text) return text;
    if (process.env.CHARTS_DEBUG) console.log(label, Math.round(x), Math.round(y), JSON.stringify(await host.innerText()), await page.evaluate(([x, y]) => { const e = document.elementFromPoint(x, y); return `${e?.tagName}.${e?.className}`; }, [x, y]));
  }
  assert.fail(`${label}: no tooltip after a tap`);
}

const canvasFits = (page, sel) => page.locator(sel).first().evaluate((el) => {
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.left >= -1 && r.right <= document.documentElement.clientWidth + 1;
});

async function quantGroup(page, name) {
  await page.locator('.tabs').getByRole('button', { name, exact: true }).click();
  await page.waitForTimeout(150);
}

async function quantPhone(state, vp) {
  const { context, page, errors, captured } = await openPage(base, 'quant', state, vp);
  const label = `quant ${state} ${tag(vp)}`;
  try {
    await page.locator('.page .tabs').waitFor();
    await page.waitForTimeout(300);
    await noPageOverflow(page, label);
    for (const sel of ['.tabs button', '.subtabs button']) {
      const list = page.locator(sel);
      for (let i = 0; i < (await list.count()); i++) await reachable(list.nth(i), `${label} ${sel} ${i}`);
    }
    if (state === 'error') {
      await page.getByText('Fixture server error', { exact: false }).first().waitFor();
    }
    if (state === 'populated') {
      // Asset group: dataset picker, window, confidence, Analyze; then the results.
      await reachable(page.locator('.combo .trigger').first(), `${label} dataset picker`);
      await page.locator('.combo .trigger').first().click();
      await reachable(page.locator('.combo .opt').nth(2), `${label} dataset option`);
      await page.locator('.combo .opt').nth(2).click();
      for (const sel of ['.slider input >> nth=0', '.slider input >> nth=1', '.full-btn', '.conf-row .preset >> nth=2', '.conf-custom input']) await reachable(page.locator(sel), `${label} ${sel}`);
      await page.locator('.conf-custom input').fill('97');
      const analyze = page.getByRole('button', { name: 'Analyze', exact: true });
      await reachable(analyze, `${label} analyze`);
      await analyze.click();
      await page.locator('.card-box canvas').first().waitFor();
      await page.waitForTimeout(300);
      await noPageOverflow(page, `${label} single results`);
      assert.ok(await canvasFits(page, '.card-box canvas'), `${label}: risk chart wider than the screen`);
      await tapReadout(page, page.locator('.card-box > div').first(), `${label} risk chart`, [[0.5, 0.2], [0.4, 0.25], [0.6, 0.75]]);
      // Long drawdown table scrolls in its own region.
      assert.ok(await page.locator('.qp-scroll').last().evaluate((el) => getComputedStyle(el).overflowX === 'auto' && el.getBoundingClientRect().right <= document.documentElement.clientWidth + 1), `${label}: drawdown table region`);
      const single = captured.find((c) => c.path === '/api/quant/single');
      assert.equal(single.body.confidence, 0.97);

      // Multi-asset: picker then correlation matrix, frontier, risk parity.
      await quantGroup(page, 'Multi-asset');
      for (const sel of ['.picker-search', '.picker .chk >> nth=0', '.picker .chk >> nth=3']) await reachable(page.locator(sel), `${label} ${sel}`);
      const boxes = page.locator('.picker .chk input');
      for (const i of [0, 1, 2]) await boxes.nth(i).check();
      await reachable(page.locator('.picker .primary'), `${label} analyze portfolio`);
      await page.locator('.picker .primary').click();
      await page.locator('.port-main .block canvas').first().waitFor();
      await page.waitForTimeout(300);
      await noPageOverflow(page, `${label} portfolio results`);
      const layout = await page.evaluate(() => { const a = document.querySelector('.picker').getBoundingClientRect(); const m = document.querySelector('.port-main').getBoundingClientRect(); return { stacked: a.bottom <= m.top + 1, fits: m.right <= document.documentElement.clientWidth + 1 }; });
      assert.ok(layout.stacked && layout.fits, `${label}: picker and results ${JSON.stringify(layout)}`);
      const matrix = page.locator('.port-main .block').first().locator('.chart');
      // The matrix keeps a readable width and scrolls in its block: scroll to the cells (they
      // sit right of the axis names), then tap across the visible part of the plot.
      const scroller = page.locator('.port-main .block').first();
      const fit = await scroller.evaluate((el) => { const over = el.scrollWidth > el.clientWidth + 1; el.scrollLeft = el.scrollWidth; return { ok: !over || getComputedStyle(el).overflowX === 'auto', width: el.querySelector('.chart').getBoundingClientRect().width }; });
      assert.ok(fit.ok, `${label}: matrix overflows its block without a scroller`);
      if (vp.width < 768) assert.ok(fit.width >= 600, `${label}: matrix squeezed to ${fit.width}px`);
      const cells = (b) => {
        const right = Math.min(b.x + b.width, vp.width) - 24;
        return [0.2, 0.4, 0.6].flatMap((fy) => [0.15, 0.4, 0.7].map((fx) => [right - (right - Math.max(b.x, 0)) * fx * 0.5, b.y + b.height * fy]));
      };
      assert.match(await tapReadout(page, matrix, `${label} correlation matrix`, cells), /↔/);

      // Strategy: the 360 px Monte Carlo panel stacks above its results.
      await quantGroup(page, 'Strategy');
      await page.locator('.mc-layout .primary').waitFor();
      for (const sel of ['.mc-layout .panel .preset >> nth=0', '.mc-layout .panel .preset >> nth=2', '.mc-layout .panel .preset >> nth=4', '.mc-layout .num-wrap input', '.mc-layout .field input[placeholder]', '.mc-layout .primary']) await reachable(page.locator(sel), `${label} ${sel}`);
      await page.locator('.mc-layout .num-wrap input').fill('40');
      await page.locator('.mc-layout .primary').click();
      await page.locator('.mc-main canvas').first().waitFor();
      await page.waitForTimeout(300);
      await noPageOverflow(page, `${label} Monte Carlo results`);
      const mc = await page.evaluate(() => { const a = document.querySelector('.mc-layout .panel').getBoundingClientRect(); const m = document.querySelector('.mc-main').getBoundingClientRect(); return { stacked: a.bottom <= m.top + 1, panel: a.width, vw: document.documentElement.clientWidth, right: m.right }; });
      assert.ok(mc.stacked && mc.panel <= mc.vw && mc.right <= mc.vw + 1, `${label}: Monte Carlo layout ${JSON.stringify(mc)}`);
      await tapReadout(page, page.locator('.mc-main .block').first().locator('div').filter({ has: page.locator('canvas') }).first(), `${label} equity fan`, [[0.6, 0.5], [0.4, 0.4]]);
      const run = captured.find((c) => /montecarlo$/.test(c.path));
      assert.equal(run.body.ruin_pct, 0.4);

      // Every other tab: no page overflow and its first input or button reachable.
      for (const g of ['Asset', 'Multi-asset', 'Strategy', 'Sizing', 'Calculators', 'Derivatives']) {
        await quantGroup(page, g);
        const subs = page.locator('.subtabs button');
        for (let i = 0; i < (await subs.count()); i++) {
          await subs.nth(i).click();
          await page.waitForTimeout(150);
          await noPageOverflow(page, `${label} ${g} tab ${i}`);
          const first = page.locator('.page > :not(header):not(h1) :is(input, button):visible').first();
          if (await first.count()) await reachable(first, `${label} ${g} tab ${i} first control`);
        }
      }
    }
    if (state === 'empty') {
      await page.locator('.hint').first().waitFor();
      await quantGroup(page, 'Strategy');
      await page.locator('.mc-layout .panel').waitFor();
      await noPageOverflow(page, `${label} Monte Carlo without runs`);
    }
    await shot(page, `quant-${state}-${tag(vp)}`);
    assert.deepEqual(errors, [], `${label}: runtime errors`);
  } catch (e) {
    await shot(page, `quant-${state}-${tag(vp)}-failure`).catch(() => {});
    throw e;
  } finally {
    await context.close();
  }
}

async function quantRuns(vp) {
  const { context, page, errors, captured } = await openPage(base, 'quant', 'populated', vp);
  try {
    await page.locator('.combo .trigger').first().click();
    await page.locator('.combo .opt').nth(2).click();
    await page.getByRole('button', { name: 'Analyze', exact: true }).click();
    await page.locator('.card-box canvas').first().waitFor();
    await quantGroup(page, 'Strategy');
    await page.locator('.mc-layout .primary').click();
    await page.locator('.mc-main canvas').first().waitFor();
    assert.deepEqual(errors, [], 'quant run errors');
    return { single: captured.find((c) => c.path === '/api/quant/single').body, mc: captured.find((c) => /montecarlo$/.test(c.path)).body };
  } finally {
    await context.close();
  }
}

async function quantSuite() {
  for (const state of ['populated', 'empty', 'error']) {
    for (const vp of PHONES) await quantPhone(state, vp);
    pass(`quant ${state}: no page overflow, group and sub tabs reachable${state === 'populated' ? '; dataset picker, window, confidence and Analyze; portfolio picker stacks above matrix and frontier; Monte Carlo panel stacks above its results; every tab has its first control reachable' : ''} at ${PHONES.map(tag).join(', ')}`);
  }
  pass('quant: a tap on the risk chart, correlation matrix and equity fan shows the chart tooltip');
  const mobile = await quantRuns({ width: 390, height: 844 });
  const desktop = await quantRuns({ width: 1440, height: 900 });
  assert.deepEqual(mobile, desktop, 'quant request payloads differ between phone and desktop');
  pass('quant 390: single-asset analysis and Monte Carlo run by tap send the same payloads as on desktop');
}

// ── Desktop no-regression ────────────────────────────────────────────────────

const READY = {
  histviz: async (page, state) => {
    if (state === 'populated') {
      await page.locator('.chart-host .nav').first().waitFor({ state: 'attached', timeout: 15000 });
      // Every chart loaded and its drawing rail out of the dimmed loading state.
      await page.waitForFunction(() => document.querySelectorAll('.chart-host').length === 4 && !document.querySelector('.rail-slot.idle, .chart-slot.busy, [aria-busy="true"]'));
    } else await page.locator('.top .tbtn.save').waitFor();
  },
  economics: async (page) => page.locator('.econ-page .tv').waitFor(),
  quant: async (page) => page.locator('.page .tabs').waitFor()
};

/** Count differing pixels between two PNGs, decoded by the browser. */
async function pixelDiff(a, b) {
  const page = await browser.newPage();
  try {
    return await page.evaluate(async ([x, y]) => {
      const load = (src) => new Promise((res, rej) => { const i = new Image(); i.onload = () => res(i); i.onerror = rej; i.src = src; });
      const [ia, ib] = await Promise.all([load(x), load(y)]);
      if (ia.width !== ib.width || ia.height !== ib.height) return { size: [ia.width, ia.height, ib.width, ib.height] };
      const read = (img) => { const c = document.createElement('canvas'); c.width = img.width; c.height = img.height; const g = c.getContext('2d'); g.drawImage(img, 0, 0); return g.getImageData(0, 0, c.width, c.height).data; };
      const da = read(ia); const db = read(ib);
      let n = 0; let box = null;
      for (let i = 0; i < da.length; i += 4) {
        if (da[i] !== db[i] || da[i + 1] !== db[i + 1] || da[i + 2] !== db[i + 2]) {
          n++;
          const px = (i / 4) % ia.width; const py = Math.floor(i / 4 / ia.width);
          box = box ? [Math.min(box[0], px), Math.min(box[1], py), Math.max(box[2], px), Math.max(box[3], py)] : [px, py, px, py];
        }
      }
      return { n, box };
    }, [`data:image/png;base64,${a.toString('base64')}`, `data:image/png;base64,${b.toString('base64')}`]);
  } finally {
    await page.close();
  }
}

async function desktopShot(url, route, state, vp) {
  const { context, page } = await openPage(url, route, state, vp, { desktopCompare: true });
  try {
    await READY[route](page, state);
    await page.waitForLoadState('networkidle').catch(() => {});
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(1500);
    return await page.screenshot({ fullPage: true, animations: 'disabled' });
  } finally {
    await context.close();
  }
}

const noise = [];
async function desktopSuite(route) {
  const diffs = [];
  for (const state of ['populated', 'empty', 'error']) {
    for (const vp of DESKTOPS) {
      // Chart canvases settle at slightly different moments from one load to the next (two
      // loads of the same server can differ by a few hundred pixels of axis text), so up to
      // six loads per server are taken and any identical pair passes.
      const befores = [await desktopShot(baseline, route, state, vp)];
      const afters = [await desktopShot(base, route, state, vp)];
      const same = () => befores.some((b) => afters.some((a) => a.equals(b)));
      while (!same() && afters.length < 6) {
        befores.push(await desktopShot(baseline, route, state, vp));
        afters.push(await desktopShot(base, route, state, vp));
      }
      if (afters.length > 1) noise.push(`${route} ${state} ${tag(vp)}: ${afters.length} loads per server${same() ? ', identical pair found' : ''}`);
      const before = befores[0];
      const after = afters[0];
      if (!same()) {
        const { writeFile } = await import('node:fs/promises');
        await writeFile(`${artifacts}/desktop-${route}-${state}-${tag(vp)}-baseline.png`, before);
        await writeFile(`${artifacts}/desktop-${route}-${state}-${tag(vp)}-current.png`, after);
        diffs.push({ state, vp: tag(vp), ...(await pixelDiff(before, after)) });
      }
    }
  }
  assert.deepEqual(diffs, [], `${route}: desktop screenshots differ from the baseline`);
  pass(`${route} desktop: 1440x900 and 1100x800 screenshots identical to the baseline in populated, empty and error states`);
}

// ── Run ──────────────────────────────────────────────────────────────────────

const suites = { histviz: histvizSuite, economics: () => economicsSuite(), quant: () => quantSuite() };
try {
  for (const [route, run] of Object.entries(suites)) {
    if (only && !only.includes(route)) continue;
    if (!process.env.CHARTS_DESKTOP_ONLY) await run();
    if (!process.env.CHARTS_SKIP_DESKTOP) await desktopSuite(route);
  }
  if (noise.length) console.log(`Desktop reloads needed:\n  ${noise.join('\n  ')}`);
  console.log(`\n${results.length} checks passed. Screenshots: ${artifacts}`);
} finally {
  await browser.close();
}
