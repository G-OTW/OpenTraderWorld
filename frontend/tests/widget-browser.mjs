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
  assert.equal(result.scrolls, true, 'Macro cards should scroll within the widget');
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
  for (const width of [1920, 1440, 800, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    await checkMacroBoard();
    await checkChartLabels();
  }
  await widget('fundamentals-board').evaluate((e) => e.style.height = '184px');
  await checkMacroBoard();
  await widget('fundamentals-board').evaluate((e) => e.style.removeProperty('height'));
  await page.setViewportSize({ width: 1440, height: 1000 });
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
    assert.equal(await widget('quant-correlation').locator('.compact').isVisible(), true);
    assert.equal(await widget('quant-correlation').locator('.matrix-scroll').isVisible(), false);
    const chartGap = await widget('fundamentals-line').evaluate((e) => e.querySelector('.statement').getBoundingClientRect().bottom <= e.querySelector('.statement + p').getBoundingClientRect().top);
    assert.ok(chartGap, 'Statement labels overlap the source footer');
    const overflow = await page.evaluate(() => ({ width: innerWidth, scroll: document.documentElement.scrollWidth, elements: [...document.querySelectorAll('body *')].filter((e) => e.getBoundingClientRect().right > innerWidth + 1).slice(0, 15).map((e) => ({ tag: e.tagName, class: e.className, width: e.getBoundingClientRect().width })) }));
    assert.ok(overflow.scroll <= overflow.width, `${theme}: page overflows horizontally ${JSON.stringify(overflow)}`);
    await page.setViewportSize({ width: 1440, height: 1000 });
  }
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
  assert.deepEqual(errors, [], 'Browser runtime errors');
  assert.equal(requests.some((r) => r.p.startsWith('/api/fundamentals/') && r.method !== 'GET'), false, 'Dashboard must only read stored fundamentals');
  console.log(`PASS: ${items.length} variants, Macro Board overflow at four widths and compact height, keyboard inspection, sorting, calendar fallback, light/dark, desktop/mobile, saved confidence/volume settings, refresh failure, removed dataset and stored-only reads.`);
} catch (error) {
  console.error('Browser errors:', errors);
  console.error('API paths:', [...new Set(requests.map((r) => r.p))]);
  await page.screenshot({ path: `${artifacts}/failure.png`, fullPage: true });
  throw error;
} finally {
  await browser.close();
}
