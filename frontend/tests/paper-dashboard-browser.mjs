// Local fixtures only: node tests/paper-dashboard-browser.mjs [base URL]
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
const base = process.argv[2] ?? 'http://127.0.0.1:5173';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/paper-dashboard-tests/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });
try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, locale: 'en-US' });
  await context.addInitScript(() => {
    window.paperSources = [];
    window.EventSource = class extends EventTarget {
      constructor(url) { super(); this.url = url; this.closed = false; window.paperSources.push(this); }
      close() { this.closed = true; }
      emit(type, payload) { this.dispatchEvent(new MessageEvent(type, { data: JSON.stringify(payload) })); }
    };
  });
  const page = await context.newPage();
  page.setDefaultTimeout(15000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  const at = new Date(Date.now() - 60000).toISOString();
  const datasets = [{ id: 'ds-a', provider: 'fixture', asset_type: 'crypto', ticker: 'BTCUSDT', timeframe: '1m', bar_count: 5000 }];
  let session = { id: 'paper-a', name: 'Bitcoin momentum · trend following', status: 'active', running: false, kind: 'interval', every_minutes: 1, dataset_ids: ['ds-a'], settings: { kind: 'signals', starting_capital: 10000, instrument: { multiplier: 1 } }, last_run_at: at, next_run_at: new Date(Date.now() + 60000).toISOString(), bars: 5000, stats: { net_pnl: 642.80, final_equity: 10642.80, return_pct: 6.428, max_drawdown_pct: 2.3, profit_factor: 1.87, sharpe: 1.42, sortino: 2.12, avg_trade: 64.28, trades: 10, open_positions: [{ ticker: 'BTCUSDT', direction: 'long', entry_ts: at, avg_price: 67200, qty: .12, stop: 66000, take_profit: 71000 }], pending_orders: [{ ticker: 'BTCUSDT', direction: 'long', kind: 'add', order: 'limit', qty: .05, price: 66500, bars_left: 3, signal_ts: at }] } };
  const trades = Array.from({ length: 10 }, (_, n) => ({ open: false, ticker: 'BTCUSDT', direction: n % 3 ? 'long' : 'short', entry_ts: new Date(Date.now() - (12 - n) * 3600000).toISOString(), exit_ts: new Date(Date.now() - (11 - n) * 3600000).toISOString(), pnl: n % 3 ? 120.4 : -20.3, fees: 2.4, trade: { qty: .1, entry_price: 66500, exit_price: 67704, exit_reason: 'take_profit', return_pct: n % 3 ? 1.81 : -0.31, mae: 40, mfe: 150 } }));
  let requests = 0, fail = false, failAction = false;
  await context.route('**/api/**', async (route) => {
    const req = route.request(), p = new URL(req.url()).pathname;
    let json = {};
    if (p === '/api/setup/status') json = { configured: true };
    else if (p === '/api/settings/me') json = { username: 'fixture', role: 'admin' };
    else if (p === '/api/settings/modules') json = { installed: ['backtest', 'histdata', 'histviz'] };
    else if (p === '/api/settings/defaults') json = { default_timezone: 'UTC' };
    else if (p === '/api/health') json = { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
    else if (p === '/api/histdata/datasets') json = { datasets };
    else if (p === '/api/backtest/paper') json = { sessions: [session] };
    else if (p === '/api/backtest/paper/events') json = { events: [] };
    else if (p.endsWith('/dashboard')) {
      requests++;
      if (fail) return route.fulfill({ status: 503, json: { error: 'Connection interrupted' } });
      json = { session, instruments: [{ ...datasets[0], price: 68450.25, price_at: at, stream: true, chart_stream: true, stream_timeframe: '1m' }], trades, live_fills: [], events: [{ id: 'event-a', kind: 'open', at, title: 'BTCUSDT · Long position opened', message: 'Entry filled at 67,200. Stop loss at 66,000; take profit at 71,000.' }] };
    } else if (p.endsWith('/status') && req.method() === 'POST') {
      if (failAction) return route.fulfill({ status: 500, json: { error: 'Cannot change session status' } });
      session = { ...session, status: req.postDataJSON().status };
      json = session;
    } else if (p.endsWith('/run') && req.method() === 'POST') json = { session, error: '' };
    else if (p === '/api/backtest/runs') json = { runs: [] };
    else if (p === '/api/backtest/strategies') json = { strategies: [] };
    else if (p === '/api/backtest/indicators') json = { indicators: [] };
    else if (p === '/api/histviz/chart-settings') json = null;
    else if (p === '/api/histdata/datasets/ds-a/bars') {
      const ts = Array.from({ length: 120 }, (_, n) => new Date(Date.now() - (120 - n) * 60000).toISOString());
      json = { ts, o: ts.map(() => 68000), h: ts.map(() => 68600), l: ts.map(() => 67800), c: ts.map(() => 68400), v: ts.map(() => 1) };
    }
    else if (p.includes('/notifications')) json = { notifications: [], unread: 0 };
    await route.fulfill({ json });
  });
  await page.goto(base + '/backtest');
  await page.getByRole('tab', { name: /Paper/ }).click();
  await page.getByRole('link', { name: 'Open strategy dashboard' }).click();
  const dashboard = page.locator('[data-paper-dashboard]');
  await dashboard.getByRole('heading', { name: session.name }).waitFor();
  await dashboard.getByText('68,450.2500', { exact: true }).first().waitFor();
  assert.equal(await dashboard.locator('canvas').count(), 1, 'performance chart renders');
  assert.equal(await dashboard.locator('.metric').count(), 4);
  await page.evaluate(() => {
    const source = window.paperSources.at(-1);
    source.emit('status', { i: 0, state: 'live' });
    source.emit('bar', { i: 0, c: 68500, lag_ms: 0 });
  });
  await dashboard.getByText('68,500.0000', { exact: true }).first().waitFor();
  await dashboard.getByText('Live feed', { exact: true }).waitFor();
  await dashboard.locator('.metric').nth(1).getByText('+156.00', { exact: true }).waitFor();
  await page.screenshot({ path: artifacts + '/desktop.png', fullPage: true });
  await page.evaluate(() => window.paperSources.at(-1).emit('status', { i: 0, state: 'retrying', message: 'Reconnecting to price feed' }));
  await dashboard.getByText('Last quote · stale or disconnected', { exact: true }).waitFor();
  await page.evaluate(() => window.paperSources.at(-1).emit('status', { i: 0, state: 'live' }));
  await dashboard.getByRole('button', { name: 'Chart', exact: true }).click();
  const chart = dashboard.locator('[data-paper-chart]');
  await chart.locator('canvas').first().waitFor();
  await page.screenshot({ path: artifacts + '/chart.png' });
  assert.ok(page.url().endsWith('/backtest/paper/paper-a'), 'dashboard is its own page');
  for (const [tab, shot] of [['Statistics', 'stats'], ['Performance', 'perf'], ['Scatter 3D', 'scatter3d'], ['Streaks 3D', 'streaks3d']]) {
    await dashboard.getByRole('button', { name: tab, exact: true }).click();
    await page.waitForTimeout(400);
    await page.screenshot({ path: artifacts + `/${shot}.png` });
  }
  await dashboard.getByRole('button', { name: 'Overview', exact: true }).click();
  await dashboard.getByRole('button', { name: /Pending orders/ }).click();
  await dashboard.getByText('66,500.0000', { exact: true }).waitFor();
  await dashboard.getByRole('button', { name: /Trade history/ }).click();
  assert.equal(await dashboard.locator('tbody tr').count(), 10);
  await dashboard.getByRole('button', { name: /Activity/ }).click();
  await dashboard.getByText('BTCUSDT · Long position opened').waitFor();
  await dashboard.getByRole('button', { name: 'Stop strategy', exact: true }).click();
  await dashboard.getByText('Stopped', { exact: true }).waitFor();
  await dashboard.getByRole('button', { name: 'Start strategy', exact: true }).click();
  await dashboard.getByText('Watching', { exact: true }).waitFor();
  failAction = true;
  await dashboard.getByRole('button', { name: 'Stop strategy', exact: true }).click();
  await dashboard.getByRole('alert').getByText('Cannot change session status').waitFor();
  assert.equal(session.status, 'active');
  failAction = false;
  fail = true;
  await dashboard.getByRole('button', { name: 'Refresh dashboard', exact: true }).click();
  await dashboard.getByText(/Updates interrupted. Showing the last successful snapshot/).waitFor();
  assert.equal(await dashboard.locator('.metric').count(), 4, 'retains snapshot after refresh failure');
  fail = false;
  await dashboard.getByRole('button', { name: 'Retry', exact: true }).click();
  await page.getByText(/Updates interrupted. Showing the last successful snapshot/).waitFor({ state: 'hidden' });
  await dashboard.getByRole('button', { name: /Positions/ }).click();
  await dashboard.getByRole('button', { name: 'Stop strategy', exact: true }).click();
  await dashboard.getByRole('button', { name: 'Start strategy', exact: true }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(200);
  await page.screenshot({ path: artifacts + '/mobile.png', fullPage: true });
  assert.equal(await dashboard.evaluate((el) => el.scrollWidth <= el.clientWidth + 1), true, 'mobile dashboard has no horizontal overflow');
  await page.getByRole('link', { name: 'Back to paper backtests' }).click();
  await dashboard.waitFor({ state: 'hidden' });
  await page.getByRole('link', { name: 'Open strategy dashboard' }).waitFor();
  assert.equal(await page.evaluate(() => window.paperSources.every((s) => s.closed)), true, 'streams close with the dashboard');
  const finalRequests = requests;
  await page.waitForTimeout(3300);
  assert.equal(requests, finalRequests, 'polling stops when dashboard closes');
  session = { ...session, stats: null, running: false };
  trades.length = 0;
  await page.getByRole('link', { name: 'Open strategy dashboard' }).click();
  await dashboard.getByText('Pending', { exact: true }).waitFor();
  await dashboard.getByText('No open positions', { exact: true }).waitFor();
  session = { ...session, running: true };
  await dashboard.getByRole('button', { name: 'Refresh dashboard', exact: true }).click();
  await dashboard.getByText('Running', { exact: true }).waitFor();
  assert.equal(await dashboard.getByRole('button', { name: 'Run now', exact: true }).isDisabled(), true);
  session = { ...session, running: false, status: 'error', last_error: 'Provider unavailable' };
  await dashboard.getByRole('button', { name: 'Refresh dashboard', exact: true }).click();
  await dashboard.getByText('Needs attention', { exact: true }).waitFor();
  await dashboard.getByRole('alert').getByText('Provider unavailable').waitFor();
  assert.deepEqual(errors, []);
  console.log('Paper dashboard browser checks passed: layout, chart, tabs, start/stop, failures, mobile and cleanup.');
} finally { await browser.close(); }
