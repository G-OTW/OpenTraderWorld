// Mobile checks for Community Docs, Mindset, Resources, Routines, Prompt Store,
// Subscriptions and Watchlists. API fixtures only, no backend.
// Run with the frontend dev server: node tests/mobile-misc-browser.mjs [base URL] [baseline URL]
// The baseline URL (default http://127.0.0.1:4190) serves the unmodified frontend for the
// desktop screenshot comparison; pass "-" to skip that comparison.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const baseline = process.argv[3] ?? 'http://127.0.0.1:4190';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-misc/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
// Single raster thread: multi-threaded tile raster can flip a few anti-aliased corner
// pixels between captures, which would break the byte-exact desktop comparison.
const browser = await chromium.launch({ executablePath, headless: true, args: ['--num-raster-threads=1', '--disable-partial-raster', '--force-color-profile=srgb'], env: { ...process.env, TMPDIR: artifacts } });

const CLOCK = new Date('2026-10-14T10:00:00');
const TODAY = '2026-10-14';
const VIEWPORTS = [
  { width: 320, height: 568 },
  { width: 390, height: 844 },
  { width: 600, height: 800 },
  { width: 667, height: 375 },
  { width: 768, height: 1024 }
];

// ── Fixtures ────────────────────────────────────────────────────────────────

const spark = (seed, n = 30) => Array.from({ length: n }, (_, i) => 100 + Math.sin((i + seed) / 3) * 8 + i * 0.4);
const quote = (price, c24, c3, c7, c30, seed) => ({ price_usd: price, change_24h: c24, change_3d: c3, change_7d: c7, change_30d: c30, spark: spark(seed) });

const WATCHLISTS = [
  { id: 'wl-1', name: 'Long-duration semiconductor and AI infrastructure leaders', item_count: 4, sync_enabled: true },
  { id: 'wl-2', name: 'Crypto majors', item_count: 2, sync_enabled: false },
  { id: 'wl-3', name: 'European dividend aristocrats (watch for entries)', item_count: 0, sync_enabled: false }
];
const WL_ITEMS = [
  { id: 'wi-1', symbol: 'NVDA', name: 'NVIDIA Corporation', exchange: 'NASDAQ', asset_class: 'stock', quote_source: null, quote_ticker: null, notes: 'Wait for a pullback to the 50-day average before adding; earnings on the 19th.', quoted_at: '2026-10-14T09:55:00Z', quote: quote(1234567.891, 2.35, -1.2, 5.75, 18.4, 1) },
  { id: 'wi-2', symbol: 'BRK.B', name: 'Berkshire Hathaway Inc. Class B shares with a very long issuer name', exchange: 'NYSE', asset_class: 'stock', quote_source: 'auto', quote_ticker: null, notes: '', quoted_at: '2026-10-14T09:55:00Z', quote: quote(452.18, -0.42, -2.9, -6.15, -12.75, 5) },
  { id: 'wi-3', symbol: 'SHIB', name: 'Shiba Inu', exchange: null, asset_class: 'crypto', quote_source: null, quote_ticker: null, notes: '', quoted_at: '2026-10-14T09:55:00Z', quote: quote(0.00001234, -12.5, 3.1, -25.4, 140.2, 9) },
  { id: 'wi-4', symbol: 'ASML', name: 'ASML Holding N.V.', exchange: 'AMS', asset_class: 'stock', quote_source: 'conn-1', quote_ticker: 'ASML.AS', notes: 'Core', quoted_at: '2026-10-14T09:55:00Z', quote: { ...quote(712.4, 0, 0, 0, 0, 3), error: 'Symbol not found on connector' } }
];
const WL_DETAIL = {
  watchlist: { id: 'wl-1', name: WATCHLISTS[0].name, description: 'Names I review every Monday before the open, with notes on the entry plan for each one.', sync_enabled: true, refresh_secs: 300, connector_id: null, refreshed_at: '2026-10-14T09:55:00Z' },
  items: WL_ITEMS,
  alerts: [{ id: 'al-1', item_id: 'wi-1', enabled: true, kind: 'above', price: 1300000, note: '' }]
};
const CONNECTORS = [{ id: 'conn-1', label: 'IBKR', name: 'Interactive Brokers paper account', provider: 'ibkr', asset_types: ['equity'] }];

const M_CATS = [
  { id: 'mc-1', name: 'Morning preparation and pre-market mindset routine', color: '#4a90d9' },
  { id: 'mc-2', name: 'Review', color: '#d98a4a' }
];
const M_TEMPLATES = [
  { id: 'mt-1', name: 'Pre-market emotional state and readiness check before the opening bell', description: 'How you arrive at the desk decides how you trade the first hour.', notes: '<p>Breathe, read the plan, check the calendar.</p>', category_id: 'mc-1', phase: 'pre', position: 0, active: true },
  { id: 'mt-2', name: 'Post-session review', description: '', notes: '', category_id: 'mc-2', phase: 'post', position: 0, active: true },
  { id: 'mt-3', name: 'Paused weekly reflection', description: 'Not used right now.', notes: '', category_id: null, phase: 'live', position: 0, active: false }
];
const M_PROMPTS = [
  { id: 'mp-1', template_id: 'mt-1', kind: 'scale', label: 'How rested and focused do you feel this morning?', hint: '1 is exhausted, 5 is sharp.', config: { low: 'Exhausted and distracted', high: 'Sharp and calm' }, active: true, position: 0 },
  { id: 'mp-2', template_id: 'mt-1', kind: 'choice', label: 'Mood', hint: '', config: { options: ['😞', '😕', '😐', '🙂', '😄'] }, active: true, position: 1 },
  { id: 'mp-3', template_id: 'mt-1', kind: 'tags', label: 'Which states are present right now?', hint: '', config: { options: ['calm', 'confident', 'disciplined', 'anxious', 'impatient', 'FOMO', 'tired', 'distracted by overnight news'] }, active: true, position: 2 },
  { id: 'mp-4', template_id: 'mt-1', kind: 'text', label: 'What is the one rule you will not break today?', hint: '', config: {}, active: true, position: 3 },
  { id: 'mp-5', template_id: 'mt-2', kind: 'scale', label: 'Did you follow the plan?', hint: '', config: { low: 'Not at all', high: 'Fully' }, active: true, position: 0 },
  { id: 'mp-6', template_id: 'mt-2', kind: 'choice', label: 'Process grade', hint: '', config: { options: ['All process', 'One slip', 'Several', 'Tilted'] }, active: true, position: 1 }
];
const M_ENTRIES = Array.from({ length: 12 }, (_, i) => {
  const d = new Date(Date.UTC(2026, 9, 13 - i)).toISOString().slice(0, 10);
  return [
    { id: `me-${i}-a`, entry_date: d, template_id: 'mt-1', phase: 'pre', answers: { 'mp-1': (i % 5) + 1, 'mp-2': '🙂', 'mp-3': ['calm', 'distracted by overnight news'], 'mp-4': 'No averaging down on a losing position, whatever the story says.' } },
    { id: `me-${i}-b`, entry_date: d, template_id: 'mt-2', phase: 'post', answers: { 'mp-5': ((i + 2) % 5) + 1, 'mp-6': 'One slip' } }
  ];
}).flat();
const MARKS = [{ day: '2026-10-13', mark: 'full' }, { day: '2026-10-12', mark: 'partial' }, { day: '2026-10-09', mark: 'full' }];

const R_CATS = [
  { id: 'rc-1', name: 'Pre-market preparation for US equities and index futures', color: '#4a90d9' },
  { id: 'rc-2', name: 'Post-mortem', color: '#9b59b6' }
];
const R_ITEMS = [
  { id: 'ri-1', label: 'Check the economic calendar for high-impact releases (CPI, FOMC, NFP) and mark them on the chart', note: '<p>Use the calendar module, filter on three stars.</p>', url: 'https://example.com/economic-calendar/with/a/very/long/path?region=us&impact=high', link_label: 'Economic calendar', checked: true },
  { id: 'ri-2', label: 'Review overnight gaps', note: '', url: '', link_label: '', checked: false },
  { id: 'ri-3', label: 'Set the max daily loss in the platform', note: '<p>Two percent of the account, never more.</p>', url: '', link_label: '', checked: false }
];
const ROUTINES = [
  { id: 'rt-1', name: 'Opening bell checklist for the New York session', description: 'Run before 9:15 every weekday.', notes: '<p>Do it in order.</p>', category_id: 'rc-1', schedule: { kind: 'weekly', weekdays: 31, interval: 1 }, start_date: '2026-09-01', end_date: '2026-12-31', active: true, position: 0 },
  // Not a monthly or nth-weekday rule: those throw "daysInMonth is not defined" in
  // routines/api.js at HEAD (reported separately), which would mask the layout checks.
  { id: 'rt-2', name: 'Weekly review', description: '', notes: '', category_id: 'rc-2', schedule: { kind: 'weekly', weekdays: 16, interval: 2 }, start_date: null, end_date: null, active: true, position: 1 },
  { id: 'rt-3', name: 'Paused routine', description: '', notes: '', category_id: null, schedule: { kind: 'daily', interval: 2 }, start_date: null, end_date: null, active: false, position: 2 }
];

const PROMPTS = [
  { id: 'pr-1', name: 'Summarize an earnings call transcript into a trading brief with risks and catalysts', body: 'You are a buy-side analyst. Read the transcript below and write:\n1. Three key numbers versus consensus\n2. Guidance changes\n3. Risks the market may be ignoring\n\nTranscript:\n{{transcript}}', tags: ['earnings', 'analysis', 'equities', 'long-form', 'buy-side'], vote: 1, version: 7, updated_at: '2026-10-10T08:00:00Z' },
  { id: 'pr-2', name: 'Risk check', body: 'List every way this trade can lose more than planned: https://example.com/a/very/long/url/that/should/wrap/inside/the/card/without/overflowing', tags: ['risk'], vote: -1, version: 1, updated_at: '2026-09-01T08:00:00Z' },
  { id: 'pr-3', name: 'Blank', body: '', tags: [], vote: 0, version: 2, updated_at: '2026-08-15T08:00:00Z' }
];
const PROMPT_TAGS = ['earnings', 'analysis', 'equities', 'long-form', 'buy-side', 'risk', 'macro-regime-identification-and-positioning'];

const SUBS = [
  { id: 'sb-1', name: 'Bloomberg Terminal professional seat with real-time exchange data', platform: 'Bloomberg L.P.', url: 'https://example.com/bloomberg', price: 2665.5, currency: 'USD', frequency: 'monthly', category: 'Market data and research', started_on: '2025-03-04', active: true },
  { id: 'sb-2', name: 'TradingView Premium', platform: 'TradingView', url: null, price: 599.4, currency: 'EUR', frequency: 'yearly', category: 'Charting', started_on: '2024-11-20', active: true },
  { id: 'sb-3', name: 'Newsletter', platform: '', url: null, price: 9.99, currency: 'GBP', frequency: 'weekly', category: '', started_on: '2026-01-02', active: false },
  { id: 'sb-4', name: 'VPS', platform: 'Hetzner', url: 'https://example.com/vps', price: 1234567.89, currency: 'JPY', frequency: 'quarterly', category: 'Infrastructure', started_on: '2026-02-15', active: true }
];
const months = Array.from({ length: 37 }, (_, i) => {
  const d = new Date(Date.UTC(2025, 9 + i, 1));
  const m = d.toISOString().slice(0, 10);
  return { month: m, amount: 2700 + (i % 4) * 120, by_sub: { 'sb-1': 2665.5, 'sb-2': i % 12 === 1 ? 650 : 0, 'sb-4': i % 3 === 0 ? 2800 : 0 }, by_category: { 'Market data and research': 2665.5, Charting: i % 12 === 1 ? 650 : 0, Infrastructure: i % 3 === 0 ? 2800 : 0 } };
});
const years = ['2025', '2026', '2027', '2028'].map((y, i) => ({ month: `${y}-01-01`, amount: 34000 + i * 900, by_sub: { 'sb-1': 31986, 'sb-2': 650, 'sb-4': 2800 }, by_category: { 'Market data and research': 31986, Charting: 650, Infrastructure: 2800 } }));
const BREAKDOWN = {
  months, years,
  subs: SUBS.map((s) => ({ id: s.id, name: s.name })),
  categories: [{ category: 'Market data and research' }, { category: 'Infrastructure' }, { category: 'Charting' }],
  monthly_total: 2987654.32, yearly_total: -35851851.84, next_month_total: 2712.5, active_count: 3, unconverted: 1
};

const RES_CATS = [
  { id: 'rs-1', name: 'Books on market microstructure and execution' },
  { id: 'rs-2', name: 'Tools' }
];
const RESOURCES = [
  { id: 'rr-1', name: 'Trading and Exchanges: Market Microstructure for Practitioners', description: 'Larry Harris. The reference on how orders meet, who provides liquidity and what it costs.', link: 'https://example.com/books/trading-and-exchanges-market-microstructure-for-practitioners', category_id: 'rs-1', thumb_url: null },
  { id: 'rr-2', name: 'Position sizing calculator', description: '', link: 'https://www.example.org/tools/size', category_id: 'rs-2', thumb_url: null },
  { id: 'rr-3', name: 'Notes', description: 'A resource with no link at all and a fairly long description that has to wrap on a phone screen.', link: '', category_id: 'rs-2', thumb_url: null }
];

const DOCS = [
  { slug: 'review', title: 'A practical guide to portfolio review on a small screen with a deliberately long title', summary: 'A compact guide with a long title and readable details about currency, positions and the latest prices.', categories: ['Guides and walkthroughs for new users', 'Portfolio'], synced_at: '2026-10-05T08:00:00Z', favorited: true },
  { slug: 'risk', title: 'Risk', summary: '', categories: ['Guides and walkthroughs for new users'], synced_at: '2026-10-04T08:00:00Z', favorited: false },
  { slug: 'api', title: 'Connecting a broker through the data broker', summary: 'Credentials, settings and grants.', categories: [], synced_at: '2026-09-01T08:00:00Z', favorited: false }
];
const DOC_BODY = `<h2>Before you start</h2><p>Review positions, currency, and the latest prices before acting. Download the checklist attachment: <a href="https://example.com/files/portfolio-review-checklist-with-a-very-long-file-name-v2-final.pdf">portfolio-review-checklist-with-a-very-long-file-name-v2-final.pdf</a>.</p>
<p>Unbroken reference: https://example.com/a/really/long/url/without/any/spaces/that/must/wrap/inside/the/reader/column/on/phones</p>
<pre>portfolio-review --include-current-currency --include-positions --since 2026-01-01 --format csv</pre>
<table><thead><tr><th>Field</th><th>Meaning</th><th>Example value</th><th>Notes</th></tr></thead><tbody><tr><td>Currency</td><td>Reporting currency of the position</td><td>EUR</td><td>Converted at the daily close rate</td></tr></tbody></table>
<figure><img src="data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='1200' height='400'><rect width='1200' height='400' fill='%23888'/></svg>" alt=""><figcaption>A wide figure that must scale down.</figcaption></figure>
<ul><li>Attachment: <a href="https://example.com/files/sample.xlsx">sample.xlsx</a></li><li>Attachment: <a href="https://example.com/files/notes.md">notes.md</a></li></ul>`;

const MODULE_PREFIXES = ['/api/watchlists', '/api/mindset', '/api/trader', '/api/prompts', '/api/subscriptions', '/api/resources', '/api/community-docs', '/api/journal/fx', '/api/connectors', '/api/portfolios'];

function shell(p, lang) {
  if (p === '/api/setup/status') return { configured: true };
  if (p === '/api/settings/me') return { username: 'fixture', role: 'admin', locale: lang };
  if (p === '/api/settings/modules') return { installed: ['community-docs', 'mindset', 'resources', 'routines', 'prompt-store', 'subscriptions', 'watchlists', 'remindme', 'settings', 'portfolios', 'journal'] };
  if (p === '/api/settings/defaults') return { default_timezone: 'UTC', display_currency: 'USD', locale: lang };
  if (p === '/api/settings/versioning') return { modules: {} };
  if (p === '/api/mcp/settings') return { enabled: false };
  if (p === '/api/health') return { status: 'ok', services: { core: 'ok', postgres: 'ok' } };
  if (p === '/api/dashboard/layout') return { pages: [] };
  if (p === '/api/tasks') return { tasks: [] };
  if (p.includes('/notifications')) return { notifications: [], unread: 0 };
  if (p === '/api/reminders') return { reminders: [] };
  return undefined;
}

function moduleData(p, method, url, body, state) {
  const empty = state === 'empty';
  const q = url.searchParams;
  // Watchlists
  if (p === '/api/watchlists' && method === 'GET') return { watchlists: empty ? [] : WATCHLISTS };
  if (p === '/api/watchlists/templates') return { templates: [{ id: 'tpl-1', name: 'US mega-cap technology leaders and semiconductor suppliers', count: 12, symbols: ['AAPL', 'MSFT', 'NVDA', 'AVGO', 'TSM', 'ASML', 'AMD', 'QCOM', 'TXN', 'MU'] }, { id: 'tpl-2', name: 'Crypto', count: 3, symbols: ['BTC', 'ETH', 'SOL'] }] };
  if (p === '/api/notif-channels') return { channels: [{ id: 'ch-1', name: 'Telegram trading desk alerts', kind: 'telegram', enabled: true, last_ok: false, last_error: 'timeout' }, { id: 'ch-2', name: '', kind: 'email', enabled: true }] };
  if (p === '/api/watchlists/search') return { results: q.get('q') ? [{ asset_class: q.get('kind'), provider: q.get('kind') === 'crypto' ? 'coingecko' : 'yahoo', provider_id: q.get('q').toUpperCase(), symbol: q.get('q').toUpperCase(), name: 'Apple Inc. common stock listed on the NASDAQ Global Select Market' }, { asset_class: 'stock', provider: 'yahoo', provider_id: 'AAPL.MX', symbol: 'AAPL.MX', name: 'Apple Inc.' }] : [] };
  if (/^\/api\/watchlists\/wl-\d\/items$/.test(p)) return { item: { ...WL_ITEMS[0], id: 'wi-new', symbol: body?.symbol ?? 'AAPL', name: body?.name ?? '' } };
  if (p === '/api/watchlists/wl-1') return WL_DETAIL;
  if (p === '/api/watchlists/wl-2') return { watchlist: { ...WL_DETAIL.watchlist, id: 'wl-2', name: 'Crypto majors', description: '', sync_enabled: false }, items: WL_ITEMS.slice(2, 3), alerts: [] };
  if (p === '/api/watchlists/wl-3') return { watchlist: { ...WL_DETAIL.watchlist, id: 'wl-3', name: WATCHLISTS[2].name, description: '' }, items: [], alerts: [] };
  if (p.startsWith('/api/watchlists/items/')) return { item: WL_ITEMS[0] };
  if (p === '/api/connectors') return { connectors: empty ? [] : CONNECTORS };
  if (p === '/api/portfolios') return { portfolios: [] };
  // Mindset
  if (p === '/api/mindset/day') return empty ? { templates: [], prompts: [], entries: [], categories: [], marks: [], mark: null } : { templates: M_TEMPLATES.filter((x) => x.active), prompts: M_PROMPTS, entries: [{ id: 'me-today', entry_date: TODAY, template_id: 'mt-2', phase: 'post', answers: { 'mp-5': 4 } }], categories: M_CATS, marks: MARKS, mark: null };
  if (p === '/api/mindset/history') return empty ? { prompts: [], templates: [], categories: [], entries: [], marks: [] } : { prompts: M_PROMPTS, templates: M_TEMPLATES, categories: M_CATS, entries: M_ENTRIES, marks: MARKS };
  if (p === '/api/mindset/templates' && method === 'GET') return { templates: empty ? [] : M_TEMPLATES.map((t) => ({ template: t, prompts: M_PROMPTS.filter((x) => x.template_id === t.id) })), categories: empty ? [] : M_CATS };
  if (/^\/api\/mindset\/templates\/mt-\d$/.test(p)) {
    const t = M_TEMPLATES.find((x) => x.id === p.split('/').pop());
    return method === 'GET' ? { template: t, prompts: M_PROMPTS.filter((x) => x.template_id === t.id) } : { template: t };
  }
  if (p === '/api/mindset/entries') return { entry: { id: 'me-new', entry_date: body?.date, template_id: body?.template_id, answers: body?.answers } };
  if (p === '/api/mindset/marks') return method === 'GET' ? { marks: MARKS } : {};
  if (p === '/api/mindset/categories') return { categories: M_CATS };
  // Routines
  if (p === '/api/trader/board') return empty ? { routines: [], categories: [], marks: [], tick_dates: [], mark: null } : { routines: ROUTINES.filter((r) => r.active).map((r, i) => ({ ...r, items: i === 0 ? structuredClone(R_ITEMS) : [{ id: `ri-${r.id}`, label: 'Journal every trade', note: '', url: '', link_label: '', checked: false }] })), categories: R_CATS, marks: MARKS, tick_dates: ['2026-10-13'], mark: 'full' };
  if (p === '/api/trader/routines' && method === 'GET') return { routines: empty ? [] : ROUTINES.map((r, i) => ({ routine: r, items: i === 0 ? R_ITEMS : [] })), categories: empty ? [] : R_CATS };
  if (/^\/api\/trader\/routines\/rt-\d$/.test(p)) {
    const r = ROUTINES.find((x) => x.id === p.split('/').pop());
    return method === 'GET' ? { routine: r, items: r.id === 'rt-1' ? R_ITEMS : [] } : { routine: r };
  }
  if (/^\/api\/trader\/items\/[^/]+\/check$/.test(p)) return { ok: true };
  if (p === '/api/trader/marks') return method === 'GET' ? { marks: MARKS } : {};
  if (p === '/api/trader/categories') return { categories: R_CATS };
  // Prompt store
  if (p === '/api/prompts' && method === 'GET') return { prompts: empty ? [] : PROMPTS };
  if (p === '/api/prompts/tags') return { tags: empty ? [] : PROMPT_TAGS };
  if (/^\/api\/prompts\/pr-\d\/versions$/.test(p)) return { versions: [7, 6, 5].map((v) => ({ version: v, name: PROMPTS[0].name, body: PROMPTS[0].body, tags: PROMPTS[0].tags, created_at: `2026-10-0${v - 2}T08:00:00Z` })) };
  if (p.startsWith('/api/prompts')) return { prompt: PROMPTS[0] };
  // Subscriptions
  if (p === '/api/subscriptions' && method === 'GET') return { subscriptions: empty ? [] : SUBS };
  if (p === '/api/subscriptions' && method === 'POST') return { subscription: { ...body, id: 'sb-new' } };
  if (p === '/api/subscriptions/breakdown') return { breakdown: empty ? { months: [], years: [], subs: [], categories: [], monthly_total: 0, yearly_total: 0, next_month_total: 0, active_count: 0, unconverted: 0 } : BREAKDOWN };
  if (p === '/api/subscriptions/settings') return { settings: { display_currency: 'EUR' } };
  if (p === '/api/subscriptions/suggestions') return { platforms: empty ? [] : ['Bloomberg L.P.', 'TradingView', 'Hetzner'], categories: empty ? [] : ['Market data and research', 'Charting'] };
  if (p.startsWith('/api/journal/fx/rates/')) return { rates: [{ quote: 'EUR', rate: 0.92 }, { quote: 'GBP', rate: 0.79 }, { quote: 'JPY', rate: 149.3 }] };
  // Resources
  if (p === '/api/resources/categories') return { categories: empty ? [] : RES_CATS };
  if (p === '/api/resources' && method === 'GET') return { resources: empty ? [] : RESOURCES };
  if (p === '/api/resources' && method === 'POST') return { resource: { ...body, id: 'rr-new' } };
  // Community docs
  if (p === '/api/community-docs' || p === '/api/community-docs/favorites') return { docs: empty ? [] : p.endsWith('favorites') ? DOCS.filter((d) => d.favorited) : DOCS };
  if (p === '/api/community-docs/refresh') return { docs: empty ? [] : DOCS };
  if (p.startsWith('/api/community-docs/') && p.endsWith('/favorite')) return { favorited: !!body?.favorite };
  if (p.startsWith('/api/community-docs/')) {
    const d = DOCS.find((x) => x.slug === p.split('/').pop()) ?? DOCS[0];
    return { doc: { ...d, body: DOC_BODY, source_url: 'https://example.com/docs/review' } };
  }
  return undefined;
}

async function newContext({ touch = false, lang = 'en', state = 'populated', viewport = VIEWPORTS[1], reducedMotion = 'no-preference' } = {}) {
  const context = await browser.newContext({ viewport, locale: lang === 'de' ? 'de-DE' : 'en-US', hasTouch: touch, isMobile: touch, reducedMotion, timezoneId: 'UTC' });
  context.captured = [];
  context.unknown = new Set();
  await context.addInitScript(([l]) => {
    try {
      localStorage.setItem('otw.locale', l);
    } catch {
      /* storage blocked */
    }
    // Record clipboard writes so the copy action can be asserted without OS clipboard access.
    window.__copied = [];
    const record = (text) => {
      window.__copied.push(text);
      return Promise.resolve();
    };
    try {
      Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: record, readText: () => Promise.resolve(window.__copied.at(-1) ?? '') } });
    } catch {
      /* keep the native clipboard */
    }
  }, [lang]);
  await context.route('**/api/**', async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const p = url.pathname;
    const method = req.method();
    let body = null;
    try {
      body = req.postDataJSON();
    } catch {
      body = req.postData();
    }
    if (method !== 'GET') context.captured.push({ method, path: p, body });
    const s = shell(p, lang);
    if (s !== undefined) return route.fulfill({ json: s });
    if (state === 'error' && MODULE_PREFIXES.some((x) => p.startsWith(x))) return route.fulfill({ status: 500, json: { error: 'Fixture backend failure (500)' } });
    const m = moduleData(p, method, url, body, state);
    if (m !== undefined) return route.fulfill({ json: m });
    context.unknown.add(`${method} ${p}`);
    return route.fulfill({ json: {} });
  });
  return context;
}

// ── Checks ──────────────────────────────────────────────────────────────────

async function gotoReady(page, url, ready) {
  // The shared dev server can full-reload a page when another file changes: retry.
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      await page.goto(url, { waitUntil: 'domcontentloaded' });
      await page.locator(ready).first().waitFor({ state: 'visible', timeout: 10000 });
      await page.waitForTimeout(250);
      return;
    } catch (e) {
      if (attempt === 2) throw e;
    }
  }
}

async function overflow(page) {
  return page.evaluate(() => {
    const doc = document.documentElement;
    const vw = doc.clientWidth;
    const out = { viewport: vw, scroll: doc.scrollWidth, offenders: [], inner: [] };
    // Scroll containers hide overflow from the document check: report those too, except
    // the local scrollers a two-dimensional block is allowed (code, tables in docs).
    for (const el of document.querySelectorAll('body *')) {
      const st = getComputedStyle(el);
      if (!/(auto|scroll)/.test(st.overflowX) || el.clientWidth === 0) continue;
      if (el.scrollWidth <= el.clientWidth + 1) continue;
      if (el.closest('.content pre, .content table, .modal-body textarea, textarea')) continue;
      const wide = [...el.querySelectorAll('*')].find((c) => c.getBoundingClientRect().right > el.getBoundingClientRect().right + 1);
      out.inner.push(`${el.tagName.toLowerCase()}.${String(el.className).split(' ').slice(0, 2).join('.')} scrolls ${el.scrollWidth}>${el.clientWidth}${wide ? ` (${wide.tagName.toLowerCase()}.${String(wide.className?.baseVal ?? wide.className).split(' ').slice(0, 2).join('.')})` : ''}`);
    }
    if (doc.scrollWidth > vw + 1) {
      for (const el of document.querySelectorAll('body *')) {
        const r = el.getBoundingClientRect();
        if (r.right > vw + 1 && r.width > 0 && !el.closest('[aria-hidden="true"]')) {
          out.offenders.push(`${el.tagName.toLowerCase()}.${String(el.className).split(' ').slice(0, 2).join('.')} right=${Math.round(r.right)}`);
          if (out.offenders.length > 6) break;
        }
      }
    }
    return out;
  });
}

/** Scroll an action into view and confirm a tap at its centre lands on it. */
async function reach(page, selector, phone) {
  // `inline:` marks a link inside running text: WCAG exempts it from the target size, and a
  // wrapped link is probed on its first line box.
  const inline = selector.startsWith('inline:');
  const loc = page.locator(inline ? selector.slice(7) : selector).first();
  if ((await loc.count()) === 0) return 'missing';
  if (!(await loc.isVisible())) return 'hidden';
  await loc.scrollIntoViewIfNeeded();
  const probe = (centre) =>
    loc.evaluate((node, [c, inl]) => {
      if (c) node.scrollIntoView({ block: 'center', inline: 'center' });
      const b = inl ? node.getClientRects()[0] : node.getBoundingClientRect();
      const cx = b.left + b.width / 2;
      const cy = b.top + b.height / 2;
      const vw = document.documentElement.clientWidth;
      const inside = cx >= 0 && cy >= 0 && cx <= vw && cy <= innerHeight && b.right <= vw + 1 && b.left >= -1;
      const hit = document.elementFromPoint(cx, cy);
      const ok = !!hit && (hit === node || node.contains(hit) || (node.tagName === 'INPUT' && hit.closest('label')?.contains(node)));
      const desc = hit ? `${hit.tagName.toLowerCase()}.${String(hit.className?.baseVal ?? hit.className).split(' ').slice(0, 2).join('.')}` : 'none';
      return { inside, ok, desc, w: Math.round(b.width), h: Math.round(b.height) };
    }, [centre, inline]);
  let r = await probe(false);
  if (!r.inside || !r.ok) r = await probe(true);
  if (!r.inside) return `outside viewport (${r.w}x${r.h})`;
  if (!r.ok) return `covered by ${r.desc}`;
  if (phone && !inline && (r.w < 24 || r.h < 24)) return `target ${r.w}x${r.h} under 24 px`;
  return null;
}

const report = { failures: [], notes: [], pageErrors: [], flows: [], desktop: [], unknown: new Set() };
const fail = (msg) => report.failures.push(msg);

/** Selectors per route and state. A view may enter a sub-state (modal, reader) first. */
const ROUTES = [
  {
    path: '/watchlists', slug: 'watchlists',
    ready: { populated: '.detail .titlerow h2', empty: '.ui-empty', error: '.page' },
    views: {
      populated: [
        { name: 'list', actions: ['.page-header .actions button.primary, .page-header .actions .btn.primary', '.rail .railitem:nth-child(2)', '.titlerow .iconbtn', '.titlerow .iconbtn.danger', '.titlerow .btn', '.desc', '.toolbar .btn', '.toolbar .filter', '.toolbar .sync', '.toolbar .interval button', 'thead .sort', 'tbody tr:nth-child(1) .ticker', 'tbody tr:nth-child(1) .notecell .note', 'tbody tr:nth-child(2) .addnote', 'tbody tr:nth-child(2) .bell', 'tbody tr:nth-child(1) .actions .iconbtn.danger'] },
        { name: 'add-symbol', enter: async (page) => { await page.locator('.toolbar .btn').first().click(); await page.locator('.modal input.search').waitFor(); await page.locator('.modal input.search').fill('AAPL'); await page.locator('.modal .hit').first().waitFor(); }, actions: ['.modal .kinds button:nth-child(2)', '.modal input.search', '.modal .hit', '.modal .x'], leave: async (page) => page.keyboard.press('Escape') },
        { name: 'alerts', enter: async (page) => { await page.locator('tbody tr:nth-child(1) .bell').click(); await page.locator('.modal .sentence').waitFor(); }, actions: ['.modal .sentence .chip', '.modal .sentence .inline button', '.modal .sentence input.num', '.modal .opts .opt', '.modal .chans .chan', '.modal .x'], leave: async (page) => page.keyboard.press('Escape') },
        { name: 'new-watchlist', enter: async (page) => { await page.locator('.page-header .actions .btn.primary').click(); await page.locator('.modal [role="tab"]').nth(1).click(); await page.locator('.modal .tplcard').first().waitFor(); }, actions: ['.modal [role="tab"]', '.modal .tplcard', '.modal .tplcard >> nth=1'], leave: async (page) => page.keyboard.press('Escape') }
      ],
      empty: [{ name: 'empty', actions: ['.page-header .actions .btn.primary', '.page .btn.primary >> nth=1'] }],
      error: [{ name: 'error', actions: ['.page-header .actions .btn.primary'], expectText: 'Fixture backend failure' }]
    }
  },
  {
    path: '/mindset', slug: 'mindset',
    ready: { populated: '.card .prompts', empty: '.page .status', error: '.page' },
    views: {
      populated: [{ name: 'day', actions: ['.modnav a:nth-child(2)', '.datenav .btn', '.datenav input[type="date"]', '.datenav .btn >> nth=1', '.periods .chip:nth-child(2)', '.tally', '.markbtn', '.gtoggle', '.card .head', '.card .iconbtn', '.card .steps button:nth-child(5)', '.card .chips .chip', '.card textarea', '.card .foot .btn.primary'] }],
      empty: [{ name: 'empty', actions: ['.markbtn', '.page a.btn.primary'] }],
      error: [{ name: 'error', actions: ['.datenav .btn'], expectText: 'Fixture backend failure' }]
    }
  },
  {
    path: '/mindset/history', slug: 'mindset-history',
    ready: { populated: '.day .daylink', empty: '.fill', error: '.page' },
    views: {
      populated: [{ name: 'history', actions: ['.limits .chip:nth-child(2)', '.limits .chip:last-child', '.day .daylink'], charts: true }],
      empty: [{ name: 'empty', actions: ['.limits .chip', '.fill a.btn'] }],
      error: [{ name: 'error', actions: ['.limits .chip'], expectText: 'Fixture backend failure' }]
    }
  },
  {
    path: '/mindset/templates', slug: 'mindset-templates',
    ready: { populated: '.card .title', empty: '.page .btn.primary', error: '.page' },
    views: {
      populated: [
        { name: 'library', actions: ['.page-header .actions .btn', '.page-header .actions .btn.primary', '.side .search', '.catlist .cat:nth-child(2)', '.sidefoot .link', '.card .title', '.card .cactions .icon', '.card .cactions .icon.del'] },
        { name: 'editor', enter: async (page) => { await page.locator('.card .cactions .icon').nth(2).click(); await page.locator('.modal .prow').nth(1).waitFor(); }, actions: ['.modal .row input', '.modal .fld button', '.modal .chips .chip', '.modal .prow .plabel', '.modal .prow .kindbtn', '.modal .prow .pbtn:not(.kindbtn):not(.del) >> nth=1', '.modal .prow .pbtn.del', '.modal .addrow .chip', '.modal .foot .btn.danger', '.modal .foot .pause', '.modal .foot .btn:not(.primary):not(.danger)', '.modal .foot .btn.primary'], leave: async (page) => page.keyboard.press('Escape') },
        { name: 'prompt-detail', enter: async (page) => { await page.locator('.card .cactions .icon').nth(2).click(); await page.locator('.modal .prow').nth(1).waitFor(); await page.locator('.modal .prow .kindbtn').nth(2).click(); await page.locator('.modal .pdetail').waitFor(); }, actions: ['.modal .pdetail .chips .chip', '.modal .pdetail .opts input', '.modal .pdetail .opts .pbtn.del', '.modal .pdetail .add', '.modal .pdetail .chip.sm', '.modal .foot .btn.primary'], leave: async (page) => page.keyboard.press('Escape') }
      ],
      empty: [{ name: 'empty', actions: ['.page-header .actions .btn.primary', '.page .btn.primary >> nth=1'] }],
      error: [{ name: 'error', actions: ['.page-header .actions .btn.primary'], expectText: 'Fixture backend failure' }]
    }
  },
  {
    path: '/resources', slug: 'resources',
    ready: { populated: '.card h3', empty: '.top .primary', error: '.page' },
    views: {
      populated: [
        { name: 'cards', actions: ['.top .search', '.seg button:nth-child(1)', '.seg button:nth-child(3)', '.tools .ghost', '.tools .primary', '.rail .cat', '.rail .add-cat', '.cat-row .cat', '.cat-row .cat-actions .icon', '.cat-row .cat-actions .icon >> nth=1', '.cards .card .row-actions .icon', '.cards .card .row-actions .icon >> nth=1', '.cards .card .host'] },
        { name: 'gallery', enter: async (page) => page.locator('.seg button').nth(0).click(), actions: ['.gallery .gthumb', '.gallery .row-actions .icon', '.gallery .row-actions .icon >> nth=1', '.gallery .host'] },
        { name: 'list', enter: async (page) => page.locator('.seg button').nth(2).click(), actions: ['.list li .row-actions .icon', '.list li .row-actions .icon >> nth=1', '.list li .host'] },
        { name: 'form', enter: async (page) => { await page.locator('.seg button').nth(1).click(); await page.locator('.tools .primary').click(); await page.locator('.modal input').first().waitFor(); }, actions: ['.modal input', '.modal .actions .primary', '.modal .actions .ghost'], leave: async (page) => page.keyboard.press('Escape') }
      ],
      empty: [{ name: 'empty', actions: ['.tools .primary', '.rail .add-cat'] }],
      error: [{ name: 'error', actions: [] }]
    }
  },
  {
    path: '/routines', slug: 'routines',
    ready: { populated: '.routine label', empty: '.page .status', error: '.page' },
    views: {
      populated: [
        { name: 'board', actions: ['.modnav a:nth-child(2)', '.datenav .btn', '.datenav input[type="date"]', '.periods .chip:nth-child(3)', '.tally', '.markbtn', '.gtoggle', '.gall', '.routine .rall', '.routine .edit', '.routine label', '.routine li:nth-child(2) label', '.routine .ilink', '.routine .inote'] },
        { name: 'note-open', enter: async (page) => { await page.locator('.routine .inote').first().click(); await page.locator('.notebody').first().waitFor(); }, actions: ['.routine .inote', '.routine li:nth-child(2) label'] }
      ],
      empty: [{ name: 'empty', actions: ['.markbtn', '.page a.btn.primary'] }],
      error: [{ name: 'error', actions: ['.datenav .btn'], expectText: 'Fixture backend failure' }]
    }
  },
  {
    path: '/routines/templates', slug: 'routines-templates',
    ready: { populated: '.card .title', empty: '.page .btn.primary', error: '.page' },
    views: {
      populated: [
        { name: 'library', actions: ['.page-header .actions .btn', '.page-header .actions .btn.primary', '.side .search', '.catlist .cat:nth-child(2)', '.sidemanage', '.card .title', '.card .cactions .icon', '.card .cactions .icon.del'] },
        { name: 'library-open', enter: async (page) => { await page.locator('.card .title').first().click(); await page.locator('.card .body').first().waitFor(); }, actions: ['.card .body .steplist a'] },
        { name: 'editor', enter: async (page) => { await page.locator('.card .cactions .icon').nth(2).click(); await page.locator('.modal .step').nth(1).waitFor(); }, actions: ['.modal .row input', '.modal .fld button', '.modal .step .slabel', '.modal .step .sbtn', '.modal .step .sbtn:not(.del) >> nth=2', '.modal .step .sbtn.del', '.modal .add', '.modal .foot .btn.danger', '.modal .foot .btn.primary'], leave: async (page) => page.keyboard.press('Escape') },
        { name: 'step-detail', enter: async (page) => { await page.locator('.card .cactions .icon').nth(2).click(); await page.locator('.modal .step').nth(1).waitFor(); await page.locator('.modal .step .sbtn').first().click(); await page.locator('.modal .sdetail').waitFor(); }, actions: ['.modal .sdetail .linkrow input', '.modal .sdetail .linkrow input >> nth=1', '.modal .foot .btn.primary'], leave: async (page) => page.keyboard.press('Escape') }
      ],
      empty: [{ name: 'empty', actions: ['.page-header .actions .btn.primary', '.page .btn.primary >> nth=1'] }],
      error: [{ name: 'error', actions: ['.page-header .actions .btn.primary'], expectText: 'Fixture backend failure' }]
    }
  },
  {
    path: '/prompt-store', slug: 'prompt-store',
    ready: { populated: '.card h3', empty: '.empty .primary', error: '.page' },
    views: {
      populated: [
        { name: 'grid', actions: ['.top .search', '.seg button:nth-child(2)', '.tools .primary', '.tag-rail .pill:nth-child(2)', '.card .row-actions .icon', '.card .row-actions .icon >> nth=1', '.card .row-actions .icon.danger', '.card .preview', '.card .tags .tag', '.card .votes .vote', '.card .votes .vote >> nth=1', '.card .votes .vote >> nth=2'] },
        { name: 'editor', enter: async (page) => { await page.locator('.card .row-actions .icon').first().click(); await page.locator('.modal textarea').waitFor(); }, actions: ['.modal .field input', '.modal .field textarea', '.modal .actions .history', '.modal .actions .ghost:not(.history)', '.modal .actions .primary'] },
        { name: 'history', enter: async (page) => { await page.locator('.modal .actions .history').click(); await page.locator('.modal .versions li').first().waitFor(); await page.locator('.modal .v-toggle').nth(1).click(); }, actions: ['.modal .v-toggle', '.modal .restore'], leave: async (page) => { await page.keyboard.press('Escape'); await page.waitForTimeout(200); await page.keyboard.press('Escape'); } }
      ],
      empty: [{ name: 'empty', actions: ['.tools .primary', '.empty .primary'] }],
      error: [{ name: 'error', actions: [] }]
    }
  },
  {
    path: '/subscriptions', slug: 'subscriptions',
    ready: { populated: 'tbody tr td.strong', empty: '.filters', error: '.subs' },
    views: {
      populated: [
        { name: 'page', actions: ['.cur-select button', '.head-actions button', '.head-actions .primary', '.chart-toggles .toggle button:nth-child(2)', '.filters .search', '.filters input >> nth=1', '.filters .band button', 'thead .sortable', 'thead th:nth-child(7) .sortable', 'tbody tr:nth-child(1) .row-actions .icon', 'tbody tr:nth-child(1) .row-actions button.icon', 'tbody tr:nth-child(1) .row-actions .danger-hover', 'tbody tr:nth-child(1) .next-wrap button'] },
        { name: 'form', enter: async (page) => { await page.locator('.head-actions .primary').click(); await page.locator('.modal .sub-form').waitFor(); }, actions: ['.modal .sub-form input', '.modal .grid input[type="number"]', '.modal .grid .field button', '.modal .grid input[type="date"]', '.modal .check', '.modal .actions .ghost', '.modal .actions .primary'], leave: async (page) => page.keyboard.press('Escape') }
      ],
      empty: [{ name: 'empty', actions: ['.head-actions .primary', '.subs .primary >> nth=1'] }],
      error: [{ name: 'error', actions: [] }]
    }
  },
  {
    path: '/community-docs', slug: 'community-docs',
    ready: { populated: '.cat-card', empty: '.browse-head', error: '.page' },
    views: {
      populated: [
        { name: 'categories', actions: ['.fav-head .search', '.fav-head .icon-btn', '.fav-cat-row', '.fav', '.browse-head .search', '.browse-head .seg button:nth-child(2)', '.cat-card'] },
        { name: 'category', enter: async (page) => { await page.locator('.cat-card').first().click(); await page.locator('.doc-card').first().waitFor(); }, actions: ['.center-top .back', '.doc-card', '.doc-card >> nth=1'] },
        { name: 'category-list', enter: async (page) => { await page.locator('.browse-head .seg button').nth(1).click(); await page.locator('.doc-row').first().waitFor(); }, actions: ['.doc-row', '.center-top .back'] },
        { name: 'reader', enter: async (page) => { await page.locator('.doc-row').first().click(); await page.locator('.article .content').waitFor(); }, actions: ['.center-top .back', '.article-head-row .star', 'inline:.article .meta a', 'inline:.article .content p a', 'inline:.article .content li a', '.article .content pre'] }
      ],
      empty: [{ name: 'empty', actions: ['.browse-head .search', '.fav-head .icon-btn'] }],
      error: [{ name: 'error', actions: [] }]
    }
  }
];

async function checkView(page, route, state, view, lang, viewports) {
  for (const vp of viewports) {
    await page.setViewportSize(vp);
    await page.waitForTimeout(150);
    const tag = `${lang} ${route.path} ${state}/${view.name} ${vp.width}x${vp.height}`;
    const o = await overflow(page);
    if (o.scroll > o.viewport + 1) fail(`${tag}: page overflow ${o.scroll} > ${o.viewport} [${o.offenders.join(', ')}]`);
    for (const inner of o.inner) fail(`${tag}: horizontal scroll inside ${inner}`);
    for (const sel of view.actions) {
      const r = await reach(page, sel, vp.width < 768);
      if (r) fail(`${tag}: ${sel} ${r}`);
    }
    if (view.expectText) {
      const shown = await page.getByText(view.expectText).first().isVisible().catch(() => false);
      if (!shown) fail(`${tag}: error message not visible`);
    }
    if (view.charts) {
      const clipped = await page.evaluate(() => {
        const out = [];
        for (const svg of document.querySelectorAll('.trend svg')) {
          const box = svg.getBoundingClientRect();
          for (const t of svg.querySelectorAll('text')) {
            const r = t.getBoundingClientRect();
            if (r.left < box.left - 0.5 || r.right > box.right + 0.5 || r.top < box.top - 0.5 || r.bottom > box.bottom + 0.5) out.push(t.textContent);
            if (r.height < 7) out.push(`${t.textContent} renders ${r.height.toFixed(1)} px tall`);
          }
        }
        return out;
      });
      if (clipped.length) fail(`${tag}: chart labels clipped or unreadable: ${clipped.slice(0, 4).join(', ')}`);
    }
    // Inner scroll containers keep fullPage captures to one screen: keep the view where the
    // last checked action left it, then the top of the view.
    if (vp.width <= 390) await page.screenshot({ path: `${artifacts}${lang}-${route.slug}-${state}-${view.name}-${vp.width}x${vp.height}-end.png`, animations: 'disabled' });
    await page.evaluate(() => {
      for (const el of document.querySelectorAll('*')) if (el.scrollTop) el.scrollTop = 0;
    });
    await page.screenshot({ path: `${artifacts}${lang}-${route.slug}-${state}-${view.name}-${vp.width}x${vp.height}.png`, fullPage: true, animations: 'disabled' });
  }
}

async function sweep(lang, states, viewports) {
  for (const state of states) {
    const context = await newContext({ touch: true, lang, state });
    await context.clock?.setFixedTime?.(CLOCK);
    for (const route of ROUTES.filter((r) => !routeFilter || routeFilter.includes(r.slug))) {
      const page = await context.newPage();
      await page.clock.setFixedTime(CLOCK);
      const errors = [];
      page.on('pageerror', (e) => errors.push(e.message));
      try {
        await gotoReady(page, `${base}${route.path}`, route.ready[state]);
        if (state === 'error') await page.waitForTimeout(600);
        for (const view of route.views[state]) {
          if (view.enter) {
            await page.setViewportSize(VIEWPORTS[1]);
            await view.enter(page);
            await page.waitForTimeout(250);
          }
          await checkView(page, route, state, view, lang, viewports);
          if (view.leave) {
            await view.leave(page);
            await page.waitForTimeout(250);
          }
        }
      } catch (e) {
        fail(`${lang} ${route.path} ${state}: ${e.message.split('\n')[0]}`);
        await page.screenshot({ path: `${artifacts}${lang}-${route.slug}-${state}-failure.png`, fullPage: true }).catch(() => {});
      }
      if (errors.length) {
        const entry = `${lang} ${route.path} ${state}: ${[...new Set(errors)].join(' | ')}`;
        if (state === 'error') report.pageErrors.push(entry);
        else fail(`pageerror ${entry}`);
      }
      await page.close();
    }
    for (const u of context.unknown) report.unknown.add(u);
    await context.close();
  }
}

// ── Primary flows by tap, with the payload compared to the desktop flow ───

const shapeOf = (v) => (Array.isArray(v) ? [v.length ? shapeOf(v[0]) : 'empty'] : v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, shapeOf(v[k])])) : typeof v);

const FLOWS = [
  {
    name: 'watchlist add symbol', path: '/watchlists', ready: '.detail .titlerow h2',
    run: async (page, act) => {
      await act(page.locator('.toolbar .btn').first());
      await act(page.locator('.modal .kinds button').nth(1));
      await page.locator('.modal input.search').fill('AAPL');
      await page.locator('.modal .hit').first().waitFor();
      await act(page.locator('.modal .hit').first());
    },
    match: (c) => c.method === 'POST' && c.path === '/api/watchlists/wl-1/items',
    expect: (b) => assert.deepEqual(b, { asset_class: 'stock', provider: 'yahoo', provider_id: 'AAPL', symbol: 'AAPL', name: 'Apple Inc. common stock listed on the NASDAQ Global Select Market' })
  },
  {
    name: 'mindset create entry', path: '/mindset', ready: '.card .prompts',
    run: async (page, act) => {
      const card = page.locator('.card').first();
      await act(card.locator('.steps button').nth(3));
      await act(card.locator('.prompt').nth(1).locator('.chip').nth(3));
      await act(card.locator('.prompt').nth(2).locator('.chip').nth(0));
      await act(card.locator('.prompt').nth(2).locator('.chip').nth(7));
      await card.locator('textarea').fill('Stop after two losses.');
      await act(card.locator('.foot .btn.primary'));
    },
    match: (c) => c.method === 'PUT' && c.path === '/api/mindset/entries',
    expect: (b) => assert.deepEqual(b, { date: TODAY, template_id: 'mt-1', answers: { 'mp-1': 4, 'mp-2': '🙂', 'mp-3': ['calm', 'distracted by overnight news'], 'mp-4': 'Stop after two losses.' } })
  },
  {
    name: 'prompt copy', path: '/prompt-store', ready: '.card h3',
    run: async (page, act) => act(page.locator('.card').first().locator('.votes .vote').nth(2)),
    clipboard: PROMPTS[0].body
  },
  {
    name: 'routine tick', path: '/routines', ready: '.routine label',
    run: async (page, act) => act(page.locator('.routine li').nth(1).locator('label')),
    match: (c) => c.method === 'POST' && c.path === '/api/trader/items/ri-2/check',
    expect: (b) => assert.deepEqual(b, { date: TODAY, checked: true })
  },
  {
    name: 'mindset template reorder by tap', path: '/mindset/templates', ready: '.card .title',
    run: async (page, act) => {
      await act(page.locator('.card .cactions .icon').nth(2));
      await page.locator('.modal .prow').nth(3).waitFor();
      await act(page.locator('.modal .prow').first().locator('.pbtn:not(.kindbtn):not(.del)').nth(1));
      await act(page.locator('.modal .foot .btn.primary'));
    },
    match: (c) => c.method === 'PATCH' && c.path === '/api/mindset/templates/mt-1',
    expect: (b) => assert.deepEqual(b.prompts.map((p) => p.id), ['mp-2', 'mp-1', 'mp-3', 'mp-4'])
  },
  {
    name: 'routine template reorder by tap', path: '/routines/templates', ready: '.card .title',
    run: async (page, act) => {
      await act(page.locator('.card .cactions .icon').nth(2));
      await page.locator('.modal .step').nth(2).waitFor();
      await act(page.locator('.modal .step').first().locator('.sbtn:not(.del)').nth(2));
      await act(page.locator('.modal .foot .btn.primary'));
    },
    match: (c) => c.method === 'PATCH' && c.path === '/api/trader/routines/rt-1',
    expect: (b) => assert.deepEqual(b.items.map((i) => i.id), ['ri-2', 'ri-1', 'ri-3'])
  },
  {
    name: 'subscription add', path: '/subscriptions', ready: 'tbody tr td.strong',
    run: async (page, act) => {
      await act(page.locator('.head-actions .primary'));
      const form = page.locator('.modal .sub-form');
      await form.locator('input').first().fill('Fixture data feed with a long product name');
      await form.locator('input[type="number"]').fill('49.5');
      await form.locator('input[type="date"]').fill('2026-10-01');
      await act(form.locator('.actions .primary'));
    },
    match: (c) => c.method === 'POST' && c.path === '/api/subscriptions',
    expect: (b) => assert.deepEqual(b, { name: 'Fixture data feed with a long product name', platform: null, url: null, price: 49.5, currency: 'EUR', frequency: 'monthly', category: null, started_on: '2026-10-01', active: true })
  }
];

async function runFlow(flow, mode) {
  const touch = mode === 'phone';
  const viewport = touch ? VIEWPORTS[1] : { width: 1440, height: 900 };
  const context = await newContext({ touch, viewport });
  const page = await context.newPage();
  await page.clock.setFixedTime(CLOCK);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  const act = async (loc) => {
    await loc.scrollIntoViewIfNeeded();
    if (touch) await loc.tap();
    else await loc.click();
  };
  let payload;
  try {
    await gotoReady(page, `${base}${flow.path}`, flow.ready);
    await flow.run(page, act);
    await page.waitForTimeout(400);
    if (flow.clipboard !== undefined) {
      payload = await page.evaluate(() => window.__copied);
      assert.deepEqual(payload, [flow.clipboard]);
    } else {
      const hits = context.captured.filter(flow.match);
      assert.equal(hits.length, 1, `${flow.name} sends one request (${context.captured.map((c) => `${c.method} ${c.path}`).join(', ')})`);
      payload = hits[0].body;
      flow.expect(payload);
    }
    assert.deepEqual(errors, []);
    if (touch) await page.screenshot({ path: `${artifacts}flow-${flow.name.replaceAll(' ', '-')}-390.png`, fullPage: true });
  } catch (e) {
    fail(`flow ${flow.name} (${mode}): ${e.message.split('\n')[0]}`);
    await page.screenshot({ path: `${artifacts}flow-${flow.name.replaceAll(' ', '-')}-${mode}-failure.png`, fullPage: true }).catch(() => {});
  }
  await context.close();
  return payload;
}

// ── Desktop no-regression against the baseline server ──────────────────────

const DESKTOP = [
  ...ROUTES.map((r) => ({ name: r.slug, path: r.path, ready: r.ready.populated })),
  { name: 'watchlists-add-symbol', path: '/watchlists', ready: '.detail .titlerow h2', enter: (p) => ROUTES[0].views.populated[1].enter(p) },
  { name: 'mindset-template-editor', path: '/mindset/templates', ready: '.card .title', enter: (p) => ROUTES[3].views.populated[2].enter(p) },
  { name: 'resources-gallery', path: '/resources', ready: '.card h3', enter: (p) => p.locator('.seg button').nth(0).click() },
  { name: 'resources-list', path: '/resources', ready: '.card h3', enter: (p) => p.locator('.seg button').nth(2).click() },
  { name: 'routines-template-editor', path: '/routines/templates', ready: '.card .title', enter: (p) => ROUTES[6].views.populated[3].enter(p) },
  { name: 'prompt-editor', path: '/prompt-store', ready: '.card h3', enter: (p) => ROUTES[7].views.populated[1].enter(p) },
  { name: 'subscription-form', path: '/subscriptions', ready: 'tbody tr td.strong', enter: (p) => ROUTES[8].views.populated[1].enter(p) },
  { name: 'community-docs-reader', path: '/community-docs', ready: '.cat-card', enter: async (p) => { await ROUTES[9].views.populated[1].enter(p); await p.locator('.doc-card').first().click(); await p.locator('.article .content').waitFor(); } },
  { name: 'routines-empty', path: '/routines', ready: '.page .status', state: 'empty' },
  { name: 'watchlists-error', path: '/watchlists', ready: '.page', state: 'error' }
];

async function desktopShot(server, item, viewport) {
  const context = await newContext({ viewport, reducedMotion: 'reduce', state: item.state ?? 'populated' });
  const page = await context.newPage();
  await page.clock.setFixedTime(CLOCK);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await gotoReady(page, `${server}${item.path}`, item.ready);
  if (item.enter) await item.enter(page);
  await page.mouse.move(0, viewport.height - 1);
  await page.waitForTimeout(600);
  const png = await page.screenshot({ fullPage: true, animations: 'disabled', caret: 'hide' });
  await context.close();
  return png;
}

// ── Run ─────────────────────────────────────────────────────────────────────

const only = process.env.MISC_ONLY?.split(',');
const desktopFilter = process.env.MISC_DESKTOP?.split(',');
const routeFilter = process.env.MISC_ROUTES?.split(',');
const t0 = Date.now();
if (!only || only.includes('sweep')) {
  await sweep('en', process.env.MISC_STATES?.split(',') ?? ['populated', 'empty', 'error'], VIEWPORTS);
  if (!process.env.MISC_STATES) await sweep('de', ['populated'], VIEWPORTS);
  console.log(`sweep done in ${Math.round((Date.now() - t0) / 1000)} s`);
}
if (!only || only.includes('flows')) {
  for (const flow of FLOWS) {
    const phone = await runFlow(flow, 'phone');
    const desk = await runFlow(flow, 'desktop');
    const same = phone !== undefined && desk !== undefined && JSON.stringify(shapeOf(phone)) === JSON.stringify(shapeOf(desk));
    if (!same) fail(`flow ${flow.name}: phone payload shape differs from desktop`);
    report.flows.push(`${flow.name}: ${same ? 'same payload shape at 390 px (tap) and 1440 px (click)' : 'MISMATCH'} ${JSON.stringify(shapeOf(phone))}`);
  }
}
if ((!only || only.includes('desktop')) && baseline !== '-') {
  for (const item of DESKTOP.filter((d) => !desktopFilter || desktopFilter.includes(d.name))) {
    for (const viewport of [{ width: 1440, height: 900 }, { width: 1100, height: 800 }]) {
      const label = `${item.name} ${viewport.width}x${viewport.height}`;
      try {
        // Up to three paired captures: text antialiasing on the shared dev servers is not
        // always stable, so a pair only fails when no attempt matches. A failing item also
        // captures the baseline twice to tell rendering noise from a real change.
        let a;
        let b;
        let same = false;
        let attempt = 0;
        while (!same && attempt < 3) {
          attempt++;
          a = await desktopShot(baseline, item, viewport);
          b = await desktopShot(base, item, viewport);
          same = a.equals(b);
        }
        let noise = '';
        if (!same) {
          await writeFile(`${artifacts}desktop-${item.name}-${viewport.width}-baseline.png`, a);
          await writeFile(`${artifacts}desktop-${item.name}-${viewport.width}-work.png`, b);
          const a2 = await desktopShot(baseline, item, viewport);
          noise = a.equals(a2) ? ' (baseline repeats identically: real difference)' : ' (baseline itself varies between captures: rendering noise)';
          fail(`desktop ${label}: differs from baseline${noise}`);
        }
        report.desktop.push(`${label}: ${same ? `identical${attempt > 1 ? ` on attempt ${attempt}` : ''}` : `DIFFERENT${noise}`}`);
      } catch (e) {
        fail(`desktop ${label}: ${e.message.split('\n')[0]}`);
      }
    }
  }
}
await browser.close();

for (const line of report.flows) console.log(`flow   ${line}`);
for (const line of report.desktop) console.log(`desk   ${line}`);
for (const line of report.pageErrors) console.log(`note   500 fixture page error: ${line}`);
if (report.unknown.size) console.log(`note   unmocked API calls answered {}: ${[...report.unknown].sort().join(', ')}`);
await writeFile(`${artifacts}report.json`, JSON.stringify({ ...report, unknown: [...report.unknown] }, null, 2));
if (report.failures.length) {
  for (const f of report.failures) console.error(`FAIL   ${f}`);
  console.error(`${report.failures.length} failures. Screenshots: ${artifacts}`);
  process.exitCode = 1;
} else {
  console.log(`PASS: ${ROUTES.length} routes x 3 states x ${VIEWPORTS.length} viewports (plus de-DE populated), ${FLOWS.length} tap flows, desktop comparison. Screenshots: ${artifacts}`);
}
