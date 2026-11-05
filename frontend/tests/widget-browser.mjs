// Run with a Vite server: node tests/widget-browser.mjs [http://127.0.0.1:4173]
// Uses repository-local Chromium and fixtures; never contacts a backend or provider.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/widget-browser-tests/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, locale: 'en-US' });
const page = await context.newPage();
page.setDefaultTimeout(15000);
await page.clock.install();
const errors = [], requests = [];
page.on('pageerror', (e) => errors.push(e.message));

const now = new Date();
const iso = (days) => new Date(+now + days * 86400000).toISOString();
const companies = ['AAA', 'BBB'].map((ticker, i) => ({ ticker, name: `Fixture company ${ticker}`, followed: true, currency: 'USD', metrics: { price: 100 + i * 10, change_pct: i ? -1.2 : .6, pe: 15 + i * 5, ev_ebitda: 10 + i, market_cap: 2e9, operating_margin: 20, fcf_yield: 5, basis: 'TTM', price_date: iso(-1).slice(0, 10) } }));
const datasets = ['ds-a', 'ds-b'].map((id, i) => ({ id, ticker: companies[i].ticker, timeframe: '1d', provider: 'fixture', bar_count: 500, range_from: iso(-500), range_to: iso(-1), last_updated: iso(-1) }));
const series = [{ id: 'macro-uuid', provider_id: 'fixture', provider: 'Fixture source', code: 'RATE', title: 'Fixture policy rate', unit: '%', freq: 'M', category: 'rates', status: 'ok', last: 4, prev: 3.5, last_period: iso(-1).slice(0, 10), updated: iso(-1) }];
// Mixed magnitudes, long names/units and missing metadata reproduce the Macro Board
// overflow. Keep the policy rate first for the single-series checks below.
series.push(...[
  { title: 'Crude Oil Prices: West Texas Intermediate (WTI) - Cushing, Oklahoma', last: 96.16, prev: 99.34, unit: '$ per Barrel', freq: 'D', provider: 'FRED (St. Louis Fed)' },
  { title: 'Henry Hub natural gas spot price', last: 3.18, prev: 3.13, unit: 'USD/MMBtu', freq: null, provider: 'EIA' },
  { title: 'US crude oil inventories, ex SPR', last: 427320, prev: 426500, unit: 'k bbl', freq: 'W', provider: 'EIA' },
  { title: 'US gasoline inventories', last: 204362, prev: 206000, unit: 'k bbl', freq: 'W', provider: 'EIA' },
  { title: 'Total public debt outstanding', last: 40171825101340.31, prev: 40100000000000, unit: 'USD', freq: 'D', provider: 'US Treasury' },
  { title: 'US central government debt, % of GDP', last: 115.77, prev: 114.74, unit: '%', freq: 'A', provider: 'World Bank' },
  { title: 'Advance Retail Sales: Retail Trade and Food Services', last: 737763, prev: 730000, unit: 'Mil. of $', freq: 'M', provider: 'FRED (St. Louis Fed)' }
].map((s, i) => ({ ...series[0], ...s, id: `macro-${i}`, code: `MACRO${i}` })));
const statements = Array.from({ length: 12 }, (_, i) => ({ fiscal_year: 2023 + Math.floor(i / 4), fiscal_period: `Q${i % 4 + 1}`, period: `Q${i % 4 + 1} FY${2023 + Math.floor(i / 4)}`, period_end: `${2023 + Math.floor(i / 4)}-${String((i % 4 + 1) * 3).padStart(2, '0')}-28`, lines: { revenue: i === 3 ? -15e6 : (i + 1) * 10e6 } }));
const variants = { fundamentals: ['series', 'board', 'company', 'line', 'companies', 'filings', 'earnings', 'valuation'], quant: ['count', 'risk', 'correlation', 'seasonality', 'volatility', 'regime'] };
const items = Object.entries(variants).flatMap(([type, names]) => names.map((variant) => ({ id: `${type}-${variant}`, type, span: 6, config: { title: `${type}-${variant}`, variant, height: 'tall', ticker: 'AAA', series: 'fixture:RATE', datasetId: 'ds-a', datasetIds: ['ds-a', 'ds-b'], peerTickers: ['BBB'], line: 'revenue', freq: 'quarterly', scope: 'followed' } })));
items.find((item) => item.id === 'fundamentals-board').span = 12;
let layout = { activePageId: 'fixture', favoriteIds: ['fixture'], pages: [{ id: 'fixture', name: 'Widget validation', layout: { cols: 12, items } }] };
let failRisk = false;
const observed = new Map();
await context.route('**/api/**', async (route) => {
  const req = route.request(), url = new URL(req.url()), p = url.pathname;
  requests.push({ p, method: req.method() });
  if (req.method() === 'POST' && p.startsWith('/api/quant/')) observed.set(p, req.postDataJSON());
  let result = {};
  if (p === '/api/setup/status') result = { configured: true };
  else if (p === '/api/settings/me') result = { username: 'fixture', role: 'admin' };
  else if (p === '/api/settings/modules') result = { installed: ['fundamentals', 'quant', 'histdata', 'backtest'] };
  else if (p === '/api/settings/defaults') result = { default_timezone: 'UTC' };
  else if (p === '/api/health') result = { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
  else if (p === '/api/dashboard/layout') { if (req.method() === 'PUT') layout = req.postDataJSON(); result = layout; }
  else if (p.includes('/notifications')) result = { notifications: [], unread: 0 };
  else if (p === '/api/tasks') result = { tasks: [] };
  else if (p === '/api/histdata/datasets') result = { datasets };
  else if (p === '/api/backtest/runs') result = { runs: [] };
  else if (p === '/api/fundamentals/series') result = { series };
  else if (p.endsWith('/observations')) result = { observations: Array.from({ length: 24 }, (_, i) => [Date.parse(iso(-720 + i * 30)), 2 + i / 10]) };
  else if (p === '/api/fundamentals/companies') result = { companies };
  else if (/\/companies\/[^/]+$/.test(p)) result = { company: companies.find((c) => p.endsWith(c.ticker)) };
  else if (p.endsWith('/statements')) result = { statements };
  else if (p === '/api/fundamentals/documents') result = { documents: [{ id: 'filing', ticker: 'AAA', form: '10-Q', title: 'Quarterly report', kind: 'filing', filed_at: iso(-5), url: 'https://example.invalid/filing' }] };
  else if (p.includes('/fundamentals/data/')) {
    const dataset = p.split('/').at(-1);
    // No earnings snapshot: verifies the stored calendar fallback still renders.
    result = { snapshot: dataset === 'earnings' ? null : { provider_label: 'Fixture source', fetched_at: iso(-1), stale: false, data: dataset === 'calendar' ? { earnings: companies.map((c, i) => ({ ticker: c.ticker, date: iso(7 + i).slice(0, 10), eps_estimate: 1.2, revenue_estimate: 1e9 })) } : { series: Array.from({ length: 30 }, (_, i) => [iso(-30 + i).slice(0, 10), 12345 + i * 10]) } } };
  } else if (p === '/api/quant/single') {
    if (failRisk) return route.fulfill({ status: 503, json: { error: 'Fixture temporarily unavailable' } });
    result = { ticker: 'AAA', timeframe: '1d', result: { confidence: req.postDataJSON().confidence, periods_per_year: 252, periods: 499, hv_annual: .2, max_drawdown: .15, var_hist: .025, drawdown_curve: Array.from({ length: 12 }, (_, i) => ({ ts: iso(-100 + i * 8), dd: -i / 100 })) } };
  } else if (p === '/api/quant/portfolio') result = { periods: 500, measured_at: '1d', alignment: { from: iso(-500), to: iso(-1) }, correlation: { labels: ['AAA', 'BBB'], matrix: [[1, -.42], [-.42, 1]] } };
  else if (p === '/api/quant/seasonality') {
    const metric = req.postDataJSON().metric;
    result = { ticker: 'AAA', timeframe: '1d', from: iso(-500), until: iso(-1), result: { metric, periods: 499, month: Array.from({ length: 12 }, (_, i) => ({ key: i, value: metric === 'volume' ? 1234 : i / 1000, count: 30 })), month_weekday: Array.from({ length: 12 }, (_, i) => Array.from({ length: 7 }, (_, j) => j === 6 ? null : metric === 'volume' ? 1234 : i === 0 && j === 0 ? 0 : (i - j) / 1000)), month_weekday_counts: Array.from({ length: 12 }, () => [5, 5, 5, 5, 5, 0, 0]) } };
  }
  else if (p === '/api/quant/volatility') result = { ticker: 'AAA', timeframe: '1d', from: iso(-500), until: iso(-1), result: { window: 21, bars: 500, cones: [{ window: 21, current: .22, p10: .1, median: .18, p90: .3, current_rank: .7, samples: 479 }], rolling: Array.from({ length: 10 }, (_, i) => ({ ts: iso(-10 + i), cc: .2 + i / 1000 })) } };
  else if (p === '/api/quant/regimes') result = { ticker: 'AAA', timeframe: '1d', from: iso(-500), until: iso(-1), bars: 500, result: { k: 2, current: [.85, .15], states: [{ vol_annual: .15, expected_duration: 25 }, { vol_annual: .4, expected_duration: 5 }], converged: true } };
  await route.fulfill({ json: result });
});

const widget = (name) => page.locator('.otw-widget').filter({ has: page.locator('.wtxt', { hasText: name }) });
async function checkMacroBoard() {
  const result = await widget('fundamentals-board').evaluate((e) => {
    const board = e.querySelector('.macro-board');
    const rect = (node) => node.getBoundingClientRect();
    const cards = [...board.querySelectorAll('.macro-card')];
    return {
      count: cards.length,
      scrolls: board.scrollHeight > board.clientHeight,
      horizontalOverflow: board.scrollWidth > board.clientWidth + 1,
      collisions: cards.flatMap((card, i) => {
        const children = [...card.children];
        const problems = children.filter((child, j) =>
          rect(child).right > rect(card).right + 1 ||
          child.scrollWidth > child.clientWidth + 1 ||
          rect(child).bottom > rect(card).bottom + 1 ||
          (j > 0 && rect(children[j - 1]).bottom > rect(child).top + 1)
        );
        return problems.map((child) => `${i}: ${child.className}`);
      })
    };
  });
  assert.equal(result.count, 8);
  if (await page.evaluate(() => innerWidth <= 767)) {
    assert.equal(result.scrolls, false, 'Macro cards should expand the widget on mobile');
  } else {
    assert.equal(result.scrolls, true, 'Macro cards should scroll within the widget on desktop');
  }
  assert.equal(result.horizontalOverflow, false, 'Macro Board overflows horizontally');
  assert.deepEqual(result.collisions, [], 'Macro Board content overlaps or is clipped');
}
async function checkChartLabels() {
  // SVG overflow does not necessarily widen the page: assert the actual text bounds,
  // including long currency prices and the first/last date labels.
  await widget('fundamentals-company').locator('.axis.y').first().waitFor();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForFunction(() => [...document.querySelectorAll('.trend')].every((chart) => {
    const svg = chart.querySelector('svg');
    if (!svg) return true;
    const bounds = svg.getBoundingClientRect();
    return [...svg.querySelectorAll('.axis')].every((text) => {
      const label = text.getBoundingClientRect();
      return label.left >= bounds.left + 2 && label.right <= bounds.right - 2;
    });
  }));
  assert.ok(await widget('fundamentals-company').locator('.axis.y').first().evaluate((e) => e.getComputedTextLength() > 46), 'Fixture must exercise labels wider than the old margin');
}
async function checkMobileWidgetWidth(width) {
  await page.setViewportSize({ width, height: 844 });
  const problems = await page.evaluate(() => {
    const problems = [];
    for (const card of document.querySelectorAll('.otw-widget')) {
      const body = card.querySelector('.wbody');
      const title = card.querySelector('.wtxt')?.textContent?.trim();
      if (!body) continue;
      if (body.scrollWidth > body.clientWidth + 1 && !['auto', 'scroll'].includes(getComputedStyle(body).overflowX)) {
        problems.push(`${title}: horizontal content cannot be scrolled`);
      }
      for (const name of body.querySelectorAll('.w-name')) {
        if (name.scrollWidth > name.clientWidth + 2 && getComputedStyle(name).textOverflow === 'ellipsis') {
          problems.push(`${title}: clipped name ${name.textContent?.trim()}`);
        }
      }
    }
    if (document.documentElement.scrollWidth > document.documentElement.clientWidth + 1) problems.push('page overflow');
    return problems;
  });
  assert.deepEqual(problems, [], `${width}px widget content overflows or clips`);
}
async function checkScrollTopScope() {
  await page.setViewportSize({ width: 390, height: 844 });
  const pageScroller = page.locator('.page');
  await pageScroller.evaluate((element) => (element.scrollTop = 0));
  const inner = widget('fundamentals-board').locator('.wbody');
  await inner.evaluate((element) => {
    const spacer = document.createElement('div');
    spacer.dataset.scrollTopFixture = '';
    spacer.style.height = '1000px';
    spacer.style.flex = 'none';
    element.append(spacer);
    element.style.height = '120px';
    element.style.overflowY = 'auto';
    element.scrollTop = element.scrollHeight;
  });
  await page.waitForTimeout(100);
  assert.equal(await page.locator('.scroll-top').count(), 0, 'A widget scroll must not show Back to top');
  await inner.evaluate((element) => {
    element.querySelector('[data-scroll-top-fixture]')?.remove();
    element.style.removeProperty('height');
    element.style.removeProperty('overflow-y');
    element.scrollTop = 0;
  });
  await pageScroller.evaluate((element) => (element.scrollTop = element.scrollHeight));
  await page.locator('.scroll-top').waitFor();
  const placement = await page.evaluate(() => {
    const button = document.querySelector('.scroll-top').getBoundingClientRect();
    const bar = document.querySelector('.topbar').getBoundingClientRect();
    const title = document.querySelector('.mobile-current').getBoundingClientRect();
    const actions = document.querySelector('.mobile-actions-trigger').getBoundingClientRect();
    return { button: { left: button.left, right: button.right, top: button.top, bottom: button.bottom }, bar: { top: bar.top, bottom: bar.bottom }, titleRight: title.right, actionsLeft: actions.left };
  });
  assert.ok(placement.button.top >= placement.bar.top && placement.button.bottom <= placement.bar.bottom, `Back to top covers page content: ${JSON.stringify(placement)}`);
  assert.ok(placement.button.left >= placement.titleRight + 7 && placement.button.right <= placement.actionsLeft - 7, `Back to top overlaps header commands: ${JSON.stringify(placement)}`);
  await pageScroller.evaluate((element) => (element.scrollTop = 0));
  await page.locator('.scroll-top').waitFor({ state: 'detached' });
}
try {
  await page.goto(`${base}/?dashboard=fixture`);
  await widget('fundamentals-earnings').getByText('AAA', { exact: true }).waitFor();
  await widget('quant-regime').getByText('HMM', { exact: false }).waitFor();
  assert.equal(await page.locator('.otw-widget').count(), items.length);
  for (const item of items) assert.equal(await widget(item.id).locator('.werr, .error').count(), 0, item.id);
  assert.match(await widget('fundamentals-earnings').innerText(), /Fixture source/);
  assert.match(await widget('fundamentals-series').innerText(), /Fixture policy rate/);
  await widget('fundamentals-board').locator('.macro-card').nth(7).waitFor();
  assert.match(await widget('fundamentals-board').getByText('40.17T', { exact: true }).locator('..').getAttribute('title'), /40[,.]171[,.]825[,.]101[,.]340/);
  await checkScrollTopScope();
  for (const width of [320, 390, 600]) await checkMobileWidgetWidth(width);
  for (const width of [1920, 1440, 800, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    await checkMacroBoard();
    await checkChartLabels();
  }
  await widget('fundamentals-board').evaluate((e) => e.style.height = '184px');
  await checkMacroBoard();
  await widget('fundamentals-board').evaluate((e) => e.style.removeProperty('height'));
  await page.setViewportSize({ width: 1440, height: 1000 });
  assert.equal(await page.getByRole('button', { name: 'Navigation', exact: true }).isVisible(), false, 'Mobile navigation must stay hidden on desktop');
  assert.equal(await widget('fundamentals-line').locator('.bar.negative').count(), 1);
  const bar = widget('fundamentals-line').getByRole('button', { name: /Q4 FY2023:/ });
  await bar.focus();
  assert.match(await widget('fundamentals-line').locator('.readout').innerText(), /Q4 FY2023:.*−/);
  // Locale formatters may use either U+2212 or an ASCII minus.
  const cell = widget('quant-correlation').locator('.cell').nth(1);
  await cell.focus();
  assert.match(await widget('quant-correlation').locator('.detail').innerText(), /AAA · BBB:.*0.42/);
  assert.equal(await widget('quant-seasonality').locator('.missing').count(), 12);
  assert.equal(await widget('quant-seasonality').locator('.cell').first().getAttribute('class').then((s) => s.includes('missing')), false);
  await widget('fundamentals-companies').getByRole('button', { name: 'P/E', exact: true }).click();
  await widget('fundamentals-companies').getByRole('button', { name: /P\/E/ }).click();
  assert.match(await widget('fundamentals-companies').locator('tbody tr').first().innerText(), /^BBB/);
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => document.documentElement.dataset.theme = value, theme);
    await checkChartLabels();
    await widget('fundamentals-company').screenshot({ path: `${artifacts}/${theme}-company-snapshot-desktop.png` });
    await widget('fundamentals-board').screenshot({ path: `${artifacts}/${theme}-macro-board-desktop.png` });
    await page.screenshot({ path: `${artifacts}/${theme}-desktop.png`, fullPage: true });
    await page.setViewportSize({ width: 390, height: 844 });
    await checkMacroBoard();
    await checkChartLabels();
    await widget('fundamentals-company').screenshot({ path: `${artifacts}/${theme}-company-snapshot-mobile.png` });
    await widget('fundamentals-board').screenshot({ path: `${artifacts}/${theme}-macro-board-mobile.png` });
    await page.screenshot({ path: `${artifacts}/${theme}-mobile.png`, fullPage: true });
    for (const name of ['Customize', 'Add widget']) {
      const action = page.getByRole('button', { name, exact: true });
      assert.equal(await action.locator('.mobile-action-label').evaluate((e) => getComputedStyle(e).display), 'none', `${name} should show only its icon on mobile`);
      assert.ok((await action.boundingBox()).width >= 44, `${name} touch target is too small`);
    }
    const navTrigger = page.getByRole('button', { name: 'Navigation', exact: true });
    await navTrigger.click();
    const drawer = page.getByRole('dialog', { name: 'Navigation' });
    await drawer.waitFor();
    await page.screenshot({ path: `${artifacts}/${theme}-nav-drawer.png` });
    assert.ok(await drawer.getByRole('link', { name: 'Fundamentals' }).isVisible());
    await drawer.getByRole('button', { name: 'Tools' }).click();
    assert.equal(await drawer.getByRole('link', { name: 'Fundamentals' }).count(), 0);
    await drawer.getByRole('button', { name: 'Tools' }).click();
    await page.keyboard.press('Escape');
    assert.equal(await drawer.count(), 0);
    assert.equal(await navTrigger.evaluate((e) => document.activeElement === e), true);
    const actionTrigger = page.getByRole('button', { name: 'More actions' });
    await actionTrigger.click();
    await page.screenshot({ path: `${artifacts}/${theme}-actions.png` });
    assert.ok(await page.getByRole('group', { name: 'More actions' }).getByRole('link', { name: 'Settings' }).isVisible());
    await page.keyboard.press('Escape');
    assert.equal(await page.getByRole('group', { name: 'More actions' }).count(), 0);
    const shortcutTrigger = page.getByRole('button', { name: 'Quick search' });
    await shortcutTrigger.click();
    await page.getByRole('textbox', { name: 'Find on this page…' }).fill('Fixture policy rate');
    await page.clock.runFor(150);
    assert.match(await page.locator('.shortcut-panel .selection strong').innerText(), /Fixture policy rate/);
    assert.match(await page.locator('.shortcut-panel .count').innerText(), /^1 \/ 2$/);
    await page.getByRole('button', { name: 'Next match' }).click();
    assert.match(await page.locator('.shortcut-panel .count').innerText(), /^2 \/ 2$/);
    await page.getByRole('button', { name: 'Previous match' }).click();
    assert.match(await page.locator('.shortcut-panel .count').innerText(), /^1 \/ 2$/);
    await page.getByRole('button', { name: 'Show', exact: true }).click();
    assert.equal(await page.evaluate(() => CSS.highlights?.has('otw-page-search') || !!document.querySelector('.otw-page-search-fallback')), true);
    await page.screenshot({ path: `${artifacts}/${theme}-page-search.png` });
    await page.getByRole('button', { name: 'Features' }).click();
    await page.screenshot({ path: `${artifacts}/${theme}-shortcut.png` });
    const choice = page.locator('.shortcut-panel .selection strong');
    const beforeChoice = await choice.innerText();
    await page.getByRole('button', { name: 'Next feature' }).click();
    assert.notEqual(await choice.innerText(), beforeChoice);
    await page.getByRole('textbox', { name: 'Search features…' }).fill('Historical Data');
    assert.match(await choice.innerText(), /Historical Data/);
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('.shortcut-panel').count(), 0);
    assert.equal(await page.evaluate(() => CSS.highlights?.has('otw-page-search') || !!document.querySelector('.otw-page-search-fallback')), false);
    await page.keyboard.press('Meta+k');
    await page.getByRole('textbox', { name: 'Find on this page…' }).waitFor();
    assert.equal(await page.getByRole('textbox', { name: 'Find on this page…' }).evaluate((e) => document.activeElement === e), true);
    await page.keyboard.press('Escape');
    assert.equal(await shortcutTrigger.evaluate((e) => document.activeElement === e), true);
    assert.equal(await widget('quant-correlation').locator('.compact').isVisible(), true);
    assert.equal(await widget('quant-correlation').locator('.matrix-scroll').isVisible(), false);
    const chartGap = await widget('fundamentals-line').evaluate((e) => e.querySelector('.statement').getBoundingClientRect().bottom <= e.querySelector('.statement + p').getBoundingClientRect().top);
    assert.ok(chartGap, 'Statement labels overlap the source footer');
    const overflow = await page.evaluate(() => ({ width: innerWidth, scroll: document.documentElement.scrollWidth, elements: [...document.querySelectorAll('body *')].filter((e) => e.getBoundingClientRect().right > innerWidth + 1).slice(0, 15).map((e) => ({ tag: e.tagName, class: e.className, width: e.getBoundingClientRect().width })) }));
    assert.ok(overflow.scroll <= overflow.width, `${theme}: page overflows horizontally ${JSON.stringify(overflow)}`);
    await page.setViewportSize({ width: 1440, height: 1000 });
  }
  for (const width of [320, 390, 600]) {
    await page.setViewportSize({ width, height: 800 });
    const dashboardBounds = await page.evaluate(() => {
      const page = document.querySelector('.page').getBoundingClientRect();
      const actions = document.querySelector('.dashboard-actions').getBoundingClientRect();
      return { pageLeft: page.left, pageRight: page.right, actionsLeft: actions.left, actionsRight: actions.right, width: innerWidth };
    });
    assert.ok(dashboardBounds.pageLeft >= -1 && dashboardBounds.pageRight <= width + 1 && dashboardBounds.actionsLeft >= -1 && dashboardBounds.actionsRight <= width + 1, `${width}px dashboard is too wide: ${JSON.stringify(dashboardBounds)}`);
    await page.getByRole('button', { name: 'Quick search' }).click();
    await page.getByRole('textbox', { name: 'Find on this page…' }).fill('Fixture policy rate');
    await page.clock.runFor(150);
    const panel = await page.locator('.shortcut-panel').evaluate((e) => {
      const r = e.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: innerWidth, height: innerHeight };
    });
    assert.ok(panel.left >= 0 && panel.right <= panel.width && panel.top >= 0 && panel.bottom <= panel.height, `${width}px search panel escapes viewport: ${JSON.stringify(panel)}`);
    const searchGap = await page.evaluate(() => {
      const panel = document.querySelector('.shortcut-panel').getBoundingClientRect();
      const icon = document.querySelector('.shortcut-trigger').getBoundingClientRect();
      return panel.bottom <= icon.top ? icon.top - panel.bottom : panel.top - icon.bottom;
    });
    assert.ok(Math.abs(searchGap) <= 1, `${width}px search panel is separated from its icon by ${searchGap}px`);
    await page.screenshot({ path: `${artifacts}/page-search-${width}.png` });
    await page.keyboard.press('Escape');
    const dims = await page.evaluate(() => ({ visible: document.documentElement.clientWidth, content: document.documentElement.scrollWidth }));
    assert.ok(dims.content <= dims.visible + 1, `${width}px overflow: ${JSON.stringify(dims)}`);
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await widget('quant-risk').getByRole('button', { name: /configure/i }).click();
  await page.getByRole('dialog').waitFor();
  await page.getByRole('button', { name: 'VaR confidence', exact: true }).click();
  await page.getByRole('option', { name: '99%', exact: true }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Save', exact: true }).click();
  await widget('quant-risk').getByText(/99%/).waitFor();
  assert.equal(observed.get('/api/quant/single').confidence, .99);
  assert.equal(layout.pages.find((p) => p.id === 'fixture').layout.items.find((i) => i.id === 'quant-risk').config.confidence, .99);
  await widget('quant-seasonality').getByRole('button', { name: /configure/i }).click();
  await page.getByRole('button', { name: 'Metric', exact: true }).click();
  await page.getByRole('option', { name: 'Mean volume', exact: true }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Save', exact: true }).click();
  await widget('quant-seasonality').getByText(/Mean volume/).waitFor();
  assert.equal(observed.get('/api/quant/seasonality').metric, 'volume');
  assert.equal(await widget('quant-seasonality').locator('.cell').first().innerText(), '1,234');
  failRisk = true;
  await page.clock.fastForward(300_001);
  await widget('quant-risk').getByRole('status').getByText(/Fixture temporarily unavailable/).waitFor();
  assert.match(await widget('quant-risk').innerText(), /20\.00%/);
  assert.match(await widget('quant-risk').innerText(), /Showing the previous result/);
  await widget('quant-risk').screenshot({ path: `${artifacts}/refresh-failure.png` });
  // A removed configured dataset gets an actionable empty state, with no analysis call.
  failRisk = false;
  layout.pages.find((p) => p.id === 'fixture').layout.items.find((i) => i.id === 'quant-risk').config.datasetId = 'removed-dataset';
  observed.delete('/api/quant/single');
  await page.reload();
  await widget('quant-risk').getByRole('button', { name: 'Choose datasets', exact: true }).waitFor();
  assert.equal(observed.has('/api/quant/single'), false);
  await widget('quant-risk').getByRole('button', { name: 'Choose datasets', exact: true }).click();
  await page.getByRole('dialog').waitFor();
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 320, height: 800 });
  await page.getByRole('button', { name: 'Add widget', exact: true }).click();
  await page.getByRole('dialog', { name: 'Add widget' }).waitFor();
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Customize' }).click();
  const editBounds = await page.evaluate(() => ({ viewport: innerWidth, content: document.documentElement.scrollWidth, actionsRight: document.querySelector('.dashboard-actions').getBoundingClientRect().right }));
  assert.ok(editBounds.content <= editBounds.viewport + 1 && editBounds.actionsRight <= editBounds.viewport + 1, `320px dashboard edit controls are too wide: ${JSON.stringify(editBounds)}`);
  await page.setViewportSize({ width: 390, height: 844 });
  const companyConfig = widget('fundamentals-company').getByRole('button', { name: /configure/i }).first();
  await companyConfig.click();
  const configDialog = page.getByRole('dialog').last();
  await configDialog.waitFor();
  assert.equal(await configDialog.evaluate((e) => e.contains(document.activeElement)), true, 'Modal focus enters');
  await configDialog.getByRole('button', { name: 'Save', exact: true }).focus();
  await page.keyboard.press('Tab');
  assert.equal(await configDialog.locator('.x').evaluate((e) => document.activeElement === e), true, 'Modal focus wraps');
  await page.keyboard.press('Escape');
  assert.equal(await companyConfig.evaluate((e) => document.activeElement === e), true, 'Modal focus returns');
  assert.equal(await page.locator('.mobile-move-tools').count(), items.length);
  const originalSpans = layout.pages.find((p) => p.id === 'fixture').layout.items.map((item) => item.span);
  await page.getByRole('button', { name: 'Move up: fundamentals-board' }).click();
  assert.match(await page.locator('.grid.free > .cell').first().innerText(), /fundamentals-board/);
  await page.getByRole('button', { name: 'Done', exact: true }).click();
  assert.equal(layout.pages.find((p) => p.id === 'fixture').layout.items[0].id, 'fundamentals-board');
  assert.deepEqual(layout.pages.find((p) => p.id === 'fixture').layout.items.map((item) => item.span).sort(), [...originalSpans].sort(), 'Mobile reorder preserves saved desktop spans');
  await page.setViewportSize({ width: 1440, height: 1000 });
  assert.deepEqual(errors, [], 'Browser runtime errors');
  assert.equal(requests.some((r) => r.p.startsWith('/api/fundamentals/') && r.method !== 'GET'), false, 'Dashboard must only read stored fundamentals');
  console.log(`PASS: ${items.length} variants, mobile shell and page/feature search, 320/390/600 px reflow, modal focus, tap reorder with desktop spans retained, stored-only reads and widget regressions.`);
} catch (error) {
  console.error('Browser errors:', errors);
  console.error('API paths:', [...new Set(requests.map((r) => r.p))]);
  await page.screenshot({ path: `${artifacts}/failure.png`, fullPage: true });
  throw error;
} finally {
  await browser.close();
}
