// Mobile checks for the data routes: Fundamentals (macro, alternative data, events,
// company, documents, ETF, library), FinanceDatabase, Historical data, Goals and Goals
// statistics. Fixtures only (context.route), never a backend or a provider.
// Run with both dev servers up: node tests/mobile-data-browser.mjs [base URL] [baseline URL]
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.argv[2] ?? 'http://127.0.0.1:4173';
const baseline = process.argv[3] ?? 'http://127.0.0.1:4190';
const only = process.env.ONLY ? process.env.ONLY.split(',') : null;
const skipDesktop = process.env.SKIP_DESKTOP === '1';
const artifacts = fileURLToPath(new URL('../node_modules/.cache/mobile-data/', import.meta.url));
await mkdir(artifacts, { recursive: true });
const executablePath = process.env.WIDGET_BROWSER_EXECUTABLE ?? fileURLToPath(new URL('../node_modules/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell', import.meta.url));
const browser = await chromium.launch({ executablePath, headless: true, env: { ...process.env, TMPDIR: artifacts } });

const NOW = new Date('2026-10-20T12:00:00Z');
const day = (offset) => new Date(+NOW + offset * 86400000).toISOString().slice(0, 10);
const ms = (offset) => +NOW + offset * 86400000;
const PHONES = [
  { width: 320, height: 568 },
  { width: 390, height: 844 },
  { width: 600, height: 800 },
  { width: 667, height: 375 },
  { width: 768, height: 1024 }
];
const DESKTOPS = [
  { width: 1440, height: 900 },
  { width: 1100, height: 800 }
];

// ── Fixture data ────────────────────────────────────────────────────────────

const LONG = 'Consolidated Statement of Long-Term Strategic Infrastructure Investment Obligations';
const monthly = (n, f) => Array.from({ length: n }, (_, i) => [Date.UTC(2016, 9 + i, 1), f(i)]);
const seriesRows = [
  { id: 'u-cpi', provider_id: 'fred', code: 'CPIAUCSL', title: 'Consumer Price Index for All Urban Consumers: All Items in U.S. City Average', category: 'inflation', status: 'ok', last: 315.605, prev: 314.796, unit: 'Index 1982-1984=100', provider: 'FRED', country: 'US', freq: 'M', updated: ms(-2) },
  { id: 'u-ff', provider_id: 'fred', code: 'FEDFUNDS', title: 'Federal Funds Effective Rate', category: 'rates', status: 'ok', last: 4.33, prev: 4.58, unit: '%', provider: 'FRED', country: 'US', freq: 'M', updated: ms(-2) },
  { id: 'u-ez', provider_id: 'eurostat', code: 'une_rt_m/M.SA.TOTAL.PC_ACT.T.EA21', title: 'Euro area unemployment rate, seasonally adjusted, percentage of active population', category: 'labour', status: 'error', error: 'Eurostat answered 503 Service Unavailable for une_rt_m', last: null, prev: null, unit: '% of active population', provider: 'Eurostat', country: 'EA', freq: 'M', updated: null },
  { id: 'u-tb', provider_id: 'treasury', code: 'debt_to_penny', title: 'Total public debt outstanding', category: 'fiscal', status: 'ok', last: 37845123456789.12, prev: 37790000000000, unit: 'USD', provider: 'US Treasury', country: 'US', freq: 'D', updated: ms(-1) },
  { id: 'u-cot', provider_id: 'cftc', code: '13874A', title: 'E-mini S&P 500, non-commercial net positioning', category: 'positioning', status: 'ok', last: -184532, prev: -120044, unit: 'contracts', provider: 'CFTC', country: 'US', freq: 'W', updated: ms(-4) }
];
const observations = {
  'u-cpi': monthly(120, (i) => 240 + i * 0.63),
  'u-ff': monthly(120, (i) => Math.max(0.05, 2 + Math.sin(i / 14) * 2.6))
};
const curve = { as_of: day(-1), tenors: ['1M', '3M', '6M', '1Y', '2Y', '5Y', '10Y', '30Y'], curves: [
  { label: 'today', values: [4.31, 4.28, 4.18, 3.98, 3.71, 3.79, 4.12, 4.66] },
  { label: '1m', values: [4.35, 4.33, 4.25, 4.05, 3.82, 3.88, 4.2, 4.71] },
  { label: '1y', values: [5.4, 5.45, 5.3, 4.9, 4.3, 4.0, 4.1, 4.3] }
] };
const banks = [
  { bank: 'Federal Reserve System of the United States', rate: 4.375, last_change: -0.25, last_change_date: day(-30), as_of: day(-1) },
  { bank: 'European Central Bank', rate: 2.15, last_change: -0.25, last_change_date: day(-120), as_of: day(-1) },
  { bank: 'Bank of Japan', rate: 0.5, last_change: 0.25, last_change_date: day(-200), as_of: day(-1) }
];
const snap = (data, provider = 'Fixture provider') => ({ snapshot: { stale: false, provider_label: provider, fetched_at: ms(-1), data } });
const company = {
  ticker: 'AAPL', name: 'Apple Inc. (Consolidated Holdings and Subsidiaries)', status: 'ok', cik: '0000320193', sic: '3571',
  exchange: 'NASDAQ Global Select Market', sector: 'Information Technology', industry: 'Technology Hardware, Storage & Peripherals', country: 'United States',
  currency: 'USD', description: 'Designs, manufactures and markets smartphones, personal computers, tablets, wearables and accessories, and sells a variety of related services.',
  fiscal_year_end: '09-27', website: 'https://www.apple.com/investor-relations-and-shareholder-information', followed: false, price: 227.48, change_pct: -1.37, updated: ms(-1),
  metrics: { basis: 'TTM', market_cap: 3412345678901, enterprise_value: 3398765432109, pe: 34.6, forward_pe: 30.1, ev_ebitda: 25.4, ps: 8.9, pb: 51.2, fcf_yield: 3.1, gross_margin: 46.2, operating_margin: 31.5, net_margin: 24.3, roe: 154.9, roic: 58.3, debt_equity: 1.87, current_ratio: 0.87, beta: 1.24, dividend_yield: 0.44, shares_out: 14935826000 }
};
const companies = [
  { ticker: 'AAPL', name: company.name, followed: true, opened: ms(-1), status: 'ok', updated: ms(-1) },
  { ticker: 'BRK.B', name: 'Berkshire Hathaway Inc. Class B Common Stock', followed: false, opened: ms(-3), status: 'ok', updated: ms(-3) },
  { ticker: 'TSM', name: 'Taiwan Semiconductor Manufacturing Company Limited (American Depositary Shares)', followed: false, opened: ms(-5), status: 'error', error: 'EDGAR did not answer', updated: null }
];
const periods = Array.from({ length: 10 }, (_, i) => `FY${2016 + i}`);
const statements = (kind) => periods.map((period, i) => ({
  period,
  tags: {},
  lines: kind === 'income'
    ? { revenue: 215e9 + i * 18.4e9, cost_of_revenue: 131e9 + i * 9e9, gross_profit: 84e9 + i * 9.4e9, rnd: 10e9 + i * 2e9, sga: 14e9 + i * 1e9, operating_income: 60e9 + i * 6.6e9, interest_expense: -(1.4e9 + i * 0.1e9), pretax_income: 61e9 + i * 6.8e9, income_tax: 15e9 + i * 0.8e9, net_income: 45.7e9 + i * 5.6e9, ebitda: 73e9 + i * 7.2e9, eps_diluted: 2.08 + i * 0.44 }
    : { cash: 20e9 + i * 1e9, total_assets: 321e9 + i * 3e9, total_liabilities: 193e9 + i * 8e9, shareholders_equity: 128e9 - i * 6e9, operating_cash_flow: 66e9 + i * 4e9, capex: -(12e9 + i * 0.2e9), free_cash_flow: 53e9 + i * 4e9, net_income: 45.7e9 + i * 5.6e9 }
}));
const documents = [
  { id: 'doc-10k', kind: 'filing', form: '10-K', filed_at: day(-20), ticker: 'AAPL', title: `Annual report pursuant to Section 13 or 15(d): ${LONG}_aapl-20260927x10k_final_revised.htm`, source: 'SEC EDGAR', url: 'https://www.sec.gov/fixture' },
  { id: 'doc-8k', kind: 'filing', form: '8-K', filed_at: day(-40), ticker: 'BRK.B', title: 'Current report, items 2.02 and 9.01', source: 'SEC EDGAR', url: 'https://www.sec.gov/fixture-2' },
  { id: 'doc-tr', kind: 'transcript', form: 'transcript', filed_at: day(-25), ticker: 'AAPL', title: 'Apple Inc. (AAPL) Q4 2026 Earnings Call Transcript with extended analyst question and answer session', source: 'Fixture transcripts' }
];
const transcript = { id: 'doc-tr', words: 9876, segments: [
  { ordinal: 1, speaker: 'Operator', role: 'operator', section: 'prepared', text: 'Good day and welcome to the fourth quarter fiscal 2026 earnings conference call.' },
  { ordinal: 2, speaker: 'Chief Executive Officer', role: 'management', section: 'prepared', text: 'Revenue reached a September quarter record, with services growing double digits and installed base at an all-time high across every geographic segment.' },
  { ordinal: 3, speaker: 'Analyst, Very Long Investment Bank Name and Partners LLC', role: 'analyst', section: 'qa', text: 'Could you walk us through gross margin guidance, specifically the supplychainunbrokenidentifierthatkeepsgoingwithoutspaces dynamics?' }
] };
const etf = {
  ticker: 'SPY', name: 'SPDR S&P 500 ETF Trust (State Street Global Advisors Trust Company)', issuer: 'State Street Global Advisors', aum: 612345678901, expense: 0.0945, holdings: 503, index: 'S&P 500 Total Return Index', inception: '1993-01-22',
  top_holdings: [{ name: 'Apple Inc.', weight: 7.1 }, { name: 'Microsoft Corporation', weight: 6.8 }, { name: 'NVIDIA Corporation', weight: 6.2 }, { name: 'Alphabet Inc. Class A', weight: 3.9 }],
  flows: monthly(24, (i) => Math.sin(i / 3) * 4.2).map(([t, f], i) => [t, f, i * 0.8 - 3]),
  sectors: [{ name: 'Information Technology', weight: 31.4 }, { name: 'Health Care', weight: 11.2 }, { name: 'Communication Services', weight: 9.1 }],
  countries: [{ name: 'United States', weight: 99.4 }, { name: 'Netherlands', weight: 0.4 }]
};
const calendar = {
  earnings: [
    { date: day(2), ticker: 'AAPL', time: 'AMC', eps_estimate: 1.6, revenue_estimate: 94123456789 },
    { date: day(3), ticker: 'BRK.B', time: 'BMO', eps_estimate: -0.42, revenue_estimate: 91234567890 }
  ],
  ipos: [{ date: day(5), ticker: 'LONGCO', company: 'Long Name Infrastructure Holdings Corporation Limited', exchange: 'NYSE', range: '$18.00 - $21.00', size: 1234567890 }],
  corporate: [{ date: day(6), ticker: 'NVDA', type: 'split', detail: '10-for-1 forward stock split effective after market close' }],
  central_banks: [{ date: day(-30), bank: 'Federal Reserve', change: -0.25, current: 4.375 }]
};
const congress = { trades: [
  { member: 'Representative Alexandria Longname-Hyphenated', party: 'D', chamber: 'house', ticker: 'AAPL', type: 'purchase', amount: '$1,000,001 - $5,000,000', traded: day(-50), disclosed: day(-20) },
  { member: 'Senator Fixture', party: 'R', chamber: 'senate', ticker: 'BRK.B', type: 'sale', amount: '$15,001 - $50,000', traded: day(-60), disclosed: day(-30) }
] };
const sources = [
  { id: 'fred', name: 'FRED (Federal Reserve Bank of St. Louis)', connected: true, key: 'needed', serves: ['macro'] },
  { id: 'fmp', name: 'Financial Modeling Prep', connected: false, key: 'needed', serves: ['statements', 'estimates', 'earnings', 'etf', 'calendar', 'transcripts'] },
  { id: 'edgar', name: 'SEC EDGAR', connected: true, key: 'none', serves: ['statements', 'filings', 'insiders'] }
];
const orders = { datasets: [
  { id: 'estimates', custom: true, providers: [{ id: 'fmp', label: 'Financial Modeling Prep', connected: false }, { id: 'av', label: 'Alpha Vantage', connected: true }, { id: 'finnhub', label: 'Finnhub', connected: true }] },
  { id: 'etf', custom: false, providers: [{ id: 'fmp', label: 'Financial Modeling Prep', connected: false }, { id: 'eodhd', label: 'EODHD', connected: false }] }
], transcripts_order: { id: 'transcripts', custom: false, providers: [{ id: 'fmp', label: 'Financial Modeling Prep', connected: false }] } };

const findbResults = [
  { id: 'i-aapl', symbol: 'AAPL', asset_type: 'equity', name: 'Apple Inc.', exchange: 'NASDAQ', currency: 'USD', country: 'United States', sector: 'Information Technology' },
  { id: 'i-long', symbol: 'VERYLONGSYMBOL.EXCHANGE', asset_type: 'etf', name: 'iShares Core MSCI Emerging Markets Investable Market Index UCITS ETF (Accumulating)', exchange: 'London Stock Exchange', currency: 'USD', category: 'Emerging Markets Equity' },
  { id: 'i-eur', symbol: 'EURUSD=X', asset_type: 'currency', name: 'EUR/USD', exchange: 'CCY', currency: 'USD' }
];
const findbFolders = [{ id: 'folder-1', name: 'Long-term holdings for retirement and family wealth' }, { id: 'folder-2', name: 'Watch' }];
const findbFavorites = [
  { id: 'fav-1', instrument_id: 'i-aapl', symbol: 'AAPL', asset_type: 'equity', name: 'Apple Inc.', folder_id: 'folder-1', note: 'Core position, rebalance above 12% of the portfolio weight' },
  { id: 'fav-2', instrument_id: null, symbol: 'GONE.L', asset_type: 'equity', name: '', folder_id: null, note: '' }
];

const connectors = [
  { id: 'c-yahoo', provider: 'yahoo', name: 'Yahoo Finance with a deliberately long connector name', label: 'Yahoo Finance', asset_types: ['equity', 'etf', 'crypto'], timeframes: ['1m', '5m', '1h', '1d', '1w'], required_secrets: [], set_secrets: [], quota: { used: 1234, max_requests: 2000, period: 'day' } },
  { id: 'c-massive', provider: 'massive', name: 'Massive', label: 'Massive', asset_types: ['equity', 'option'], timeframes: ['1m', '1d'], required_secrets: ['api_key'], set_secrets: [], quota: null }
];
const jobs = [
  { id: 'job-1', batch_id: null, ticker: 'VERYLONGTICKER.EXCHANGE', provider: 'yahoo', asset_type: 'equity', timeframe: '1m', kind: 'download', status: 'running', chunks_done: 37, chunks_total: 120, bars_written: 1234567, dataset_id: 'ds-1', error: null },
  { id: 'job-2', batch_id: null, ticker: 'AAPL', provider: 'massive', asset_type: 'equity', timeframe: '1d', kind: 'download', status: 'error', chunks_done: 0, chunks_total: 4, bars_written: 0, dataset_id: null, error: 'Massive answered 403 Forbidden: your plan does not include historical aggregates before 2020-01-01 for this ticker' },
  { id: 'job-3', batch_id: null, ticker: 'BTC-USD', provider: 'yahoo', asset_type: 'crypto', timeframe: '1h', kind: 'download', status: 'waiting', wait_reason: 'quota', resume_at: new Date(ms(1)).toISOString(), chunks_done: 3, chunks_total: 9, bars_written: 4567, dataset_id: 'ds-3', error: null }
];
const datasets = [
  { id: 'ds-1', ticker: 'VERYLONGTICKER.EXCHANGE', asset_type: 'equity', timeframe: '1m', provider: 'yahoo', bar_count: 1234567, size_bytes: 98765432, range_from: '2024-01-02T14:30:00Z', range_to: '2026-10-16T20:59:00Z', status: 'partial', last_updated: '2026-10-19T08:00:00Z', label: 'Intraday research set with a long descriptive label', tags: ['research', 'intraday'] },
  { id: 'ds-2', ticker: 'AAPL', asset_type: 'equity', timeframe: '1d', provider: 'massive', bar_count: 5034, size_bytes: 456789, range_from: '2006-01-03T00:00:00Z', range_to: '2026-10-16T00:00:00Z', status: 'complete', last_updated: '2026-10-19T08:00:00Z', tags: [] },
  { id: 'ds-3', ticker: 'BTC-USD', asset_type: 'crypto', timeframe: '1h', provider: 'import', source: 'binance-export-2026-10-very-long-file-name_BTCUSDT_1h_full_history.csv', bar_count: 70123, size_bytes: 3456789, range_from: '2018-01-01T00:00:00Z', range_to: '2026-10-01T00:00:00Z', status: 'error', last_updated: '2026-10-19T08:00:00Z', tags: [] }
];

const goals = [
  { id: 'g-1', position: 1, name: 'Reach a diversified emergency fund covering twelve months of household expenses', deadline: day(4), details: 'Keep it in a money-market fund.', category: 'Savings and long-term wealth', color: '#6f8bab', kpis: [
    { name: 'Monthly transfer automated', target: 1, current: 1, points: 2, reached: true },
    { name: 'Balance in thousands', target: 36000, current: 21450.5, points: 3, reached: false },
    { name: 'Expenses reviewed', target: 12, current: -1, points: 1, reached: false }
  ] },
  { id: 'g-2', position: 2, name: 'Journal every trade', deadline: day(-3), details: '', category: 'Trading', color: '', kpis: [{ name: 'Trades journaled', target: 50, current: 12, points: 1, reached: false }] },
  { id: 'g-3', position: 3, name: 'Read ten books', deadline: null, details: '', category: '', color: '#7fb894', kpis: [{ name: 'Books', target: 10, current: 10, points: 1, reached: true }] },
  { id: 'g-4', position: 4, name: 'Backtest three strategies', deadline: day(20), details: '', category: 'Trading', color: '', kpis: [] }
];

// ── Fixture router ─────────────────────────────────────────────────────────

const INSTALLED = ['fundamentals', 'findb', 'histdata', 'goals', 'histviz', 'remindme', 'connectors', 'settings'];

/** state: 'populated' | 'empty' | 'error'. `fail` lists path prefixes answering 500. */
function router(state, requests, fail = []) {
  const pop = state === 'populated';
  const err = state === 'error';
  return async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const p = url.pathname;
    const method = req.method();
    requests.push({ method, path: p, search: url.search, body: req.postData() });
    const json = (result, status = 200) => route.fulfill({ status, json: result });
    if (fail.some((f) => p.startsWith(f) && (method !== 'GET' || f.endsWith('!get')))) return json({ error: 'Fixture server error: the request could not be saved' }, 500);
    // Shell
    if (p === '/api/setup/status') return json({ configured: true });
    if (p === '/api/settings/me') return json({ username: 'fixture', role: 'admin', locale: 'en' });
    if (p === '/api/settings/modules') return json({ installed: INSTALLED });
    if (p === '/api/settings/defaults') return json({ default_timezone: 'UTC', display_currency: 'USD' });
    if (p === '/api/settings/versioning') return json({ modules: {} });
    if (p === '/api/mcp/settings') return json({ enabled: false });
    if (p === '/api/health') return json({ status: 'ok', services: { core: 'ok', postgres: 'ok' } });
    if (p === '/api/dashboard/layout') return json({ pages: [] });
    if (p === '/api/tasks') return json({ tasks: [] });
    if (p === '/api/demo') return json({ demo: false });
    if (p.includes('/notifications')) return json({ notifications: [], unread: 0, channels: [] });
    if (p === '/api/feeds/stream') return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
    if (p === '/api/reminders') return json({ reminders: [] });
    if (p === '/api/connectors/providers') return json({ providers: [] });
    if (p === '/api/connectors/modules') return json({ modules: [] });
    if (p === '/api/connectors') return err ? json({ error: 'Fixture connector service failed' }, 500) : json({ connectors: pop ? connectors : [] });

    // Mutations echo something sensible.
    if (method !== 'GET') {
      if (p === '/api/goals') return json({ goal: { id: 'g-new', position: 9, ...JSON.parse(req.postData() || '{}') } });
      if (p === '/api/findb/favorites') return json({ id: 'fav-new' });
      if (p === '/api/findb/folders') return json({ folder: { id: 'folder-new', name: JSON.parse(req.postData() || '{}').name } });
      if (p === '/api/fundamentals/companies') return json({ company: { ...company, ticker: JSON.parse(req.postData() || '{}').ticker ?? 'AAPL' } });
      if (p.startsWith('/api/fundamentals/data/') && p.endsWith('/refresh')) return json(err ? { error: 'x' } : { snapshot: null, deferred: [] }, err ? 500 : 200);
      return json({});
    }

    if (err && /^\/api\/(fundamentals|findb|histdata|goals)\//.test(p + '/') && !p.startsWith('/api/fundamentals/lookup/companies')) {
      return json({ error: 'Fixture server error: the data service is unavailable' }, 500);
    }

    // Fundamentals
    if (p === '/api/fundamentals/series') return json({ series: pop ? seriesRows : [] });
    const obsMatch = p.match(/^\/api\/fundamentals\/series\/([^/]+)\/observations$/);
    if (obsMatch) return json({ observations: observations[obsMatch[1]] ?? [] });
    if (p === '/api/fundamentals/lookup/yield-curve') return json({ curve: pop ? curve : null });
    if (p === '/api/fundamentals/lookup/series') return json({ series: [
      { provider: 'fred', code: 'CPIAUCSL', title: 'Consumer Price Index for All Urban Consumers: All Items in U.S. City Average', unit: 'Index 1982-1984=100', frequency: 'Monthly', seasonal_adj: true },
      { provider: 'fred', code: 'CPILFESL_EXTREMELY_LONG_SERIES_IDENTIFIER', title: 'Core CPI less food and energy', unit: 'Index', frequency: 'Monthly' }
    ] });
    if (p === '/api/fundamentals/lookup/companies') return json({ companies: [
      { ticker: 'MSFT', name: 'Microsoft Corporation', exchange: 'NASDAQ', sector: 'Technology' },
      { ticker: 'MSTR', name: 'Strategy Incorporated (formerly MicroStrategy)', exchange: 'NASDAQ', sector: 'Software' }
    ] });
    if (p === '/api/fundamentals/companies') return json({ companies: pop ? companies : [] });
    const cMatch = p.match(/^\/api\/fundamentals\/companies\/([^/]+)$/);
    if (cMatch) return pop ? json({ company: { ...company, ticker: decodeURIComponent(cMatch[1]) } }) : json({ error: 'not found' }, 404);
    if (p.endsWith('/statements')) return json({ statements: pop ? statements(url.searchParams.get('kind')) : [] });
    if (p.match(/^\/api\/fundamentals\/companies\/[^/]+\/documents$/)) return json({ documents: pop ? documents.filter((d) => (url.searchParams.get('form') === 'transcript' ? d.kind === 'transcript' : true)) : [] });
    if (p.match(/^\/api\/fundamentals\/companies\/[^/]+\/insiders$/)) return json({ insiders: [] });
    if (p.match(/^\/api\/fundamentals\/companies\/[^/]+\/view\//)) return json({});
    if (p === '/api/fundamentals/documents') return json({ documents: pop ? documents : [] });
    if (p === '/api/fundamentals/documents/doc-tr') return json({ document: transcript });
    if (p === '/api/fundamentals/etfs') return json({ etfs: pop ? [{ ticker: 'SPY', name: etf.name, followed: true, opened: ms(-1) }, { ticker: 'VWCE.DE', name: 'Vanguard FTSE All-World UCITS ETF (USD) Accumulating', followed: false, opened: ms(-2) }] : [] });
    if (p === '/api/fundamentals/calendar') return json(pop ? calendar : { earnings: [], ipos: [], corporate: [], central_banks: [] });
    if (p === '/api/fundamentals/sources') return json({ sources: pop ? sources : [] });
    if (p === '/api/fundamentals/datasets') return json(pop ? orders : { datasets: [], transcripts_order: { id: 'transcripts', providers: [] } });
    const dMatch = p.match(/^\/api\/fundamentals\/data\/([^/]+)$/);
    if (dMatch) {
      if (!pop) return json({ snapshot: null });
      const ds = dMatch[1];
      if (ds === 'central_banks') return json(snap(banks, 'BIS'));
      if (ds === 'price') return json(snap({ series: monthly(60, (i) => 150 + i * 1.3) }));
      if (ds === 'etf') return json(snap(etf, 'Financial Modeling Prep'));
      if (ds === 'calendar') return json(snap({}));
      if (ds === 'congress') return json(snap(congress, 'Quiver Quantitative'));
      if (ds === 'lobbying') return json(snap({ match: 'APPLE', series: monthly(12, (i) => 2e6 + i * 1e5), filings: [{ period: '2026 Q3', registrant: 'Apple Inc. Government Affairs Office and Associated Registrants', amount: 2340000, issues: 'Taxation, trade, consumer product safety, privacy and data security', url: 'https://lda.gov/fixture' }] }, 'LDA.gov'));
      if (ds === 'contracts') return json(snap({ match: 'APPLE', series: monthly(12, (i) => 1e6 + i * 2e5), awards: [{ date: day(-10), agency: 'Department of Veterans Affairs', amount: 12345678, description: 'Enterprise mobile devices, accessories and multi-year support services' }] }, 'USAspending'));
      if (ds === 'patents') return json(snap({ match: 'APPLE INC', total: 2345, series: monthly(12, (i) => 180 + i) }, 'USPTO'));
      return json({ snapshot: null });
    }

    // FinanceDatabase
    if (p === '/api/findb/status') return json({ installed: true, importing: false, count: 312345, version: '2026.10.01' });
    if (p === '/api/findb/update-check') return json({ installed: '2026.10.01', latest: '2026.10.01', update_available: false });
    if (p === '/api/findb/search') return json({ results: pop ? findbResults : [], has_more: false });
    if (p === '/api/findb/facets') return json({ values: pop ? ['NASDAQ', 'NYSE', 'London Stock Exchange'] : [] });
    if (p === '/api/findb/folders') return json({ folders: pop ? findbFolders : [] });
    if (p === '/api/findb/favorites') return json({ favorites: pop ? findbFavorites : [] });

    // Historical data
    if (p === '/api/histdata/jobs') return json({ jobs: pop ? jobs : [] });
    if (p === '/api/histdata/datasets') return json({ datasets: pop ? datasets : [] });

    // Goals
    if (p === '/api/goals') return json({ goals: pop ? goals : [] });

    return json({});
  };
}

// ── Routes under test ───────────────────────────────────────────────────────

// `ready`: a selector present once the populated page has rendered. `actions`: the primary
// controls that must be reachable by tap at every phone width (populated state). `regions`:
// wide tables that must scroll inside a labeled region rather than the page.
const ROUTES = [
  { path: '/fundamentals', ready: '.layout', actions: ['.mobile-catalog-toggle', '.views button', '.fd-toolbar .chip', '.fd-section header .toggle'], regions: true },
  { path: '/fundamentals/alt', ready: '.fd-toolbar', actions: ['.fd-toolbar input[type="search"]', '[role="tab"]', '.note .link'], regions: true },
  { path: '/fundamentals/calendar', ready: '.page', actions: ['.note .link'], regions: true },
  { path: '/fundamentals/company?t=AAPL&tab=financials', name: 'company', ready: '.head', actions: ['.search input[type="search"]', '.head button:has-text("Follow")', '.head button:has-text("Refresh")', '[role="tab"]', '.fd-section .actions .chip'], regions: true },
  { path: '/fundamentals/documents', ready: '.fd-section', actions: ['input.q', '.filters .chip', 'td.act .link'], regions: true },
  { path: '/fundamentals/etf', ready: '.page', actions: ['.sp input[type="search"]', '.note .link'], regions: true },
  { path: '/fundamentals/library', ready: '.fd-section', actions: ['[role="tab"]', 'input.filter', 'td.act button'], regions: true },
  { path: '/findb', ready: '.findb', actions: ['input.search', '.tabs button'] },
  { path: '/histdata', ready: '.page', actions: ['.page nav button', '.page nav button.broker', '.job .btn'] },
  { path: '/goals', ready: '.goals', actions: ['.head-actions button.primary', '.filters .chip', '.toolbar-right a.chip', '.card', '.reorder button:not([disabled])'] },
  { path: '/goals/stats', ready: '.stats', actions: ['a.btn', '.legend li.clickable'] }
];

const failures = [];
const notes = [];
function check(cond, msg) {
  if (!cond) failures.push(msg);
}

async function newPage(state, { fail = [], width = 390, height = 844, url = base } = {}) {
  const context = await browser.newContext({ viewport: { width, height }, locale: 'en-US', reducedMotion: 'reduce', hasTouch: width < 1100 });
  // Remembered tabs and filters must not leak between runs.
  await context.addInitScript(() => {
    try {
      localStorage.clear();
    } catch {
      /* storage unavailable */
    }
  });
  const requests = [];
  await context.route('**/api/**', router(state, requests, fail));
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  await page.clock.setFixedTime(NOW);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.base = url;
  return { context, page, requests, errors };
}

async function open(page, path, ready) {
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      await page.goto(`${page.base}${path}`, { waitUntil: 'domcontentloaded' });
      if (ready) await page.locator(ready).first().waitFor({ state: 'attached', timeout: 12000 });
      await page.waitForLoadState('networkidle', { timeout: 5000 }).catch(() => {});
      await page.waitForTimeout(150);
      return;
    } catch (e) {
      // First navigation after a cold dependency optimization can reload the page.
      if (attempt) throw e;
    }
  }
}

async function overflow(page) {
  return page.evaluate(() => {
    const vw = document.documentElement.clientWidth;
    const over = [...document.querySelectorAll('body *')].filter((el) => {
      const r = el.getBoundingClientRect();
      if (!r.width || r.right <= vw + 1) return false;
      const s = getComputedStyle(el);
      if (s.position === 'fixed' || s.visibility === 'hidden') return false;
      // Inside a local scroller: allowed, that is the point of the region.
      for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
        const ps = getComputedStyle(p);
        if (/(auto|scroll|hidden)/.test(ps.overflowX) && p.getBoundingClientRect().right <= vw + 1) return false;
      }
      return true;
    }).slice(0, 6).map((el) => `${el.tagName}.${typeof el.className === 'string' ? el.className.split(' ').slice(0, 3).join('.') : ''}`);
    return { vw, sw: document.documentElement.scrollWidth, over };
  });
}

/** Scroll into view, then the element's centre must be inside the viewport and hit it. */
async function reachable(page, locator) {
  await locator.scrollIntoViewIfNeeded({ timeout: 3000 });
  const first = await hits(locator);
  if (!first) return '';
  // A floating control may sit over the very edge: the user scrolls a little further.
  await locator.evaluate((el) => el.scrollIntoView({ block: 'center', inline: 'center' }));
  return hits(locator);
}
function hits(locator) {
  return locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return `centre outside viewport (${Math.round(x)},${Math.round(y)})`;
    const hit = document.elementFromPoint(x, y);
    if (!hit || !(hit === el || el.contains(hit))) return `covered by ${hit?.tagName}.${hit?.className}`;
    return '';
  });
}

async function shot(page, name) {
  await page.screenshot({ path: `${artifacts}/${name}.png`, fullPage: true });
}

const slug = (r) => r.name ?? (r.path.replace(/^\//, '').replace(/[/?&=]/g, '-') || 'root');

if (process.env.PROBE) {
  // Debug helper: PROBE='<path>|<width>|<expression>' prints the expression's value.
  const [path, width, expr] = process.env.PROBE.split('|');
  const { context, page } = await newPage('populated', { width: +width, height: 844 });
  await open(page, path, null);
  await page.waitForTimeout(800);
  console.log(JSON.stringify(await page.evaluate(expr), null, 1));
  await context.close();
  await browser.close();
  process.exit(0);
}

// ── 1. Viewport matrix: every route, three fixture states, five widths ──────

for (const r of ROUTES) {
  if (only && !only.includes(slug(r))) continue;
  for (const state of ['populated', 'empty', 'error']) {
    const { context, page, errors } = await newPage(state);
    try {
      await open(page, r.path, state === 'populated' ? r.ready : null);
      await page.waitForFunction(() => document.body.innerText.trim().length > 20, undefined, { timeout: 8000 }).catch(() => {});
      // The catalog stays reachable on the macro page: open it, the populated check covers it.
      for (const vp of PHONES) {
        await page.setViewportSize(vp);
        await page.waitForTimeout(80);
        const m = await overflow(page);
        check(m.sw <= m.vw + 1 && !m.over.length, `${r.path} [${state}] ${vp.width}x${vp.height}: page overflow ${m.sw}>${m.vw} ${m.over.join(', ')}`);
        if (state === 'populated') {
          for (const sel of r.actions) {
            if ((sel === '.mobile-catalog-toggle' || sel.startsWith('.reorder')) && vp.width > 767) continue;
            const loc = page.locator(sel).first();
            if (!(await loc.count())) {
              failures.push(`${r.path} ${vp.width}: action ${sel} missing`);
              continue;
            }
            const why = await reachable(page, loc).catch((e) => e.message.split('\n')[0]);
            check(!why, `${r.path} ${vp.width}x${vp.height}: ${sel} not reachable: ${why}`);
          }
          if (r.regions) {
            const cards = await page.evaluate(() => [...document.querySelectorAll('.fd-scroll:has(table.fd-cards)')].filter((el) => getComputedStyle(el.querySelector('table')).display === 'block' && el.scrollWidth > el.clientWidth + 1).length);
            check(!cards, `${r.path} ${vp.width}: ${cards} record card list(s) wider than their region`);
            const unlabeled = await page.evaluate(() => [...document.querySelectorAll('.fd-scroll:has(table)')].filter((el) => el.scrollWidth > el.clientWidth + 1 && !(el.getAttribute('role') === 'region' && el.getAttribute('aria-label'))).length);
            check(!unlabeled, `${r.path} ${vp.width}: ${unlabeled} wide table region(s) without role=region and a label`);
          }
        }
        if (vp.width === 320 || vp.width === 390 || state === 'populated') await shot(page, `${slug(r)}-${state}-${vp.width}x${vp.height}`);
      }
      if (process.env.REVIEW) {
        for (const width of [320, 390]) {
          await page.setViewportSize({ width, height: 2400 });
          await page.waitForTimeout(100);
          await page.screenshot({ path: `${artifacts}/review-${slug(r)}-${state}-${width}.png` });
        }
      }
      if (state === 'error' && errors.length) {
        // The page code does not catch this load failure: a logic gap, reported, not a layout one.
        notes.push(`${r.path} [error]: uncaught load failure (${errors[0]})`);
      } else check(!errors.length, `${r.path} [${state}]: page errors ${errors.join(' | ')}`);
    } catch (e) {
      failures.push(`${r.path} [${state}]: ${e.message.split('\n')[0]}`);
      await shot(page, `${slug(r)}-${state}-failure`).catch(() => {});
    } finally {
      await context.close();
    }
  }
}

// ── 2. Primary flows by tap at 390 px (and the same flow at desktop for the payload) ──

async function tap(page, locator) {
  const why = await reachable(page, locator);
  if (why) throw new Error(`not reachable by tap: ${why}`);
  await locator.tap();
}

async function noOverflow(page, label) {
  const m = await overflow(page);
  check(m.sw <= m.vw + 1 && !m.over.length, `${label}: page overflow ${m.sw}>${m.vw} ${m.over.join(', ')}`);
}

/** Every element of `sel` inside the open dialog fits the viewport horizontally. */
async function dialogFits(page, label) {
  const r = await page.evaluate(() => {
    const d = document.querySelector('.modal');
    if (!d) return 'no dialog';
    const out = [...d.querySelectorAll('*')].filter((el) => {
      const b = el.getBoundingClientRect();
      if (!b.width) return false;
      for (let p = el.parentElement; p && p !== d; p = p.parentElement) if (/(auto|scroll|hidden)/.test(getComputedStyle(p).overflowX)) return false;
      return b.left < -1 || b.right > innerWidth + 1;
    });
    return out.length ? out.slice(0, 4).map((el) => `${el.tagName}.${el.className}`).join(', ') : '';
  });
  check(!r, `${label}: dialog content outside the viewport: ${r}`);
}

const want = (flow) => !only || only.includes(flow);

// Company: search a company and open it.
if (want('flow-company')) {
  for (const width of [390, 320]) {
    const { context, page, requests, errors } = await newPage('populated', { width, height: width === 320 ? 568 : 844 });
    try {
      await open(page, '/fundamentals/company?t=AAPL&tab=overview', '.head');
      const input = page.locator('.search input[type="search"]');
      await tap(page, input);
      await input.fill('MS');
      const hit = page.locator('.menu .row button.main', { hasText: 'MSFT' });
      await hit.waitFor();
      await noOverflow(page, `company search menu ${width}`);
      await tap(page, hit);
      await page.locator('h2.tk', { hasText: 'MSFT' }).waitFor();
      check(requests.some((q) => q.method === 'GET' && q.path === '/api/fundamentals/companies/MSFT'), `company ${width}: picking MSFT did not load it`);
      await tap(page, page.locator('[role="tab"]', { hasText: 'Financials' }));
      await page.locator('.fd-scroll[role="region"] table tbody td.num').first().waitFor();
      await noOverflow(page, `company financials ${width}`);
      const region = await page.locator('.fd-scroll[role="region"]').first().evaluate((el) => ({ label: el.getAttribute('aria-label'), scrolls: el.scrollWidth > el.clientWidth, sticky: getComputedStyle(el.querySelector('tbody td')).position }));
      check(region.label && region.scrolls && region.sticky === 'sticky', `company ${width}: statement region ${JSON.stringify(region)}`);
      const hide = page.locator('td.hide button').first();
      check((await hide.evaluate((el) => getComputedStyle(el).opacity)) === '1', `company ${width}: hide-line control invisible on touch`);
      await shot(page, `flow-company-${width}`);
      check(!errors.length, `company flow ${width}: ${errors.join(' | ')}`);
    } catch (e) {
      failures.push(`company flow ${width}: ${e.message.split('\n')[0]}`);
      await shot(page, `flow-company-${width}-failure`);
    } finally {
      await context.close();
    }
  }
}

// Documents: the list is a card per document and the reader fits.
if (want('flow-documents')) {
  for (const vp of [{ width: 320, height: 568 }, { width: 390, height: 844 }, { width: 667, height: 375 }]) {
    const { context, page, errors } = await newPage('populated', vp);
    try {
      await open(page, '/fundamentals/documents', '.fd-section');
      const title = page.locator('td.fd-card-main').first();
      const wraps = await title.evaluate((el) => el.getBoundingClientRect().right <= innerWidth && el.scrollWidth <= el.clientWidth + 1);
      check(wraps, `documents ${vp.width}: long filename does not wrap inside the card`);
      await tap(page, page.locator('.filters .chip', { hasText: '10-K' }));
      check((await page.locator('tbody tr').count()) === 1, `documents ${vp.width}: form filter did not apply`);
      await tap(page, page.locator('.filters .chip').first());
      await tap(page, page.locator('td.act .link').first());
      await page.locator('.seg').first().waitFor();
      await noOverflow(page, `transcript reader ${vp.width}`);
      await tap(page, page.locator('.fd-row.bar .chip', { hasText: 'Q&A' }));
      await shot(page, `flow-documents-${vp.width}`);
      await tap(page, page.locator('button', { hasText: 'Back' }).first());
      await page.locator('td.fd-card-main').first().waitFor();
      check(!errors.length, `documents flow ${vp.width}: ${errors.join(' | ')}`);
    } catch (e) {
      failures.push(`documents flow ${vp.width}: ${e.message.split('\n')[0]}`);
      await shot(page, `flow-documents-${vp.width}-failure`);
    } finally {
      await context.close();
    }
  }
}

// Macro: the catalog opens from its labeled toggle and a series can be picked by tap.
if (want('flow-macro')) {
  const { context, page, requests, errors } = await newPage('populated', { width: 390, height: 844 });
  try {
    await open(page, '/fundamentals', '.layout');
    const toggle = page.locator('.mobile-catalog-toggle');
    check(/Series catalog/.test(await toggle.innerText()) && (await toggle.getAttribute('aria-expanded')) === 'false', 'macro: catalog toggle not labeled or not collapsed');
    await tap(page, toggle);
    await page.locator('#macro-catalog').waitFor({ state: 'visible' });
    await tap(page, page.locator('#macro-catalog .item', { hasText: 'Total public debt' }));
    await page.locator('.picked .pname', { hasText: 'Total public debt' }).waitFor();
    check(requests.some((q) => q.path === '/api/fundamentals/series/u-tb/observations'), 'macro: picking a series did not load it');
    await noOverflow(page, 'macro catalog open');
    await shot(page, 'flow-macro-390');
    await tap(page, page.locator('#macro-catalog button', { hasText: 'Add series' }));
    await page.locator('.modal .addform').waitFor();
    await page.locator('.modal .addform input[type="search"]').fill('cpi');
    await tap(page, page.locator('.modal .addform button[type="submit"]'));
    await page.waitForTimeout(300);
    await dialogFits(page, 'macro add series dialog');
    await shot(page, 'flow-macro-add-390');
    await tap(page, page.locator('.modal .modal-head .x'));
    await page.locator('.modal').waitFor({ state: 'hidden' });
    await tap(page, page.locator('.page-header button', { hasText: 'Customize' }));
    await page.locator('.modal .cust').waitFor();
    await dialogFits(page, 'macro customize dialog');
    const why = await reachable(page, page.locator('.modal .modal-foot button').last());
    check(!why, `macro customize: Done ${why}`);
    await shot(page, 'flow-macro-customize-390');
    check(!errors.length, `macro flow: ${errors.join(' | ')}`);
  } catch (e) {
    failures.push(`macro flow: ${e.message.split('\n')[0]}`);
    await shot(page, 'flow-macro-failure');
  } finally {
    await context.close();
  }
}

// FinanceDatabase: populated search, open a record, save it, then a failing save.
async function findbSave(width, height, fail = []) {
  const run = await newPage('populated', { width, height, fail });
  const { page } = run;
  await open(page, '/findb', '.findb .tabs');
  const input = page.locator('input.search');
  await input.click();
  await input.fill('apple');
  await page.locator('.tbl .row:not(.head)').first().waitFor();
  await page.waitForLoadState('networkidle').catch(() => {});
  await page.waitForTimeout(400);
  return run;
}
if (want('flow-findb')) {
  const payloads = {};
  for (const vp of [{ width: 1440, height: 900 }, { width: 390, height: 844 }, { width: 320, height: 568 }]) {
    const { context, page, requests, errors } = await findbSave(vp.width, vp.height);
    try {
      if (vp.width < 1100) {
        await noOverflow(page, `findb results ${vp.width}`);
        const card = await page.locator('.tbl .row:not(.head)').nth(1).evaluate((el) => ({ right: el.getBoundingClientRect().right, name: getComputedStyle(el.querySelector('.c-name')).whiteSpace }));
        check(card.right <= vp.width + 1 && card.name === 'normal', `findb ${vp.width}: record card ${JSON.stringify(card)}`);
        await shot(page, `flow-findb-results-${vp.width}`);
      }
      const star = page.locator('.tbl .row:not(.head) .star').first();
      if (vp.width < 1100) await tap(page, star);
      else await star.click();
      await page.locator('.modal').waitFor();
      if (vp.width < 1100) await dialogFits(page, `findb save dialog ${vp.width}`);
      const note = page.locator('.modal input').last();
      await note.fill('Core position');
      const save = page.locator('.modal .actions button.primary');
      if (vp.width < 1100) await tap(page, save);
      else await save.click();
      await page.locator('.modal').waitFor({ state: 'detached' });
      payloads[vp.width] = requests.filter((q) => q.method === 'POST' && q.path === '/api/findb/favorites').map((q) => JSON.parse(q.body));
      check(!errors.length, `findb flow ${vp.width}: ${errors.join(' | ')}`);
      if (vp.width < 1100) {
        // Favorites: the folder picker replaces the side pane.
        await tap(page, page.locator('.tabs button', { hasText: 'Favorites' }));
        await page.locator('#mobile-folder').selectOption('folder-1');
        await page.locator('.detail-head h3', { hasText: 'Long-term holdings' }).waitFor();
        await noOverflow(page, `findb favorites ${vp.width}`);
        for (const sel of ['#mobile-folder', '.mobile-folders button', '.folder-actions .link', '.tbl .row:not(.head) .star']) {
          const why = await reachable(page, page.locator(sel).first()).catch((e) => e.message);
          check(!why, `findb favorites ${vp.width}: ${sel} ${why}`);
        }
        await shot(page, `flow-findb-favorites-${vp.width}`);
      }
    } catch (e) {
      failures.push(`findb flow ${vp.width}: ${e.message.split('\n')[0]}`);
      await shot(page, `flow-findb-${vp.width}-failure`);
    } finally {
      await context.close();
    }
  }
  check(JSON.stringify(payloads[390]) === JSON.stringify(payloads[1440]) && payloads[390]?.length === 1, `findb: mobile save payload ${JSON.stringify(payloads[390])} differs from desktop ${JSON.stringify(payloads[1440])}`);
  check(JSON.stringify(payloads[320]) === JSON.stringify(payloads[1440]), 'findb: 320 px save payload differs from desktop');
  // A failing save keeps the dialog and the typed note.
  const { context, page, errors } = await findbSave(390, 844, ['/api/findb/favorites']);
  try {
    await tap(page, page.locator('.tbl .row:not(.head) .star').first());
    await page.locator('.modal input').last().fill('Kept after error');
    await tap(page, page.locator('.modal .actions button.primary'));
    await page.waitForTimeout(400);
    check(await page.locator('.modal').isVisible(), 'findb: failed save closed the dialog');
    check((await page.locator('.modal input').last().inputValue()) === 'Kept after error', 'findb: failed save lost the note');
    const shown = await page.locator('.modal').innerText();
    if (!/error|fail/i.test(shown)) notes.push(`findb: a failed save shows no message in the dialog${errors.length ? ` (uncaught: ${errors[0]})` : ''}`);
    await shot(page, 'flow-findb-save-error-390');
  } catch (e) {
    failures.push(`findb error flow: ${e.message.split('\n')[0]}`);
  } finally {
    await context.close();
  }
}

// Historical data: dataset cards, row actions, import dialog, provider configuration.
if (want('flow-histdata')) {
  for (const vp of [{ width: 320, height: 568 }, { width: 390, height: 844 }, { width: 667, height: 375 }]) {
    const { context, page, errors } = await newPage('populated', vp);
    try {
      await open(page, '/histdata', '.page nav');
      await noOverflow(page, `histdata download ${vp.width}`);
      const head = await page.locator('.job .head').first().evaluate((el) => [...el.children].every((c) => c.getBoundingClientRect().right <= el.getBoundingClientRect().right + 1));
      check(head, `histdata ${vp.width}: job header controls clipped`);
      await tap(page, page.locator('.page nav button', { hasText: 'Datasets' }));
      await page.locator('.set').first().waitFor();
      await noOverflow(page, `histdata datasets ${vp.width}`);
      const card = await page.locator('.set').first().evaluate((el) => {
        const r = (sel) => el.querySelector(sel).getBoundingClientRect();
        return { display: getComputedStyle(el).display, right: el.getBoundingClientRect().right, rangeOwnRow: r('.range').top > r('.tf').bottom - 1, barsOneLine: r('.num').height < 30 };
      });
      check(card.display === 'flex' && card.right <= vp.width + 1 && card.rangeOwnRow && card.barsOneLine, `histdata ${vp.width}: dataset row is not a readable card ${JSON.stringify(card)}`);
      for (const label of ['Check gaps', 'CSV', 'Delete']) {
        const why = await reachable(page, page.locator('.set .acts button', { hasText: label }).first()).catch((e) => e.message);
        check(!why, `histdata ${vp.width}: ${label} ${why}`);
      }
      await shot(page, `flow-histdata-datasets-${vp.width}`);
      await tap(page, page.getByRole('button', { name: 'Import', exact: true }));
      await page.locator('.modal').waitFor();
      await page.waitForTimeout(250);
      await dialogFits(page, `histdata import ${vp.width}`);
      await shot(page, `flow-histdata-import-${vp.width}`);
      await tap(page, page.locator('.modal .modal-head .x'));
      await page.locator('.modal').waitFor({ state: 'hidden' });
      await tap(page, page.locator('.page nav button.broker'));
      await page.locator('.modal').waitFor();
      await page.waitForTimeout(250);
      await dialogFits(page, `histdata connectors ${vp.width}`);
      await shot(page, `flow-histdata-connectors-${vp.width}`);
      check(!errors.length, `histdata flow ${vp.width}: ${errors.join(' | ')}`);
    } catch (e) {
      failures.push(`histdata flow ${vp.width}: ${e.message.split('\n')[0]}`);
      await shot(page, `flow-histdata-${vp.width}-failure`);
    } finally {
      await context.close();
    }
  }
}

// Goals: create a goal by tap (payload equal to the desktop one), reorder by tap,
// open a goal's detail; Goals statistics drill-down.
async function createGoal(width, height) {
  const run = await newPage('populated', { width, height });
  const { page } = run;
  const touch = width < 1100;
  const act = (loc) => (touch ? tap(page, loc) : loc.click());
  await open(page, '/goals', '.goals .grid');
  await act(page.locator('.head-actions button.primary'));
  const form = page.locator('.goal-form');
  await form.waitFor();
  if (touch) await dialogFits(page, `goal form ${width}`);
  await form.locator('label.field input').first().fill('Run a marathon');
  await form.locator('input[type="date"]').fill('2027-04-12');
  await form.locator('input[list="goal-categories"]').fill('Health');
  await act(form.locator('.swatch').nth(2));
  const row = form.locator('.kpi-scroll .kpi-row').first();
  await row.locator('input').nth(0).fill('Long run km');
  await row.locator('input').nth(1).fill('42.2');
  await row.locator('input').nth(2).fill('-3');
  await row.locator('input').nth(3).fill('2');
  await act(form.locator('.kpi-head .mini'));
  const second = form.locator('.kpi-scroll .kpi-row').nth(1);
  await second.locator('input').nth(0).fill('Races');
  await act(second.locator('input[type="checkbox"]'));
  await act(form.locator('.kpi-head .mini'));
  // The third, empty metric is removed with its own control.
  await act(form.locator('.kpi-scroll .kpi-row').nth(2).locator('button.x'));
  check((await form.locator('.kpi-scroll .kpi-row').count()) === 2, `goal form ${width}: remove metric did not remove the row`);
  if (touch) {
    const rows = await form.locator('.kpi-scroll .kpi-row').first().evaluate((el) => [...el.children].map((c) => { const b = c.getBoundingClientRect(); return { w: Math.round(b.width), right: b.right }; }));
    check(rows.every((c) => c.w >= 24 && c.right <= innerWidthOf(width) + 1), `goal form ${width}: metric row cramped ${JSON.stringify(rows)}`);
    await shot(page, `flow-goal-form-${width}`);
  }
  await act(form.locator('.actions button.primary'));
  await page.locator('.goal-form').waitFor({ state: 'detached' });
  return run;
}
const innerWidthOf = (w) => w;
if (want('flow-goals')) {
  const payloads = {};
  for (const vp of [{ width: 1440, height: 900 }, { width: 390, height: 844 }, { width: 320, height: 568 }]) {
    let run;
    try {
      run = await createGoal(vp.width, vp.height);
      payloads[vp.width] = run.requests.filter((q) => q.method === 'POST' && q.path === '/api/goals').map((q) => JSON.parse(q.body));
      check(!run.errors.length, `goals flow ${vp.width}: ${run.errors.join(' | ')}`);
      if (vp.width < 1100) {
        const { page, requests } = run;
        await noOverflow(page, `goals after create ${vp.width}`);
        // Tap reorder: the second card moves up through the same position call as a drop.
        await tap(page, page.locator('.card').nth(1).locator('.reorder button').first());
        await page.waitForTimeout(300);
        const pos = requests.filter((q) => q.method === 'POST' && /\/api\/goals\/[^/]+\/position$/.test(q.path));
        check(pos.length === 1 && pos[0].path === '/api/goals/g-2/position' && typeof JSON.parse(pos[0].body).position === 'number', `goals ${vp.width}: tap reorder sent ${JSON.stringify(pos)}`);
        await tap(page, page.locator('.card h3').first());
        await page.locator('.modal .kpi-table').waitFor();
        await dialogFits(page, `goal detail ${vp.width}`);
        for (const sel of ['.modal .step', '.modal .chk', '.modal .detail-actions button']) {
          const why = await reachable(page, page.locator(sel).last()).catch((e) => e.message);
          check(!why, `goal detail ${vp.width}: ${sel} ${why}`);
        }
        await shot(page, `flow-goal-detail-${vp.width}`);
      }
    } catch (e) {
      failures.push(`goals flow ${vp.width}: ${e.message.split('\n')[0]}`);
      if (run) await shot(run.page, `flow-goals-${vp.width}-failure`);
    } finally {
      await run?.context.close();
    }
  }
  check(payloads[1440]?.length === 1 && JSON.stringify(payloads[390]) === JSON.stringify(payloads[1440]) && JSON.stringify(payloads[320]) === JSON.stringify(payloads[1440]), `goals: mobile payload ${JSON.stringify(payloads[390])} differs from desktop ${JSON.stringify(payloads[1440])}`);

  for (const vp of [{ width: 320, height: 568 }, { width: 390, height: 844 }]) {
    const { context, page, errors } = await newPage('populated', vp);
    try {
      await open(page, '/goals/stats', '.legend');
      await tap(page, page.locator('.legend li.clickable', { hasText: 'Overdue' }));
      await page.locator('.drill table').waitFor();
      await noOverflow(page, `goals stats drill ${vp.width}`);
      const region = await page.locator('.drill-scroll').evaluate((el) => el.getAttribute('role') === 'region' && !!el.getAttribute('aria-label'));
      check(region, `goals stats ${vp.width}: drill table region unlabeled`);
      const why = await reachable(page, page.locator('.drill header .chip'));
      check(!why, `goals stats ${vp.width}: clear ${why}`);
      await shot(page, `flow-goals-stats-${vp.width}`);
      check(!errors.length, `goals stats flow ${vp.width}: ${errors.join(' | ')}`);
    } catch (e) {
      failures.push(`goals stats flow ${vp.width}: ${e.message.split('\n')[0]}`);
    } finally {
      await context.close();
    }
  }
}

// ── 3. Desktop: identical pixels against the clean baseline server ──────────

/** Charts animate on a timer: wait until two consecutive frames agree. */
async function stableShot(page, only) {
  await page.evaluate(() => document.fonts.ready);
  const take = () => (only ? page.locator(only).screenshot({ animations: 'disabled' }) : page.screenshot({ animations: 'disabled' }));
  let prev = await take();
  let same = 0;
  for (let i = 0; i < 20 && same < 2; i++) {
    await page.waitForTimeout(400);
    const next = await take();
    same = next.equals(prev) ? same + 1 : 0;
    prev = next;
  }
  return prev;
}

// Desktop states behind a click, where the mobile work added wrappers or attributes.
const DESKTOP_STATES = [
  { path: '/goals', name: 'goals-detail', ready: '.goals .grid', prep: (p) => p.locator('.card h3').first().click().then(() => p.locator('.modal .kpi-table').waitFor()) },
  { path: '/goals/stats', name: 'goals-stats-drill', ready: '.legend', prep: (p) => p.locator('.legend li.clickable', { hasText: 'Overdue' }).click().then(() => p.locator('.drill table').waitFor()) },
  { path: '/fundamentals/documents', name: 'documents-reader', ready: '.fd-section', prep: (p) => p.locator('td.act .link').first().click().then(() => p.locator('.seg').first().waitFor()) },
  { path: '/fundamentals/library', name: 'library-companies', ready: '.fd-section', prep: (p) => p.locator('[role="tab"]', { hasText: 'Companies' }).click().then(() => p.locator('td.main').first().waitFor()) },
  { path: '/histdata', name: 'histdata-datasets', ready: '.page nav', prep: (p) => p.locator('.page nav button', { hasText: 'Datasets' }).click().then(() => p.locator('.set').first().waitFor()) },
  // The blurred backdrop over the animated chart canvas varies between loads of the same
  // server: compare the dialog, the page behind it is covered by the macro route itself.
  { path: '/fundamentals', name: 'macro-add', only: '.modal', ready: '.layout', prep: (p) => p.locator('.rail button', { hasText: 'Add series' }).click().then(() => p.locator('.modal .addform').waitFor()) },
  { path: '/goals', name: 'goals-form', ready: '.goals .grid', prep: (p) => p.locator('.head-actions button.primary').click().then(() => p.locator('.goal-form').waitFor()) }
];

if (!skipDesktop) {
  for (const r of [...ROUTES, ...DESKTOP_STATES]) {
    if (only && !only.includes(slug(r))) continue;
    for (const vp of DESKTOPS) {
      // Some pages render a few antialiased pixels differently from one load to the next
      // on the same server (chart canvas, SVG arcs). Up to three loads per server: the
      // check passes when a working-tree frame equals a baseline frame byte for byte.
      const seen = { [baseline]: [], [base]: [] };
      let same = false;
      for (let attempt = 0; attempt < 3 && !same; attempt++) {
        for (const url of [baseline, base]) {
          const { context, page } = await newPage('populated', { ...vp, url });
          try {
            await page.emulateMedia({ reducedMotion: 'reduce' });
            await open(page, r.path, r.ready);
            await r.prep?.(page);
            await page.mouse.move(0, vp.height - 1);
            seen[url].push(await stableShot(page, r.only));
          } finally {
            await context.close();
          }
        }
        same = seen[base].some((w) => seen[baseline].some((b) => b.equals(w)));
      }
      const selfNoise = seen[baseline].length > 1 && !seen[baseline].every((b) => b.equals(seen[baseline][0]));
      if (!same) {
        await mkdir(`${artifacts}/desktop`, { recursive: true });
        const { writeFile } = await import('node:fs/promises');
        await writeFile(`${artifacts}/desktop/${slug(r)}-${vp.width}-baseline.png`, seen[baseline][0]);
        await writeFile(`${artifacts}/desktop/${slug(r)}-${vp.width}-work.png`, seen[base][0]);
      }
      check(same, `${r.name ?? r.path} desktop ${vp.width}x${vp.height}: differs from baseline`);
      console.log(`desktop ${r.name ?? r.path} ${vp.width}x${vp.height}: ${same ? 'identical' : 'DIFFERENT'} (loads per server: ${seen[base].length}${selfNoise ? ', baseline differs from itself between loads' : ''})`);
    }
  }
}

for (const n of notes) console.log(`NOTE: ${n}`);
console.log(failures.length ? `FAIL (${failures.length})\n${failures.join('\n')}` : 'PASS');
await browser.close();
process.exit(failures.length ? 1 : 0);
