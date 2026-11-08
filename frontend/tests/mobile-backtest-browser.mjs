// Backtest mobile check: /backtest (builder, library, finished run), optimizer, paper and report.
// Fixtures only, no backend: node tests/mobile-backtest-browser.mjs [base URL] [baseline URL]
// The baseline server (clean HEAD) is only used for the desktop screenshot comparison.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { chromium } from 'playwright';

const { PNG } = createRequire(import.meta.url)('playwright-core/lib/utilsBundle');
/** Changed pixels between two screenshots and the box they fall in. */
function pixelDiff(x, y) {
  const a = PNG.sync.read(x);
  const b = PNG.sync.read(y);
  if (a.width !== b.width || a.height !== b.height) return { n: -1, text: 'size differs' };
  let n = 0, x0 = Infinity, y0 = Infinity, x1 = -1, y1 = -1;
  for (let j = 0; j < a.height; j++) for (let i = 0; i < a.width; i++) {
    const k = (j * a.width + i) * 4;
    if (a.data[k] !== b.data[k] || a.data[k + 1] !== b.data[k + 1] || a.data[k + 2] !== b.data[k + 2]) {
      n++; x0 = Math.min(x0, i); y0 = Math.min(y0, j); x1 = Math.max(x1, i); y1 = Math.max(y1, j);
    }
  }
  return { n, text: n ? `${n} px in ${x0},${y0}..${x1},${y1}` : '0 px' };
}

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const baseline = process.argv[3] ?? 'http://127.0.0.1:4190';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-backtest/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });

const VIEWPORTS = [[320, 568], [390, 844], [600, 800], [667, 375], [768, 1024]];
const DESKTOP = [[1440, 900], [1100, 800]];
const FIXED = new Date('2026-10-15T12:00:00Z');
const T0 = Date.UTC(2026, 0, 1);
const day = (i) => new Date(T0 + i * 86400000).toISOString();

// ── Fixtures ──
const datasets = [
  { id: 'ds-aapl', provider: 'fixture-provider-with-a-long-name', asset_type: 'equity', ticker: 'AAPL', timeframe: '1d', bar_count: 160, range_from: day(0), range_to: day(159) },
  { id: 'ds-brk', provider: 'fixture', asset_type: 'equity', ticker: 'BRK.B-LONG-INSTRUMENT-NAME', timeframe: '1d', bar_count: 160, range_from: day(0), range_to: day(159) }
];
const close = Array.from({ length: 160 }, (_, i) => 180 + Math.sin(i / 9) * 12 + i * 0.2);
const bars = {
  count: 160, ts: close.map((_, i) => day(i)), provider: 'fixture',
  o: close.map((v) => v - 0.8), h: close.map((v) => v + 1.6), l: close.map((v) => v - 1.9), c: close,
  v: close.map((_, i) => 1200000 + i * 900)
};
const side = (n) => ({ net_pnl: n, gross_profit: 18450.12, gross_loss: 9120.55, profit_factor: 2.02, total_fees: 412.33, trades: 24, wins: 15, losses: 9, win_rate: 62.5, avg_trade: n / 24, avg_win: 1230, avg_loss: 1013.4, payoff_ratio: 1.21, expectancy_pct: 0.412, largest_win: 4210.9, largest_loss: 2988.12, max_consec_wins: 5, max_consec_losses: 3, avg_bars_held: 4.7 });
const stats = {
  return_pct: -12.348, buy_hold_return_pct: 18.2, net_pnl: -123456.78, total_fees: 412.33, final_equity: 876543.22,
  win_rate: 62.5, wins: 15, losses: 9, trades: 24, profit_factor: 2.02, expectancy_pct: 0.412,
  max_drawdown_pct: 23.81, max_drawdown: 248112.4, sharpe: -0.42, sortino: -0.61,
  exit_reasons: { take_profit: 11, stop_loss: 9, signal: 4 },
  all: side(-123456.78), long: side(9330.11), short: side(-132786.89)
};
const trades = Array.from({ length: 24 }, (_, k) => {
  const e = 10 + k * 6;
  const win = k % 3 !== 2;
  return {
    direction: k % 4 ? 'long' : 'short', entry_ts: day(e), exit_ts: day(e + 4),
    entry_price: close[e], exit_price: close[e + 4], qty: 1234.5678, bars_held: 4,
    pnl: win ? 1230.45 : -2988.12, return_pct: win ? 1.24 : -3.1, fees: 17.2, mae: 412.5, mfe: 1890.25,
    exit_reason: win ? 'take_profit' : 'stop_loss', entries: k % 5 ? 1 : 2
  };
});
const result = {
  run_id: 'run-new', ticker: 'AAPL', timeframe: '1d', bars: 160, warmup_bars: 20, trading_start_ts: day(20),
  stats, trades, per_asset: [],
  equity: close.map((_, i) => ({ ts: day(i), equity: 1000000 - i * 700 })), benchmark: [],
  execution: { orders_placed: 52, orders_filled: 48, orders_expired: 4, ambiguous_bars: 3, fills: 48, quoted_fills: 0 },
  warnings: ['Commission schedule ends before the last bar: the remaining fills used the final tier.'],
  skipped_min_size: 2, filtered_bars: 14, data_gaps: []
};
const run = (id, name, ret) => ({ id, name, pinned: id === 'run-1', ticker: 'AAPL', timeframe: '1d', dataset_ids: ['ds-aapl'], settings: { kind: 'signals' }, stats: { return_pct: ret, trades: 24 }, bars_from: day(0), bars_to: day(159), created_at: day(150) });
const runs = [
  run('run-1', 'Momentum crossover with a deliberately long descriptive name for narrow phones', -12.35),
  run('run-2', 'Mean reversion', 4.2)
];
const strategies = [
  { id: 'st-1', name: 'Trend following on daily candles with volatility filter and a long name', tags: ['trend', 'daily', 'volatility-filter'], updated_at: day(140), settings: { kind: 'signals' } },
  { id: 'st-2', name: 'Grid', tags: [], updated_at: day(120), settings: { kind: 'grid' } }
];
const paperAt = new Date(FIXED.getTime() - 60000).toISOString();
const session = {
  window_bars: 5000, start_ts: null, feed: 'hist', timezone: 'UTC', at_hour: 9, at_minute: 0, weekdays: 31, day_of_month: 1, run_at: null,
  notify_when: 'on_change', cadence: 'each', channel_ids: [], notify_open: true, notify_close: true, notify_tickers: [],
  entry_title_template: '', entry_template: '', exit_title_template: '', exit_template: '', summary_title_template: '', summary_template: '',
  id: 'fixture-id', name: 'Bitcoin momentum, trend following with a very long session name', status: 'active', running: false, kind: 'interval', every_minutes: 5,
  dataset_ids: ['ds-aapl'], settings: { kind: 'signals', starting_capital: 1000000 }, last_run_at: paperAt, next_run_at: new Date(FIXED.getTime() + 240000).toISOString(),
  stats: { net_pnl: -1642.8, open_positions: [{ ticker: 'AAPL', direction: 'long', entry_ts: paperAt, avg_price: 192.3311, qty: 1234.5678, stop: 180.5, take_profit: 210 }],
    pending_orders: [{ ticker: 'AAPL', direction: 'short', kind: 'entry', order: 'limit', qty: 500, amount: 98000, price: 196.42, bars_left: 3, signal_ts: paperAt }] }
};
const paperTrades = Array.from({ length: 8 }, (_, n) => ({ open: false, ticker: 'AAPL', direction: n % 3 ? 'long' : 'short', entry_ts: day(100 + n), exit_ts: day(101 + n), pnl: n % 3 ? 1220.4 : -3020.3, fees: 2.4, trade: { qty: 1234.5678, entry_price: 190.12, exit_price: 191.33, exit_reason: 'take_profit', return_pct: n % 3 ? 1.81 : -0.31, mae: 40, mfe: 150 } }));
const dashboard = (empty) => ({
  session: empty ? { ...session, stats: { open_positions: [], pending_orders: [] } } : session,
  instruments: [{ ...datasets[0], price: 193.1234, price_at: paperAt, stream: false }],
  trades: empty ? [] : paperTrades, live_fills: [],
  events: empty ? [] : [{ id: 'ev-1', kind: 'error', at: paperAt, title: 'AAPL, order rejected by the simulated venue', message: 'Order size 1,234.5678 exceeds the configured maximum position of 1,000 units.' }]
});
const axes = [
  { path: 'long.entry.conditions.0.left.period', label: 'Fast moving average period on the entry signal', unit: '', values: [5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16] },
  { path: 'long.stop_loss_pct', label: 'Stop loss', unit: '%', values: [1.5, 1.75, 2, 2.25, 2.5, 2.75, 3, 3.25, 3.5, 3.75, 4, 4.25] }
];
const optRows = Array.from({ length: 12 }, (_, k) => ({ i: k, rank: k + 1, params: [5 + k, 1.5 + k / 4], sharpe: 1.9 - k * 0.3, sortino: 2.1 - k * 0.3, return_pct: 31.2 - k * 6, net_pnl: 312000 - k * 61000, profit_factor: 1.8 - k * 0.1, win_rate: 58 - k, expectancy_pct: 0.3 - k * 0.05, avg_trade: 1300 - k * 260, max_drawdown_pct: 8 + k, trades: 40 + k }));
const optJob = (empty) => ({
  job: { id: 'fixture-id', ticker: 'AAPL', timeframe: '1d', bars: 160, workers: 8, axes, settings: { kind: 'signals', long: { entry: { conditions: [{ left: { period: 10 } }] } } }, dataset_ids: ['ds-aapl'] },
  progress: { status: 'done', done: empty ? 0 : 12, total: 12, elapsed_ms: 8423, per_trial_ms: 702.4, failed: empty ? 0 : 1, error: 'One variant had no trades' },
  rows: empty ? [] : optRows, row_count: empty ? 0 : optRows.length, sort: 'sharpe', dir: 'desc',
  sensitivity: empty ? [] : axes.map((a, k) => ({ path: a.path, label: a.label, values: [0, 1, 2].map((j) => ({ value: k ? 1.5 + j : 5 + j, avg: 0.4 - j * 0.3, best: 1.9 - j * 0.2, n: 4 })) })),
  distribution: empty ? null : { n: 12, min: -1.4, max: 1.9, mean: 0.25, positive: 7, buckets: [1, 2, 3, 3, 2, 1] },
  deflated_sharpe: empty ? null : { best_sharpe: 1.9, expected_max_sharpe: 1.2, deflated_sharpe: 0.62, trials: 12, selection_explains_it: false }
});

function fixtures(state) {
  return async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const p = url.pathname;
    const fail = () => route.fulfill({ status: 500, json: { error: 'Fixture server error: the backtest service is unavailable' } });
    let json = {};
    if (p === '/api/setup/status') json = { configured: true };
    else if (p === '/api/settings/me') json = { username: 'fixture', role: 'admin', locale: 'en' };
    else if (p === '/api/settings/modules') json = { installed: ['backtest', 'histdata', 'histviz'] };
    else if (p === '/api/settings/defaults') json = { default_timezone: 'UTC', display_currency: 'USD' };
    else if (p === '/api/settings/version') json = { version: 'fixture' };
    else if (p === '/api/settings/versioning') json = { modules: {} };
    else if (p === '/api/health') json = { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
    else if (p === '/api/dashboard/layout') json = { pages: [] };
    else if (p === '/api/tasks') json = { tasks: [] };
    else if (p.includes('/notifications')) json = { notifications: [], unread: 0 };
    else if (p === '/api/histdata/datasets') {
      if (state === 'error') return fail();
      json = { datasets: state === 'empty' ? [] : datasets };
    } else if (/^\/api\/histdata\/datasets\/[^/]+\/bars$/.test(p)) json = bars;
    else if (p === '/api/backtest/runs' && req.method() === 'GET') {
      if (state === 'error') return fail();
      json = { runs: state === 'empty' ? [] : runs };
    } else if (p === '/api/backtest/strategies') json = { strategies: state === 'empty' ? [] : strategies };
    else if (p === '/api/backtest/indicators') json = { indicators: [] };
    else if (p === '/api/backtest/optimize') json = { jobs: [] };
    else if (p === '/api/backtest/paper') json = { sessions: state === 'empty' ? [] : [session] };
    else if (p === '/api/backtest/paper/events') json = { events: [] };
    else if (p === '/api/notif-channels') json = { channels: [{ id: 'ch-1', name: 'Trading desk alerts with a long channel name', kind: 'telegram' }] };
    else if (p === '/api/automator/timezones') json = { timezones: ['UTC', 'America/Argentina/ComodRivadavia', 'Europe/Paris'] };
    else if (p === '/api/backtest/paper/variables') json = { variables: [], filters: [] };
    else if (p === '/api/backtest/paper/preview') json = { title: 'AAPL closed', body: 'Closed at 191.33' };
    else if (p === '/api/backtest/align') json = { alignment: null };
    else if (p === '/api/backtest/execution-data') json = { datasets: [] };
    else if (p === '/api/backtest/run' && req.method() === 'POST') {
      captured.push({ base: url.origin, body: req.postDataJSON() });
      json = state === 'empty' ? { ...result, trades: [], stats: { ...stats, trades: 0, exit_reasons: {} } } : result;
    } else if (p === '/api/backtest/paper/fixture-id/dashboard') {
      if (state === 'error') return fail();
      json = dashboard(state === 'empty');
    } else if (p === '/api/backtest/optimize/fixture-id' && req.method() === 'GET') {
      if (state === 'error') return fail();
      json = optJob(state === 'empty');
    } else if (p === '/api/backtest/optimize/fixture-id/cloud') {
      json = { total: 12, matched: 12, skipped: 0, points: optRows.map((r) => [r.i, r.params[0], r.params[1], r.sharpe, r.return_pct]), bounds: [[5, 16], [1.5, 4.25], [-1.4, 1.9]] };
    } else if (p === '/api/histviz/chart-settings') json = {};
    await route.fulfill({ json });
  };
}
const captured = [];

// ── Helpers ──
async function newPage(state, { width = 390, height = 844, notice = true, at = base } = {}) {
  const context = await browser.newContext({ viewport: { width, height }, locale: 'en-US', hasTouch: width < 1100 });
  if (notice) await context.addInitScript(() => localStorage.setItem('otw.simNotice', JSON.stringify({ backtest: 'fixture', paper: 'fixture' })));
  await context.route('**/api/**', fixtures(state));
  const page = await context.newPage();
  page.setDefaultTimeout(15000);
  page.errors = [];
  page.on('pageerror', (e) => page.errors.push(e.message));
  page.base = at;
  return page;
}
async function goto(page, path, ready) {
  for (let attempt = 0; ; attempt++) {
    try {
      await page.goto(page.base + path);
      await page.locator(ready).first().waitFor({ timeout: 15000 });
      return;
    } catch (e) {
      if (attempt) throw e;
    }
  }
}
// The document must not pan sideways, and nothing may sit past the right edge unless a local
// region (a table or chart scroller, or a deliberate clip such as an ellipsis) holds it. The
// route's own page scroller and its ancestors do not count as such a region: content they clip
// or scroll sideways is content a phone cannot read.
const ROOTS = '.page, [data-paper-dashboard], .scroll:has(> .report)';
// Panes that scroll vertically as the page body: overflow-y makes their overflow-x auto too,
// but a reader does not pan them sideways either.
const PANES = '.result-body, .step-body, .dock-body, .page > .body';
async function noOverflow(page, label) {
  const r = await page.evaluate(([roots, panes]) => {
    const doc = document.documentElement;
    const vw = doc.clientWidth;
    const pageLevel = (el) => el === document.body || el === doc || el.matches(roots) || el.matches(panes) || !!el.querySelector(roots);
    const contained = (el) => {
      for (let a = el.parentElement; a; a = a.parentElement) {
        if (pageLevel(a)) return false;
        const ox = getComputedStyle(a).overflowX;
        if (ox !== 'visible' && a.getBoundingClientRect().right <= vw + 1) return true;
      }
      return false;
    };
    const wide = [...document.querySelectorAll('body *')].filter((el) => {
      const b = el.getBoundingClientRect();
      if (!b.width || !b.height || b.right <= vw + 1) return false;
      const cs = getComputedStyle(el);
      if (cs.visibility === 'hidden' || cs.position === 'fixed') return false;
      return !contained(el);
    }).slice(0, 5).map((el) => `${el.tagName.toLowerCase()}.${[...el.classList].join('.')} ${Math.round(el.getBoundingClientRect().right)}`);
    return { scroll: doc.scrollWidth, client: vw, wide };
  }, [ROOTS, PANES]);
  assert.ok(r.scroll <= r.client + 1, `${label}: page overflows (${r.scroll} > ${r.client}) ${r.wide.join(', ')}`);
  assert.deepEqual(r.wide, [], `${label}: content past the right edge`);
}
async function reachable(page, locator, label) {
  const el = locator.first();
  await el.scrollIntoViewIfNeeded();
  const ok = await el.evaluate((node) => {
    const b = node.getBoundingClientRect();
    const x = b.left + b.width / 2;
    const y = b.top + b.height / 2;
    const vw = document.documentElement.clientWidth;
    const vh = window.visualViewport?.height ?? innerHeight;
    if (!(x >= 0 && x <= vw && y >= 0 && y <= vh)) return `center ${Math.round(x)},${Math.round(y)} outside ${vw}x${Math.round(vh)}`;
    const hit = document.elementFromPoint(x, y);
    if (!(hit && (hit === node || node.contains(hit)))) return `covered by ${hit?.tagName.toLowerCase()}.${[...(hit?.classList ?? [])].join('.')}`;
    return '';
  });
  assert.equal(ok, '', `${label} not reachable`);
}
const shot = (page, name) => page.screenshot({ path: `${artifacts}/${name}.png`, fullPage: true });
const shape = (v) => Array.isArray(v) ? (v.length ? [shape(v[0])] : []) : v && typeof v === 'object'
  ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, shape(v[k])])) : typeof v;

async function runByTap(page) {
  await page.locator('.steps button').first().tap();
  await page.locator('.picker .trigger').first().tap();
  await page.locator('.picker .menu .pick').first().tap();
  await page.locator('.dchip').first().waitFor();
  const runButton = page.locator('.step-foot .run');
  await reachable(page, runButton, 'Run');
  await runButton.tap();
}

const results = [];
const done = (line) => { results.push(line); console.log(`ok  ${line}`); };

try {
  // ── /backtest builder and library: populated, empty and error ──
  for (const state of ['populated', 'empty', 'error']) {
    const page = await newPage(state);
    await goto(page, '/backtest', '.step-foot .run');
    if (state === 'error') await page.locator('.err').waitFor();
    for (const [w, h] of VIEWPORTS) {
      await page.setViewportSize({ width: w, height: h });
      await page.locator('.mobile-sections button').first().click().catch(() => {});
      const tag = `backtest-${state}-${w}x${h}`;
      await noOverflow(page, tag);
      if (w < 768) {
        await reachable(page, page.locator('.mobile-sections button').first(), `${tag} configure switch`);
        await reachable(page, page.locator('.mobile-sections button').nth(1), `${tag} library switch`);
      }
      await reachable(page, page.locator('.top .acts button.primary'), `${tag} Save strategy`);
      await reachable(page, page.locator('.top .acts button').filter({ hasText: 'New' }), `${tag} New strategy`);
      for (const [i, id] of ['data', 'strategy', 'filters', 'sizing', 'advanced'].entries()) {
        const stepButton = page.locator('.steps button').nth(i);
        await reachable(page, stepButton, `${tag} step ${id}`);
        await stepButton.click();
        await noOverflow(page, `${tag} step ${id}`);
        if (w === 320 && state === 'populated') await shot(page, `${tag}-step-${id}`);
      }
      await reachable(page, page.locator('.step-foot .run'), `${tag} Run`);
      await reachable(page, page.locator('.step-foot .nav').first(), `${tag} Previous`);
      await shot(page, `${tag}-configure`);
      if (w < 768) {
        await page.locator('.mobile-sections button').nth(1).click();
        for (const name of ['History', 'Strategies', 'Paper']) {
          const tab = page.locator('.dock-bar [role="tab"]').filter({ hasText: name });
          await reachable(page, tab, `${tag} dock ${name}`);
          await tab.click();
          await noOverflow(page, `${tag} dock ${name}`);
          if (state === 'populated') {
            const rowAct = page.locator('.dock-body .act, .dock-body button').first();
            await reachable(page, rowAct, `${tag} dock ${name} row action`);
          }
        }
        await shot(page, `${tag}-library`);
        await page.locator('.mobile-sections button').first().click();
      }
    }
    assert.deepEqual(page.errors, [], `backtest ${state} runtime errors`);
    await page.context().close();
    done(`/backtest ${state}: builder, steps and library at ${VIEWPORTS.map((v) => v.join('x')).join(', ')}`);
  }

  // ── Back from a paper dashboard lands on the paper list, also on a phone ──
  {
    const page = await newPage('populated');
    await goto(page, '/backtest/paper/fixture-id', '.hero');
    await page.locator('.page-bar .back').tap();
    const open = page.locator('.dock-body a.dashboard').first();
    await open.waitFor();
    await reachable(page, open, 'paper list after Back');
    await reachable(page, page.locator('.dock-body .danger').first(), 'paper delete');
    await page.locator('.dock-body .danger').first().tap();
    const confirm = page.getByRole('dialog');
    await confirm.waitFor();
    await reachable(page, confirm.getByRole('button', { name: 'Delete' }), 'paper delete confirmation');
    await confirm.getByRole('button', { name: 'Cancel' }).tap();
    assert.deepEqual(page.errors, [], 'paper back runtime errors');
    await page.context().close();
    done('/backtest?paper=1 at 390 px: Back lands on the paper list; delete keeps its confirmation');
  }

  // ── /backtest run by tap at 390 px, through the one-time notice ──
  {
    const page = await newPage('populated', { notice: false });
    await goto(page, '/backtest', '.step-foot .run');
    captured.length = 0;
    await runByTap(page);
    const ok = page.getByRole('button', { name: 'Understood' });
    await ok.waitFor();
    await reachable(page, ok, 'Simulation notice Understood');
    await ok.tap();
    await page.locator('.result-head').waitFor();
    assert.equal(captured.length, 1, 'one run request');
    const mobileBody = captured[0].body;
    assert.deepEqual(mobileBody.dataset_ids, ['ds-aapl']);
    assert.equal(mobileBody.record, true);

    // Same flow at desktop by mouse: the payload must have the same shape.
    const desk = await newPage('populated', { width: 1440, height: 900 });
    await goto(desk, '/backtest', '.step-foot .run');
    await desk.locator('.picker .trigger').first().click();
    await desk.locator('.picker .menu .pick').first().click();
    await desk.locator('.step-foot .run').click();
    await desk.locator('.result-head').waitFor();
    const deskBody = captured[1].body;
    assert.deepEqual(shape(mobileBody), shape(deskBody), 'mobile and desktop run payloads share one shape');
    assert.deepEqual(mobileBody, deskBody, 'mobile and desktop run payloads are identical');
    await desk.context().close();
    done('/backtest Run by tap at 390 px: payload identical to the desktop click flow');

    // ── Finished run: every result pane at every viewport ──
    const tabs = ['Performance summary', 'Statistics', 'Performance', 'Scatter 3D', 'Streaks 3D', 'List of trades'];
    for (const [w, h] of VIEWPORTS) {
      await page.setViewportSize({ width: w, height: h });
      const tag = `result-${w}x${h}`;
      await page.waitForTimeout(150);
      await noOverflow(page, tag);
      for (const name of ['Edit strategy', 'Hide chart', 'Indicators', 'Optimizer', 'Paper trade', 'Reports', 'Save results']) {
        const button = page.locator('.result-head button').filter({ hasText: name });
        if (!(await button.count())) continue;
        await reachable(page, button, `${tag} ${name}`);
      }
      await reachable(page, page.locator('.chart-pane'), `${tag} chart`);
      const chartBox = await page.locator('.chart-pane').boundingBox();
      assert.ok(chartBox.height >= 200, `${tag} chart is readable (${Math.round(chartBox.height)} px)`);
      await reachable(page, page.locator('.result-inner .badges').first(), `${tag} order and log badges`);
      for (const name of tabs) {
        const tab = page.locator('.result-inner').getByRole('tab', { name: new RegExp(`^${name}( \\d+)?$`) });
        await reachable(page, tab, `${tag} tab ${name}`);
        await tab.click();
        await page.waitForTimeout(120);
        await noOverflow(page, `${tag} tab ${name}`);
      }
      const region = page.getByRole('region', { name: 'List of trades' });
      await region.waitFor();
      await reachable(page, region, `${tag} trades region`);
      const scroller = await region.evaluate((el) => ({ sw: el.scrollWidth, cw: el.clientWidth, overflow: getComputedStyle(el).overflowX }));
      assert.ok(scroller.overflow === 'auto' && scroller.cw <= w, `${tag} trades table scrolls inside its own region`);
      await reachable(page, page.locator('.chips .chip').nth(1), `${tag} trade filter chip`);
      await region.locator('tbody tr').first().tap();
      await shot(page, `${tag}-trades`);
      await page.locator('.result-inner [role="tab"]').first().click();
      await shot(page, `${tag}-summary`);
      // Hide and show the chart: the tap path that replaces the drag on the split.
      await page.locator('.result-head button').filter({ hasText: 'Hide chart' }).tap();
      await page.locator('.result-head button').filter({ hasText: 'Show chart' }).tap();
      await page.locator('.chart-pane').waitFor();
      // The report menu opens by tap and its two items stay inside the viewport.
      await page.locator('.result-head .menu-wrap > button').tap();
      for (const item of await page.locator('.menu-wrap .menu [role="menuitem"]').all()) await reachable(page, item, `${tag} report menu item`);
      await page.locator('.result-head .menu-wrap > button').tap();
    }
    // The Save results dialog opens and its Save button is reachable at 320 px.
    await page.setViewportSize({ width: 320, height: 568 });
    await page.locator('.result-head button').filter({ hasText: 'Save results' }).tap();
    const dialog = page.getByRole('dialog');
    await dialog.waitFor();
    await reachable(page, dialog.getByRole('button', { name: 'Save' }), 'Save results dialog Save');
    await page.keyboard.press('Escape');
    await page.locator('.result-head .back').tap();
    await page.locator('.step-foot .run').waitFor();
    assert.deepEqual(page.errors, [], 'result runtime errors');
    await page.context().close();
    done('/backtest finished run: metrics, chart, orders/log badges, every tab, labelled trades region, report menu, save dialog');
  }

  // ── Optimizer ──
  for (const state of ['populated', 'empty', 'error']) {
    const page = await newPage(state);
    await goto(page, '/backtest/optimize/fixture-id', state === 'error' ? '.err' : '.tabs');
    for (const [w, h] of VIEWPORTS) {
      await page.setViewportSize({ width: w, height: h });
      const tag = `optimize-${state}-${w}x${h}`;
      await noOverflow(page, tag);
      await reachable(page, page.locator('.top .back'), `${tag} back`);
      if (state === 'error') { await shot(page, tag); continue; }
      await reachable(page, page.locator('.top .acts button'), `${tag} forget`);
      await page.locator('.tabs button').first().click();
      if (state === 'populated') {
        const scroller = page.getByRole('region', { name: 'Ranking' });
        await reachable(page, scroller, `${tag} ranking region`);
        await reachable(page, page.locator('.rank-link').first(), `${tag} inspect row`);
        await reachable(page, page.locator('.pager .btn').last(), `${tag} pager`);
        await reachable(page, page.locator('th.metric button').first(), `${tag} sort`);
      }
      await shot(page, `${tag}-ranking`);
      await page.locator('.tabs button').nth(1).click();
      await page.waitForTimeout(150);
      await noOverflow(page, `${tag} analysis`);
      await shot(page, `${tag}-analysis`);
      await page.locator('.tabs button').nth(2).click();
      await page.waitForTimeout(250);
      await noOverflow(page, `${tag} cloud`);
      await shot(page, `${tag}-cloud`);
      if (state === 'populated') {
        await page.locator('.tabs button').first().click();
        await page.locator('.rank-link').nth(1).tap();
        await page.locator('.variant .keep').waitFor();
        await page.waitForTimeout(150);
        await noOverflow(page, `${tag} variant`);
        await reachable(page, page.locator('.variant .keep'), `${tag} keep run`);
        await shot(page, `${tag}-variant`);
      }
    }
    assert.deepEqual(page.errors, [], `optimizer ${state} runtime errors`);
    await page.context().close();
    done(`/backtest/optimize/[id] ${state}: ranking region, inspect, analysis, cloud, variant and keep`);
  }

  // ── Paper dashboard ──
  for (const state of ['populated', 'empty', 'error']) {
    const page = await newPage(state);
    await goto(page, '/backtest/paper/fixture-id', state === 'error' ? '.notice.error' : '.hero');
    for (const [w, h] of VIEWPORTS) {
      await page.setViewportSize({ width: w, height: h });
      const tag = `paper-${state}-${w}x${h}`;
      await noOverflow(page, tag);
      await reachable(page, page.locator('.page-bar .back'), `${tag} back`);
      if (state === 'error') {
        await reachable(page, page.getByRole('button', { name: 'Retry' }), `${tag} retry`);
        await shot(page, tag);
        continue;
      }
      await reachable(page, page.getByRole('button', { name: 'Session settings', exact: true }), `${tag} settings`);
      await reachable(page, page.getByRole('button', { name: 'Run now' }), `${tag} run now`);
      await reachable(page, page.getByRole('button', { name: 'Stop strategy' }), `${tag} stop`);
      await reachable(page, page.getByRole('button', { name: 'Refresh dashboard' }), `${tag} refresh`);
      await reachable(page, page.locator('.metric').last(), `${tag} metrics`);
      for (const tab of ['Positions', 'Pending orders', 'Trade history', 'Activity']) {
        const button = page.locator('.book .tabs button').filter({ hasText: tab });
        await reachable(page, button, `${tag} ${tab}`);
        await button.click();
        await noOverflow(page, `${tag} ${tab}`);
        const region = page.locator('.book .table-scroll');
        if (state === 'populated' && tab !== 'Activity') await reachable(page, region, `${tag} ${tab} region`);
      }
      await shot(page, `${tag}`);
    }
    if (state === 'populated') {
      // The settings dialog keeps its delete confirmation, if it has one, and its Save reachable.
      await page.setViewportSize({ width: 320, height: 568 });
      await page.getByRole('button', { name: 'Session settings', exact: true }).tap();
      const dialog = page.getByRole('dialog');
      await dialog.waitFor();
      await page.waitForTimeout(200);
      await noOverflow(page, 'paper settings dialog 320');
      await reachable(page, dialog.locator('footer button, .footer button').last(), 'paper settings dialog primary action');
      await shot(page, 'paper-settings-dialog-320x568');
      await page.keyboard.press('Escape');
    }
    assert.deepEqual(page.errors, [], `paper ${state} runtime errors`);
    await page.context().close();
    done(`/backtest/paper/[id] ${state}: controls, metrics, orders, history and activity`);
  }

  // ── Report ──
  for (const state of ['populated', 'empty', 'error']) {
    const page = await newPage(state);
    // Empty: the run list no longer holds this run, which the report states as not found.
    await goto(page, '/backtest/report/run-1', state === 'populated' ? '.doc-head' : '.report .err');
    for (const [w, h] of VIEWPORTS) {
      await page.setViewportSize({ width: w, height: h });
      await page.waitForTimeout(200);
      const tag = `report-${state}-${w}x${h}`;
      await noOverflow(page, tag);
      await reachable(page, page.locator('.toolbar a').first(), `${tag} back`);
      if (state === 'populated') {
        await reachable(page, page.locator('.toolbar a[download]'), `${tag} raw export`);
        await reachable(page, page.locator('.toolbar button.primary'), `${tag} print`);
        await reachable(page, page.locator('.chart-box').first(), `${tag} chart`);
        const chartWidth = await page.locator('.chart-box').first().evaluate((el) => el.getBoundingClientRect().width);
        assert.ok(chartWidth <= w, `${tag} chart fits`);
      }
      await shot(page, tag);
    }
    assert.deepEqual(page.errors, [], `report ${state} runtime errors`);
    await page.context().close();
    done(`/backtest/report/[id] ${state}: toolbar, raw export, figures and long report`);
  }

  // ── Desktop no-regression: the same fixtures on the baseline server and on this one ──
  const scenes = [
    ['builder', '/backtest', '.step-foot .run', null],
    ['builder-empty', '/backtest', '.step-foot .run', null, 'empty'],
    ['result', '/backtest', '.step-foot .run', async (page) => {
      await page.locator('.picker .trigger').first().click();
      await page.locator('.picker .menu .pick').first().click();
      await page.locator('.step-foot .run').click();
      await page.locator('.result-head').waitFor();
    }],
    ['result-trades', '/backtest', '.step-foot .run', async (page) => {
      await page.locator('.picker .trigger').first().click();
      await page.locator('.picker .menu .pick').first().click();
      await page.locator('.step-foot .run').click();
      await page.locator('.result-inner [role="tab"]').last().click();
    }],
    ['optimize', '/backtest/optimize/fixture-id', '.rank-link', null],
    ['paper', '/backtest/paper/fixture-id', '.hero', null],
    ['report', '/backtest/report/run-1', '.doc-head', null]
  ];
  // A scene is a regression only when each server repeats its own picture and the two
  // pictures differ. A server that cannot repeat itself (a chart still settling) is noise,
  // reported with the region it moved in.
  const capture = async (at, w, h, [name, path, ready, act, state = 'populated'], k) => {
    const page = await newPage(state, { width: w, height: h, at });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.clock.setFixedTime(FIXED);
    await goto(page, path, ready);
    if (act) await act(page);
    await page.mouse.move(0, h - 1);
    await page.waitForTimeout(4000);
    const buf = await page.screenshot({ path: `${artifacts}/desktop-${name}-${w}-${at === base ? 'work' : 'baseline'}${k ? `-${k}` : ''}.png` });
    await page.context().close();
    return buf;
  };
  const diffs = [];
  const noise = [];
  for (const [w, h] of DESKTOP) {
    for (const scene of scenes) {
      const tag = `${scene[0]} ${w}x${h}`;
      const before = [await capture(baseline, w, h, scene, 0)];
      const after = [await capture(base, w, h, scene, 0)];
      const match = () => before.some((x) => after.some((y) => x.equals(y)));
      for (let k = 2; k <= 3 && !match(); k++) {
        before.push(await capture(baseline, w, h, scene, k));
        after.push(await capture(base, w, h, scene, k));
      }
      if (match()) { console.log(`ok   desktop ${tag}${before.length > 1 ? ' (equal on retake)' : ''}`); continue; }
      const selfBase = before.slice(1).map((x) => pixelDiff(before[0], x)).find((d) => d.n) ?? { n: 0, text: '0 px' };
      const selfWork = after.slice(1).map((y) => pixelDiff(after[0], y)).find((d) => d.n) ?? { n: 0, text: '0 px' };
      const cross = pixelDiff(before[0], after[0]);
      if (selfBase.n || selfWork.n) {
        noise.push(`${tag}: baseline self ${selfBase.text}, work self ${selfWork.text}, cross ${cross.text}`);
        console.log(`NOISE desktop ${tag}: baseline self ${selfBase.text}, work self ${selfWork.text}, cross ${cross.text}`);
      } else {
        diffs.push(`${tag}: ${cross.text}`);
        console.log(`DIFF desktop ${tag}: ${cross.text}`);
      }
    }
  }
  assert.deepEqual(diffs, [], 'desktop screenshots must match the baseline');
  done(`desktop ${DESKTOP.map((v) => v.join('x')).join(', ')}: ${scenes.length} scenes, no repeatable difference from baseline${noise.length ? `; noise in ${noise.length}` : ''}`);
  console.log(`PASS: ${results.length} checks. Screenshots in ${artifacts}`);
} finally {
  await browser.close();
}
