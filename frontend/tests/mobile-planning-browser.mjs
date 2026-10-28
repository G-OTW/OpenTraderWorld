// Planning routes on phones and tablets: automator, calendar, remindme, todos and time.
// Fixture-backed (no backend). Run with the frontend dev server:
//   node tests/mobile-planning-browser.mjs [base URL] [baseline URL]
// The baseline URL (default http://127.0.0.1:4190) serves the unmodified tree for the
// desktop screenshot comparison at 1440 and 1100 px.
import assert from 'node:assert/strict';
import { mkdir, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const baseline = process.argv[3] ?? 'http://127.0.0.1:4190';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-planning/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const en = JSON.parse(await readFile(new URL('../src/lib/i18n/en.json', import.meta.url), 'utf8'));
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });

const VIEWPORTS = [[320, 568], [390, 844], [600, 800], [667, 375], [768, 1024]];
const NOW = new Date('2026-10-20T10:00:00+02:00');
const iso = (offsetHours) => new Date(NOW.getTime() + offsetHours * 3600_000).toISOString();
const LONG = 'Quarterly portfolio rebalance and broker statement reconciliation for the international margin account';
const TOKEN = 'AAPL,MSFT,NVDA,AMZN,GOOGL,META,TSLA,BRK.B,JPM,V,UNH,XOM,JNJ,WMT,MA,PG,AVGO,HD,CVX,MRK,ABBV,KO,PEP,COST';
const LONG_ERR = `upstream returned 502 Bad Gateway for https://api.example-broker.com/v3/accounts/U1234567890/positions?include=greeks,margin,fx&cursor=${'x'.repeat(80)}`;

// ── Fixtures ────────────────────────────────────────────────────────────────
const workflows = [
  { id: 'wf-1', name: LONG, description: 'Pulls the latest fills, reshapes them and posts a digest to the trading channel every weekday morning.', enabled: true, favorite: true, updated_at: iso(-30), version_id: 'v-new', max_runtime_secs: 300, mcp_token_id: 'tok-1', draft: null },
  { id: 'wf-2', name: 'Daily FX snapshot', description: '', enabled: false, favorite: false, updated_at: iso(-90), version_id: 'v-2', max_runtime_secs: 120, mcp_token_id: null, draft: null }
];
const schedules = [
  { id: 's-1', workflow_id: 'wf-1', kind: 'weekly', timezone: 'America/Argentina/ComodRivadavia', every_minutes: 60, at_hour: 8, at_minute: 30, weekdays: 31, day_of_month: 1, run_at: null, active: true, catch_up: true, next_run_at: iso(22), last_run_at: iso(-2), version_id: 'v-old' },
  { id: 's-2', workflow_id: 'wf-2', kind: 'interval', timezone: 'UTC', every_minutes: 90, at_hour: 0, at_minute: 0, weekdays: 0, day_of_month: 1, run_at: null, active: false, catch_up: false, next_run_at: null, last_run_at: null, version_id: null }
];
const graph = {
  nodes: [
    { id: 'n1', kind: 'api', name: 'Fetch the latest journal fills for every connected broker account', pos: { x: 0, y: 0 }, config: { method: 'GET', path: '/api/journal/trades' }, on_error: 'stop', retry: { count: 2, backoff_secs: 5 }, timeout_secs: null },
    { id: 'n2', kind: 'transform', name: 'Reshape', pos: { x: 0, y: 1 }, config: { op: 'pick' }, on_error: 'continue', retry: { count: 0, backoff_secs: 5 }, timeout_secs: null },
    { id: 'n3', kind: 'notify', name: 'Digest', pos: { x: 1, y: 1 }, config: { title: 'Morning digest' }, on_error: 'stop', retry: { count: 0, backoff_secs: 5 }, timeout_secs: null }
  ],
  edges: [{ from: 'n1', to: 'n2', port: 'out' }, { from: 'n2', to: 'n3', port: 'out' }]
};
const runs = [
  { id: 'r-1', workflow_id: 'wf-1', mode: 'live', trigger: 'schedule', status: 'failed', started_at: iso(-2), duration_ms: 12345, error: LONG_ERR },
  { id: 'r-2', workflow_id: 'wf-1', mode: 'live', trigger: 'manual', status: 'running', started_at: iso(-0.1), duration_ms: null, error: null },
  { id: 'r-3', workflow_id: 'wf-2', mode: 'test', trigger: 'test', status: 'ok', started_at: iso(-26), duration_ms: 842, error: null }
];
const steps = [
  { seq: 1, node_id: 'n1', node_name: graph.nodes[0].name, kind: 'api', status: 'ok', duration_ms: 420, request: { method: 'GET', path: '/api/journal/trades', query: { symbols: TOKEN } }, output: { rows: [{ symbol: 'AAPL', qty: -1200, pnl: -48213.55 }], cursor: 'x'.repeat(120) } },
  { seq: 2, node_id: 'n2', node_name: 'Reshape', kind: 'transform', status: 'failed', duration_ms: 3, error: LONG_ERR, request: {}, output: null },
  { seq: 3, node_id: 'n3', node_name: 'Digest', kind: 'notify', status: 'skipped', duration_ms: 0, request: {}, output: null }
];
const jobs = [
  { id: 'journal_sync:ibkr', kind: 'journal_sync', target: 'Interactive Brokers', account: 'U1234567-LONG-ACCOUNT-IDENTIFIER-00000000', link: '/journal', paused: false, conflicts: 2, interval_editable: true, interval_minutes: 60, running: false, last_run_at: iso(-1), last_status: 'failed', last_error: LONG_ERR, last_summary: null, next_run_at: iso(1), deletable: true },
  { id: 'fx', kind: 'fx', target: '', account: '', link: '/settings', paused: true, conflicts: 0, interval_editable: false, interval_minutes: 720, running: false, last_run_at: iso(-12), last_status: 'ok', last_error: null, last_summary: 'Fetched 32 rates against USD, EUR, GBP, JPY and CHF', next_run_at: iso(12), deletable: false }
];
const jobRuns = [
  { id: 'jr-1', started_at: iso(-1), status: 'failed', trigger: 'schedule', error: LONG_ERR, summary: null },
  { id: 'jr-2', started_at: iso(-2), status: 'ok', trigger: 'manual', error: null, summary: 'Imported 1,284 fills and 3 corrections' }
];
const calEvents = [
  { id: 'ev-1', title: LONG, start_at: '2026-10-20T09:00:00', end_at: '2026-10-20T10:30:00', all_day: false, category: 'Macro', color: '', location: 'Frankfurt', notes: 'ECB press conference' },
  { id: 'ev-2', title: 'Options expiry', start_at: '2026-10-23', end_at: null, all_day: true, category: 'Markets', color: '', location: '', notes: '' },
  ...Array.from({ length: 5 }, (_, i) => ({ id: `ev-m${i}`, title: `Earnings call ${i + 1}`, start_at: `2026-10-21T${String(13 + i).padStart(2, '0')}:00:00`, end_at: null, all_day: false, category: 'Earnings', color: '', location: '', notes: '' }))
];
const reminders = [
  { id: 'rem-1', name: LONG, kind: 'todo', linked_id: 'td-1', details: TOKEN, url: 'https://example.com/a/very/long/path/that/never/breaks/naturally/statement-2026-10.pdf', link_label: '', frequency: 'weekly', weekdays: 21, start_date: '2026-10-21', start_time: '08:30', end_date: '2027-03-31', max_count: 12, fired_count: 3, active: true, next_fire_at: '2026-10-21T06:30:00Z' },
  { id: 'rem-2', name: 'Check margin requirement', kind: 'custom', linked_id: null, details: '', url: '', link_label: '', frequency: 'once', weekdays: null, start_date: '2026-10-25', start_time: '18:00', end_date: null, max_count: null, fired_count: 0, active: false, next_fire_at: null },
  { id: 'rem-3', name: 'Monthly dividend review', kind: 'custom', linked_id: null, details: '', url: '', link_label: '', frequency: 'monthly', weekdays: null, start_date: '2026-09-01', start_time: '00:00', end_date: null, max_count: null, fired_count: 2, active: true, next_fire_at: null }
];
const goals = [{ id: 'g-1', name: 'Max drawdown under 8%' }];
const todos = [
  { id: 'td-1', name: LONG, due_date: '2026-10-18', due_time: '14:00', details: `Bring the statements.\n${TOKEN}`, category: 'Admin', done: false },
  { id: 'td-2', name: 'Call the broker about the margin call', due_date: '2026-10-20', due_time: null, details: '', category: 'Brokerage', done: false },
  { id: 'td-3', name: 'Renew the data feed subscription', due_date: '2026-11-02', due_time: null, details: '', category: '', done: false },
  { id: 'td-4', name: 'Archive 2025 statements', due_date: null, due_time: null, details: '', category: 'Admin', done: true }
];
const notifications = [
  { id: 'nt-1', name: LONG, kind: 'todo', linked_id: 'td-1', created_at: iso(-1), read_at: null, details: TOKEN, url: 'https://example.com/a/very/long/path/that/never/breaks/naturally/statement-2026-10.pdf', link_label: '' },
  { id: 'nt-2', name: 'AAPL crossed 250.00', kind: 'watchlist', linked_id: null, created_at: iso(-30), read_at: iso(-29), details: '', url: '/watchlists', link_label: 'Open the watchlist' }
];
const projects = [
  { id: 1, name: LONG, category: 'Client work', color: '#7c6cf0', running: true, tracked_seconds: 98765, time_budget_hours: 30, hourly_rate: 85, rate_currency: 'EUR', planned_end: '2026-11-30', position: 1, archived: false },
  { id: 2, name: 'Research', category: '', color: '', running: false, tracked_seconds: 19800, time_budget_hours: null, hourly_rate: null, rate_currency: 'USD', planned_end: null, position: 2, archived: false },
  { id: 3, name: 'Admin', category: 'Internal', color: '#e0a030', running: false, tracked_seconds: 600, time_budget_hours: 0.1, hourly_rate: 1250000, rate_currency: 'JPY', planned_end: null, position: 3, archived: false }
];
const entries = [
  { id: 11, started_at: iso(-30), ended_at: iso(-27.5), note: `Reconciled ${TOKEN}` },
  { id: 12, started_at: iso(-1), ended_at: null, note: '' }
];
const breakdown = {
  points: [
    { bucket: '2026-10-18', hours: 3.5, value: 297.5, by_project: { 1: 3, 2: 0.5 }, value_by_project: { 1: 255, 2: 42.5 } },
    { bucket: '2026-10-19', hours: 6.25, value: 1234567.89, by_project: { 1: 4, 3: 2.25 }, value_by_project: { 1: 340, 3: 1234227.89 } }
  ],
  projects: [
    { id: 1, name: LONG, color: '#7c6cf0', hours: 7, value: 595 },
    { id: 3, name: 'Admin', color: '#e0a030', hours: 2.25, value: 1234227.89 },
    { id: 2, name: 'Research', color: '', hours: 0.5, value: 0 }
  ],
  total_hours: 9.75,
  total_value: 1234822.89
};

const SHELL = (p) => {
  if (p === '/api/setup/status') return { configured: true };
  if (p === '/api/settings/me') return { username: 'fixture', role: 'admin', locale: 'en' };
  if (p === '/api/settings/modules') return { installed: ['automator', 'calendar', 'remindme', 'todos', 'time', 'goals', 'settings'] };
  if (p === '/api/settings/defaults') return { default_timezone: 'Europe/Paris', display_currency: 'USD' };
  if (p === '/api/settings/versioning') return { modules: {} };
  if (p === '/api/mcp/settings') return { enabled: false };
  if (p === '/api/health') return { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
  if (p === '/api/dashboard/layout') return { pages: [] };
  if (p === '/api/tasks') return { tasks: [] };
  if (p === '/api/notifications/unread') return { notifications: [], unread: 0 };
  if (p.startsWith('/api/notif-channels')) return { channels: [], modules: [] };
  return undefined;
};

// Module routes whose data a 500 fixture breaks; the shell keeps answering.
const MODULE_PREFIXES = ['/api/automator', '/api/jobs', '/api/calendar', '/api/reminders', '/api/todos', '/api/time', '/api/goals'];

function moduleData(method, p, url, body, mode) {
  const empty = mode === 'empty';
  if (p === '/api/automator/workflows' && method === 'GET') return empty ? { workflows: [], schedules: [], last_runs: {} } : { workflows, schedules, last_runs: { 'wf-1': { status: 'failed', at: iso(-2) }, 'wf-2': { status: 'ok', at: iso(-26) } } };
  if (p === '/api/automator/workflows' && method === 'POST') return { workflow: { ...workflows[1], id: 'wf-new', name: body?.name } };
  if (p.startsWith('/api/automator/workflows/wf-')) {
    const id = p.split('/')[4];
    const wf = workflows.find((w) => w.id === id) ?? workflows[0];
    if (method === 'PUT') return { workflow: { ...wf, graph: body?.graph }, draft: false };
    if (method === 'PATCH') return { workflow: { ...wf, ...body } };
    return { workflow: { ...wf, graph: empty ? { nodes: [], edges: [] } : graph }, schedules: empty ? [] : schedules.filter((s) => s.workflow_id === wf.id), tokens: [{ id: 'tok-1', name: 'Automation token' }], draft_valid: false };
  }
  if (p === '/api/automator/catalog') return { endpoints: empty ? [] : [{ module: 'journal', method: 'GET', path: '/api/journal/trades/{id}/legs/with/a/very/long/path/segment', desc: 'Legs of one trade' }, { module: 'todos', method: 'POST', path: '/api/todos', desc: 'Create a todo' }], modules: [{ id: 'journal', label: 'Journal' }, { id: 'todos', label: 'ToDo' }] };
  if (p === '/api/automator/timezones') return { timezones: ['UTC', 'Europe/Paris', 'America/Argentina/ComodRivadavia'] };
  if (p === '/api/automator/schedules' && method === 'GET') return empty ? { schedules: [], workflows: [] } : { schedules, workflows };
  if (p.startsWith('/api/automator/schedules')) return { schedule: schedules[0] };
  if (p === '/api/automator/agenda') {
    if (empty) return { events: [], truncated: false };
    const from = new Date(url.searchParams.get('from'));
    const to = new Date(url.searchParams.get('to'));
    const events = [];
    for (let d = new Date(from); d < to; d.setDate(d.getDate() + 1)) {
      for (const [h, wf] of [[8, workflows[0]], [9, workflows[1]], [14, workflows[1]]]) {
        const at = new Date(d);
        at.setHours(h, 30, 0, 0);
        events.push({ workflow: wf.name, workflow_id: wf.id, at: at.toISOString() });
      }
    }
    return { events, truncated: false };
  }
  if (p === '/api/automator/runs') return empty ? { runs: [], workflows: [] } : { runs, workflows };
  if (p.startsWith('/api/automator/runs/') && p.endsWith('/cancel')) return {};
  if (p.startsWith('/api/automator/runs/')) return { run: runs[0], steps, workflow: workflows[0] };
  if (p.startsWith('/api/automator/workflows/') && p.endsWith('/test')) return { steps };
  if (p === '/api/jobs') return { jobs: empty ? [] : jobs };
  if (p === '/api/jobs/runs') return { runs: jobRuns };
  if (p.startsWith('/api/jobs/')) return {};
  if (p === '/api/calendar/events' && method === 'GET') return { events: empty ? [] : calEvents };
  if (p === '/api/calendar/events' && method === 'POST') return { event: { id: 'ev-new', ...body } };
  if (p.startsWith('/api/calendar/events/')) return { event: calEvents.find((e) => e.id === p.split('/').pop()) ?? calEvents[0] };
  if (p === '/api/reminders' && method === 'GET') return { reminders: empty ? [] : reminders };
  if (p === '/api/reminders' && method === 'POST') return { reminder: { id: 'rem-new', ...body } };
  if (p.startsWith('/api/reminders/')) return {};
  if (p === '/api/goals') return { goals: empty ? [] : goals };
  if (p === '/api/todos' && method === 'GET') return { todos: empty ? [] : todos };
  if (p === '/api/todos' && method === 'POST') return { todo: { id: 'td-new', ...body } };
  if (p.startsWith('/api/todos/')) return {};
  if (p === '/api/notifications') return empty ? { notifications: [], unread: 0 } : { notifications, unread: 1 };
  if (p.startsWith('/api/notifications/')) return {};
  if (p === '/api/time/projects' && method === 'GET') return { projects: empty ? [] : projects };
  if (p === '/api/time/projects' && method === 'POST') return { id: 99 };
  if (/^\/api\/time\/projects\/\d+\/entries$/.test(p) && method === 'GET') return { entries };
  if (p === '/api/time/state') return { state: { display_currency: 'USD', any_running: !empty, last_seen_at: iso(-1) } };
  if (p === '/api/time/breakdown') return { breakdown: empty ? { points: [], projects: [], total_hours: 0, total_value: 0 } : breakdown };
  if (p === '/api/time/settings') return { state: { display_currency: body?.display_currency ?? 'USD', any_running: true, last_seen_at: iso(-0.01) } };
  if (p.startsWith('/api/time/')) return {};
  return {};
}

/** A context whose API answers from the fixtures; `ctl.mode` switches the data set and
 *  `ctl.requests` records every write with its JSON body. */
async function fixtureContext({ width = 390, height = 844, hasTouch = false } = {}) {
  const context = await browser.newContext({ viewport: { width, height }, locale: 'en-US', timezoneId: 'Europe/Paris', hasTouch });
  const ctl = { mode: 'populated', requests: [], context };
  await context.route('**/api/**', async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const p = url.pathname;
    const method = req.method();
    let body = null;
    try {
      body = req.postData() ? JSON.parse(req.postData()) : null;
    } catch {
      body = req.postData();
    }
    if (method !== 'GET') ctl.requests.push({ method, path: p, body });
    const shell = SHELL(p);
    if (shell !== undefined) return route.fulfill({ json: shell });
    if (ctl.mode === 'error' && method === 'GET' && (MODULE_PREFIXES.some((m) => p.startsWith(m)) || p === '/api/notifications')) {
      return route.fulfill({ status: 500, json: { error: 'fixture failure' } });
    }
    return route.fulfill({ json: moduleData(method, p, url, body, ctl.mode) });
  });
  return ctl;
}

async function open(page, path) {
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      await page.goto(`${base}${path}`, { waitUntil: 'domcontentloaded', timeout: 20000 });
      await page.waitForFunction(() => document.body.innerText.trim().length > 20, undefined, { timeout: 10000 });
      await page.waitForLoadState('networkidle', { timeout: 4000 }).catch(() => {});
      await page.waitForTimeout(250);
      return;
    } catch (e) {
      if (attempt) throw e;
    }
  }
}

// Scrollers allowed to pan sideways: genuinely two-dimensional content (a calendar
// grid, a JSON payload, a time-entry table) and the sideways chip and nav rows.
const LOCAL_SCROLLERS = ['pre', '.calendar-scroll', '.fc-scroller', '.fc-host', '.entries-wrap', 'table.runs', 'table.rank', '.chips.cat', '.modnav', '.tabs'];

async function overflow(page) {
  return page.evaluate((allowed) => {
    const root = document.documentElement;
    const out = { scroll: root.scrollWidth, client: root.clientWidth, offenders: [], inner: [] };
    if (out.scroll > out.client + 1) {
      for (const el of document.querySelectorAll('body *')) {
        const r = el.getBoundingClientRect();
        if (r.right > out.client + 1 && r.width > 0) out.offenders.push(`${el.tagName.toLowerCase()}.${[...el.classList].join('.')} right=${Math.round(r.right)}`);
        if (out.offenders.length > 8) break;
      }
    }
    // A page container that scrolls vertically also scrolls sideways: the document then
    // looks fine while the content pans inside it. Report those too.
    for (const el of document.querySelectorAll('main *, .app *')) {
      if (el.scrollWidth <= el.clientWidth + 1 || !el.clientWidth) continue;
      const ox = getComputedStyle(el).overflowX;
      if (ox !== 'auto' && ox !== 'scroll') continue;
      if (allowed.some((sel) => el.matches(sel) || el.closest(sel))) continue;
      if (el.closest('.fc')) continue;
      const edge = el.getBoundingClientRect().right;
      const wide = [...el.querySelectorAll('*')].filter((c) => c.getBoundingClientRect().right > edge + 1 && ![...c.children].some((k) => k.getBoundingClientRect().right > edge + 1)).slice(0, 3);
      out.inner.push(`${el.tagName.toLowerCase()}.${[...el.classList].join('.')} ${el.scrollWidth}>${el.clientWidth} (${wide.map((c) => `${c.tagName.toLowerCase()}.${[...c.classList].join('.')}`).join(' ')})`);
    }
    return out;
  }, LOCAL_SCROLLERS);
}

/** Scroll the control into view, then prove a tap at its centre lands on it and that it
 *  is drawn (no zero opacity on it or an ancestor). */
async function reachable(page, locator, label) {
  const el = locator.first();
  await el.waitFor({ state: 'attached', timeout: 5000 });
  await el.scrollIntoViewIfNeeded({ timeout: 5000 });
  const probe = (centre) => el.evaluate((node, centre) => {
    // A floating shell control (assistant, quick search) may sit over the control where
    // the browser left it; a user scrolls it clear, so try again from the middle.
    if (centre) node.scrollIntoView({ block: 'center', inline: 'center' });
    // An inline link wrapping over two lines has a box whose middle is empty space.
    const r = node.getClientRects()[0] ?? node.getBoundingClientRect();
    const cx = r.left + r.width / 2;
    const cy = r.top + r.height / 2;
    const vw = document.documentElement.clientWidth;
    const vh = window.innerHeight;
    const hit = document.elementFromPoint(cx, cy);
    let faded = false;
    for (let n = node; n && n !== document.body; n = n.parentElement) {
      if (Number(getComputedStyle(n).opacity) < 0.1) faded = true;
    }
    return { inside: cx >= 0 && cx <= vw && cy >= 0 && cy <= vh && r.width > 0, hits: !!hit && (hit === node || node.contains(hit)), faded, hit: hit ? `${hit.tagName}.${hit.className}` : null, w: r.width, h: r.height };
  }, centre);
  let res = await probe(false);
  if (!res.hits && !res.faded) res = await probe(true);
  assert.ok(res.inside && res.hits && !res.faded, `${label} not reachable: ${JSON.stringify(res)}`);
  return res;
}

/** Shape of a payload: keys and value types, recursively; arrays by their first item. */
function shape(v) {
  if (Array.isArray(v)) return v.length ? [shape(v[0])] : [];
  if (v && typeof v === 'object') return Object.fromEntries(Object.keys(v).sort().map((k) => [k, shape(v[k])]));
  return v === null ? 'null' : typeof v;
}

const t = (key) => en[key] ?? key;
const btn = (page, key, scope = page) => scope.getByRole('button', { name: t(key), exact: true });

// Routes, what proves they rendered (populated), and the actions to reach at each width.
const ROUTES = [
  {
    path: '/automator', ready: '.card',
    actions: (page) => [
      ['new workflow', btn(page, 'automator.new')],
      ['favorite', page.locator('.card .star')],
      ['run now', page.locator('.card .actions button').first()],
      ['edit', page.locator('.card .actions a')],
      ['delete', page.locator('.card .actions button.danger-hover')]
    ]
  },
  {
    path: '/automator/wf-1', ready: '.board .task',
    actions: (page, w) => [
      ['add schedule', btn(page, 'automator.addSchedule')],
      ['settings', btn(page, 'automator.settingsBtn')],
      ['test run', btn(page, 'automator.testRun')],
      ['run now', btn(page, 'automator.runNow')],
      ['repoint save', btn(page, 'automator.repointBtn')],
      ['schedule rule', page.locator('.rulebar .rulebtn')],
      ['task', page.locator('.board .task')],
      ['task delete', page.locator('.board .task .kill')],
      ['move down', page.locator('.mobile-move button').nth(1)],
      ['own step', page.locator('.mobile-move button.split').first()],
      ...(w < 768 ? [['blocks pane', page.locator('.mobile-workspace button').nth(1)]] : [['palette block', page.locator('.rail .block')]])
    ]
  },
  {
    path: '/automator/jobs', ready: 'table.tbl',
    actions: (page) => [
      ['interval', page.locator('td.interval .trigger')],
      ['run now', page.locator('td.actions button').first()],
      ['pause', page.locator('td.actions button').nth(1)],
      ['history', page.locator('td.actions button').nth(2)],
      ['delete', page.locator('td.actions button').nth(3)]
    ]
  },
  {
    path: '/automator/runs', ready: 'table.tbl',
    actions: (page) => [
      ['search', page.getByRole('textbox', { name: t('automator.runs.searchPlaceholder') })],
      ['workflow filter', page.locator('.filter .trigger, .filter button').first()],
      ['run link', page.locator('td.strong a')],
      ['stop', page.locator('td.actions button')],
      ['open', page.locator('td.actions a')]
    ]
  },
  {
    path: '/automator/runs/r-1', ready: '.trace',
    actions: (page) => [
      ['back', page.locator('a.back')],
      ['step', page.locator('.trace .row').first()],
      ['failed step', page.locator('.trace .row').nth(1)]
    ]
  },
  {
    path: '/automator/schedule', ready: 'table.tbl',
    actions: (page) => [
      ['workflow link', page.locator('td.strong a')],
      ['pause', page.locator('td.actions button').first()],
      ['delete', page.locator('td.actions button.danger-hover')]
    ]
  },
  {
    path: '/automator/agenda', ready: '.cal .fc-view',
    actions: (page, w) => [
      ['today', page.locator('.today-button')],
      ['previous', page.getByRole('button', { name: t('calendar.toolbar.previous') })],
      ['next', page.getByRole('button', { name: t('calendar.toolbar.next') })],
      ['month', page.locator('.view-switch button').nth(0)],
      ['week', page.locator('.view-switch button').nth(1)],
      ...(w < 768 ? [['day', page.locator('.view-switch button.mobile-day')]] : []),
      ['filter', page.locator('.filter .trigger, .filter button').first()]
    ]
  },
  {
    path: '/calendar', ready: '.fc-host .fc-view',
    actions: (page) => [
      ['add event', page.locator('.head .primary')],
      ['previous', page.getByRole('button', { name: t('calendar.toolbar.previous') })],
      ['today', page.locator('.seg .today')],
      ['next', page.getByRole('button', { name: t('calendar.toolbar.next') })],
      ['day view', page.locator('.views .chip').nth(3)],
      ['zoom in', page.getByRole('button', { name: t('calendar.toolbar.zoomIn') })],
      ['overlay', page.locator('.ov').first()]
    ]
  },
  {
    path: '/remindme', ready: 'table.tbl',
    actions: (page) => [
      ['add reminder', page.locator('.head-actions .primary')],
      ['filter', page.locator('.seg-btn').nth(1)],
      ['type filter', page.locator('.type-sel .trigger')],
      ['sort', page.locator('.controls .sort').first()],
      ['collapse all', page.locator('.controls-right .sort').nth(1)],
      ['month toggle', page.locator('.month-head').first()],
      ['edit', page.locator('.row-actions button').first()],
      ['delete', page.locator('.row-actions button.danger-hover').first()],
      ['link', page.locator('.urllink').first()]
    ]
  },
  {
    path: '/remindme/notifications', ready: '.list .item',
    actions: (page) => [
      ['back', page.locator('.back')],
      ['acknowledge all', page.locator('.tools .ack').first()],
      ['clear all', page.locator('.tools .ack.clear')],
      ['open', page.locator('.item .body').first()],
      ['mark read', page.locator('.item .actions button').first()],
      ['delete', page.locator('.item .actions button.danger-hover').first()],
      ['link', page.locator('.item .urllink').first()]
    ]
  },
  {
    path: '/todos', ready: '.list .item',
    actions: (page) => [
      ['add task', page.locator('.head-actions .primary')],
      ['timeline toggle', page.locator('.tl-toggle')],
      ['status chip', page.locator('.toolbar .chip').nth(1)],
      ['sort', page.locator('.toolbar .chip.sort')],
      ['category chip', page.locator('.chips.cat .chip').nth(1)],
      ['check', page.locator('.list .item .check').first()],
      ['note', page.locator('.body.as-button').first()],
      ['edit', page.locator('.list .item .actions button').first()],
      ['delete', page.locator('.list .item .actions button.danger-hover').first()],
      ['done toggle', page.locator('.done-toggle')]
    ]
  },
  {
    path: '/time', ready: '.cards .card',
    prepare: async (page) => {
      await page.locator('.cards .card').first().click();
      await page.locator('.details .add-range').waitFor();
    },
    actions: (page) => [
      ['tab breakdown', page.locator('.tabs button').nth(1)],
      ['add project', page.locator('.head-right .primary')],
      ['currency', page.locator('.cur-select .trigger')],
      ['timer toggle', page.locator('.card .toggle').first()],
      ['card edit', page.locator('.card .card-actions button').first()],
      ['card delete', page.locator('.card .card-actions button').nth(1)],
      ['move down', page.locator('.card .card-move button').nth(1)],
      ['move up', page.locator('.cards .card').nth(1).locator('.card-move button').first()],
      ['resume keep', page.locator('.resume .ghost')],
      ['range start', page.locator('.add-range input[type=datetime-local]').first()],
      ['range mode', page.locator('.add-range .seg button').nth(1)],
      ['add range', page.locator('.add-range .add-btn')],
      ['delete range', page.locator('table.entries .row-actions button').first()],
      ['close details', page.locator('.details .close')]
    ]
  },
  {
    path: '/time', name: '/time#breakdown', ready: 'table.rank',
    prepare: async (page) => {
      await page.locator('.tabs button').nth(1).click();
      await page.locator('table.rank').waitFor();
      await page.locator('.rank-row .name').first().click();
      await page.locator('table.runs').waitFor();
    },
    actions: (page) => [
      ['bucket', page.locator('.filters .trigger').first()],
      ['since', page.locator('.filters input[type=date]').first()],
      ['metric', page.locator('.chart-toggles .seg button').nth(1)],
      ['rank by', page.locator('.rank-card .seg button').nth(1)],
      ['rank row', page.locator('.rank-row .name').first()],
      ['tab timers', page.locator('.tabs button').first()]
    ]
  }
];

const ONLY_MODES = process.env.PLANNING_MODES?.split(',');
const ONLY_ROUTES = process.env.PLANNING_ROUTES?.split(',');
const SKIP = process.env.PLANNING_SKIP?.split(',') ?? [];

const failures = [];
const results = [];
function check(label, fn) {
  return fn().then(
    () => results.push(`ok   ${label}`),
    (e) => {
      const at = (e.stack ?? '').split('\n').find((l) => l.includes('mobile-planning-browser.mjs'))?.trim() ?? '';
      failures.push(`${label}: ${e.message.split('\n')[0]} ${at}`);
      results.push(`FAIL ${label}`);
    }
  );
}

// ── 1. Route x state x viewport sweep ───────────────────────────────────────
for (const mode of ['populated', 'empty', 'error'].filter((m) => !ONLY_MODES || ONLY_MODES.includes(m))) {
  const ctl = await fixtureContext();
  ctl.mode = mode;
  for (const route of ROUTES.filter((r) => !ONLY_ROUTES || ONLY_ROUTES.includes(r.name ?? r.path))) {
    const page = await ctl.context.newPage();
    await page.clock.setFixedTime(NOW);
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    const slug = (route.name ?? route.path).replaceAll('/', '_').replace('#', '-').replace(/^_/, '');
    await check(`${mode} ${route.name ?? route.path} renders`, async () => {
      await open(page, route.path);
      if (mode === 'populated') await page.locator(route.ready.split(',')[0]).first().waitFor({ timeout: 8000 }).catch(() => {});
      if (mode === 'populated' && route.prepare) await route.prepare(page);
      if (mode === 'populated') await page.locator(route.ready).first().waitFor({ timeout: 8000 });
    });
    for (const [w, h] of VIEWPORTS) {
      await page.setViewportSize({ width: w, height: h });
      await page.waitForTimeout(200);
      await check(`${mode} ${route.name ?? route.path} ${w}x${h} no page overflow`, async () => {
        const o = await overflow(page);
        assert.ok(o.scroll <= o.client + 1, `scrollWidth ${o.scroll} > ${o.client}: ${o.offenders.join(', ')}`);
        assert.ok(!o.inner.length, `content pans sideways inside a page container: ${o.inner.join(', ')}`);
      });
      if (mode === 'populated') {
        for (const [name, loc] of route.actions(page, w)) {
          await check(`${mode} ${route.name ?? route.path} ${w}x${h} ${name} reachable`, () => reachable(page, loc, name));
        }
      }
      await page.screenshot({ path: `${artifacts}/${mode}-${slug}-${w}x${h}.png`, fullPage: false });
    }
    // The app reports a failed API call as a thrown request error in some routes; that is
    // the route's own error handling (a logic matter), so only other errors fail here.
    const unexpected = errors.filter((m) => !(mode === 'error' && /fixture failure|request failed/.test(m)));
    await check(`${mode} ${route.name ?? route.path} no page error`, async () => assert.deepEqual(unexpected, []));
    if (mode === 'error' && errors.length) results.push(`note ${route.name ?? route.path} error state throws: ${[...new Set(errors)].join(' | ')}`);
    await page.close();
  }
  await ctl.context.close();
}

// ── 2. Phone interactions by tap at 390 px ──────────────────────────────────
async function tapFlows(width, height) {
  const out = {};
  const phone = width < 768;
  const ctl = await fixtureContext({ width, height, hasTouch: phone });
  const page = await ctl.context.newPage();
  await page.clock.setFixedTime(NOW);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  // A finger on the phone, the mouse on desktop: the same controls either way.
  const press = (loc) => (phone ? loc.first().tap() : loc.first().click());
  const last = (method, path) => ctl.requests.filter((r) => r.method === method && (typeof path === 'string' ? r.path === path : path.test(r.path))).at(-1);

  // Todo: create, then reorder the list by tap (the due-date sort is the list's only order).
  await open(page, '/todos');
  await page.locator('.list .item').first().waitFor();
  await press(page.locator('.head-actions .primary'));
  const form = page.locator('.todo-form');
  await form.locator('input').first().fill('Mobile created task');
  await form.locator('input[type=date]').fill('2026-10-24');
  await form.locator('input[list=todo-categories]').fill('Brokerage');
  await reachable(page, form.locator('button[type=submit]'), 'todo submit');
  await press(form.locator('button[type=submit]'));
  await page.waitForTimeout(300);
  out.todo = last('POST', '/api/todos')?.body;
  assert.equal(out.todo?.name, 'Mobile created task', 'todo POST captured');
  const before = await page.locator('.list:not(.done-list) .item .name').first().textContent();
  await press(page.locator('.toolbar .chip.sort'));
  const after = await page.locator('.list:not(.done-list) .item .name').first().textContent();
  assert.notEqual(before, after, 'todo order changes by tap');

  // Reminder: create a weekly reminder by tap.
  await open(page, '/remindme');
  await page.locator('table.tbl').first().waitFor();
  await press(page.locator('.head-actions .primary'));
  const rf = page.locator('.rem-form');
  await rf.locator('input').first().fill('Mobile reminder');
  await press(rf.getByRole('button', { name: t('remindme.form.frequency') }));
  await press(page.getByRole('option', { name: 'Weekly', exact: true }));
  await press(rf.locator('.daybtns button').nth(4));
  await reachable(page, rf.locator('button[type=submit]'), 'reminder submit');
  await press(rf.locator('button[type=submit]'));
  await page.waitForTimeout(300);
  out.reminder = last('POST', '/api/reminders')?.body;
  assert.equal(out.reminder?.frequency, 'weekly', 'reminder POST captured');

  // Calendar: create by the Add button, edit an event by tap.
  await open(page, '/calendar');
  await page.locator('.fc-host .fc-view').waitFor();
  await press(page.locator('.head .primary'));
  const ef = page.locator('.ev-form');
  await ef.locator('input').first().fill('Mobile event');
  await reachable(page, ef.locator('button[type=submit]'), 'event submit');
  await press(ef.locator('button[type=submit]'));
  await page.waitForTimeout(300);
  out.event = last('POST', '/api/calendar/events')?.body;
  assert.equal(out.event?.title, 'Mobile event', 'event POST captured');
  await press(page.locator('.views .chip').nth(3)); // Day view: today's events as one list
  await press(page.locator('.fc-host .fc-event').first());
  await page.locator('.ev-form').waitFor();
  await press(page.locator('.ev-form button[type=submit]'));
  await page.waitForTimeout(300);
  out.eventEdit = last('PATCH', /^\/api\/calendar\/events\//)?.body;
  assert.ok(out.eventEdit, 'event PATCH captured');
  await press(page.locator('.views .chip').nth(1)); // back to Month for the next run

  // Month views: the "+N more" overflow opens inside the screen, by tap.
  if (phone) {
    for (const [path, host] of [['/calendar', '.fc-host'], ['/automator/agenda', '.cal']]) {
      await open(page, path);
      await page.locator(`${host} .fc-daygrid-more-link`).first().waitFor();
      await press(page.locator(`${host} .fc-daygrid-more-link`));
      const pop = await page.locator('.fc-popover').first().evaluate((el) => {
        const r = el.getBoundingClientRect();
        return { left: r.left, right: r.right, top: r.top, vw: document.documentElement.clientWidth };
      });
      assert.ok(pop.left >= 0 && pop.right <= pop.vw + 1 && pop.top >= 0, `${path} more popover clipped: ${JSON.stringify(pop)}`);
      await page.screenshot({ path: `${artifacts}/popover-${path.replaceAll('/', '_')}-${width}.png` });
    }
    // Agenda Day: one column that fits the phone, no sideways panning.
    await press(page.locator('.view-switch .mobile-day'));
    await page.locator('.fc-timeGridDay-view').waitFor();
    const day = await overflow(page);
    assert.ok(!day.inner.length && day.scroll <= day.client + 1, `agenda day view pans: ${day.inner.join(', ')}`);
    const calWidth = await page.locator('.calendar-scroll').evaluate((el) => [el.scrollWidth, el.clientWidth]);
    assert.ok(calWidth[0] <= calWidth[1] + 1, `agenda day view wider than the screen: ${calWidth}`);
  }

  // Automator: add a block by tap, move it, then save through the Save button.
  await open(page, '/automator/wf-1');
  await page.locator('.board .task').first().waitFor();
  // Schedule edit: type, one-off date and time, timezone, then Save, all inside the screen.
  await press(page.locator('.rulebar .rulebtn'));
  const dlg = page.locator('[role=dialog]').last();
  await dlg.waitFor();
  await press(dlg.getByRole('button', { name: t('automator.sched.type') }));
  await press(page.getByRole('option', { name: t('automator.sched.kind.once'), exact: true }));
  await dlg.locator('input[type=datetime-local]').fill('2026-10-27T07:45');
  await reachable(page, dlg.getByRole('button', { name: t('automator.sched.timezone') }), 'schedule timezone');
  const fit = await dlg.evaluate((el) => {
    const body = el.querySelector('.modal-body') ?? el;
    const r = el.getBoundingClientRect();
    return { body: [body.scrollWidth, body.clientWidth], left: r.left, right: r.right, vw: document.documentElement.clientWidth };
  });
  assert.ok(fit.body[0] <= fit.body[1] + 1 && fit.left >= 0 && fit.right <= fit.vw + 1, `schedule form does not fit: ${JSON.stringify(fit)}`);
  await reachable(page, dlg.getByRole('button', { name: t('common.save'), exact: true }), 'schedule save');
  if (phone) await page.screenshot({ path: `${artifacts}/schedule-form-${width}.png` });
  await press(dlg.getByRole('button', { name: t('common.save'), exact: true }));
  await page.waitForTimeout(300);
  out.schedule = last('PATCH', '/api/automator/schedules/s-1')?.body;
  assert.equal(out.schedule?.kind, 'once', 'schedule PATCH captured');
  await page.locator('.board .task').first().waitFor();
  const tasksBefore = await page.locator('.board .task').count();
  if (phone) await press(page.locator('.mobile-workspace button').nth(1));
  await press(page.locator('.rail .block').filter({ hasText: t('automator.kind.delay') }).first());
  await page.waitForTimeout(300);
  // The block opens in its editor; close it to go back to the grid.
  await page.keyboard.press('Escape');
  await page.waitForTimeout(200);
  assert.equal(await page.locator('.board .task').count(), tasksBefore + 1, 'block added by tap');
  if (phone) {
    const moveUp = page.locator('.hold').last().locator('.mobile-move button').first();
    await press(moveUp);
    assert.equal(await page.locator('.board .task').count(), tasksBefore + 1, 'moved block still there');
  }
  await page.waitForTimeout(1600); // autosave
  out.autosave = last('PUT', '/api/automator/workflows/wf-1')?.body;
  assert.ok(out.autosave, 'autosave PUT captured');
  await press(page.getByRole('button', { name: t('automator.repointBtn') }));
  await press(page.getByRole('button', { name: t('automator.repointKeep') }));
  await page.waitForTimeout(300);
  out.save = last('PUT', '/api/automator/workflows/wf-1')?.body;
  assert.equal(out.save?.draft, false, 'manual save PUT captured');

  // Time: reorder a timer by tap on phone, by drag on desktop.
  await open(page, '/time');
  await page.locator('.cards .card').first().waitFor();
  if (phone) {
    await press(page.locator('.cards .card').first().locator('.card-move button').nth(1));
  } else {
    await page.locator('.cards .card').first().dragTo(page.locator('.cards .card').nth(1));
  }
  await page.waitForTimeout(300);
  out.position = last('POST', /^\/api\/time\/projects\/1\/position$/)?.body;
  assert.ok(out.position, 'timer position POST captured');

  assert.deepEqual(errors, [], 'tap flow page errors');
  await page.screenshot({ path: `${artifacts}/flows-${width}.png` });
  await ctl.context.close();
  return out;
}

let phoneFlows;
let desktopFlows;
if (!SKIP.includes('flows')) await check('tap flows at 390 px', async () => {
  phoneFlows = await tapFlows(390, 844);
});
if (!SKIP.includes('flows')) await check('same flows at 1440 px (desktop reference)', async () => {
  desktopFlows = await tapFlows(1440, 900);
});
if (phoneFlows && desktopFlows) {
  for (const key of Object.keys(desktopFlows)) {
    await check(`payload shape ${key} matches desktop`, async () => assert.deepEqual(shape(phoneFlows[key]), shape(desktopFlows[key])));
  }
}

// ── 3. Desktop no-regression against the baseline server ────────────────────
// A real CSS change shows on every capture. Antialiasing and shell animations (the topbar
// module icon, a chip transition) flicker a few dozen pixels now and then, even between two
// captures of the baseline itself, so a pair that differs is captured again, up to 3 times.
async function capture(server, route, w, h) {
  const ctl = await fixtureContext({ width: w, height: h });
  const page = await ctl.context.newPage();
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.clock.setFixedTime(NOW);
  await page.goto(`${server}${route.path}`, { waitUntil: 'domcontentloaded' });
  await page.locator(route.ready).first().waitFor({ timeout: 15000 }).catch(() => {});
  await page.waitForLoadState('networkidle', { timeout: 4000 }).catch(() => {});
  if (route.prepare) await route.prepare(page).catch(() => {});
  await page.waitForTimeout(600);
  // FullCalendar measures its rows once; a late font or scrollbar change leaves it laid
  // out for the old size. A resize makes both servers measure the final layout.
  await page.evaluate(() => window.dispatchEvent(new Event('resize')));
  await page.waitForTimeout(600);
  const shot = await page.screenshot({ fullPage: false });
  await ctl.context.close();
  return shot;
}
for (const route of SKIP.includes('desktop') ? [] : ROUTES.filter((r) => !ONLY_ROUTES || ONLY_ROUTES.includes(r.name ?? r.path))) {
  for (const [w, h] of [[1440, 900], [1100, 800]]) {
    const slug = (route.name ?? route.path).replaceAll('/', '_').replace('#', '-').replace(/^_/, '');
    await check(`desktop ${route.name ?? route.path} ${w}x${h} identical to baseline`, async () => {
      let shots;
      for (let attempt = 1; attempt <= 3; attempt++) {
        shots = [await capture(baseline, route, w, h), await capture(base, route, w, h)];
        if (shots[0].equals(shots[1])) {
          if (attempt > 1) results.push(`note desktop ${route.name ?? route.path} ${w}x${h} identical on capture ${attempt}`);
          return;
        }
      }
      const { writeFile } = await import('node:fs/promises');
      await writeFile(`${artifacts}/desktop-${slug}-${w}-baseline.png`, shots[0]);
      await writeFile(`${artifacts}/desktop-${slug}-${w}-current.png`, shots[1]);
      assert.fail('screenshots differ on 3 captures');
    });
  }
}

await browser.close();
for (const line of results) if (!line.startsWith('ok')) console.log(line);
console.log(`${results.filter((r) => r.startsWith('ok')).length} checks passed, ${failures.length} failed`);
if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log('PASS: planning routes at 320/390/600/667/768 px (populated, empty, error), tap flows and desktop baseline.');
