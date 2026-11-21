// Finance routes at phone, tablet and desktop widths: Journal, Portfolios, Mportfolios,
// Wealth and Taxcalc. Fixture API only (no backend, no credentials).
// Run with the frontend dev server: node tests/mobile-finance-browser.mjs [base URL] [baseline URL]
// Flags: --explore (report overflow without failing), --only=journal,wealth (route subset),
// --skip-desktop (no baseline comparison).
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { PNG } from 'playwright-core/lib/utilsBundle';

const argv = process.argv.slice(2);
const flags = Object.fromEntries(argv.filter((a) => a.startsWith('--')).map((a) => {
  const [k, ...v] = a.slice(2).split('=');
  return [k, v.length ? v.join('=') : true];
}));
const positional = argv.filter((a) => !a.startsWith('--'));
const base = positional[0] ?? 'http://127.0.0.1:4173';
const baseline = positional[1] ?? 'http://127.0.0.1:4190';
const only = flags.only ? String(flags.only).split(',') : null;
const explore = !!flags.explore;
const want = (name) => !only || only.includes(name);

const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-finance/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });

const VIEWPORTS = [
  { width: 320, height: 568 },
  { width: 390, height: 844 },
  { width: 600, height: 800 },
  { width: 667, height: 375 },
  { width: 768, height: 1024 }
];
const DESKTOP = [
  { width: 1440, height: 900 },
  { width: 1100, height: 800 }
];
const FIXED_NOW = new Date('2026-10-14T10:00:00Z');

// ── Fixtures ────────────────────────────────────────────────────────────────────────
const LONG = 'Swing book with a deliberately long category name for narrow screens';
const journal = {
  categories: [
    { id: 'cat-1', name: LONG, is_default: true, position: 0, color: '#4a90d9', description: 'Daily swing trades, reviewed every Friday.' },
    { id: 'cat-2', name: 'Futures', is_default: false, position: 1, color: '#d98a4a', description: '' },
    { id: 'cat-3', name: 'Crypto', is_default: false, position: 2, color: '', description: '' }
  ],
  strategies: [{ id: 'st-1', name: 'Breakout continuation', category_ids: ['cat-1'], description: '' }],
  templates: [{ id: 'tpl-1', name: 'Equity swing', fields: [], default_fee_schedule_id: null }],
  tags: [
    { id: 'tag-1', name: 'Moved stop', kind: 'mistake' },
    { id: 'tag-2', name: 'Followed plan', kind: 'rule' }
  ],
  trades: [
    { id: 't-1', category_id: 'cat-1', template_id: null, strategy_id: 'st-1', ticker: 'BRK.B', asset_class: 'stock', exchange: 'NYSE', side: 'long', currency: 'USD', unit_type: 'shares', fee_schedule_id: null, entry_at: '2026-09-02T13:35:00Z', exit_at: '2026-09-09T19:50:00Z', entry_price: 412.37, exit_price: 431.02, quantity: 125, fees: 2.5, leverage: 1, multiplier: 1, signal_name: 'Breakout', feedback: '', images: [], fields: {}, advanced: false, entries: [], exits: [], brackets: [], stop_price: 401.5, tags: ['tag-2'], net_pnl: 2328.75, open_qty: 0 },
    { id: 't-2', category_id: 'cat-2', template_id: null, strategy_id: null, ticker: 'ESZ6', asset_class: 'future', exchange: 'CME', side: 'short', currency: 'USD', unit_type: 'contracts', fee_schedule_id: null, entry_at: '2026-09-15T14:00:00Z', exit_at: '2026-09-15T18:20:00Z', entry_price: 5812.25, exit_price: 5868.5, quantity: 3, fees: 12.6, leverage: 1, multiplier: 50, signal_name: '', feedback: 'Faded the open too early.', images: [], fields: {}, advanced: false, entries: [], exits: [], brackets: [], stop_price: null, tags: ['tag-1'], net_pnl: -8450.1, open_qty: 0 },
    { id: 't-3', category_id: 'cat-3', template_id: null, strategy_id: null, ticker: 'BTCUSDT', asset_class: 'crypto', exchange: 'Binance', side: 'long', currency: 'EUR', unit_type: 'coins', fee_schedule_id: null, entry_at: '2026-10-01T08:00:00Z', exit_at: null, entry_price: null, exit_price: null, quantity: null, fees: 0, leverage: 1, multiplier: 1, signal_name: '', feedback: '', images: [], fields: {}, advanced: true, cost_basis_method: 'avg', entries: [{ id: 'l1', price: 61234.5, qty: 0.75, at: '2026-10-01T08:00:00Z', fees: 4.2, signal: '' }, { id: 'l2', price: 59876.25, qty: 0.5, at: '2026-10-03T08:00:00Z', fees: 2.8, signal: '' }], exits: [{ id: 'l3', price: 64010, qty: 0.6, at: '2026-10-08T08:00:00Z', fees: 3.1, signal: '' }], brackets: [], stop_price: 57000, tags: [], avg_entry: 60691.2, net_pnl: 1999.08, open_qty: 0.65 },
    { id: 't-4', category_id: 'cat-1', template_id: 'tpl-1', strategy_id: null, ticker: 'NVDA', asset_class: 'stock', exchange: 'NASDAQ', side: 'long', currency: 'USD', unit_type: 'shares', fee_schedule_id: null, entry_at: '2026-10-10T00:00:00', exit_at: null, entry_price: 182.4, exit_price: null, quantity: 40, fees: 1, leverage: 1, multiplier: 1, signal_name: '', feedback: '', images: [], fields: {}, advanced: false, entries: [], exits: [], brackets: [], stop_price: null, tags: [], net_pnl: null, open_qty: 40 }
  ],
  breakdown: {
    invested_capital: 250000, current_equity: 245877.73, realized_pnl: -4122.27, return_pct: -1.649,
    win_rate: 0.5, profit_factor: 0.51, sharpe: -0.42, sortino: -0.61, max_drawdown: 3.38,
    closed_count: 3, open_count: 1, peak_margin: 51546.25, return_on_margin: -7.99, total_fees: 26.2,
    avg_win: 2163.92, best_trade: 2328.75, avg_loss: -8450.1, worst_trade: -8450.1, expectancy: -1374.09,
    unconverted_trades: 0,
    equity_curve: [
      { at: '2026-09-09T19:50:00Z', equity: 252328.75, pnl: 2328.75 },
      { at: '2026-09-15T18:20:00Z', equity: 243878.65, pnl: -8450.1 },
      { at: '2026-10-08T08:00:00Z', equity: 245877.73, pnl: 1999.08 }
    ]
  },
  analytics() {
    const bucket = (key, trades, net) => ({ key, trades, net, avg: trades ? net / trades : 0, win_rate: trades ? 50 : null });
    const group = (key, name, trades, net) => ({ key, name, trades, net, win_rate: 50, avg: net / trades, expectancy: net / trades, profit_factor: 1.2, avg_r: 0.4 });
    return {
      closed_count: 3, unconverted_trades: 0,
      r: { avg_r: -0.31, total_r: -0.93, avg_win_r: 1.62, avg_loss_r: -4.17, avg_risk_pct: 1.25, with_stop: 2, without_stop: 1, buckets: [bucket('lt3', 1, -8450.1), bucket('r1r2', 1, 2328.75)] },
      daily: { sharpe: -0.42, sortino: -0.61, max_drawdown: 3.38, max_drawdown_days: 23, days: 3, win_days: 2, loss_days: 1, avg_day: -1374.09, best_day: 2328.75, worst_day: -8450.1 },
      streaks: { current: 1, best_win: 1, worst_loss: 1 },
      mistakes: { tagged: 0 },
      distributions: { hold: [bucket('day', 2, -6121.35)], hour: [bucket('13', 1, 2328.75)], weekday: [bucket('1', 2, -6121.35)], size: [{ ...bucket('a', 3, -4122.27), lo: 0, hi: 100000 }] },
      groups: {
        strategy: [group('st-1', 'Breakout continuation', 1, 2328.75)],
        ticker: [group('BRK.B', 'BRK.B', 1, 2328.75), group('ESZ6', 'ESZ6', 1, -8450.1)],
        tag: [], asset_class: [group('stock', 'Stock', 1, 2328.75), group('future', 'Future', 1, -8450.1)],
        side: [group('long', 'long', 2, 4327.83), group('short', 'short', 1, -8450.1)]
      },
      points: [], behavior: null, market: null
    };
  }
};

const portfolios = {
  list: [
    { id: 'pf-1', name: 'Retirement core holdings with a long descriptive name', currency: 'EUR', description: 'Long-horizon ETF and dividend book, rebalanced each quarter, never touched intraday.', market_value: 1284563.42, cost_basis: 1301200, unrealized: -16636.58, asset_count: 12, auto_refresh: true, refreshed_at: '2026-10-13T06:00:00Z', sparkline: [1290000, 1301000, 1296000, 1284563.42] },
    { id: 'pf-2', name: 'Crypto', currency: 'USD', description: '', market_value: 48210.9, cost_basis: 31000, unrealized: 17210.9, asset_count: 3, auto_refresh: false, refreshed_at: null, sparkline: [40000, 45000, 48210.9] }
  ],
  detail() {
    return {
      portfolio: { id: 'pf-1', name: 'Retirement core holdings with a long descriptive name', currency: 'EUR', description: 'Long-horizon ETF and dividend book.', notes: '', auto_refresh: true },
      summary: { market_value: 1284563.42, cost_basis: 1301200, unrealized: -16636.58, realized: 4210.33 },
      assets: [
        { id: 'a-1', symbol: 'VWCE', name: 'Vanguard FTSE All-World UCITS ETF (USD) Accumulating', asset_class: 'etf', quantity: 5120.5, avg_cost: 118.42, price: 121.06, market_value: 619887.73, weight: 48.3, unrealized: 13518.12, realized: 0, recon_status: 'ok', spot_provider: null },
        { id: 'a-2', symbol: 'BTC', name: 'Bitcoin', asset_class: 'crypto', quantity: 0.84211, avg_cost: 71234.11, price: 61022.4, market_value: 51387.53, weight: 4, unrealized: -8599.24, realized: 4210.33, recon_status: 'manual', spot_provider: 'kraken' },
        { id: 'a-3', symbol: 'AIR.PA', name: 'Airbus SE', asset_class: 'stock', quantity: 1500, avg_cost: 162.1, price: 148.33, market_value: 222495, weight: 17.3, unrealized: -20655, realized: 0, recon_status: 'unresolved', spot_provider: null }
      ],
      operations: [
        { operation: { id: 'op-1', op_date: '2026-03-14', side: 'buy', quantity: 5120.5, price: 118.42, fee: 9.95, note: 'Monthly savings plan with a note that is long enough to be cut on a phone' }, symbol: 'VWCE', currency: 'EUR' },
        { operation: { id: 'op-2', op_date: '2026-06-02', side: 'sell', quantity: 0.2, price: 92011.5, fee: 14.2, note: '' }, symbol: 'BTC', currency: 'USD' },
        { operation: { id: 'op-3', op_date: '2026-07-01', side: 'dividend', quantity: 1, price: 1834.27, fee: 0, note: 'Q2 distribution' }, symbol: 'AIR.PA', currency: 'EUR' }
      ],
      snapshots: [
        { at: '2026-08-01T00:00:00Z', market_value: 1250000, cost_basis: 1290000 },
        { at: '2026-09-01T00:00:00Z', market_value: 1296000, cost_basis: 1301200 },
        { at: '2026-10-01T00:00:00Z', market_value: 1284563.42, cost_basis: 1301200 }
      ]
    };
  },
  analytics: {
    blocks: { book: { status: 'ok', data: { net_worth: 1284563.42, invested: 893770.26, cash_total: 390793.16, cash_pct: 30.4, cash_tracked: true, unrealized: -16636.58, realized: 4210.33, income: { total: 1834.27 }, cash: [{ currency: 'EUR', amount: 400000 }, { currency: 'USD', amount: -9206.84 }], unpriced: 0, allocation: [{ bucket: 'etf', value: 619887.73, pct: 48.3 }, { bucket: 'stock', value: 222495, pct: 17.3 }, { bucket: 'cash', value: 390793.16, pct: 30.4 }] }, sample: null } }
  }
};

const importAnalysis = {
  stats: { operations: 2, duplicates: 0, skipped: 0, unresolved: 1, errors: 1, bought: 1, sold: 1, first_date: '2026-03-14', last_date: '2026-06-02', flows: [{ currency: 'EUR', bought: 1481.75, sold: 296.3 }] },
  row_count: 3, headers: ['Date', 'Ticker', 'Side', 'Quantity', 'Price', 'Fee'], sections: [], preamble: [], mapping_match: null,
  columns: [
    { index: 0, header: 'Date', name: 'Date', kind: 'date', samples: ['2026-03-14', '2026-06-02'], target: 'op_date', confidence: 'high', ambiguous_dates: false },
    { index: 1, header: 'Ticker', name: 'Ticker', kind: 'text', samples: ['VWCE', 'IWDA'], target: 'ticker', confidence: 'high', ambiguous_dates: false },
    { index: 2, header: 'Side', name: 'Side', kind: 'text', samples: ['BUY', 'SELL'], target: 'side', confidence: 'medium', ambiguous_dates: false },
    { index: 3, header: 'Quantity', name: 'Quantity', kind: 'num', samples: ['12.5', '-3'], target: 'quantity', confidence: 'high', ambiguous_dates: false },
    { index: 4, header: 'Price', name: 'Price', kind: 'num', samples: ['118.42'], target: 'price', confidence: 'high', ambiguous_dates: false },
    { index: 5, header: 'Commission and regulatory fees (EUR)', name: 'Commission and regulatory fees (EUR)', kind: 'num', samples: ['1.50'], target: 'fee', confidence: 'low', ambiguous_dates: false }
  ],
  mapping: { columns: { 0: 'op_date', 1: 'ticker', 2: 'side', 3: 'quantity', 4: 'price', 5: 'fee' }, defaults: { side: '' }, conventions: { fees_abs: false, qty_sign_is_side: false }, date_order: 'auto', decimal: 'auto', section: null, symbols: {} },
  preview: [
    { index: 0, op_date: '2026-03-14', symbol: 'VWCE', target: 'VWCE', side: 'buy', quantity: 12.5, price: 118.42, fee: 1.5, amount: -1481.75, currency: 'EUR', duplicate: false, skipped: false, unresolved: false, warnings: [] },
    { index: 1, op_date: '2026-06-02', symbol: 'IWDA', target: null, side: 'sell', quantity: 3, price: 98.765432, fee: 0, amount: 296.3, currency: 'EUR', duplicate: false, skipped: false, unresolved: true, warnings: [] }
  ],
  errors: [{ row: 4, message: 'Dividend is not a buy or a sell', raw: '2026-07-01,AIR.PA,DIV,,1834.27,' }],
  symbols: [
    { symbol: 'VWCE', name: 'Vanguard FTSE All-World UCITS ETF', rows: 1, source: 'VWCE', state: 'matched', currency: 'EUR', file_currency: 'EUR', currency_clash: false },
    { symbol: 'IWDA', name: '', rows: 1, source: 'IWDA', state: 'unresolved', currency: null, file_currency: 'EUR', currency_clash: false }
  ]
};

const mportfolios = {
  list: [
    { slug: 'BRK', name: 'Warren Buffett - Berkshire Hathaway', period: 'Q2 2026', value_num: 267300000000, value_text: '$267.3B', stock_count: 41 },
    { slug: 'pabrai', name: 'Mohnish Pabrai - Pabrai Investments with a longer name', period: 'Q2 2026', value_num: 219000000, value_text: '$219M', stock_count: 6 }
  ],
  holdings: [
    { position: 1, ticker: 'AAPL', company: 'Apple Inc. with a long company name', pct: 22.31, activity: 'Reduce 13.24%', shares: 280000000, reported_price: 213.25, value: 59710000000, current_price: 229.87, change_pct: 7.79, week52_low: 169.21, week52_high: 260.1 },
    { position: 2, ticker: 'OXY', company: 'Occidental Petroleum', pct: 4.9, activity: 'Add 2.10%', shares: 264941431, reported_price: 49.5, value: 13114600000, current_price: 44.12, change_pct: -10.87, week52_low: 34.78, week52_high: 64.75 }
  ],
  snapshots: [
    { id: 'snap-1', source_slug: 'BRK', name: 'Warren Buffett - Berkshire Hathaway', taken_at: '2026-09-30T08:12:00Z', period: 'Q2 2026', value_num: 267300000000, value_text: '$267.3B', stock_count: 41 },
    { id: 'snap-2', source_slug: 'BRK', name: 'Warren Buffett - Berkshire Hathaway', taken_at: '2026-06-30T08:12:00Z', period: 'Q1 2026', value_num: 258100000000, value_text: '$258.1B', stock_count: 43 }
  ]
};

const wealth = {
  assets: [
    { id: 'w-1', name: 'Main residence, 14 rue des Longues Avenues, Lyon', asset_type: 'house', category: 'Real estate holdings', latest_value: 1450000, currency: 'EUR', latest_at: '2026-06-30', template_id: null, fields: {} },
    { id: 'w-2', name: 'Mortgage', asset_type: 'loan', category: 'Real estate holdings', latest_value: 612345.67, currency: 'EUR', latest_at: '2026-09-30', template_id: null, fields: {} },
    { id: 'w-3', name: 'Checking account', asset_type: 'money', category: 'Cash', latest_value: 18234.5, currency: 'USD', latest_at: '2026-10-12', template_id: null, fields: {} },
    { id: 'w-4', name: 'Rolex Submariner', asset_type: 'watch', category: '', latest_value: 11800, currency: 'CHF', latest_at: '2025-11-02', template_id: null, fields: {} }
  ],
  breakdown: {
    net_worth: 879123.45, assets_value: 1491469.12, liabilities_value: 612345.67, asset_count: 4, unconverted: 0,
    stale: [{ id: 'w-4', name: 'Rolex Submariner', age_days: 346 }],
    by_category: { 'Real estate holdings': 837654.33, Cash: 16874.5, '': 13560.12 },
    points: [
      { at: '2026-08-01', net_worth: 860000 },
      { at: '2026-09-01', net_worth: 871000 },
      { at: '2026-10-01', net_worth: 879123.45 }
    ]
  },
  templates: [
    { id: 'tpl-house', name: 'House', asset_type: 'house', description: 'Address, surface and purchase price.', fields: [{ key: 'address', label: 'Address', type: 'text' }], is_builtin: true },
    { id: 'tpl-car', name: 'Classic car collection with a long template name', asset_type: 'vehicle', description: '', fields: [], is_builtin: false }
  ],
  revisions: [{ id: 'rev-1', asset_id: 'w-1', value: 1450000, at: '2026-06-30', note: 'Notary estimate', fields: {} }]
};

const taxcalc = {
  templates: [
    { regime: 'fr_pfu', label: 'France: flat tax (PFU)', country: 'FR', person_type: 'individual', currency: 'EUR', inputs: [], custom: false, status: 'verified', verified_on: '2026-09-25', years: [2026], calendar: { year_start: '01-01' }, rules: [] },
    { regime: 'custom_flat', label: 'Custom flat rate', country: 'XX', person_type: 'individual', currency: 'USD', inputs: [], custom: true, rules: [] }
  ],
  profiles: [
    { id: 'pr-1', name: 'France individual, flat tax with a long profile name', country: 'FR', person_type: 'individual', currency: 'EUR', regime: 'fr_pfu', wealth_tax: [] },
    { id: 'pr-2', name: 'Custom 20%', country: 'XX', person_type: 'company', currency: 'USD', regime: 'custom_flat', wealth_tax: [] }
  ],
  scenarios: [
    { id: 'sc-1', name: 'My 2025 estimate', tax_year: 2025, context: 'investing', mode: 'summary', profile_id: 'pr-1', result: { total_tax: 3412.55, currency: 'EUR' } }
  ],
  result: {
    currency: 'EUR', context: 'investing', total_tax: 12345.67, total_base: 41153.9, effective_rate_pct: 30,
    regime: { id: 'fr_pfu', label: 'France: flat tax (PFU)', year: 2026, status: 'verified', coverage: 'full', verified_on: '2026-09-25' },
    lines: [
      { label: 'Capital gains, income tax share at 12.8%', taxable: 41153.9, allowance: 0, base: 41153.9, rate_pct: 12.8, tax: 5267.7 },
      { label: 'Capital gains, social contributions', taxable: 41153.9, allowance: 0, base: 41153.9, rate_pct: 17.2, tax: 7078.47 },
      { label: 'Crypto disposals', taxable: -1250.33, allowance: 305, base: 0, rate_pct: null, tax: 0, exempt: true }
    ],
    pools: [{ pool: 'securities', net: 41153.9, carried_in: 1200, carried_used: 1200, carried_left: 0, loss_created: 0 }],
    warnings: ['Crypto losses cannot offset securities gains.'],
    disclaimer: 'Estimate only. Not tax advice.'
  },
  losses: {
    year: 2026, pools: [{ key: 'securities' }, { key: 'crypto' }],
    carry: [{ pool: 'securities', years: 10, available: 1200, losses: [{ origin: 2024, left: 1200, expires: 2034 }] }],
    rows: [{ tax_year: 2024, pool: 'securities', net: -1200, note: 'Carried from the 2024 return' }]
  }
};

const shell = {
  '/api/setup/status': { configured: true },
  '/api/settings/me': { username: 'fixture', role: 'admin', locale: 'en' },
  '/api/settings/modules': { installed: ['journal', 'portfolios', 'mportfolios', 'wealth', 'taxcalc', 'remindme'] },
  '/api/settings/defaults': { default_timezone: 'UTC', display_currency: 'USD' },
  '/api/health': { status: 'ok', services: { core: 'ok', postgres: 'ok' } },
  '/api/dashboard/layout': { pages: [], favoriteIds: [] },
  '/api/tasks': { tasks: [] },
  '/api/brokers': { accounts: [] },
  '/api/connectors': { connectors: [] },
  '/api/histdata/jobs': { jobs: [] }
};

/** Module responses. `state` is populated | empty | error; requests land in `log`. */
function moduleFixture(p, method, state, body) {
  const empty = state === 'empty';
  // Journal
  if (p === '/api/journal/categories') return { categories: empty ? [{ ...journal.categories[0], name: 'General' }] : journal.categories };
  if (p === '/api/journal/strategies') return { strategies: empty ? [] : journal.strategies };
  if (p === '/api/journal/templates') return { templates: empty ? [] : journal.templates };
  if (p === '/api/journal/tags') return { tags: empty ? [] : journal.tags };
  if (p === '/api/journal/fee-schedules') return { fee_schedules: [] };
  if (p === '/api/journal/settings') return { settings: { display_currency: 'USD' } };
  if (p === '/api/journal/trade-suggestions') return { tickers: empty ? [] : ['BRK.B', 'ESZ6', 'BTCUSDT', 'NVDA'], exchanges: [], signals: ['Breakout'] };
  if (p === '/api/journal/fx/pending') return { pending: [] };
  if (p.startsWith('/api/journal/fx/rates/')) return { rates: [{ quote: 'EUR', rate: 0.92 }, { quote: 'CHF', rate: 0.88 }, { quote: 'USD', rate: 1 }] };
  if (p.includes('/journal/import/broker/conflicts')) return { conflicts: [] };
  if (p === '/api/journal/trades' && method === 'POST') return { trade: { id: 't-new', ...body } };
  if (/^\/api\/journal\/trades\/[^/]+$/.test(p) && method !== 'GET') return { ok: true };
  if (p === '/api/journal/trades') return { trades: empty ? [] : journal.trades };
  if (p === '/api/journal/breakdown') return { breakdown: empty ? { ...journal.breakdown, closed_count: 0, open_count: 0, equity_curve: [] } : journal.breakdown };
  if (p === '/api/journal/analytics') return { analytics: journal.analytics() };
  if (p === '/api/journal/exits/ready') return { ready: { trades: [], closed: 3 } };
  if (p === '/api/journal/market/settings') return { settings: { timeframe: '', sync_mode: 'manual', market_connectors: {}, market_symbols: {} } };
  if (p === '/api/journal/market/coverage') return { coverage: { measured: 0, measurable: 0, timeframe: '1h', timeframe_auto: true, missing_windows: 0, missing_instruments: 0, needs_symbol: 0, unsupported: 0, ready: 0, instruments: [] } };
  if (p === '/api/journal/calendar') return { days: empty ? [] : [{ date: '2026-10-08', net_pnl: 1999.08, trades: 1, wins: 1, losses: 0 }, { date: '2026-10-09', net_pnl: -12450.75, trades: 3, wins: 1, losses: 2 }] };
  if (p === '/api/journal/routines/status') return { days: [] };
  if (p === '/api/journal/import/batches') return { batches: empty ? [] : [{ id: 'b-1', source: 'csv', filename: 'interactive-brokers-activity-statement-2026-q3-with-a-long-name.csv', created_at: '2026-10-02T09:00:00Z', mapping_name: 'IBKR flex', live_trades: 214, duplicates: 3 }] };
  if (p === '/api/journal/import/mappings') return { mappings: [] };
  if (p.endsWith('/capital')) return { events: [] };
  // Portfolios
  if (p === '/api/portfolios' && method === 'POST') return { portfolio: { id: 'pf-new', ...body } };
  if (p === '/api/portfolios') return { portfolios: empty ? [] : portfolios.list };
  if (p === '/api/portfolios/blocks') return { blocks: ['book'], windows: [] };
  if (/^\/api\/portfolios\/[^/]+\/analytics$/.test(p)) return portfolios.analytics;
  if (/^\/api\/portfolios\/[^/]+\/import\/analyze$/.test(p)) return { analysis: importAnalysis };
  if (p === '/api/portfolios/search') return { results: [{ provider: 'yahoo', provider_id: 'IWDA.AS', symbol: 'IWDA.AS', name: 'iShares Core MSCI World UCITS ETF USD (Acc)', asset_class: 'etf' }] };
  if (/^\/api\/portfolios\/[^/]+\/import\/broker\/last$/.test(p)) return { last: null };
  if (/^\/api\/portfolios\/[^/]+\/import\/batches$/.test(p)) return { batches: [] };
  if (p === '/api/portfolios/import/mappings') return { mappings: [] };
  if (/^\/api\/portfolios\/[^/]+$/.test(p) && method === 'GET') {
    const d = portfolios.detail();
    if (p.endsWith('/pf-new')) return { ...d, portfolio: { ...d.portfolio, id: 'pf-new', name: 'Fixture portfolio' }, assets: [], operations: [], snapshots: [] };
    return d;
  }
  // Mportfolios
  if (p === '/api/mportfolios') return { portfolios: empty ? [] : mportfolios.list, updated_at: '2026-10-12T06:00:00Z' };
  if (p === '/api/mportfolios/snapshots' && method === 'GET') return { snapshots: empty ? [] : mportfolios.snapshots };
  if (p === '/api/mportfolios/snapshots') return { id: 'snap-new' };
  if (/^\/api\/mportfolios\/snapshots\/[^/]+$/.test(p)) return { snapshot: { ...mportfolios.snapshots[0], source_url: 'https://www.dataroma.com/' }, holdings: mportfolios.holdings };
  if (/^\/api\/mportfolios\/[^/]+$/.test(p)) return { portfolio: { ...mportfolios.list[0], source_url: 'https://www.dataroma.com/' }, holdings: mportfolios.holdings };
  // Wealth
  if (p === '/api/wealth/assets' && method === 'POST') return { id: 'w-new' };
  if (p === '/api/wealth/assets') return { assets: empty ? [] : wealth.assets };
  if (/^\/api\/wealth\/assets\/[^/]+\/revisions$/.test(p)) return { revisions: wealth.revisions };
  if (p === '/api/wealth/breakdown') return { breakdown: empty ? { net_worth: 0, assets_value: 0, liabilities_value: 0, asset_count: 0, points: [], by_category: {}, stale: [] } : wealth.breakdown };
  if (p === '/api/wealth/settings') return { settings: { display_currency: 'EUR' } };
  if (p === '/api/wealth/templates') return { templates: empty ? [] : wealth.templates };
  // Taxcalc
  if (p === '/api/taxcalc/templates') return { templates: taxcalc.templates };
  if (p === '/api/taxcalc/profiles' && method === 'POST') return { id: 'pr-new' };
  if (p === '/api/taxcalc/profiles') return { profiles: empty ? [] : taxcalc.profiles };
  if (/^\/api\/taxcalc\/profiles\/[^/]+$/.test(p)) return { profile: taxcalc.profiles[0] };
  if (/^\/api\/taxcalc\/profiles\/[^/]+\/losses/.test(p)) return empty ? { ...taxcalc.losses, carry: [], rows: [] } : taxcalc.losses;
  if (p === '/api/taxcalc/scenarios' && method === 'POST') return { id: 'sc-new' };
  if (p === '/api/taxcalc/scenarios') return { scenarios: empty ? [] : taxcalc.scenarios };
  if (/^\/api\/taxcalc\/scenarios\/[^/]+$/.test(p)) return { scenario: { ...taxcalc.scenarios[0], inputs: { start_value: 100000, end_value: 141153.9 }, result: taxcalc.result } };
  if (p === '/api/taxcalc/compute') return { result: taxcalc.result };
  return null;
}

const MODULE_PREFIXES = ['/api/journal', '/api/portfolios', '/api/mportfolios', '/api/wealth', '/api/taxcalc'];

async function newContext({ state = 'populated', viewport = VIEWPORTS[1], log = [], unknown = new Set() } = {}) {
  const context = await browser.newContext({ viewport, locale: 'en-US', timezoneId: 'UTC', reducedMotion: 'reduce' });
  await context.route('**/api/**', async (route) => {
    const req = route.request();
    const p = new URL(req.url()).pathname;
    const method = req.method();
    if (p === '/api/feeds/stream') return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
    if (shell[p]) return route.fulfill({ json: shell[p] });
    if (p.includes('/notifications')) return route.fulfill({ json: { notifications: [], unread: 0 } });
    let body = null;
    try { body = req.postDataJSON(); } catch { /* no JSON body */ }
    if (method !== 'GET') log.push({ method, path: p, body });
    const isModule = MODULE_PREFIXES.some((x) => p.startsWith(x));
    // The FX map is shared infrastructure, not the route's own data: keep it in the error state.
    if (state === 'error' && isModule && method === 'GET' && !p.startsWith('/api/journal/fx/rates/')) {
      return route.fulfill({ status: 500, json: { error: 'fixture failure' } });
    }
    const result = moduleFixture(p, method, state, body);
    if (result == null) unknown.add(`${method} ${p}`);
    return route.fulfill({ json: result ?? {} });
  });
  return context;
}

// ── Measurements ────────────────────────────────────────────────────────────────────
const measure = (page) => page.evaluate(() => {
  const vw = document.documentElement.clientWidth;
  const overflowers = [...document.querySelectorAll('body *')].filter((el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    if (r.width === 0 || s.visibility === 'hidden' || s.position === 'fixed') return false;
    // Inside a local scroller the overflow is contained; report only what escapes the page.
    for (let a = el.parentElement; a && a !== document.body; a = a.parentElement) {
      const as = getComputedStyle(a);
      if (/(auto|scroll|hidden|clip)/.test(as.overflowX) && a.getBoundingClientRect().right <= vw + 1) return false;
    }
    return r.right > vw + 1;
  }).slice(0, 6).map((el) => `${el.tagName}.${typeof el.className === 'string' ? el.className.split(' ').slice(0, 3).join('.') : ''}`);
  // Content cut by an overflow-hidden box (an ellipsis is a deliberate cut, not a loss).
  const clipped = [...document.querySelectorAll('body *')].filter((el) => {
    const s = getComputedStyle(el);
    if (!/(hidden|clip)/.test(s.overflowX) || s.textOverflow === 'ellipsis') return false;
    if (el.getBoundingClientRect().width <= 1 || el.closest('[aria-hidden="true"], .sr-only')) return false;
    return el.scrollWidth > el.clientWidth + 2;
  }).slice(0, 6).map((el) => `${el.tagName}.${typeof el.className === 'string' ? el.className.split(' ').slice(0, 3).join('.') : ''}`);
  const scrollers = [...document.querySelectorAll('body *')].filter((el) => {
    const s = getComputedStyle(el);
    return /(auto|scroll)/.test(s.overflowX) && el.scrollWidth > el.clientWidth + 2 && el.getBoundingClientRect().width > 0;
  }).slice(0, 6).map((el) => `${el.tagName}.${typeof el.className === 'string' ? el.className.split(' ').slice(0, 3).join('.') : ''}`);
  // The app scrolls inside its route pane, so a pane that scrolls sideways is a page-level
  // overflow the document width cannot see. Name what sticks out of it.
  const panes = [...document.querySelectorAll('main.content, .journal-module, .page, .wealth')];
  const paneOverflow = panes.filter((el) => el.scrollWidth > el.clientWidth + 1).map((pane) => {
    const right = pane.getBoundingClientRect().left + pane.clientWidth;
    const out = [...pane.querySelectorAll('*')].filter((el) => {
      const r = el.getBoundingClientRect();
      if (r.width === 0 || r.right <= right + 1) return false;
      for (let a = el.parentElement; a && a !== pane; a = a.parentElement) {
        if (/(auto|scroll|hidden|clip)/.test(getComputedStyle(a).overflowX)) return false;
      }
      return true;
    }).slice(-4).map((el) => `${el.tagName}.${typeof el.className === 'string' ? el.className.split(' ').slice(0, 2).join('.') : ''}`);
    return `${pane.className.split(' ')[0]}(${pane.scrollWidth}/${pane.clientWidth}): ${out.join(', ')}`;
  });
  return { vw, scroll: document.documentElement.scrollWidth, overflowers, clipped: clipped.filter((c) => !c.includes('aw-fab')), scrollers, paneOverflow };
});

/** Scroll the element into view, then require its centre to be on screen and hit-testable. */
async function reachable(locator, label) {
  await locator.scrollIntoViewIfNeeded();
  const ok = await locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return `centre off screen (${Math.round(x)},${Math.round(y)})`;
    const hit = document.elementFromPoint(x, y);
    if (!hit || !(hit === el || el.contains(hit))) return `covered by ${hit?.tagName}.${hit?.className}`;
    return true;
  });
  assert.equal(ok, true, `${label} must be reachable: ${ok}`);
}

const failures = [];
function check(cond, message) {
  if (cond) return;
  if (explore) failures.push(message);
  else assert.fail(message);
}

async function sweep(page, name, { states = ['populated'] } = {}) {
  for (const vp of VIEWPORTS) {
    await page.setViewportSize(vp);
    await page.waitForTimeout(80);
    const m = await measure(page);
    check(m.scroll <= m.vw + 1, `${name} ${vp.width}x${vp.height} overflows (${m.scroll}/${m.vw}): ${m.overflowers.join(', ')}`);
    check(m.paneOverflow.length === 0, `${name} ${vp.width}x${vp.height} route pane scrolls sideways: ${m.paneOverflow.join(' | ')}`);
    check(m.clipped.length === 0, `${name} ${vp.width}x${vp.height} clips content: ${m.clipped.join(', ')}`);
    if (explore && m.scrollers.length) console.log(`  ${name} ${vp.width}: local scrollers ${m.scrollers.join(', ')}`);
    await page.screenshot({ path: `${artifacts}/${name}-${vp.width}x${vp.height}.png`, fullPage: true });
  }
  // The app scrolls inside its own panes, so a full-page capture stops at the viewport. A
  // tall window shows the whole pane in one image for review.
  for (const width of [320, 390]) {
    await page.setViewportSize({ width, height: 2600 });
    await page.waitForTimeout(80);
    await page.screenshot({ path: `${artifacts}/${name}-${width}-tall.png` });
  }
  await page.setViewportSize(VIEWPORTS[1]);
}

async function open(page, path, ready, root = base) {
  for (let attempt = 0; attempt < 2; attempt++) {
    await page.goto(`${root}${path}`);
    try {
      await page.locator(ready).first().waitFor({ timeout: 8000 });
      return;
    } catch (e) {
      if (attempt) throw e; // the first visit after a cold dependency optimisation may reload
    }
  }
}

// ── Routes ──────────────────────────────────────────────────────────────────────────
const errors = [];
// In the API-error state some views leave the rejected request unhandled (existing
// behaviour, reported, not changed here): that one message is expected there and only there.
const watch = (page, state) => page.on('pageerror', (e) => {
  if (state === 'error' && e.message === 'fixture failure') return;
  errors.push(`${page.url()}: ${e.message}`);
});
const unknown = new Set();
const summary = [];

async function withPage(opts, fn) {
  const log = [];
  opts = { state: 'populated', ...opts };
  const context = await newContext({ ...opts, log, unknown });
  const page = await context.newPage();
  await page.clock.setFixedTime(FIXED_NOW);
  page.setDefaultTimeout(8000);
  watch(page, opts.state);
  try {
    await fn(page, log);
  } finally {
    await context.close();
  }
}

if (want('journal') && !flags['desktop-only']) {
  for (const view of ['breakdown', 'trades', 'analytics', 'calendar', 'strategies', 'tags', 'templates', 'fees', 'import', 'pending']) {
    await withPage({}, async (page) => {
      await open(page, `/journal?view=${view}`, '.journal-module .content-head h1');
      await page.waitForTimeout(300);
      await sweep(page, `journal-${view}`);
    });
  }
  for (const state of ['empty', 'error']) {
    await withPage({ state }, async (page) => {
      await open(page, '/journal?view=trades', '.journal-module .content-head h1');
      await page.waitForTimeout(300);
      await sweep(page, `journal-trades-${state}`);
    });
  }
  summary.push('journal views');
}

if (want('portfolios') && !flags['desktop-only']) {
  for (const state of ['populated', 'empty', 'error']) {
    await withPage({ state }, async (page) => {
      await open(page, '/portfolios', '.page-head, .page h1');
      await page.waitForTimeout(300);
      await sweep(page, `portfolios-${state}`);
    });
  }
  await withPage({}, async (page) => {
    await open(page, '/portfolios', '.card');
    await page.locator('.card').first().click();
    await page.locator('.detail .totals').waitFor();
    await page.waitForTimeout(300);
    await sweep(page, 'portfolios-detail-assets');
    await page.locator('.tabs > button').nth(1).click();
    await sweep(page, 'portfolios-detail-operations');
    await page.locator('.tabs > button').nth(2).click();
    await page.locator('.analytics').waitFor();
    await page.waitForTimeout(300);
    await sweep(page, 'portfolios-detail-analysis');
  });
  summary.push('portfolios');
}

if (want('mportfolios') && !flags['desktop-only']) {
  for (const state of ['populated', 'empty', 'error']) {
    await withPage({ state }, async (page) => {
      await open(page, '/mportfolios', '.page h1');
      await page.waitForTimeout(400);
      await sweep(page, `mportfolios-${state}`);
    });
  }
  await withPage({}, async (page) => {
    await open(page, '/mportfolios', 'tbody tr td.name');
    await page.locator('tbody tr td.name').first().click();
    await page.locator('[role="dialog"] .tbl').waitFor();
    await sweep(page, 'mportfolios-detail');
  });
  summary.push('mportfolios');
}

if (want('wealth') && !flags['desktop-only']) {
  for (const state of ['populated', 'empty', 'error']) {
    await withPage({ state }, async (page) => {
      await open(page, '/wealth', '.wealth h1');
      await page.waitForTimeout(400);
      await sweep(page, `wealth-${state}`);
    });
  }
  summary.push('wealth');
}

if (want('taxcalc') && !flags['desktop-only']) {
  for (const state of ['populated', 'empty', 'error']) {
    await withPage({ state }, async (page) => {
      await open(page, '/taxcalc', '.page h1');
      await page.waitForTimeout(400);
      if (state === 'populated') {
        await page.locator('.actions-row .primary').click();
        await page.locator('.breakdown').waitFor();
      }
      await sweep(page, `taxcalc-${state}`);
    });
  }
  summary.push('taxcalc');
}


// ── Tap flows: the same steps at 390 px on the working tree and at 1440 px on the
// unmodified baseline. The captured request bodies must be identical. ──────────────
const writes = (log) => log.filter((l) => MODULE_PREFIXES.some((x) => l.path.startsWith(x))).map(({ method, path, body }) => ({ method, path, body }));
const dialog = (page) => page.locator('[role="dialog"]').last();

/** Fit check for an open dialog: inside the viewport, nothing sideways, action reachable. */
async function dialogFits(page, label, action) {
  const vp = page.viewportSize();
  const box = await dialog(page).boundingBox();
  check(box.x >= -1 && box.x + box.width <= vp.width + 1, `${label} dialog wider than ${vp.width}px`);
  const m = await measure(page);
  check(m.scroll <= m.vw + 1 && m.clipped.length === 0, `${label} dialog overflows: ${JSON.stringify(m)}`);
  const inner = await dialog(page).evaluate((d) => [...d.querySelectorAll('*')].filter((el) => {
    const r = el.getBoundingClientRect();
    if (r.width === 0) return false;
    for (let a = el.parentElement; a && a !== d; a = a.parentElement) {
      if (/(auto|scroll|hidden|clip)/.test(getComputedStyle(a).overflowX)) return false;
    }
    return r.right > d.getBoundingClientRect().right + 1;
  }).slice(0, 4).map((el) => `${el.tagName}.${el.className}`));
  check(inner.length === 0, `${label} dialog content runs past its edge: ${inner.join(', ')}`);
  if (action) await reachable(action, `${label} action`);
}

async function closeDialog(page, mobile) {
  const n = await page.locator('[role="dialog"]').count();
  const x = dialog(page).getByRole('button', { name: 'Close', exact: true }).first();
  if (mobile) await reachable(x, 'dialog close');
  await x.click();
  await page.waitForFunction((count) => document.querySelectorAll('[role="dialog"]').length < count, n);
}

async function tapOrClick(locator, mobile, label) {
  if (mobile) await reachable(locator, label);
  await locator.click();
}

async function journalFlow(root, viewport, mobile) {
  let out = [];
  await withPage({ viewport }, async (page, log) => {
    await open(page, '/journal?view=trades', '.journal-module .content-head h1', root);
    await page.locator('.trades .table-wrap tbody tr').first().waitFor();
    if (mobile) {
      // Category reorder by tap, from the sections rail.
      await page.locator('.mobile-selector').click();
      const down = page.getByRole('button', { name: `Move down: ${LONG}` });
      await tapOrClick(down, mobile, 'category move down');
      await page.waitForTimeout(200);
      const moves = log.filter((l) => l.path.startsWith('/api/journal/categories/')).sort((a, b) => a.body.position - b.body.position);
      assert.deepEqual(moves.map((l) => [l.path, l.body]), [
        ['/api/journal/categories/cat-2', { position: 0 }],
        ['/api/journal/categories/cat-1', { position: 1 }],
        ['/api/journal/categories/cat-3', { position: 2 }]
      ], 'Tap reorder persists through the drop handler');
      log.length = 0;
      await reachable(page.locator('.cat-list li').nth(1).getByTitle('Rename'), 'category rename');
      await page.screenshot({ path: `${artifacts}/flow-journal-categories-390.png` });
      await page.locator('.mobile-selector').click();
      await page.screenshot({ path: `${artifacts}/flow-journal-cards-390.png` });
    }
    // Add a trade.
    await tapOrClick(page.getByRole('button', { name: 'Quick (all fields)' }).first(), mobile, 'quick trade');
    await dialog(page).locator('.trade-form').waitFor();
    if (mobile) await dialogFits(page, 'trade form', dialog(page).getByRole('button', { name: 'Log trade' }));
    const form = dialog(page);
    await form.locator('input[list="sg-tickers"]').fill('MSFT');
    await form.locator('input[type="date"][aria-label="Entry time"]').fill('2026-10-05');
    await form.locator('label.field', { hasText: 'Entry price' }).locator('input').fill('412.5');
    await form.locator('label.field', { hasText: 'Exit price' }).locator('input').fill('398.25');
    await form.locator('label.field', { hasText: 'Quantity' }).locator('input').fill('15');
    await tapOrClick(form.getByRole('button', { name: 'Log trade' }), mobile, 'log trade');
    await page.locator('.trade-form').waitFor({ state: 'detached' });
    // Edit the first trade.
    const row = page.locator('.trades .table-wrap tbody tr', { hasText: 'BRK.B' });
    await row.waitFor();
    await tapOrClick(row.getByTitle('Edit'), mobile, 'edit trade');
    await dialog(page).locator('.trade-form').waitFor();
    if (mobile) await dialogFits(page, 'edit trade form', dialog(page).getByRole('button', { name: 'Save trade' }));
    await dialog(page).locator('label.field', { hasText: 'Exit price' }).locator('input').fill('433.1');
    await tapOrClick(dialog(page).getByRole('button', { name: 'Save trade' }), mobile, 'save trade');
    await page.locator('.trade-form').waitFor({ state: 'detached' });
    // Delete a trade.
    const loss = page.locator('.trades .table-wrap tbody tr', { hasText: 'ESZ6' });
    await tapOrClick(loss.getByTitle('Delete'), mobile, 'delete trade');
    await tapOrClick(dialog(page).getByRole('button', { name: 'Delete', exact: true }), mobile, 'confirm delete');
    await page.waitForTimeout(300);
    out = writes(log);
  });
  return out;
}

async function portfoliosFlow(root, viewport, mobile) {
  let out = [];
  await withPage({ viewport }, async (page, log) => {
    await open(page, '/portfolios', '.card', root);
    await tapOrClick(page.getByRole('button', { name: 'New portfolio' }).first(), mobile, 'new portfolio');
    await page.getByPlaceholder('Portfolio name').fill('Fixture portfolio');
    await page.getByPlaceholder('Description (optional)').fill('Created by tap');
    await tapOrClick(page.getByRole('button', { name: 'Create', exact: true }), mobile, 'create portfolio');
    await page.locator('.detail .head h1').waitFor();
    await page.locator('.detail .back').click();
    // Book an operation on an existing holding.
    await page.locator('.card').first().click();
    await page.locator('.detail .totals').waitFor();
    const holding = page.locator('.table-wrap tbody tr', { hasText: 'VWCE' });
    await tapOrClick(holding.getByRole('button', { name: 'New operation' }), mobile, 'new operation');
    await dialog(page).getByRole('button', { name: 'Add operation' }).waitFor();
    if (mobile) await dialogFits(page, 'operation form', dialog(page).getByRole('button', { name: 'Add operation' }));
    await dialog(page).locator('label', { hasText: 'Quantity' }).locator('input').fill('10');
    await dialog(page).locator('label', { hasText: 'Price (' }).locator('input').fill('121.06');
    await tapOrClick(dialog(page).getByRole('button', { name: 'Add operation' }), mobile, 'add operation');
    await page.waitForTimeout(300);
    out = writes(log);
    if (!mobile) return;
    // Every header action and ledger action is reachable, and every dialog fits.
    for (const name of ['Refresh prices', 'Import', 'From a broker']) await reachable(page.getByRole('button', { name }).first(), name);
    await reachable(holding.getByRole('button', { name: 'Delete' }), 'delete holding');
    for (const [name, ready] of [['Import', '[role="dialog"]'], ['From a broker', '[role="dialog"]'], ['+ Add asset', '[role="dialog"]']]) {
      await page.getByRole('button', { name, exact: true }).last().click();
      await page.locator(ready).last().waitFor();
      await page.waitForTimeout(250);
      await dialogFits(page, name);
      await page.screenshot({ path: `${artifacts}/flow-portfolios-${name.replace(/\W+/g, '-')}-390.png` });
      await closeDialog(page, true);
    }
    // Import mapping step with a file: columns, symbols and preview inside the dialog.
    await page.getByRole('button', { name: 'Import', exact: true }).click();
    await dialog(page).locator('input[type="file"]').setInputFiles({ name: 'broker-ledger-2026.csv', mimeType: 'text/csv', buffer: Buffer.from('Date,Ticker,Side,Quantity,Price,Fee\n2026-03-14,VWCE,BUY,12.5,118.42,1.50\n') });
    await dialog(page).locator('table.pv').waitFor();
    await page.waitForTimeout(500);
    if (!(await dialog(page).locator('.picker').count())) await dialog(page).locator('.list li', { hasText: 'IWDA' }).locator('.act button').click();
    await dialog(page).locator('.picker input[type="search"]').fill('IWDA');
    await dialog(page).locator('.picker .hit').first().waitFor();
    await dialogFits(page, 'import mapping');
    for (const width of [320, 390]) {
      await page.setViewportSize({ width, height: 2000 });
      await page.waitForTimeout(150);
      await dialogFits(page, `import mapping ${width}`);
      await page.screenshot({ path: `${artifacts}/flow-portfolios-import-map-${width}.png` });
      // The footer row belongs to the shared Modal (not this module): report, do not fail.
      const footer = await dialog(page).evaluate((d) => {
        const box = d.getBoundingClientRect();
        return [...d.querySelectorAll('.modal-foot button')].filter((b) => {
          const r = b.getBoundingClientRect();
          return r.left < box.left - 1 || r.right > box.right + 1;
        }).map((b) => b.textContent.trim());
      });
      if (footer.length) console.log(`  NOTE ${width}px: import footer buttons cut by the shared Modal footer (no wrap): ${footer.join(' | ')}`);
    }
    await page.setViewportSize(VIEWPORTS[1]);
    await closeDialog(page, true);
    await page.locator('.tabs > button').nth(1).click();
    await reachable(page.getByRole('button', { name: '+ Cash & income' }), 'cash and income');
    await reachable(page.locator('.table-wrap tbody tr').first().getByRole('button', { name: 'Delete' }), 'delete operation');
    await page.locator('.tabs > button').nth(2).click();
    await page.locator('.analytics .stats').waitFor();
    await reachable(page.locator('.analytics .gear'), 'analytics settings');
  });
  return out;
}

async function taxcalcFlow(root, viewport, mobile) {
  let out = [];
  await withPage({ viewport }, async (page, log) => {
    await open(page, '/taxcalc', '.page h1', root);
    await page.locator('.calc .dd').first().filter({ hasText: 'France individual' }).waitFor();
    await page.locator('label', { hasText: 'Invested at start of year' }).locator('input').fill('100000');
    await page.locator('label', { hasText: 'Stack now (end value)' }).locator('input').fill('141153.9');
    await tapOrClick(page.getByRole('button', { name: 'Calculate tax' }), mobile, 'calculate');
    await page.locator('.breakdown').waitFor();
    await tapOrClick(page.getByRole('button', { name: /Save to history/ }), mobile, 'save to history');
    await page.waitForTimeout(300);
    out = writes(log);
    if (!mobile) return;
    // The long result table scrolls inside its labelled frame and reaches its last column.
    const lines = page.getByRole('region', { name: 'Estimated tax' });
    const scroll = await lines.evaluate((el) => { el.scrollLeft = el.scrollWidth; return { left: el.scrollLeft, max: el.scrollWidth - el.clientWidth }; });
    check(scroll.max > 0 && scroll.left >= scroll.max - 1, 'result table scrolls to its last column');
    const lastTax = lines.locator('tfoot td.tax');
    const visible = await lastTax.evaluate((el) => { const r = el.getBoundingClientRect(); const f = el.closest('.lines').getBoundingClientRect(); return r.right <= f.right + 1 && r.left >= f.left - 1; });
    check(visible, 'total tax cell visible after scrolling the result table');
    await page.screenshot({ path: `${artifacts}/flow-taxcalc-result-390.png` });
    // Profiles: fold-out list with edit and delete.
    const toggle = page.locator('.mobile-profiles-toggle');
    await tapOrClick(toggle, mobile, 'profiles toggle');
    assert.equal(await toggle.getAttribute('aria-expanded'), 'true');
    await page.screenshot({ path: `${artifacts}/flow-taxcalc-profiles-390.png` });
    await reachable(page.locator('.profiles .profile').first().getByRole('button', { name: 'edit' }), 'edit profile');
    await reachable(page.locator('.profiles .profile').first().getByRole('button', { name: 'Delete' }), 'delete profile');
    await page.locator('.profiles .profile').nth(1).locator('.pick').click();
    await page.locator('.calc .dd').first().filter({ hasText: 'Custom 20%' }).waitFor();
    await page.locator('.profiles .profile').first().getByRole('button', { name: 'edit' }).click();
    await dialog(page).waitFor();
    await page.waitForTimeout(250);
    await dialogFits(page, 'profile editor');
    await closeDialog(page, true);
    await reachable(page.locator('.hist-open').first(), 'history entry');
    await reachable(page.locator('.hist-row').first().getByRole('button', { name: 'Delete' }), 'delete history entry');
  });
  return out;
}

async function mportfoliosFlow(root, viewport, mobile) {
  let out = [];
  await withPage({ viewport }, async (page, log) => {
    await open(page, '/mportfolios', 'tbody tr td.name', root);
    await tapOrClick(page.locator('tbody tr', { hasText: 'Pabrai' }).getByTitle('Save a snapshot'), mobile, 'snapshot');
    await page.waitForTimeout(300);
    await tapOrClick(page.locator('tbody tr td.name').first(), mobile, 'open manager');
    await dialog(page).locator('.tbl').waitFor();
    if (mobile) {
      await dialogFits(page, 'holdings');
      const sticky = await dialog(page).locator('.table-wrap').evaluate((el) => {
        el.scrollLeft = el.scrollWidth;
        const tk = el.querySelector('tbody td').getBoundingClientRect();
        return { scrolled: el.scrollLeft > 0, pinned: tk.left >= el.getBoundingClientRect().left - 1 };
      });
      check(sticky.scrolled && sticky.pinned, `holdings scroll with the ticker pinned: ${JSON.stringify(sticky)}`);
      await page.screenshot({ path: `${artifacts}/flow-mportfolios-holdings-390.png` });
    }
    await tapOrClick(dialog(page).getByRole('button', { name: /Snapshot/ }), mobile, 'snapshot from detail');
    await page.waitForTimeout(300);
    out = writes(log);
    if (!mobile) return;
    await closeDialog(page, true);
    await page.getByRole('button', { name: 'Snapshots', exact: true }).click();
    await page.locator('.group-toggle').first().click();
    await reachable(page.locator('.snap-list .details').first(), 'snapshot details');
    await reachable(page.locator('.snap-list .del').first(), 'snapshot delete');
    await reachable(page.locator('.group-head .del').first(), 'delete snapshot group');
  });
  return out;
}

async function wealthFlow(root, viewport, mobile) {
  let out = [];
  await withPage({ viewport }, async (page, log) => {
    await open(page, '/wealth', '.wealth tbody tr', root);
    await tapOrClick(page.getByRole('button', { name: 'Collapse all' }), mobile, 'collapse all');
    const row = page.locator('tbody tr', { hasText: 'Checking account' });
    await tapOrClick(page.locator('tr.group-row', { hasText: 'Cash' }), mobile, 'expand cash');
    await tapOrClick(row.getByRole('button', { name: 'Update' }), mobile, 'update value');
    await dialog(page).waitFor();
    await page.waitForTimeout(250);
    if (mobile) await dialogFits(page, 'update form', dialog(page).getByRole('button', { name: 'Record update' }));
    await dialog(page).locator('label.field', { hasText: 'Price (USD)' }).locator('input').fill('19000.25');
    await dialog(page).locator('label.field.wide').locator('input').fill('Statement');
    await tapOrClick(dialog(page).getByRole('button', { name: 'Record update' }), mobile, 'record update');
    await page.waitForTimeout(300);
    await closeDialog(page, mobile);
    await tapOrClick(row.getByRole('button', { name: /^Edit/ }), mobile, 'edit asset');
    await dialog(page).waitFor();
    await page.waitForTimeout(250);
    if (mobile) await dialogFits(page, 'asset form');
    await closeDialog(page, mobile);
    await tapOrClick(page.getByRole('button', { name: 'Add asset' }).first(), mobile, 'add asset');
    await dialog(page).waitFor();
    await page.waitForTimeout(250);
    if (mobile) await dialogFits(page, 'new asset form');
    await closeDialog(page, mobile);
    if (mobile) {
      await reachable(row.getByRole('button', { name: /^Delete/ }), 'delete asset');
      await page.getByRole('button', { name: 'Templates', exact: true }).click();
      await reachable(page.locator('.tcard .link').first(), 'edit template');
    }
    out = writes(log);
  });
  return out;
}

const flows = flags['desktop-only'] ? {} : { journal: journalFlow, portfolios: portfoliosFlow, taxcalc: taxcalcFlow, mportfolios: mportfoliosFlow, wealth: wealthFlow };
for (const [name, flow] of Object.entries(flows)) {
  if (!want(name)) continue;
  const phone = await flow(base, VIEWPORTS[1], true);
  check(phone.length > 0, `${name}: the tap flow sent no request`);
  if (flags['skip-desktop']) continue;
  const desktop = await flow(baseline, DESKTOP[0], false);
  // Category reorder exists only as a tap path; everything else must match the desktop.
  assert.deepEqual(phone, desktop, `${name}: phone payloads differ from the desktop baseline`);
  console.log(`${name} flow: ${phone.length} write(s) identical at 390 px and on the 1440 px baseline`);
}

// ── Desktop no-regression: baseline and working tree, same fixtures, same clock. ──
async function desktopShot(root, path, ready, viewport, state, prepare) {
  let buf;
  await withPage({ viewport, state }, async (page) => {
    await open(page, path, ready, root);
    if (prepare) await prepare(page);
    await page.waitForTimeout(1000);
    buf = await page.screenshot({ animations: 'disabled', caret: 'hide' });
  });
  return buf;
}
function pixelDiff(x, y) {
  const a = PNG.sync.read(x);
  const b = PNG.sync.read(y);
  if (a.width !== b.width || a.height !== b.height) return 'size differs';
  let n = 0, x0 = Infinity, y0 = Infinity, x1 = -1, y1 = -1;
  for (let i = 0; i < a.data.length; i += 4) {
    if (a.data[i] === b.data[i] && a.data[i + 1] === b.data[i + 1] && a.data[i + 2] === b.data[i + 2]) continue;
    const px = (i / 4) % a.width, py = Math.floor(i / 4 / a.width);
    n++; x0 = Math.min(x0, px); y0 = Math.min(y0, py); x1 = Math.max(x1, px); y1 = Math.max(y1, py);
  }
  return n ? `${n} px in ${x0},${y0}..${x1},${y1}` : '0 px';
}
const DESKTOP_CASES = [
  ...['breakdown', 'analytics', 'calendar', 'trades', 'strategies', 'tags', 'templates', 'fees', 'import', 'pending'].map((v) => ({ module: 'journal', name: `journal-${v}`, path: `/journal?view=${v}`, ready: '.journal-module .content-head h1' })),
  { module: 'journal', name: 'journal-trades-empty', path: '/journal?view=trades', ready: '.journal-module .content-head h1', state: 'empty' },
  { module: 'portfolios', name: 'portfolios', path: '/portfolios', ready: '.card' },
  { module: 'portfolios', name: 'portfolios-empty', path: '/portfolios', ready: '.page', state: 'empty' },
  ...[0, 1, 2].map((tab) => ({ module: 'portfolios', name: `portfolios-detail-tab${tab}`, path: '/portfolios', ready: '.card', prepare: async (page) => {
    await page.locator('.card').first().click();
    await page.locator('.detail .totals').waitFor();
    await page.locator('.tabs > button').nth(tab).click();
    if (tab === 2) await page.locator('.analytics .stats').waitFor();
  } })),
  { module: 'mportfolios', name: 'mportfolios', path: '/mportfolios', ready: 'tbody td.name' },
  { module: 'mportfolios', name: 'mportfolios-detail', path: '/mportfolios', ready: 'tbody td.name', prepare: async (page) => {
    await page.locator('tbody td.name').first().click();
    await page.locator('[role="dialog"] .tbl').waitFor();
  } },
  { module: 'wealth', name: 'wealth', path: '/wealth', ready: '.wealth tbody tr' },
  { module: 'wealth', name: 'wealth-empty', path: '/wealth', ready: '.wealth h1', state: 'empty' },
  { module: 'taxcalc', name: 'taxcalc', path: '/taxcalc', ready: '.page h1', prepare: async (page) => {
    await page.locator('.calc .dd').first().filter({ hasText: 'France individual' }).waitFor();
    await page.locator('.actions-row .primary').click();
    await page.locator('.breakdown').waitFor();
  } },
  { module: 'taxcalc', name: 'taxcalc-empty', path: '/taxcalc', ready: '.page h1', state: 'empty' }
];
if (!flags['skip-desktop']) {
  const diffs = [];
  for (const c of DESKTOP_CASES) {
    if (!want(c.module)) continue;
    for (const vp of DESKTOP) {
      const before = await desktopShot(baseline, c.path, c.ready, vp, c.state, c.prepare);
      const after = await desktopShot(base, c.path, c.ready, vp, c.state, c.prepare);
      const tag = `${c.name}-${vp.width}`;
      if (Buffer.compare(before, after) !== 0) {
        // Retake up to five pairs: a rendering race shows up as the baseline differing from
        // itself; a real change differs on every pair.
        let matched = 0;
        const noise = [];
        for (let k = 0; k < 5 && !matched; k++) {
          const before2 = await desktopShot(baseline, c.path, c.ready, vp, c.state, c.prepare);
          const after2 = await desktopShot(base, c.path, c.ready, vp, c.state, c.prepare);
          noise.push(pixelDiff(before, before2));
          if (Buffer.compare(before2, after2) === 0) matched = k + 2;
        }
        if (matched) {
          console.log(`  ${tag}: pair 1 differed (${pixelDiff(before, after)}); baseline vs itself ${noise.join(' / ')}; pair ${matched} identical`);
          continue;
        }
        await writeFile(`${artifacts}/desktop-${tag}-baseline.png`, before);
        await writeFile(`${artifacts}/desktop-${tag}-tree.png`, after);
        diffs.push(`${tag} [${pixelDiff(before, after)}; baseline vs itself: ${noise.join(' / ')}]`);
      }
    }
  }
  console.log(diffs.length ? `Desktop differences: ${diffs.join(', ')}` : 'Desktop 1440x900 and 1100x800: identical to the baseline for every case');
  check(diffs.length === 0, `desktop screenshots differ from the baseline: ${diffs.join(', ')}`);
}

await writeFile(`${artifacts}/unknown-api.txt`, [...unknown].sort().join('\n'));
await browser.close();
if (explore) {
  console.log(failures.join('\n'));
  console.log('Unknown API calls:', [...unknown].sort().join(', '));
}
assert.deepEqual(errors, [], 'Runtime errors');
console.log(`PASS: ${summary.join(', ')}. Screenshots: ${artifacts}`);
