// Run with the frontend dev server: node tests/mobile-routes-browser.mjs [base URL].
// Fixture API keeps this reproducible without account credentials or a database.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-route-tests/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });
const context = await browser.newContext({ viewport: { width: 390, height: 844 }, locale: 'en-US' });
const page = await context.newPage();
const errors = [];
const capturedTrades = [];
page.on('pageerror', (e) => errors.push({ route: page.url(), message: e.message, stack: e.stack }));
page.setDefaultTimeout(12000);
const documents = [
  { id: 'page-1', kind: 'page', title: 'Research notes', parent_id: null, position: 1 },
  { id: 'page-2', kind: 'page', title: 'Trade thesis', parent_id: null, position: 2 },
  { id: 'folder-1', kind: 'folder', title: 'Archive', parent_id: null, position: 3 }
];

await context.route('**/api/**', async (route) => {
  const p = new URL(route.request().url()).pathname;
  let result = {};
  if (p === '/api/setup/status') result = { configured: true };
  else if (p === '/api/settings/me') result = { username: 'fixture', role: 'admin', locale: 'en' };
  else if (p === '/api/settings/modules') result = { installed: ['editor', 'news', 'journal', 'fundamentals', 'quant', 'histdata', 'backtest'] };
  else if (p === '/api/settings/defaults') result = { default_timezone: 'UTC', display_currency: 'USD' };
  else if (p === '/api/health') result = { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
  else if (p === '/api/dashboard/layout') result = { pages: [], favoriteIds: ['__preset_investor__'] };
  else if (p === '/api/tasks') result = { tasks: [] };
  else if (p.includes('/notifications')) result = { notifications: [], unread: 0 };
  else if (p === '/api/journal/categories') result = { categories: [{ id: 'default', name: 'General', is_default: true, position: 0 }] };
  else if (p === '/api/journal/strategies') result = { strategies: [] };
  else if (p === '/api/journal/templates') result = { templates: [] };
  else if (p === '/api/journal/fee-schedules') result = { fee_schedules: [] };
  else if (p === '/api/journal/settings') result = { settings: { display_currency: 'USD' } };
  else if (p === '/api/journal/tags') result = { tags: [] };
  else if (p === '/api/journal/trades' && route.request().method() === 'POST') {
    capturedTrades.push(route.request().postDataJSON());
    result = { trade: { id: 'fixture-trade' } };
  }
  else if (p === '/api/journal/trades') result = { trades: [] };
  else if (p.endsWith('/capital')) result = { events: [] };
  else if (p === '/api/journal/trade-suggestions') result = { tickers: [], exchanges: [], signals: [] };
  else if (p === '/api/journal/fx/pending') result = { pending: [] };
  else if (p.includes('/journal/brokers/conflicts')) result = { conflicts: [] };
  else if (p === '/api/feed-dashboards') result = { dashboards: [] };
  else if (p === '/api/feed-items') result = { items: [] };
  else if (p === '/api/feed-sources') result = { source_names: [], source_types: [] };
  else if (p === '/api/documents') result = { documents: [...documents].sort((a, b) => a.position - b.position) };
  else if (/^\/api\/documents\/[^/]+\/move$/.test(p)) {
    const id = p.split('/')[3];
    const data = route.request().postDataJSON();
    const item = documents.find((d) => d.id === id);
    item.parent_id = data.parent_id;
    item.position = data.position;
    result = { document: item };
  }
  else if (/^\/api\/documents\/[^/]+$/.test(p)) {
    const id = p.split('/')[3];
    const item = documents.find((d) => d.id === id);
    result = { document: { ...item, content: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Mobile research sample' }] }] } } };
  }
  else if (p === '/api/feeds/stream') return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
  await route.fulfill({ json: result });
});

const dimensions = () => page.evaluate(() => {
  const viewport = document.documentElement.clientWidth;
  const overflowers = [...document.querySelectorAll('body *')].filter((e) => {
    const r = e.getBoundingClientRect();
    return r.right > viewport + 1 && getComputedStyle(e).position !== 'fixed';
  }).slice(0, 8).map((e) => `${e.tagName}.${typeof e.className === 'string' ? e.className : ''}`);
  return { viewport, scroll: document.documentElement.scrollWidth, overflowers };
});

try {
  const cases = [
    { route: '/settings', selector: '.mobile-selector', name: 'settings' },
    { route: '/journal?view=strategies', selector: '.mobile-selector', name: 'journal' },
    { route: '/news', selector: '.mobile-selector', name: 'news' },
    { route: '/editor', selector: '.mobile-selector', name: 'editor' }
  ];
  for (const item of cases) {
    await page.goto(`${base}${item.route}`);
    await page.locator(item.selector).waitFor();
    for (const width of [320, 390, 600]) {
      await page.setViewportSize({ width, height: 844 });
      const trigger = page.locator(item.selector);
      await trigger.click();
      assert.equal(await trigger.getAttribute('aria-expanded'), 'true', `${item.name}: opens at ${width}`);
      await page.screenshot({ path: `${artifacts}/${item.name}-${width}-open.png`, fullPage: true });
      const size = await dimensions();
      assert.ok(size.scroll <= size.viewport + 1, `${item.name}/${width} overflows: ${JSON.stringify(size)}`);
      await trigger.click();
      assert.equal(await trigger.getAttribute('aria-expanded'), 'false', `${item.name}: closes at ${width}`);
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: `${artifacts}/${item.name}-390-closed.png`, fullPage: true });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.screenshot({ path: `${artifacts}/${item.name}-1440.png`, fullPage: true });
    assert.equal(await page.locator(item.selector).isVisible(), false, `${item.name}: mobile selector appears on desktop`);
    await page.setViewportSize({ width: 390, height: 844 });
  }
  await page.goto(`${base}/settings`);
  await page.locator('.mobile-selector').click();
  await page.locator('.rail').getByRole('button', { name: 'About', exact: true }).click();
  assert.equal(await page.locator('.mobile-selector').getAttribute('aria-expanded'), 'false');
  assert.match(await page.locator('.mobile-selector').innerText(), /About/);
  await page.goto(`${base}/editor`);
  await page.locator('.mobile-selector').click();
  await page.locator('.tree-body .row').filter({ hasText: 'Trade thesis' }).getByRole('button', { name: 'Trade thesis', exact: true }).click();
  await page.locator('.mobile-selector[aria-expanded="false"]').waitFor();
  assert.equal(await page.locator('.mobile-selector').getAttribute('aria-expanded'), 'false', 'Editor document selection closes tree');
  assert.match(await page.locator('.mobile-selector').innerText(), /Trade thesis/);
  await page.locator('.mobile-selector').click();
  const secondRow = page.locator('.tree-body .row').filter({ hasText: 'Trade thesis' });
  await secondRow.getByRole('button', { name: /Document actions: Trade thesis/ }).click();
  await page.screenshot({ path: `${artifacts}/editor-390-actions.png`, fullPage: true });
  const actionSize = await dimensions();
  assert.ok(actionSize.scroll <= actionSize.viewport + 1, 'Editor actions overflow phone width');
  await secondRow.getByRole('button', { name: 'Move up' }).click();
  assert.equal(documents.find((d) => d.id === 'page-2').position, 0, 'Tap reorder persists its new position');
  await page.locator('.tree-body .row').first().getByRole('button', { name: 'Trade thesis', exact: true }).waitFor();
  assert.match(await page.locator('.tree-body .row').first().innerText(), /Trade thesis/);
  await page.getByRole('button', { name: 'Quick search' }).click();
  await page.getByRole('textbox', { name: 'Find on this page…' }).fill('Mobile research sample');
  await page.locator('.shortcut-panel .selection strong').filter({ hasText: 'Mobile research sample' }).waitFor();
  await page.getByRole('button', { name: 'Show', exact: true }).click();
  assert.equal(await page.evaluate(() => CSS.highlights?.has('otw-page-search') || !!document.querySelector('.otw-page-search-fallback')), true);
  await page.screenshot({ path: `${artifacts}/editor-390-page-search.png`, fullPage: true });
  await page.evaluate(() => {
    const field = document.createElement('input');
    field.setAttribute('aria-label', 'Fixture value');
    field.value = 'Visible field signal';
    document.querySelector('.workarea').append(field);
  });
  await page.getByRole('textbox', { name: 'Find on this page…' }).fill('Visible field signal');
  await page.locator('.shortcut-panel .selection strong').filter({ hasText: 'Visible field signal' }).waitFor();
  await page.evaluate(() => {
    const field = document.querySelector('input[aria-label="Fixture value"]');
    field.value = 'Changed field signal';
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await page.locator('.shortcut-panel .selection .empty-hint').waitFor();
  await page.getByRole('button', { name: 'Features' }).click();
  await page.getByRole('textbox', { name: 'Search features…' }).fill('News');
  await page.getByRole('button', { name: 'Open', exact: true }).click();
  await page.waitForURL(`${base}/news`);
  const quickSearch = page.getByRole('button', { name: 'Quick search' });
  assert.equal(await quickSearch.getAttribute('aria-expanded'), 'false');
  const initialPosition = await quickSearch.boundingBox();
  await page.mouse.move(initialPosition.x + 22, initialPosition.y + 22);
  await page.mouse.down();
  await page.mouse.move(310, 110, { steps: 8 });
  await page.mouse.up();
  const movedPosition = await quickSearch.boundingBox();
  assert.ok(movedPosition.x > initialPosition.x + 100 && movedPosition.y < initialPosition.y - 100, 'Quick-search button moves with the pointer');
  assert.equal(await quickSearch.getAttribute('aria-expanded'), 'false', 'Dragging does not open quick search');
  const savedPosition = await page.evaluate(() => JSON.parse(localStorage.getItem('otw.mobile.quick-search.position')));
  assert.ok(savedPosition.x > .5 && savedPosition.y < .5, 'Dragged position is saved as viewport fractions');
  await quickSearch.click();
  const panelBounds = await page.locator('.shortcut-panel').boundingBox();
  assert.ok(panelBounds.x >= 0 && panelBounds.y >= 0 && panelBounds.x + panelBounds.width <= 390 && panelBounds.y + panelBounds.height <= 844, 'Panel stays in the viewport near the moved button');
  assert.ok(Math.abs(panelBounds.y - (movedPosition.y + movedPosition.height)) <= 1, 'Search panel meets the moved button');
  await page.screenshot({ path: `${artifacts}/quick-search-dragged-390.png`, fullPage: true });
  await page.setViewportSize({ width: 320, height: 568 });
  await page.waitForTimeout(60);
  const narrowButton = await quickSearch.boundingBox();
  const narrowPanel = await page.locator('.shortcut-panel').boundingBox();
  assert.ok(narrowButton.x >= 0 && narrowButton.y >= 0 && narrowButton.x + narrowButton.width <= 320 && narrowButton.y + narrowButton.height <= 568, 'Saved button position adapts to a smaller viewport');
  assert.ok(narrowPanel.x >= 0 && narrowPanel.y >= 0 && narrowPanel.x + narrowPanel.width <= 320 && narrowPanel.y + narrowPanel.height <= 568, 'Panel adapts to a smaller viewport');
  assert.ok(Math.abs(narrowPanel.y - (narrowButton.y + narrowButton.height)) <= 1, 'Search panel stays attached after resize');
  await page.screenshot({ path: `${artifacts}/quick-search-dragged-320.png`, fullPage: true });
  await page.keyboard.press('Escape');
  await page.reload();
  await page.locator('.mobile-shortcut[style*="top:"]').waitFor();
  const restored = await quickSearch.boundingBox();
  assert.ok(restored.x > 200 && restored.y < 200, 'Quick-search position survives reload');
  const cdp = await context.newCDPSession(page);
  await cdp.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 1 });
  const startX = restored.x + 22, startY = restored.y + 22;
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ id: 1, x: startX, y: startY }] });
  for (let step = 1; step <= 6; step++) {
    await cdp.send('Input.dispatchTouchEvent', {
      type: 'touchMove',
      touchPoints: [{ id: 1, x: startX + (48 - startX) * step / 6, y: startY + (480 - startY) * step / 6 }]
    });
  }
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  const touched = await quickSearch.boundingBox();
  assert.ok(touched.x < 80 && touched.y > 400, 'Touch drag moves the shortcut');
  assert.equal(await quickSearch.getAttribute('aria-expanded'), 'false', 'Touch drag does not open quick search');
  await cdp.detach();
  await quickSearch.focus();
  await page.keyboard.press('Enter');
  assert.equal(await quickSearch.getAttribute('aria-expanded'), 'true', 'Keyboard activation still works after dragging');
  await page.keyboard.press('Escape');
  await page.goto(`${base}/journal?view=trades`);
  await page.locator('.trades .toolbar').waitFor();
  await page.getByRole('button', { name: 'Quick (all fields)' }).first().click();
  await page.locator('[role="dialog"]').waitFor();
  await page.locator('.advanced-toggle input').check();
  await page.getByRole('button', { name: '+ Entry' }).click();
  for (const width of [320, 390, 600]) {
    await page.setViewportSize({ width, height: 844 });
    const modal = await page.locator('[role="dialog"]').boundingBox();
    assert.ok(modal.x >= 0 && modal.x + modal.width <= width + 1, `Trade form dialog fits at ${width}px`);
    const size = await dimensions();
    assert.ok(size.scroll <= size.viewport + 1, `Trade form overflows at ${width}px: ${JSON.stringify(size)}`);
    await page.screenshot({ path: `${artifacts}/trade-form-${width}.png`, fullPage: true });
  }
  await page.getByRole('button', { name: 'Log trade' }).scrollIntoViewIfNeeded();
  assert.equal(await page.getByRole('button', { name: 'Log trade' }).isVisible(), true);
  await page.setViewportSize({ width: 320, height: 568 });
  await page.locator('.trade-form input[list="sg-tickers"]').fill('AAPL');
  await page.locator('.leg-group').first().locator('.leg-row .lp').first().fill('125.50');
  await page.locator('.leg-group').first().locator('.leg-row .lq').first().fill('2');
  await page.locator('.leg-group').first().locator('.leg-row input[type="date"]').first().fill('2026-10-05');
  await page.getByRole('button', { name: 'Log trade' }).click();
  await page.locator('[role="dialog"]').waitFor({ state: 'hidden' });
  assert.equal(capturedTrades.length, 1, 'Advanced trade submits once');
  assert.equal(capturedTrades[0].ticker, 'AAPL');
  assert.equal(capturedTrades[0].advanced, true);
  assert.equal(capturedTrades[0].entries[0].price, 125.5);
  assert.deepEqual(errors, [], 'Runtime errors');
  console.log('PASS: Settings, Journal, News and Editor at 320/390/600 px; advanced trade form and submit, content search, shortcut navigation and persistent mouse/touch drag.');
} catch (error) {
  console.error('Runtime errors:', errors);
  await page.screenshot({ path: `${artifacts}/failure.png`, fullPage: true });
  throw error;
} finally {
  await browser.close();
}
