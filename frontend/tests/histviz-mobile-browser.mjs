// Populated mobile chart check. Run with the frontend dev server.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/histviz-mobile-tests/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });
const context = await browser.newContext({ viewport: { width: 390, height: 844 }, locale: 'en-US' });
const page = await context.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
const days = Array.from({ length: 120 }, (_, i) => new Date(Date.UTC(2026, 0, i + 1)).toISOString());
const close = days.map((_, i) => 100 + Math.sin(i / 8) * 5 + i * 0.15);
const bars = {
  count: days.length, ts: days,
  o: close.map((v) => v - 0.6), h: close.map((v) => v + 1.2),
  l: close.map((v) => v - 1.4), c: close,
  v: close.map((_, i) => 100000 + i * 250),
  history_start: true, stream: false
};
const workspace = {
  id: 'mobile-chart', name: 'Mobile chart', grid_rows: 1, grid_cols: 1,
  panes: [{ id: 'chart-1', coords: { provider: 'fixture', asset_type: 'equity', ticker: 'AAPL', timeframe: '1d' }, type: 'candlestick', instances: [] }],
  settings: { active: 'chart-1' }
};
await context.route('**/api/**', async (route) => {
  const p = new URL(route.request().url()).pathname;
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
  else if (p === '/api/connectors/providers') result = { providers: [] };
  else if (p === '/api/connectors') result = { connectors: [] };
  else if (p === '/api/histviz/workspaces') result = { workspaces: [workspace] };
  else if (p === '/api/histviz/lists') result = { lists: [] };
  else if (p === '/api/histviz/alerts') result = { alerts: [] };
  else if (p === '/api/watchlists') result = { watchlists: [] };
  else if (p === '/api/histviz/series/batch') result = { results: [bars] };
  else if (p === '/api/histviz/layout') result = {};
  else if (p === '/api/histviz/chart-settings') result = {};
  await route.fulfill({ json: result });
});

try {
  await page.goto(`${base}/histviz`);
  await page.locator('.chart-host .nav').waitFor({ state: 'attached', timeout: 15000 });
  await page.locator('.mobile-pane-nav button').nth(1).click();
  await page.locator('.chart-host .nav').waitFor({ state: 'visible' });
  for (const width of [320, 390, 600]) {
    await page.setViewportSize({ width, height: 844 });
    const nav = page.locator('.chart-host .nav');
    await nav.getByRole('button', { name: 'Zoom in' }).click();
    await nav.getByRole('button', { name: 'Zoom out' }).click();
    await nav.getByRole('button', { name: 'Move left' }).click();
    await nav.getByRole('button', { name: 'Move right' }).click();
    assert.equal(await page.locator('.chart').evaluate((el) => getComputedStyle(el).touchAction), 'pan-y');
    const { viewport, scroll } = await page.evaluate(() => ({ viewport: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
    assert.ok(scroll <= viewport + 1, `Chart page overflows at ${width}px`);
    await page.screenshot({ path: `${artifacts}/chart-${width}.png`, fullPage: true });
  }
  assert.deepEqual(errors, [], 'Chart runtime errors');
  console.log('PASS: populated chart at 320/390/600 px; tap pan and zoom controls and vertical touch scrolling.');
} catch (error) {
  console.error('Chart errors:', errors);
  console.error('Chart state:', await page.evaluate(() => ({
    panes: [...document.querySelectorAll('.mobile-pane-nav button')].map((el) => ({ text: el.textContent?.trim(), current: el.classList.contains('current') })),
    rail: document.querySelector('.grid-wrap')?.className,
    cell: document.querySelector('.cell')?.className,
    nav: document.querySelector('.chart-host .nav')?.getBoundingClientRect().toJSON()
  })));
  await page.screenshot({ path: `${artifacts}/failure.png`, fullPage: true });
  throw error;
} finally {
  await browser.close();
}
