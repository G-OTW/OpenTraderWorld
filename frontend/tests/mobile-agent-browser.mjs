// Agent mobile check: /agent and /agent/mcp with fixture data, no backend.
// Run with the frontend dev server: node tests/mobile-agent-browser.mjs [base URL] [baseline URL].
// The baseline URL (default http://127.0.0.1:4190) serves clean HEAD for the desktop comparison.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const { PNG } = createRequire(import.meta.url)('playwright-core/lib/utilsBundle');
function pixelDiff(a, b) {
  const x = PNG.sync.read(a);
  const y = PNG.sync.read(b);
  let count = 0;
  let max = 0;
  for (let i = 0; i < x.data.length; i += 4) {
    const delta = Math.max(Math.abs(x.data[i] - y.data[i]), Math.abs(x.data[i + 1] - y.data[i + 1]), Math.abs(x.data[i + 2] - y.data[i + 2]));
    if (delta) count++;
    max = Math.max(max, delta);
  }
  return { count, max };
}
const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const baseline = process.argv[3] ?? 'http://127.0.0.1:4190';
// AGENT_ONLY=matrix|flow|desktop runs one part (all by default).
const only = process.env.AGENT_ONLY ?? '';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-agent/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });

const FIXED_NOW = new Date('2026-10-20T09:30:00Z');
const at = (min) => new Date(FIXED_NOW.getTime() - min * 60_000).toISOString();
const LONG_URL = 'https://data.example-market-research.com/api/v3/instruments/XNAS/AAPL/fundamentals/quarterly?fields=revenue,net_income,eps_diluted,free_cash_flow&from=2015-01-01&to=2026-09-30&token=abcdefghijklmnopqrstuvwxyz0123456789';
const REPLY = [
  '## Portfolio risk review for the **Long-term global equity and fixed income core holdings** portfolio',
  '',
  'Your largest drawdown was -18.42% (EUR -1,234,567.89) over 143 trading days. Source: ' + LONG_URL,
  '',
  '| Ticker | Name | Quantity | Market value (EUR) | Unrealized P&L (EUR) | Weight | Beta | Sharpe | Max drawdown |',
  '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |',
  '| AAPL | Apple Inc. | 1,250 | 271,234.56 | -12,345.67 | 21.4% | 1.21 | 0.84 | -27.31% |',
  '| MSFT | Microsoft Corporation | 640 | 263,998.12 | 45,678.90 | 20.8% | 0.98 | 1.12 | -19.04% |',
  '| IWDA.AS | iShares Core MSCI World UCITS ETF USD (Acc) | 3,900 | 389,112.00 | -1,234,567.89 | 30.7% | 1.00 | 0.71 | -33.80% |',
  '',
  '```python',
  'drawdown = (equity / equity.cummax() - 1.0).min()  # rolling peak-to-trough over the whole ledger, including the 2020 and 2022 crashes',
  'print(f"max drawdown: {drawdown:.2%}")',
  '```',
  '',
  'Unbroken token: `' + 'x'.repeat(160) + '`'
].join('\n');

const providers = [
  { id: 'prov-1', label: 'Anthropic production account with a long label', kind: 'anthropic', enabled: true, default_model: 'claude-model-fixture' }
];
const agentCfg = { id: 'agent-default', provider_id: 'prov-1', model: 'claude-model-fixture-with-a-very-long-identifier-2026-10-01', mcp_token_id: 'tok-1', system_prompt: 'You review portfolios.', params: { max_tokens: 4096, temperature: 0.2 } };
const personas = [
  { agent: { id: 'agent-default', name: 'Default assistant' }, effective_skills: [] },
  { agent: { id: 'agent-risk', name: 'Risk reviewer for multi-currency long-term portfolios' }, effective_skills: [{ id: 's1' }, { id: 's2' }] }
];
const tokens = [
  { id: 'tok-1', name: 'Read-only journal, portfolios and watchlists access token', permissions: { journal: 'r', portfolios: 'r', watchlists: 'r' } },
  { id: 'tok-2', name: 'Full access', permissions: { journal: 'rwd' } }
];
const servers = [
  { id: 'srv-1', name: 'Market research data with an extremely long server name for testing', url: LONG_URL, auth_header: 'Authorization', has_auth: true, catalog_id: '', enabled: true },
  { id: 'srv-2', name: 'Disabled server', url: 'https://mcp.example.com/mcp', auth_header: 'Authorization', has_auth: false, catalog_id: '', enabled: false }
];
const conv = (id, title, min, extra = {}) => ({
  id, title, agent_id: 'agent-default', provider_id: null, model: '', effective_provider_id: 'prov-1',
  effective_model: agentCfg.model, mcp_token_id: 'tok-1', mcp_servers: ['srv-1'], memory_enabled: true,
  limit_output_tokens: 200000, limit_cost_usd: 5, last_message_at: at(min), updated_at: at(min), ...extra
});
const conversations = [
  conv('conv-1', 'Quarterly drawdown analysis of the long-term global equity and fixed income core holdings', 3),
  conv('conv-2', 'Short chat', 190),
  conv('conv-3', 'EURUSD hedge sizing with negative carry -0.0042 per day', 60 * 30)
];
const threadMessages = [
  { id: 'm1', role: 'user', content: [{ type: 'text', text: 'Review the drawdown of my core portfolio and include ' + LONG_URL }], created_at: at(12) },
  { id: 'm2', role: 'assistant', content: [{ type: 'thinking', text: 'Checking the ledger.' }, { type: 'tool_use', id: 'tu-1', name: 'portfolios_get_positions_with_a_long_tool_name', input: { portfolio_id: 'core', include_closed: false, url: LONG_URL } }], created_at: at(11) },
  { id: 'm3', role: 'tool', content: [{ type: 'tool_result', tool_use_id: 'tu-1', content: JSON.stringify({ positions: 3, url: LONG_URL }) }], created_at: at(11) },
  { id: 'm4', role: 'assistant', content: [{ type: 'text', text: REPLY }], created_at: at(10) },
  { id: 'm5', role: 'user', content: [{ type: 'text', text: 'Thanks. And the EUR exposure?' }], created_at: at(4) },
  { id: 'm6', role: 'assistant', content: [{ type: 'text', text: 'EUR exposure is 62.5% (EUR 791,234.12). The remaining -37.5% hedge leg is short USD.' }], created_at: at(3) }
];

function routeHandler(state, captured) {
  const created = [];
  const listed = structuredClone(conversations);
  return async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const p = url.pathname;
    const method = req.method();
    if (method !== 'GET' && (p.startsWith('/api/agent/') || p.startsWith('/api/mcp/'))) captured.push({ method, path: p, body: req.postData() ? JSON.parse(req.postData()) : null });
    const fail = state === 'error' && (p.startsWith('/api/agent/') || p.startsWith('/api/mcp/'));
    if (fail) return route.fulfill({ status: 500, json: { error: 'fixture: internal server error' } });
    const pop = state === 'populated';
    let result = {};
    if (p === '/api/setup/status') result = { configured: true };
    else if (p === '/api/settings/me') result = { username: 'fixture', role: 'admin', locale: 'en' };
    else if (p === '/api/settings/modules') result = { installed: ['agent', 'journal', 'portfolios', 'watchlists', 'settings'] };
    else if (p === '/api/settings/defaults') result = { default_timezone: 'UTC', display_currency: 'USD' };
    else if (p === '/api/health') result = { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
    else if (p === '/api/dashboard/layout') result = { pages: [] };
    else if (p === '/api/tasks') result = { tasks: [] };
    else if (p.includes('/notifications')) result = { notifications: [], unread: 0 };
    else if (p === '/api/agent/providers') result = { providers: pop ? providers : [] };
    else if (p === '/api/agent/agent') result = { agent: pop ? agentCfg : {} };
    else if (p === '/api/agent/agents') result = { agents: pop ? personas : [] };
    else if (p === '/api/mcp/tokens' && method === 'POST') result = { token: 'otw_' + 'a1b2c3d4'.repeat(8) };
    else if (p === '/api/mcp/tokens') result = { tokens: pop ? tokens : [] };
    else if (p === '/api/mcp/settings' && method === 'POST') result = { enabled: true };
    else if (p === '/api/mcp/settings') result = { enabled: false, modules: [{ id: 'journal', label: 'Journal' }, { id: 'portfolios', label: 'Portfolios and multi-currency holdings' }, { id: 'watchlists', label: 'Watchlists' }] };
    else if (p === '/api/agent/mcp-servers' && method === 'POST') result = { server: { ...servers[0], id: 'srv-new' } };
    else if (p === '/api/agent/mcp-servers') result = { servers: pop ? servers : [] };
    else if (/^\/api\/agent\/mcp-servers\/[^/]+$/.test(p)) result = method === 'DELETE' ? {} : { server: servers[0] };
    else if (p === '/api/agent/mcp-servers/srv-1/test') result = { tools: ['get_quote', 'get_fundamentals', 'search_instruments_by_isin_or_long_name', 'list_filings', 'get_insider_trades', 'get_options_chain', 'get_dividends'] };
    else if (p === '/api/agent/conversations' && method === 'POST') {
      created.unshift(conv('conv-new', '', 0, { limit_output_tokens: null, limit_cost_usd: null }));
      result = { conversation: created[0] };
    } else if (p === '/api/agent/conversations') result = { conversations: [...created, ...(pop ? listed : [])] };
    else if (/^\/api\/agent\/conversations\/[^/]+\/run$/.test(p)) {
      const body = 'event: delta\ndata: {"text":"Fixture reply."}\n\nevent: done\ndata: {}\n\n';
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body });
    } else if (/^\/api\/agent\/conversations\/[^/]+$/.test(p)) {
      if (method === 'PATCH') {
        const id = p.split('/').pop();
        const row = [...created, ...listed].find((c) => c.id === id) ?? conv(id, '', 0);
        Object.assign(row, JSON.parse(req.postData() || '{}'));
        result = { conversation: { ...row } };
      } else if (method === 'DELETE') result = {};
      else result = { messages: pop && p.endsWith('conv-1') ? threadMessages : [], usage: pop ? { input_tokens: 12345, output_tokens: 6789, cache_read_tokens: 4000, cost_usd: 0.42 } : {} };
    } else if (p === '/api/agent/usage') result = { totals: {}, series: [], by_persona: [], by_model: [], by_provider: [] };
    else if (p === '/api/prompts') result = { prompts: [] };
    else if (p === '/api/prompts/tags') result = { tags: [] };
    else if (p === '/api/vault') result = { vaults: [] };
    await route.fulfill({ json: result });
  };
}

async function newPage(viewport, state, url, root = base) {
  const context = await browser.newContext({ viewport, locale: 'en-US', timezoneId: 'UTC', hasTouch: viewport.width < 1100 });
  const captured = [];
  await context.route('**/api/**', routeHandler(state, captured));
  const page = await context.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.clock.setFixedTime(FIXED_NOW);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      await page.goto(`${root}${url}`, { waitUntil: 'networkidle', timeout: 20000 });
      break;
    } catch (e) {
      if (attempt) throw e;
    }
  }
  return { context, page, errors, captured };
}


const viewports = [
  { width: 320, height: 568 },
  { width: 390, height: 844 },
  { width: 600, height: 800 },
  { width: 667, height: 375 },
  { width: 768, height: 1024 },
  { width: 390, height: 500 } // software keyboard open
];
const results = [];
const failures = [];
async function check(label, fn) {
  try {
    await fn();
    results.push(`ok   ${label}`);
  } catch (e) {
    failures.push(`FAIL ${label}: ${e.message.split('\n').filter((l) => l.trim() && !l.includes('Call log')).slice(0, 3).join(' / ')}`);
  }
}

async function noOverflow(page, label) {
  const m = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    scroll: document.documentElement.scrollWidth,
    culprits: [...document.querySelectorAll('body *')].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.right > document.documentElement.clientWidth + 1 && getComputedStyle(el).position !== 'fixed';
    }).slice(0, 4).map((el) => `${el.tagName}.${typeof el.className === 'string' ? el.className.split(' ')[0] : ''}`)
  }));
  assert.ok(m.scroll <= m.viewport + 1, `${label}: page overflows ${m.scroll} > ${m.viewport} (${m.culprits.join(', ')})`);
}

/** Scroll the control into view, then its centre must be on screen and hit-test to it. */
async function reachable(locator, label) {
  await locator.scrollIntoViewIfNeeded({ timeout: 5000 });
  const r = await locator.evaluate((el) => {
    const b = el.getBoundingClientRect();
    const x = b.left + b.width / 2;
    const y = b.top + b.height / 2;
    const hit = document.elementFromPoint(x, y);
    return { x, y, w: innerWidth, h: innerHeight, ok: !!hit && (hit === el || el.contains(hit)), hit: hit ? `${hit.tagName}.${typeof hit.className === 'string' ? hit.className.split(' ')[0] : ''}` : null };
  });
  assert.ok(r.x >= 0 && r.x <= r.w && r.y >= 0 && r.y <= r.h, `${label}: centre off screen (${Math.round(r.x)},${Math.round(r.y)})`);
  assert.ok(r.ok, `${label}: covered by ${r.hit}`);
  // The scroll that brought it into view must be one a finger can make: no overflow-hidden
  // ancestor may have been scrolled to reach it.
  const forced = await locator.evaluate((el) => {
    for (let a = el.parentElement; a; a = a.parentElement) {
      const s = getComputedStyle(a);
      if ((a.scrollTop || a.scrollLeft) && /hidden|clip/.test(s.overflowY + s.overflowX)) return `${a.tagName}.${a.className.split(' ')[0]}`;
    }
    return null;
  });
  assert.equal(forced, null, `${label}: only reachable by scrolling a clipped box`);
}

const phone = (vp) => vp.width <= 720;
for (const vp of only && only !== 'matrix' ? [] : viewports) {
  const tag = `${vp.width}x${vp.height}`;
  for (const state of ['populated', 'empty', 'error']) {
    // /agent
    await check(`/agent ${state} ${tag}`, async () => {
      const { context, page, errors } = await newPage(vp, state, '/agent');
      try {
        await page.locator('.composer textarea').waitFor();
        await page.waitForTimeout(300);
        await page.screenshot({ path: `${artifacts}/agent-${state}-${tag}.png` });
        await noOverflow(page, 'agent');
        await reachable(page.locator('.composer textarea'), 'composer');
        await reachable(page.locator('.composer .btn').last(), 'send');
        await reachable(page.locator('.composer-btn'), 'prompt picker');
        if (state === 'error') await reachable(page.locator('.err-banner .err-close'), 'error dismiss');
        if (state === 'empty') await reachable(page.getByRole('button', { name: 'Open settings' }), 'open settings');
        if (phone(vp)) {
          const picker = page.locator('.menu-btn');
          await reachable(picker, 'conversation picker');
          await picker.click();
          await page.locator('.sidebar.open').waitFor();
          await page.waitForTimeout(250);
          await reachable(page.locator('.side-head .btn'), 'new chat');
          await reachable(page.locator('.side-head .mcp-btn'), 'mcp link');
          await reachable(page.locator('.side-head button.icon'), 'agent settings');
          if (state === 'populated') {
            for (const name of ['Rename', 'Delete']) await reachable(page.locator('.conv-list li').last().getByTitle(name), `row ${name}`);
            await noOverflow(page, 'agent drawer');
          }
          await page.screenshot({ path: `${artifacts}/agent-${state}-${tag}-drawer.png` });
          await page.keyboard.press('Escape');
          await page.locator('.sidebar.open').waitFor({ state: 'detached' });
          if (state === 'populated') {
            await page.locator('.options-btn').click();
            await page.waitForTimeout(100);
            for (const sel of ['.model-pick', '.persona-wrap > .pick', '.tools-wrap > .pick', '.mem-toggle', '.chat-meta-controls button[title="Usage"]']) await reachable(page.locator(sel), sel);
            await page.locator('.tools-wrap > .pick').click();
            await noOverflow(page, 'tools popover');
            const pop = await page.locator('.tools-wrap .pick-pop').boundingBox();
            assert.ok(pop.x >= 0 && pop.x + pop.width <= vp.width + 1, 'tools popover inside viewport');
            await page.screenshot({ path: `${artifacts}/agent-${state}-${tag}-options.png` });
            await page.locator('.tools-wrap .pick-backdrop').click({ position: { x: 5, y: vp.height - 5 } });
            await page.locator('.options-btn').click();
          }
        } else {
          await reachable(page.locator('.side-head .btn'), 'new chat');
          if (state === 'populated') await reachable(page.locator('.model-pick'), 'model picker');
        }
        if (state === 'populated') {
          // Long replies: tables and code scroll in their own box; the thread scrolls.
          const thread = await page.locator('.thread').evaluate((el) => ({ sh: el.scrollHeight, ch: el.clientHeight }));
          assert.ok(thread.ch >= 90, `thread keeps a usable height (${thread.ch}px)`);
          assert.ok(thread.sh > thread.ch, 'thread scrolls');
          await page.locator('.thread').evaluate((el) => { el.scrollTop = 0; });
          await page.locator('.md-table-wrap').scrollIntoViewIfNeeded();
          const local = await page.evaluate(() => {
            const wrap = document.querySelector('.md-table-wrap');
            const pre = document.querySelector('.md pre');
            const msgs = document.querySelector('.msgs');
            return { wrapFits: wrap.getBoundingClientRect().right <= msgs.getBoundingClientRect().right + 1, preFits: pre.getBoundingClientRect().right <= msgs.getBoundingClientRect().right + 1, preScroll: getComputedStyle(pre).overflowX, wrapScroll: getComputedStyle(wrap).overflowX };
          });
          assert.ok(local.wrapFits && local.preFits, 'table and code stay inside the thread');
          assert.equal(local.wrapScroll, 'auto');
          assert.equal(local.preScroll, 'auto');
          await noOverflow(page, 'long reply');
          await page.screenshot({ path: `${artifacts}/agent-${state}-${tag}-reply.png` });
        }
        assert.deepEqual(errors, [], `page errors: ${errors.join(' | ')}`);
      } finally {
        await context.close();
      }
    });
    // /agent/mcp
    await check(`/agent/mcp ${state} ${tag}`, async () => {
      const { context, page, errors } = await newPage(vp, state, '/agent/mcp');
      try {
        await page.locator('.store h2').first().waitFor();
        await page.waitForTimeout(300);
        await page.screenshot({ path: `${artifacts}/mcp-${state}-${tag}.png` });
        await noOverflow(page, 'mcp');
        await reachable(page.getByRole('button', { name: 'Back' }), 'back');
        await reachable(page.getByRole('button', { name: 'Add custom server' }), 'add custom');
        if (state === 'populated') {
          const row = page.locator('.list li').first();
          await reachable(row.getByRole('button', { name: 'Test' }), 'test');
          for (const title of ['Edit', 'Delete']) await reachable(row.getByTitle(title), title);
        }
        await reachable(page.locator('.card .btn').first(), 'catalog connect');
        assert.deepEqual(errors, [], `page errors: ${errors.join(' | ')}`);
      } finally {
        await context.close();
      }
    });
  }
}

// Primary flows by tap at 390 px; the same flow at 1440 px gives the desktop payloads.
async function agentFlow(vp) {
  const { context, page, errors, captured } = await newPage(vp, 'populated', '/agent');
  const mobile = phone(vp);
  const tap = (loc) => (mobile ? loc.tap() : loc.click());
  try {
    await page.locator('.composer textarea').waitFor();
    if (mobile) await tap(page.locator('.menu-btn'));
    await tap(page.locator('.conv-open', { hasText: 'Short chat' }));
    await page.locator('.chat-head h1', { hasText: 'Short chat' }).waitFor();
    if (mobile) await page.locator('.sidebar.open').waitFor({ state: 'detached' });
    if (mobile) await tap(page.locator('.menu-btn'));
    await tap(page.locator('.side-head .btn'));
    await page.waitForTimeout(150);
    if (mobile) await page.locator('.composer textarea').tap();
    await page.locator('.composer textarea').fill('Hello from a phone');
    await Promise.all([page.waitForResponse((r) => r.url().endsWith('/run')), tap(page.getByRole('button', { name: 'Send' }))]);
    await page.waitForTimeout(300);
    if (mobile) await tap(page.locator('.menu-btn'));
    await tap(page.locator('.side-head button.icon'));
    await page.getByRole('dialog', { name: 'Agent settings' }).waitFor();
    if (mobile) await page.screenshot({ path: `${artifacts}/flow-agent-settings-${vp.width}.png` });
    await page.keyboard.press('Escape');
    await page.getByRole('dialog').waitFor({ state: 'detached' });
    if (mobile) {
      // The drawer is still open under the closed dialog; Escape closes it too.
      await page.keyboard.press('Escape');
      await page.locator('.sidebar.open').waitFor({ state: 'detached' });
      await tap(page.locator('.options-btn'));
      await tap(page.locator('.mem-toggle'));
      await page.waitForTimeout(150);
      await tap(page.locator('.menu-btn'));
    } else {
      // Desktop header: long fixture names push the toggle past the clipped header edge.
      await page.locator('.mem-toggle').evaluate((el) => el.click());
      await page.waitForTimeout(150);
    }
    if (!mobile) await page.locator('.conv-list li').last().hover();
    await tap(page.locator('.conv-list li').last().getByTitle('Delete'));
    const dialog = page.getByRole('dialog');
    await dialog.waitFor();
    if (mobile) {
      await reachable(dialog.getByRole('button', { name: 'Delete' }), 'confirm delete');
      await page.screenshot({ path: `${artifacts}/flow-agent-confirm-${vp.width}.png` });
    }
    await tap(dialog.getByRole('button', { name: 'Delete' }));
    await page.waitForTimeout(200);
    assert.deepEqual(errors, [], `page errors: ${errors.join(' | ')}`);
    return captured.map((c) => `${c.method} ${c.path.replace(/conv-[a-z0-9]+/, ':id')} ${JSON.stringify(c.body)}`);
  } catch (e) {
    await page.screenshot({ path: `${artifacts}/flow-agent-failure-${vp.width}.png` });
    throw e;
  } finally {
    await context.close();
  }
}

async function mcpFlow(vp) {
  const { context, page, errors, captured } = await newPage(vp, 'populated', '/agent/mcp');
  const mobile = phone(vp);
  const tap = (loc) => (mobile ? loc.tap() : loc.click());
  try {
    await page.locator('.list li').first().waitFor();
    const row = page.locator('.list li').first();
    await tap(row.getByRole('button', { name: 'Test' }));
    await row.locator('.test.ok').waitFor();
    if (mobile) await noOverflow(page, 'test result');
    await tap(page.getByRole('button', { name: 'Add custom server' }));
    await page.locator('.store > .form').waitFor();
    await page.getByLabel('Name').fill('Phone server');
    await page.getByLabel('URL').fill('https://mcp.example.com/a/very/long/path/that/keeps/going/for/a/while/mcp');
    if (mobile) await noOverflow(page, 'add form');
    const saveBtn = page.locator('.store > .form').getByRole('button', { name: 'Save', exact: true });
    await reachable(saveBtn, 'save server');
    await tap(saveBtn);
    await page.locator('.store > .form').waitFor({ state: 'detached' });
    await tap(row.getByTitle('Delete'));
    let dialog = page.getByRole('dialog');
    await dialog.waitFor();
    await reachable(dialog.getByRole('button', { name: 'Delete' }), 'confirm delete server');
    await tap(dialog.getByRole('button', { name: 'Delete' }));
    await dialog.waitFor({ state: 'detached' });
    await tap(page.locator('.card .btn').first());
    dialog = page.getByRole('dialog', { name: 'Connect OpenTraderWorld' });
    await dialog.waitFor();
    await dialog.locator('.permrow').first().waitFor();
    if (mobile) {
      await noOverflow(page, 'connect dialog');
      await page.screenshot({ path: `${artifacts}/flow-mcp-connect-${vp.width}.png` });
    }
    await reachable(dialog.getByRole('button', { name: 'Connect' }), 'connect');
    await tap(dialog.getByRole('button', { name: 'Connect' }));
    await dialog.locator('.tok').waitFor();
    if (mobile) {
      await noOverflow(page, 'token view');
      const tok = await dialog.locator('.tok').evaluate((el) => el.scrollWidth <= el.clientWidth + 1);
      assert.ok(tok, 'token wraps inside its box');
      await reachable(dialog.getByRole('button', { name: 'Copy' }), 'copy token');
      await reachable(dialog.getByRole('button', { name: 'Done' }), 'done');
      await page.screenshot({ path: `${artifacts}/flow-mcp-token-${vp.width}.png` });
    }
    assert.deepEqual(errors, [], `page errors: ${errors.join(' | ')}`);
    return captured.map((c) => `${c.method} ${c.path} ${JSON.stringify(c.body)}`);
  } catch (e) {
    await page.screenshot({ path: `${artifacts}/flow-mcp-failure-${vp.width}.png` });
    throw e;
  } finally {
    await context.close();
  }
}

if (!only || only === 'flow') {
  for (const [name, flow] of [['agent', agentFlow], ['mcp', mcpFlow]]) {
    await check(`${name} flow by tap at 390 matches 1440 payloads`, async () => {
      const desktop = await flow({ width: 1440, height: 900 });
      const mobileCtx = { width: 390, height: 844 };
      const mobile = await flow(mobileCtx);
      assert.deepEqual(mobile, desktop);
      results.push(`     ${name} payloads: ${mobile.join(' | ')}`);
    });
  }
  // The keyboard case: type and send with a short viewport.
  await check('send with keyboard-height viewport 390x500', async () => {
    const { context, page, captured } = await newPage({ width: 390, height: 500 }, 'populated', '/agent');
    try {
      await page.locator('.composer textarea').tap();
      await page.locator('.composer textarea').fill('Short screen message');
      await reachable(page.getByRole('button', { name: 'Send' }), 'send at 500');
      await Promise.all([page.waitForResponse((r) => r.url().endsWith('/run')), page.getByRole('button', { name: 'Send' }).tap()]);
      const run = captured.find((c) => c.path.endsWith('/run'));
      assert.deepEqual(run.body, { message: 'Short screen message' });
    } finally {
      await context.close();
    }
  });
}

// Desktop no-regression: baseline server (clean HEAD) against the working tree.
for (const vp of only && only !== 'desktop' ? [] : [{ width: 1440, height: 900 }, { width: 1100, height: 800 }]) {
  for (const state of ['populated', 'empty', 'error']) {
    for (const url of ['/agent', '/agent/mcp']) {
      await check(`desktop identical ${url} ${state} ${vp.width}x${vp.height}`, async () => {
        const shoot = async (root) => {
          const { context, page } = await newPage(vp, state, url, root);
          try {
            await page.waitForTimeout(500);
            return await page.screenshot({ animations: 'disabled', caret: 'hide' });
          } finally {
            await context.close();
          }
        };
        // Up to three pairs: a pair that matches byte for byte passes.
        let last;
        for (let attempt = 0; attempt < 3; attempt++) {
          const pair = [await shoot(baseline), await shoot(base)];
          if (pair[0].equals(pair[1])) {
            if (attempt) results.push(`     matched on pair ${attempt + 1}; earlier pair: ${JSON.stringify(pixelDiff(...last))}`);
            return;
          }
          last = pair;
        }
        const name = `desktop-${url.replaceAll('/', '_')}-${state}-${vp.width}`;
        await writeFile(`${artifacts}/${name}-baseline.png`, last[0]);
        await writeFile(`${artifacts}/${name}-current.png`, last[1]);
        const d = pixelDiff(last[0], last[1]);
        // Rasterisation noise check: the baseline against itself. A one-level difference that
        // the clean HEAD also shows against itself is antialiasing, not a layout change.
        let self = null;
        for (let attempt = 0; attempt < 3 && !self; attempt++) {
          const a = await shoot(baseline);
          const b = await shoot(baseline);
          if (!a.equals(b)) self = pixelDiff(a, b);
        }
        if (self && d.max <= 1) {
          results.push(`     noise only: current vs baseline ${d.count} px max delta ${d.max}; baseline vs itself ${self.count} px max delta ${self.max}`);
          return;
        }
        throw new Error(`screenshots differ: ${d.count} px, max channel delta ${d.max}`);
      });
    }
  }
}

await browser.close();
for (const line of results) console.log(line);
for (const line of failures) console.log(line);
console.log(`${results.filter((l) => l.startsWith('ok')).length} passed, ${failures.length} failed. Screenshots: ${artifacts}`);
if (failures.length) process.exitCode = 1;
