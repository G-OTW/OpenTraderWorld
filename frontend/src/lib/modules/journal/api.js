/** Trading Journal API client — categories, capital, strategies, templates, trades, breakdown. Single-user. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

function filterQs(filter = {}) {
  const qs = new URLSearchParams();
  for (const [k, v] of Object.entries(filter)) {
    if (v !== undefined && v !== null && v !== '') qs.set(k, v);
  }
  const q = qs.toString();
  return q ? `?${q}` : '';
}

/** Fetch a file endpoint and hand it to the browser as a download. */
async function downloadFile(path, fallbackName) {
  const res = await fetch(`/api${path}`);
  redirectIfUnauthorized(res);
  if (!res.ok) {
    let body = null;
    try {
      body = await res.json();
    } catch {
      /* not JSON */
    }
    throw new Error(body?.error ?? `request failed (${res.status})`);
  }
  const blob = await res.blob();
  const name =
    res.headers.get('content-disposition')?.match(/filename="([^"]+)"/)?.[1] ?? fallbackName;
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
  return name;
}

export const journalApi = {
  // Categories
  listCategories: () => req('/journal/categories').then((r) => r.categories),
  addCategory: (name, color = null) =>
    req('/journal/categories', { method: 'POST', body: JSON.stringify({ name, color }) }).then(
      (r) => r.category
    ),
  updateCategory: (id, patch) =>
    req(`/journal/categories/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteCategory: (id) => req(`/journal/categories/${id}`, { method: 'DELETE' }),

  // Capital events
  listCapital: (categoryId) =>
    req(`/journal/categories/${categoryId}/capital`).then((r) => r.events),
  addCapital: (categoryId, event) =>
    req(`/journal/categories/${categoryId}/capital`, {
      method: 'POST',
      body: JSON.stringify(event)
    }).then((r) => r.event),
  deleteCapital: (id) => req(`/journal/capital/${id}`, { method: 'DELETE' }),

  // Strategies
  listStrategies: () => req('/journal/strategies').then((r) => r.strategies),
  addStrategy: (strategy) =>
    req('/journal/strategies', { method: 'POST', body: JSON.stringify(strategy) }).then(
      (r) => r.strategy
    ),
  updateStrategy: (id, patch) =>
    req(`/journal/strategies/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteStrategy: (id) => req(`/journal/strategies/${id}`, { method: 'DELETE' }),

  // Templates
  listTemplates: () => req('/journal/templates').then((r) => r.templates),
  addTemplate: (template) =>
    req('/journal/templates', { method: 'POST', body: JSON.stringify(template) }).then(
      (r) => r.template
    ),
  updateTemplate: (id, patch) =>
    req(`/journal/templates/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteTemplate: (id) => req(`/journal/templates/${id}`, { method: 'DELETE' }),

  // Trades
  listTrades: (filter = {}) => {
    const qs = new URLSearchParams();
    for (const [k, v] of Object.entries(filter)) {
      if (v !== undefined && v !== null && v !== '') qs.set(k, v);
    }
    const q = qs.toString();
    return req(`/journal/trades${q ? `?${q}` : ''}`).then((r) => r.trades);
  },
  getTrade: (id) => req(`/journal/trades/${id}`).then((r) => r.trade),
  addTrade: (trade) =>
    req('/journal/trades', { method: 'POST', body: JSON.stringify(trade) }).then((r) => r.trade),
  updateTrade: (id, trade) =>
    req(`/journal/trades/${id}`, { method: 'PATCH', body: JSON.stringify(trade) }),
  deleteTrade: (id) => req(`/journal/trades/${id}`, { method: 'DELETE' }),

  // Fee schedules — reusable fee templates applied to a trade as a shortcut.
  listFeeSchedules: () => req('/journal/fee-schedules').then((r) => r.fee_schedules),
  addFeeSchedule: (schedule) =>
    req('/journal/fee-schedules', { method: 'POST', body: JSON.stringify(schedule) }).then(
      (r) => r.fee_schedule
    ),
  updateFeeSchedule: (id, patch) =>
    req(`/journal/fee-schedules/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteFeeSchedule: (id) => req(`/journal/fee-schedules/${id}`, { method: 'DELETE' }),

  // Journal settings (display currency for the breakdown).
  getSettings: () => req('/journal/settings').then((r) => r.settings),
  updateSettings: (patch) =>
    req('/journal/settings', { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.settings
    ),

  // FX pending tasks (dates no source could supply) + manual rate entry to resolve them.
  fxPending: () => req('/journal/fx/pending').then((r) => r.pending),
  fxQuotes: () => req('/journal/fx/quotes'), // { base:'USD', quotes:[...] }
  fxRatesOn: (date) => req(`/journal/fx/rates/${date}`).then((r) => r.rates),
  fxResolve: (date, rates) =>
    req(`/journal/fx/rates/${date}`, { method: 'POST', body: JSON.stringify({ rates }) }),

  // Autocomplete suggestions from previously-used trade values.
  suggestions: () => req('/journal/trade-suggestions'), // { tickers, exchanges, signals }

  // Exports — trades CSV (same filter set as listTrades) and the periodic
  // performance report ({ period: 'week'|'month', anchor: 'YYYY-MM-DD',
  // format: 'md'|'pdf', ...filters }). Both trigger a browser download.
  exportTradesCsv: (filter = {}) =>
    downloadFile(`/journal/export/trades.csv${filterQs(filter)}`, 'journal_trades.csv'),
  exportReport: (params = {}) =>
    downloadFile(`/journal/report${filterQs(params)}`, `journal_report.${params.format ?? 'md'}`),

  // Calendar — daily realized-PnL buckets for the month-grid heatmap. Same filter
  // set as breakdown, plus the viewer's tz offset so days are bucketed in local time
  // (matching what the Trades date filter shows). Returns { days: [{ date, net_pnl,
  // trades, wins, losses }], display_currency }.
  calendar: (filter = {}) =>
    req(`/journal/calendar${filterQs({ ...filter, tz_offset: new Date().getTimezoneOffset() })}`),

  // Breakdown — accepts the same filter set as listTrades
  // ({ category_id, strategy_id, asset_class, side, ticker, signal_name, since, until }).
  breakdown: (filter = {}) => {
    const qs = new URLSearchParams();
    for (const [k, v] of Object.entries(filter)) {
      if (v !== undefined && v !== null && v !== '') qs.set(k, v);
    }
    const q = qs.toString();
    return req(`/journal/breakdown${q ? `?${q}` : ''}`).then((r) => r.breakdown);
  }
};

/** Asset classes (mirror of the backend ASSET_CLASSES whitelist). */
export const ASSET_CLASSES = [
  { id: 'stock', label: 'Stock' },
  { id: 'option', label: 'Option' },
  { id: 'crypto', label: 'Crypto' },
  { id: 'etf', label: 'ETF' },
  { id: 'future', label: 'Future' },
  { id: 'forex', label: 'Forex' },
  { id: 'other', label: 'Other' }
];

/** Supported currencies (12 majors; mirror of the backend CURRENCIES whitelist). */
export const CURRENCIES = [
  { id: 'USD', label: 'USD — US Dollar' },
  { id: 'EUR', label: 'EUR — Euro' },
  { id: 'GBP', label: 'GBP — British Pound' },
  { id: 'JPY', label: 'JPY — Japanese Yen' },
  { id: 'CNY', label: 'CNY — Renminbi' },
  { id: 'CHF', label: 'CHF — Swiss Franc' },
  { id: 'CAD', label: 'CAD — Canadian Dollar' },
  { id: 'AUD', label: 'AUD — Australian Dollar' },
  { id: 'HKD', label: 'HKD — Hong Kong Dollar' },
  { id: 'SEK', label: 'SEK — Swedish Krona' },
  { id: 'NOK', label: 'NOK — Norwegian Krone' },
  { id: 'DKK', label: 'DKK — Danish Krone' }
];

/** What `quantity` counts on a trade (mirror of the backend UNIT_TYPES whitelist). */
export const UNIT_TYPES = [
  { id: 'unit', label: 'Unit' },
  { id: 'share', label: 'Share' },
  { id: 'lot', label: 'Lot' },
  { id: 'contract', label: 'Contract' }
];

/** Fee-schedule rate kinds and how they are charged. */
export const FEE_AMOUNT_KINDS = [
  { id: 'fixed', label: 'Fixed amount' },
  { id: 'pct', label: 'Percentage' }
];
export const FEE_PER = [
  { id: 'trade', label: 'Per trade' },
  { id: 'lot', label: 'Per lot' },
  { id: 'unit', label: 'Per unit' },
  { id: 'contract', label: 'Per contract' }
];

/** Reserved fields a template may bind to (feed the typed trade columns used for stats). */
export const RESERVED_FIELDS = [
  { reserved: 'ticker', label: 'Ticker', type: 'text' },
  { reserved: 'asset_class', label: 'Category', type: 'select' },
  { reserved: 'exchange', label: 'Exchange', type: 'text' },
  { reserved: 'side', label: 'Side', type: 'select' },
  { reserved: 'currency', label: 'Currency', type: 'select' },
  { reserved: 'unit_type', label: 'Unit type', type: 'select' },
  { reserved: 'fee_schedule_id', label: 'Fee schedule', type: 'select' },
  { reserved: 'entry_at', label: 'Entry time', type: 'datetime' },
  { reserved: 'exit_at', label: 'Exit time', type: 'datetime' },
  { reserved: 'entry_price', label: 'Entry price', type: 'number' },
  { reserved: 'exit_price', label: 'Exit price', type: 'number' },
  { reserved: 'quantity', label: 'Quantity', type: 'number' },
  { reserved: 'fees', label: 'Fees', type: 'number' },
  { reserved: 'leverage', label: 'Leverage', type: 'number' },
  { reserved: 'multiplier', label: 'Multiplier', type: 'number' },
  { reserved: 'signal_name', label: 'Signal name', type: 'text' },
  { reserved: 'feedback', label: 'My feedback', type: 'textarea' },
  { reserved: 'images', label: 'Images (up to 2)', type: 'images' }
];

/** Custom field types for the from-scratch template grid. */
export const CUSTOM_FIELD_TYPES = [
  { id: 'text', label: 'Text' },
  { id: 'textarea', label: 'Long text' },
  { id: 'number', label: 'Number' },
  { id: 'select', label: 'Select' },
  { id: 'date', label: 'Date' },
  { id: 'datetime', label: 'Date & time' },
  { id: 'checkbox', label: 'Checkbox' },
  { id: 'url', label: 'URL' }
];

/** The prebuilt template described in the spec, used to seed the "standard" form. */
export const PREBUILT_TEMPLATE = {
  name: 'Standard trade',
  description: 'Ticker, category, side, entry/exit, fees, leverage, strategy, feedback, images.',
  fields: [
    { key: 'ticker', label: 'Ticker', type: 'text', reserved: 'ticker' },
    { key: 'asset_class', label: 'Category', type: 'select', reserved: 'asset_class' },
    { key: 'exchange', label: 'Exchange', type: 'text', reserved: 'exchange' },
    { key: 'side', label: 'Side', type: 'select', reserved: 'side' },
    { key: 'currency', label: 'Currency', type: 'select', reserved: 'currency' },
    { key: 'unit_type', label: 'Unit type', type: 'select', reserved: 'unit_type' },
    { key: 'fee_schedule_id', label: 'Fee schedule', type: 'select', reserved: 'fee_schedule_id' },
    { key: 'entry_at', label: 'Entry time', type: 'datetime', reserved: 'entry_at' },
    { key: 'exit_at', label: 'Exit time', type: 'datetime', reserved: 'exit_at' },
    { key: 'entry_price', label: 'Entry price', type: 'number', reserved: 'entry_price' },
    { key: 'exit_price', label: 'Exit price', type: 'number', reserved: 'exit_price' },
    { key: 'quantity', label: 'Quantity', type: 'number', reserved: 'quantity' },
    { key: 'fees', label: 'Fees', type: 'number', reserved: 'fees' },
    { key: 'leverage', label: 'Leverage', type: 'number', reserved: 'leverage' },
    { key: 'signal_name', label: 'Signal name', type: 'text', reserved: 'signal_name' },
    { key: 'feedback', label: 'My feedback', type: 'textarea', reserved: 'feedback' },
    { key: 'images', label: 'Images', type: 'images', reserved: 'images' }
  ]
};

export function shortId() {
  return Math.random().toString(36).slice(2, 8);
}

/**
 * Fee a schedule charges for a trade of `qty` units at `avgPrice`. Mirrors the backend
 * `compute_fee`: `per === 'trade'` charges the amount once; otherwise fixed → amount × qty,
 * pct → amount% of notional (avgPrice × qty). Used for the live preview in the trade form;
 * the resulting number is written into the trade's `fees` and can be overridden manually.
 */
export function computeFee(schedule, qty, avgPrice) {
  if (!schedule) return 0;
  const q = Math.abs(Number(qty) || 0);
  const p = Math.abs(Number(avgPrice) || 0);
  const amt = Number(schedule.amount) || 0;
  let raw;
  if (schedule.per === 'trade') {
    raw = schedule.amount_kind === 'pct' ? (p * q * amt) / 100 : amt;
  } else {
    raw = schedule.amount_kind === 'pct' ? (p * q * amt) / 100 : amt * q;
  }
  // Mirror the backend: percentage fees keep 6 decimals, currency amounts 4 — both
  // strip floating-point noise so the previewed/stored fee is clean.
  return roundDp(raw, schedule.amount_kind === 'pct' ? 6 : 4);
}

/** Round to `dp` decimals (half-away-from-zero), stripping float noise. Mirrors backend round_dp. */
export function roundDp(v, dp) {
  const n = Number(v);
  if (!Number.isFinite(n)) return n;
  const f = 10 ** dp;
  return Math.round(n * f) / f;
}

// Formatting lives in $lib/format.js. Journal's percentages are already percentages
// (5 means 5%), so it takes fmtPct — not quant's fmtRatioPct.
//
// `fmtNum` maps to fmtFixed: the old local version used toFixed, keeping trailing zeros
// ("1.50"). Its two call sites are profit factor and Sharpe, where the shared fmtNum's
// "1.5" would sit ragged next to its neighbours. fmtFixed keeps the zeros and adds the
// thousands separator the old one lacked.
export { fmtMoney, fmtSignedMoney, fmtPct, fmtFixed as fmtNum } from '$lib/format.js';
