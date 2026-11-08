// Script indicator end to end in the chart, against intercepted API fixtures (no backend):
// paste a script in the Script tab, add it, read the drawn ECharts series (plots, labels), open the warnings
// panel, change an input, reload and find the instance again. Then the realtime model: the
// first drawn bar runs after 300 stored warmup bars, and live ticks advance the worker session
// instead of reopening it. Run with the frontend dev server:
//   node tests/script-chart-browser.mjs http://127.0.0.1:5173
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:5173';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/script-chart-tests/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.WIDGET_BROWSER_EXECUTABLE, headless: true });
const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: 'en-US' });
const page = await context.newPage();
// Dev mode loads hundreds of modules: keep every resource entry so the echarts URL is found.
await page.addInitScript(() => performance.setResourceTimingBufferSize(10000));
// Count the ops the page posts to the script worker (open, advance, run, …).
await page.addInitScript(() => {
  window.__ops = [];
  const W = window.Worker;
  window.Worker = class extends W {
    postMessage(m, t) {
      if (m?.op) window.__ops.push(m.op);
      return super.postMessage(m, t);
    }
  };
});
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));

const SOURCE = `//@version=6
indicator("Fixture", overlay=true)
len = input.int(5, "Length", minval=1)
plot(ta.sma(close, len), "Avg")
plot(bar_index, "Idx")
bgcolor(close > open ? color.new(color.green, 90) : na)
label.new(bar_index, high, "x")
alertcondition(close > open, "Up", "up")
`;

// Daily bars ending today, so the newest one is forming and the pane goes live by itself.
const DAY = 86400000;
const today = Math.floor(Date.now() / DAY) * DAY;
const days = Array.from({ length: 120 }, (_, i) => new Date(today - (119 - i) * DAY).toISOString());
const close = days.map((_, i) => 100 + Math.sin(i / 8) * 5 + i * 0.15);
// 300 stored bars before the first drawn one: the script's warmup.
const warmDays = Array.from({ length: 300 }, (_, i) => new Date(today - (419 - i) * DAY).toISOString());
const warm = { ts: warmDays, o: warmDays.map(() => 90), h: warmDays.map(() => 91), l: warmDays.map(() => 89), c: warmDays.map(() => 90), v: warmDays.map(() => 1000) };
const empty = { ts: [], o: [], h: [], l: [], c: [], v: [] };
// Live events for today's bar, one per stream connection (the stream reconnects after each).
const liveQueue = [];
const bars = {
  count: days.length, ts: days,
  o: close.map((v, i) => v + (i % 2 ? 0.6 : -0.6)), h: close.map((v) => v + 1.2),
  l: close.map((v) => v - 1.4), c: close,
  v: close.map((_, i) => 100000 + i * 250),
  history_start: true, stream: true
};
// The workspace holds the charts and their studies: Save (Ctrl/Cmd+S) PUTs it, the next GET
// serves it back, so a reload finds the instance.
const workspace = {
  id: 'script-chart', name: 'Script chart', grid_rows: 1, grid_cols: 1,
  panes: [{ id: 'chart-1', coords: { provider: 'fixture', asset_type: 'equity', ticker: 'AAPL', timeframe: '1d' }, type: 'candlestick', instances: [] }],
  settings: { active: 'chart-1' }
};
await context.route('**/api/**', async (route) => {
  const req = route.request();
  const p = new URL(req.url()).pathname;
  let result = {};
  if (p === '/api/setup/status') result = { configured: true };
  else if (p === '/api/settings/me') result = { username: 'fixture', role: 'admin', locale: 'en' };
  else if (p === '/api/settings/modules') result = { installed: ['histviz', 'histdata', 'watchlists'] };
  else if (p === '/api/settings/defaults') result = { default_timezone: 'UTC', display_currency: 'USD' };
  else if (p === '/api/health') result = { status: 'ok', services: { core: 'ok' } };
  else if (p === '/api/dashboard/layout') result = { pages: [] };
  else if (p === '/api/tasks') result = { tasks: [] };
  else if (p.includes('/notifications')) result = { notifications: [], unread: 0 };
  else if (p === '/api/histdata/datasets') result = { datasets: [] };
  else if (p === '/api/histdata/instrument-meta') result = { meta: null };
  else if (p === '/api/connectors/providers') result = { providers: [] };
  else if (p === '/api/connectors') result = { connectors: [] };
  else if (p === '/api/backtest/indicators') result = { indicators: [] };
  else if (p === '/api/histviz/workspaces') result = { workspaces: [workspace] };
  else if (p === `/api/histviz/workspaces/${workspace.id}` && req.method() === 'PUT') {
    const body = JSON.parse(req.postData());
    Object.assign(workspace, { panes: body.panes, settings: body.settings });
    result = { ok: true };
  } else if (p === '/api/histviz/layout') result = null;
  else if (p === '/api/histviz/lists') result = { lists: [] };
  else if (p === '/api/histviz/alerts') result = { alerts: [] };
  else if (p === '/api/watchlists') result = { watchlists: [] };
  else if (p === '/api/histviz/series/batch') result = { results: [bars] };
  else if (p === '/api/histviz/series') {
    const b = JSON.parse(req.postData());
    result = b.stored_only && b.to === bars.ts[0] ? warm : empty;
  } else if (p === '/api/histviz/streams') {
    const e = liveQueue.shift();
    const body = e ? `event: ${e.closed ? 'bar' : 'snapshot'}\ndata: ${JSON.stringify({ i: 0, ...e })}\n\n` : '';
    return route.fulfill({ status: 200, headers: { 'content-type': 'text/event-stream' }, body });
  }
  await route.fulfill({ json: result });
});

/** The chart's ECharts option, read through the same module instance the app loaded. */
async function scriptSeries() {
  return page.evaluate(async () => {
    const url = performance.getEntriesByType('resource').map((e) => e.name).find((n) => /\/deps\/echarts\.js/.test(n));
    const echarts = await import(url);
    const el = document.querySelector('[_echarts_instance_]');
    const opt = echarts.getInstanceByDom(el).getOption();
    return opt.series.filter((s) => String(s.name ?? s.id ?? '').startsWith('script-')).map((s) => ({ name: s.name ?? s.id, data: s.data }));
  });
}
const avg = (list) => list.find((s) => /^script-\d+-p0(-\d+)?$/.test(s.name));
/** The script's series once drawn: the chart rebuilds a frame (at most ~120 ms) after the
 *  run lands, so the warnings badge can show before the series do. */
async function drawnSeries() {
  let list = await scriptSeries();
  for (let k = 0; k < 40 && !avg(list); k++) {
    await page.waitForTimeout(100);
    list = await scriptSeries();
  }
  return list;
}
const idx = (list) => list.find((s) => /^script-\d+-p1(-\d+)?$/.test(s.name));
const num = (d) => (Array.isArray(d) ? d[d.length - 1] : d && typeof d === 'object' ? d.value : d);
const lastValue = (s) => num(s.data.filter((d) => num(d) != null).at(-1));

try {
  await page.goto(`${base}/histviz`);
  await page.locator('[_echarts_instance_]').first().waitFor({ state: 'attached', timeout: 20000 });

  // Paste the script in the Script tab and add it.
  await page.getByRole('button', { name: 'Add an indicator' }).first().click();
  await page.getByRole('tab', { name: 'Pine Script (v5/v6 compatible)' }).click();
  await page.locator('.cm-content').waitFor({ timeout: 20000 });
  await page.locator('.cm-content').click();
  await page.keyboard.insertText(SOURCE);
  await page.getByText('Length', { exact: true }).waitFor({ timeout: 20000 });
  await page.getByRole('button', { name: 'Add', exact: true }).click();

  // Drawn: the plot series, the background and the newest 50 labels; one warning (alerts are
  // not run on the chart).
  await page.waitForFunction(() => document.querySelector('.sissue')?.textContent?.trim() === '1', null, { timeout: 20000 });
  let series = await drawnSeries();
  assert.ok(avg(series), `no script plot series: ${series.map((s) => s.name)}`);
  // Warmup: the first drawn bar is bar 300 of the script's run.
  assert.equal(num(idx(series).data.find((d) => num(d) != null)), 300, 'first drawn bar_index');
  assert.ok(series.some((s) => /-bg0$/.test(s.name)), 'no bgcolor series');
  assert.equal(series.find((s) => /-dt$/.test(s.name))?.data.length, 50, 'labels series');
  const before = JSON.stringify(avg(series).data);

  await page.locator('.sissue').click();
  await page.getByText(/alertcondition skipped/).waitFor();
  await page.keyboard.press('Escape');

  // A new input value reruns the script.
  const row = page.locator('.line.ind', { hasText: 'Fixture' });
  await row.hover();
  await row.getByRole('button', { name: 'Edit' }).click();
  const len = page.locator('.field', { hasText: 'Length' }).locator('input[type=number]');
  await len.fill('20');
  await page.locator('button.primary', { hasText: 'Save' }).click();
  await page.waitForFunction(
    async (prev) => {
      const url = performance.getEntriesByType('resource').map((e) => e.name).find((n) => /\/deps\/echarts\.js/.test(n));
      const echarts = await import(url);
      const opt = echarts.getInstanceByDom(document.querySelector('[_echarts_instance_]')).getOption();
      const s = opt.series.find((x) => /^script-\d+-p0(-\d+)?$/.test(String(x.name ?? x.id)));
      return s && JSON.stringify(s.data) !== prev;
    },
    before,
    { timeout: 20000 }
  );

  // Save the workspace, reload: the instance comes back and draws again.
  await page.keyboard.press('Control+s');
  await page.waitForTimeout(800);
  const saved = workspace.panes[0].instances?.find((i) => i.type === 'script');
  assert.ok(saved, 'workspace not saved');
  assert.equal(saved.script.inputs.len, 20);
  await page.reload();
  await page.waitForFunction(() => document.querySelector('.sissue')?.textContent?.trim() === '1', null, { timeout: 20000 });
  series = await drawnSeries();
  assert.ok(avg(series), 'script series missing after reload');

  // Live: two forming updates and the close of today's bar advance the session, never reopen it.
  await page.waitForFunction(() => window.__ops.includes('open'), null, { timeout: 20000 });
  await page.waitForTimeout(1000);
  await page.evaluate(() => (window.__ops = []));
  const last = bars.c.length - 1;
  const tick = (dc, closed) => ({ ts: bars.ts[last], o: bars.o[last], h: bars.h[last] + 3, l: bars.l[last], c: bars.c[last] + dc, v: bars.v[last] + 10, closed, lag_ms: 5 });
  liveQueue.push(tick(0.5, false), tick(1, false), tick(1.5, true));
  await page.waitForFunction(() => window.__ops.filter((o) => o === 'advance').length >= 3, null, { timeout: 30000 });
  await page.waitForTimeout(1500);
  const ops = await page.evaluate(() => window.__ops);
  assert.ok(!ops.includes('open'), `a live tick reopened the session: ${ops}`);
  const liveLast = lastValue(avg(await scriptSeries()));
  // A fresh load of the same bars draws the same value.
  Object.assign(bars, { h: bars.h.with(last, bars.h[last] + 3), c: bars.c.with(last, bars.c[last] + 1.5), v: bars.v.with(last, bars.v[last] + 10) });
  await page.reload();
  await page.waitForFunction(() => document.querySelector('.sissue')?.textContent?.trim() === '1', null, { timeout: 20000 });
  await page.waitForTimeout(1000);
  assert.ok(Math.abs(lastValue(avg(await scriptSeries())) - liveLast) < 1e-9, 'live value differs from a reload');

  // Save right after opening the sheet, before the editor's first compile: accepted.
  const again = page.locator('.line.ind', { hasText: 'Fixture' });
  await again.hover();
  await again.getByRole('button', { name: 'Edit' }).click();
  await page.locator('button.primary', { hasText: 'Save' }).click();
  await page.locator('button.primary', { hasText: 'Save' }).waitFor({ state: 'detached', timeout: 10000 });

  assert.deepEqual(errors, [], 'page errors');
  console.log('PASS: script tab add, series drawn, warnings panel, input rerun, reload persistence, warmup, live rollback.');
} catch (error) {
  console.error('Page errors:', errors);
  await page.screenshot({ path: `${artifacts}/failure.png`, fullPage: true });
  throw error;
} finally {
  await browser.close();
}
