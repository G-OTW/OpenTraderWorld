// Local fixtures only: node tests/robots-dashboard-browser.mjs [base URL]
// A robot's page on a fixed book, every API answered here: the header keeps the robot's state
// in sight, the live hub moves the price and the unrealized figure, each tab renders, a failed
// refresh keeps the last snapshot, a phone has no horizontal scroll, and the streams and the
// polling stop with the page.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:5173';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/robots-dashboard-tests/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const browser = await chromium.launch({ headless: true });
try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, locale: 'en-US' });
  await context.addInitScript(() => {
    window.robotSources = [];
    window.EventSource = class extends EventTarget {
      constructor(url) { super(); this.url = url; this.closed = false; window.robotSources.push(this); }
      close() { this.closed = true; }
      emit(type, payload) { this.dispatchEvent(new MessageEvent(type, { data: JSON.stringify(payload) })); }
    };
  });
  const page = await context.newPage();
  page.setDefaultTimeout(15000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  const at = new Date(Date.now() - 60000).toISOString();
  const instrument = { id: 'ds-a', provider: 'fixture', asset_type: 'crypto', ticker: 'BTCUSDT', timeframe: '1m', price: 68450.25, price_at: at, stream: true, chart_stream: true, stream_timeframe: '1m' };
  let session = {
    id: 'robot-a', name: 'Bitcoin momentum', status: 'active', running: false, kind: 'interval', every_minutes: 1, timezone: 'UTC',
    dataset_ids: ['ds-a'], execution: { target: 'broker', account: 'Demo paper', broker: 'paper', account_id: 'acct-a' }, settings: { kind: 'signals', starting_capital: 10000, instrument: { multiplier: 1 } },
    last_run_at: at, next_run_at: new Date(Date.now() + 45000).toISOString(), bars: 5000, priority: 20,
    capital: { allocation: '10000', currency: 'USDT', percent_in: '50' }, guards: [], guard_state: { scale: '0.5' }, strategy_id: null,
    stats: { open_positions: [{ ticker: 'BTCUSDT', direction: 'long', entry_ts: at, avg_price: 67200, qty: 0.12, stop: 66000, take_profit: 71000 }] }
  };
  const trades = Array.from({ length: 10 }, (_, n) => ({
    trade_key: `k${n}`, open: false, ticker: 'BTCUSDT', direction: 'long', entry_ts: new Date(Date.now() - (12 - n) * 3600000).toISOString(),
    exit_ts: new Date(Date.now() - (11 - n) * 3600000).toISOString(), pnl: n % 3 ? 120.4 : -20.3, fees: 2.4, robot_qty: '0.1', scale: '1',
    trade: { qty: 0.1, entry_price: 66500, exit_price: 67704, exit_reason: 'take_profit', return_pct: 1.81, mae: 40, mfe: 150 }
  }));
  const robot = () => ({
    environment: 'paper', start_equity: 10000, capital: session.capital, equity: 10900, realized: 760, unrealized: 150, curve: [{ at, equity: 10000 }, { at: new Date().toISOString(), equity: 10760 }],
    stats: { trades: 10, wins: 6, losses: 4, net_pnl: 760, win_rate: 60, profit_factor: 2.1, expectancy: 76, max_drawdown: 40, max_drawdown_pct: 0.4, fees: 24 },
    calendar: [{ date: at.slice(0, 10), net_pnl: 760, trades: 10, wins: 6, losses: 4 }],
    breakdown: { instrument: [{ key: 'BTCUSDT', trades: 10, net: 760, avg: 76 }], direction: [{ key: 'long', trades: 10, net: 760, avg: 76 }] },
    positions: [{ key: 'p1', ticker: 'BTCUSDT', direction: 'long', qty: 0.06, scale: '0.5', avg_price: 67200, price: 68450.25, price_at: at, unrealized: 75.01, stop: 66000, target: 71000, entry_ts: at, age_secs: 60 }],
    pending: [{ source: 'engine', ticker: 'BTCUSDT', direction: 'long', order: 'limit', qty: 0.05, price: 66500, bars_left: 3, signal_ts: at }],
    guards: { rules: [], state: session.guard_state, badge: 'x0.5', holds: null, scale: 0.5, described: ['divide the size by 2, restore after 5 trade(s) after a 10% drawdown from the peak'], timeline: [{ id: 'g1', at, action: { kind: 'scale', scale: '0.5' }, trigger: { drawdown_pct: 10.2 }, cleared_at: null }] },
    today: { orders_sent: 2, realized: 120.4, fees: 2.4 }, strategy: { id: null, name: '', version: null }
  });
  let requests = 0;
  let fail = false;
  await context.route('**/api/**', async (route) => {
    const req = route.request();
    const p = new URL(req.url()).pathname;
    let json = {};
    if (p === '/api/setup/status') json = { configured: true };
    else if (p === '/api/settings/me') json = { username: 'fixture', role: 'admin' };
    else if (p === '/api/settings/modules') json = { installed: ['backtest', 'histdata', 'histviz', 'robots'] };
    else if (p === '/api/settings/defaults') json = { default_timezone: 'UTC' };
    else if (p === '/api/health') json = { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
    else if (p === '/api/robots/robot-a/dashboard') {
      requests++;
      if (fail) return route.fulfill({ status: 503, json: { error: 'Connection interrupted' } });
      json = { session, instruments: [instrument], trades, live_fills: [], events: [{ id: 'e1', kind: 'guard', at, title: 'Bitcoin momentum: guard', message: 'Guard: new entries are sized x0.5.' }], robot: robot() };
    } else if (p === '/api/robots/fleet') json = { robots: [], overview: { active: 1, errored: 0, unseen: 0 } };
    else if (p === '/api/histdata/datasets/ds-a/bars') {
      const ts = Array.from({ length: 120 }, (_, n) => new Date(Date.now() - (120 - n) * 60000).toISOString());
      json = { ts, o: ts.map(() => 68000), h: ts.map(() => 68600), l: ts.map(() => 67800), c: ts.map(() => 68400), v: ts.map(() => 1) };
    } else if (p.includes('/notifications')) json = { notifications: [], unread: 0 };
    else if (p === '/api/histviz/chart-settings') json = null;
    await route.fulfill({ json });
  });

  await page.goto(base + '/robots/robot-a');
  const dashboard = page.locator('[data-robot-dashboard]');
  await dashboard.getByRole('heading', { name: session.name }).waitFor();
  await dashboard.getByText('Paper', { exact: true }).first().waitFor();
  await dashboard.getByText('x0.5', { exact: true }).first().waitFor();
  await dashboard.locator('[data-next-tick]').waitFor();
  await page.screenshot({ path: artifacts + '/overview.png', fullPage: true });
  // The page's stream and the chart's both carry BTCUSDT: the price reaches both.
  await page.evaluate(() => {
    for (const source of window.robotSources.filter((s) => !s.closed)) {
      source.emit('status', { i: 0, state: 'live' });
      source.emit('bar', { i: 0, c: 68500, lag_ms: 0 });
    }
  });
  await page.getByRole('tab', { name: /^Positions/ }).click();
  // 0.06 at 67,200 marked live at 68,500: +78.00.
  await dashboard.getByText('+78', { exact: true }).waitFor();
  for (const name of ['Pending', 'Trades', 'Guards', 'Log', 'Settings', 'Overview']) {
    await page.getByRole('tab', { name: new RegExp(`^${name}`) }).click();
    await page.screenshot({ path: artifacts + `/${name.toLowerCase()}.png` });
  }
  fail = true;
  await dashboard.getByRole('button', { name: 'Refresh dashboard', exact: true }).click();
  await dashboard.getByRole('alert').getByText(/Updates interrupted/).waitFor();
  await dashboard.getByRole('heading', { name: session.name }).waitFor();
  fail = false;
  await dashboard.getByRole('button', { name: 'Retry', exact: true }).click();
  await dashboard.getByRole('alert').getByText(/Updates interrupted/).waitFor({ state: 'hidden' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(200);
  await page.screenshot({ path: artifacts + '/mobile.png', fullPage: true });
  assert.equal(await dashboard.evaluate((el) => el.scrollWidth <= el.clientWidth + 1), true, 'a phone has no horizontal scroll');
  await page.getByRole('link', { name: 'All robots' }).click();
  await dashboard.waitFor({ state: 'hidden' });
  // The app's own notification stream stays: it is not the page's.
  const open = await page.evaluate(() => window.robotSources.filter((s) => !s.closed && s.url.startsWith('/api/robots/streams')).map((s) => s.url));
  assert.deepEqual(open, [], 'the price streams close with the page');
  const finalRequests = requests;
  await page.waitForTimeout(5500);
  assert.equal(requests, finalRequests, 'polling stops with the page');
  assert.deepEqual(errors, []);
  console.log('Robot dashboard browser checks passed: state in sight, live marks, tabs, failure kept, mobile and cleanup.');
} finally {
  await browser.close();
}
