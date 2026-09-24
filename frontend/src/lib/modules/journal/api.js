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

  // FX pending tasks. One task is a (date, currency) pair, and each row says what can
  // still resolve it: `histdata_ticker`/`histdata_rate` when a stored series prices it,
  // `can_download` when a connector could go and fetch one. Neither present means the
  // currency has no market anywhere and only a manual rate will do.
  fxPending: () => req('/journal/fx/pending').then((r) => r.pending),
  // { base:'USD', quotes:[...] } — everything the journal is booked in, plus the majors.
  fxQuotes: () => req('/journal/fx/quotes'),
  // Resolve one task off a stored series; rejects when no market prices that currency.
  fxFromHistdata: (date, quote) =>
    req(`/journal/fx/pending/${date}/${quote}/histdata`, { method: 'POST' }),
  // Queue the download of a series that could price the currency.
  fxDownload: (date, quote) =>
    req(`/journal/fx/pending/${date}/${quote}/download`, { method: 'POST' }),
  // Both return an array of { rate_date, quote, rate, source }. `fxRatesAsOf` carries each
  // quote forward from its last known date, so it still answers for weekends, holidays and
  // dates the fetch job hasn't reached yet; `fxRatesOn` is exact-date (resolve UI).
  fxRatesOn: (date) => req(`/journal/fx/rates/${date}`).then((r) => r.rates),
  fxRatesAsOf: (date) => req(`/journal/fx/rates/${date}?asof=true`).then((r) => r.rates),
  fxResolve: (date, rates) =>
    req(`/journal/fx/rates/${date}`, { method: 'POST', body: JSON.stringify({ rates }) }),
  // The market a currency is priced on, stated once and used for every date of it.
  // `fxSourceOptions` returns { source, connectors, datasets }: what it is pointed at now,
  // the connectors that could fetch a pair, and the daily series already in store.
  fxSources: () => req('/journal/fx/sources').then((r) => r.sources),
  fxSourceOptions: (quote) => req(`/journal/fx/sources/${quote}`),
  // body: { connector_id | provider, asset_type, ticker, invert }.
  fxSetSource: (quote, body) =>
    req(`/journal/fx/sources/${quote}`, { method: 'PUT', body: JSON.stringify(body) }),
  fxClearSource: (quote) => req(`/journal/fx/sources/${quote}`, { method: 'DELETE' }),

  // Autocomplete suggestions from previously-used trade values.
  suggestions: () => req('/journal/trade-suggestions'), // { tickers, exchanges, signals }

  // ── Import (trade book from another journal / a broker export) ──
  // `analyze` writes nothing: it reads the file, proposes or applies a mapping, and
  // returns the trades that mapping would create. It is re-called on every mapping
  // edit, so the preview always shows what `commit` will actually write.
  // `section` points a fresh detection at one table of a stacked statement ('' reads
  // the file flat); it is ignored when a mapping is supplied, which carries its own.
  importAnalyze: ({ filename, content, mapping = null, section = null, limit = 25 }) =>
    req('/journal/import/analyze', {
      method: 'POST',
      body: JSON.stringify({ filename, content, mapping, section, limit })
    }).then((r) => r.analysis),
  importCommit: ({ filename, content, mapping, save_as = null }) =>
    req('/journal/import/commit', {
      method: 'POST',
      body: JSON.stringify({ filename, content, mapping, save_as })
    }).then((r) => r.report),

  // ── Import from a broker account (no file) ──
  // Same contract as `importAnalyze`: the preview writes nothing and is exactly what
  // `brokerSyncCommit` will file. The identity of an imported position is its opening
  // execution, so re-pulling a wider period refreshes what it already wrote instead of
  // filing it twice. `last` is what this journal pulled last time, so the modal reopens
  // on the account, the instruments and the window the user left it on.
  brokerSyncLast: (categoryId) =>
    req(`/journal/import/broker/last${filterQs({ category_id: categoryId })}`).then((r) => r.sync),
  brokerSyncPreview: (payload) =>
    req('/journal/import/broker/preview', {
      method: 'POST',
      body: JSON.stringify(payload)
    }).then((r) => r.preview),
  brokerSyncCommit: (payload) =>
    req('/journal/import/broker/commit', {
      method: 'POST',
      body: JSON.stringify(payload)
    }).then((r) => r.report),

  // Saved import mappings (source columns → trade fields). Not journal templates:
  // a template is the form used to log a trade by hand.
  listImportMappings: () => req('/journal/import/mappings').then((r) => r.mappings),
  // Save without importing: a mapping is worth keeping even on a run that gets cancelled.
  // A name that already exists is replaced in place (the UI asks first).
  saveImportMapping: ({ name, headers, mapping }) =>
    req('/journal/import/mappings', {
      method: 'POST',
      body: JSON.stringify({ name, headers, mapping })
    }).then((r) => r.mapping),
  updateImportMapping: (id, patch) =>
    req(`/journal/import/mappings/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteImportMapping: (id) => req(`/journal/import/mappings/${id}`, { method: 'DELETE' }),

  // Import history. Reverting deletes exactly the trades that import created.
  listImportBatches: () => req('/journal/import/batches').then((r) => r.batches),
  revertImportBatch: (id) => req(`/journal/import/batches/${id}/revert`, { method: 'POST' }),
  forgetImportBatch: (id) => req(`/journal/import/batches/${id}`, { method: 'DELETE' }),

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

  // ── Routines on the calendar ──
  // The routines themselves belong to the Trading routines module; a *link* says this
  // book runs one of them from `start_date` to `end_date` (null = unlimited). The day's
  // standing is derived server-side from that module's recurrence and its own ticks.
  routineLinks: (categoryId = '') =>
    req(`/journal/routines/links${filterQs({ category_id: categoryId })}`).then((r) => r.links),
  attachRoutines: (body) =>
    req('/journal/routines/links', { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.links
    ),
  updateRoutineLink: (id, patch) =>
    req(`/journal/routines/links/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  detachRoutine: (id) => req(`/journal/routines/links/${id}`, { method: 'DELETE' }),
  // One month at a time: an unlimited link would otherwise walk the whole book.
  routineStatus: (categoryId, from, to) =>
    req(`/journal/routines/status${filterQs({ category_id: categoryId, from, to })}`).then(
      (r) => r.days
    ),

  // ── Discipline tags (kind: mistake | rule | setup) ──
  // A `mistake` tag is what turns "I over-traded again" into a number: the analytics
  // endpoint prices the tagged trades against the ones logged without any breach.
  listTags: () => req('/journal/tags').then((r) => r.tags),
  addTag: (tag) =>
    req('/journal/tags', { method: 'POST', body: JSON.stringify(tag) }).then((r) => r.tag),
  updateTag: (id, patch) =>
    req(`/journal/tags/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteTag: (id) => req(`/journal/tags/${id}`, { method: 'DELETE' }),

  // Analytics — R-multiples, streaks, Sharpe/Sortino over trading days, hold-time /
  // hour / weekday / size distributions, per-group tables and the cost of mistakes.
  // Same filter set as breakdown; the tz offset buckets hours and days locally.
  analytics: (filter = {}) =>
    req(
      `/journal/analytics${filterQs({ ...filter, tz_offset: new Date().getTimezoneOffset() })}`
    ).then((r) => r.analytics),

  // Period vs period — `period` is day | week | month | quarter | year | custom.
  // `custom` compares the filter's own since/until against the same length just before,
  // so no extra parameter is needed. Returns { current, previous, history }.
  // ── Market data ──
  // The journal reads candles for its own tickers through the shared data broker: no
  // provider code here, and the download itself is an ordinary histdata job.
  marketSettings: () => req('/journal/market/settings'),
  saveMarketSettings: (patch) =>
    req('/journal/market/settings', { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.market
    ),
  /** What the filtered trades need against what the catalog holds. Reads only. */
  marketCoverage: (filter = {}) =>
    req(`/journal/market/coverage${filterQs(filter)}`).then((r) => r.coverage),
  /** Queue the missing windows; the batch id follows on /api/histdata/jobs. */
  marketSync: (filter = {}) =>
    req(`/journal/market/sync${filterQs(filter)}`, { method: 'POST' }).then((r) => r.sync),
  /** Measure the filtered trades against the bars already in store. Incremental by
   *  default: only trades edited since their measurement, or whose bars or grain moved,
   *  are read again. `full` forces the whole scope. */
  marketCompute: (filter = {}, full = false) =>
    req(`/journal/market/compute${filterQs({ ...filter, full: full ? 'true' : '' })}`, {
      method: 'POST'
    }).then((r) => r.compute),

  /** Open-position risk: simultaneous exposure, concentration, correlation. Reads bars
   *  already in store, never fetches. */
  exposure: (filter = {}) =>
    req(`/journal/exposure${filterQs(filter)}`).then((r) => r.exposure),

  compare: (filter = {}, period = 'month', anchor = '') =>
    req(
      `/journal/compare${filterQs({
        ...filter,
        period,
        anchor,
        tz_offset: new Date().getTimezoneOffset()
      })}`
    ).then((r) => r.comparison),

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

/** Tag kinds (mirror of the backend TAG_KINDS whitelist). The kind never changes the
 * math, only how the stat reads: a mistake is a breach, a rule is a habit honoured. */
/** Histdata asset types the journal can source, and the journal classes that map to them.
 *  `option`, `future` and `other` map to nothing: no connector serves them, and matching
 *  an option to its underlying would measure a different instrument. Mirrors
 *  `journal_market::asset_type_for`. */
export const MARKET_ASSET_TYPES = [
  { id: 'equity', classes: ['stock'] },
  { id: 'etf', classes: ['etf'] },
  { id: 'crypto', classes: ['crypto'] },
  { id: 'fx', classes: ['forex'] },
  { id: 'future', classes: ['future'] }
];

/** Grains the download pipeline supports, plus the derive-it-for-me option. */
export const MARKET_TIMEFRAMES = ['auto', '1m', '5m', '15m', '1h', '4h', '1d', '1w'];

export const MARKET_SYNC_MODES = ['off', 'manual', 'auto'];

export const TAG_KINDS = ['mistake', 'rule', 'setup'];

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

/**
 * Where a source column can be mapped when importing a trade book. Mirrors the backend
 * `TARGETS` table. `ignore` drops the column, `field:` keeps it as a custom trade field.
 *
 * In `executions` shape one row is one fill, so three of these read differently — hence
 * the `exec` label — and the two exit fields don't apply at all (`roundtripOnly`).
 */
export const IMPORT_TARGETS = [
  { id: 'ticker', key: 'ticker' },
  { id: 'side', key: 'side', exec: true },
  { id: 'quantity', key: 'quantity' },
  { id: 'entry_price', key: 'entryPrice', exec: true },
  { id: 'exit_price', key: 'exitPrice', roundtripOnly: true },
  { id: 'entry_at', key: 'entryAt', exec: true },
  { id: 'exit_at', key: 'exitAt', roundtripOnly: true },
  { id: 'fees', key: 'fees' },
  { id: 'currency', key: 'currency' },
  { id: 'asset_class', key: 'assetClass' },
  { id: 'unit_type', key: 'unitType' },
  { id: 'exchange', key: 'exchange' },
  { id: 'leverage', key: 'leverage' },
  { id: 'multiplier', key: 'multiplier' },
  { id: 'strategy', key: 'strategy' },
  { id: 'category', key: 'category' },
  { id: 'signal_name', key: 'signalName' },
  { id: 'feedback', key: 'feedback' },
  { id: 'external_id', key: 'externalId' },
  { id: 'pnl_check', key: 'pnlCheck' }
];

/** Read a File as base64 (what the import endpoints expect in the request body). */
export { fileToBase64 } from '$lib/import/file.js';

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
