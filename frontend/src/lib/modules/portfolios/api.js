/** Portfolio Tracker API client.
 *
 * Portfolios → assets (resolved provider symbols) → operations ledger. The list endpoint returns
 * per-portfolio totals; the detail endpoint returns assets (with derived position/PnL), the
 * operations list, and valuation snapshots for the chart. Symbol search resolves crypto via
 * CoinGecko and stocks/ETFs via Yahoo. Prices are fetched in USD and converted to each portfolio's
 * display currency server-side. */
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
  if (!res.ok) {
    // Carry the status so callers can tell an expected condition (429 refresh cooldown)
    // from a real failure that belongs in the error block.
    const err = new Error(body?.error ?? `request failed (${res.status})`);
    err.status = res.status;
    throw err;
  }
  return body;
}

export const portfoliosApi = {
  list: () => req('/portfolios').then((r) => r.portfolios),
  create: (body) =>
    req('/portfolios', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.portfolio),
  detail: (id) => req(`/portfolios/${id}`),
  update: (id, patch) =>
    req(`/portfolios/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.portfolio
    ),
  remove: (id) => req(`/portfolios/${id}`, { method: 'DELETE' }),
  refresh: (id) => req(`/portfolios/${id}/refresh`, { method: 'POST' }).then((r) => r.summary),

  /** Check every asset against its price source. Returns { results, unresolved }. */
  reconcile: (id) => req(`/portfolios/${id}/reconcile`, { method: 'POST' }),

  /** Patch an asset's price-source override / reconcile status.
   *  { spot_provider: 'binance'|null, spot_symbol, recon_status }. */
  updateAsset: (assetId, patch) =>
    req(`/portfolios/assets/${assetId}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.asset
    ),

  /** Symbol search. kind: 'crypto' | 'stock' (stock also returns ETFs). */
  search: (kind, q) =>
    req(`/portfolios/search?kind=${kind}&q=${encodeURIComponent(q)}`).then((r) => r.results),

  addAsset: (portfolioId, body) =>
    req(`/portfolios/${portfolioId}/assets`, { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.asset
    ),
  deleteAsset: (assetId) => req(`/portfolios/assets/${assetId}`, { method: 'DELETE' }),

  addOperation: (assetId, body) =>
    req(`/portfolios/assets/${assetId}/operations`, {
      method: 'POST',
      body: JSON.stringify(body)
    }).then((r) => r.operation),

  /** Book a row against the portfolio itself: a deposit, a withdrawal, a fee, a tax, or
   *  income that names its asset in the body. */
  addPortfolioOperation: (portfolioId, body) =>
    req(`/portfolios/${portfolioId}/operations`, {
      method: 'POST',
      body: JSON.stringify(body)
    }).then((r) => r.operation),
  deleteOperation: (opId) => req(`/portfolios/operations/${opId}`, { method: 'DELETE' }),

  // ── Analytics ──
  // One call serves every measure. `blocks` names which analyses to run; the server builds
  // each substrate once for the union of what they need, so asking for the cheap blocks
  // alone costs no market-data read at all.
  analytics: (id, { blocks, window, from, to } = {}) => {
    const q = new URLSearchParams();
    if (blocks?.length) q.set('blocks', blocks.join(','));
    if (window) q.set('window', window);
    if (from) q.set('from', from);
    if (to) q.set('to', to);
    const qs = q.toString();
    return req(`/portfolios/${id}/analytics${qs ? `?${qs}` : ''}`);
  },

  /** The blocks this build serves and the named windows, so the tab bar is not hardcoded. */
  blocks: () => req('/portfolios/blocks'),

  targets: (id) => req(`/portfolios/${id}/targets`).then((r) => r.targets),
  /** Replace the whole target allocation. An empty list turns the drift table off. */
  saveTargets: (id, rows, dimension = 'asset_class') =>
    req(`/portfolios/${id}/targets`, {
      method: 'PUT',
      body: JSON.stringify({ dimension, rows })
    }).then((r) => r.targets),

  scenarios: () => req('/portfolios/scenarios').then((r) => r.scenarios),
  saveScenario: (id, body) =>
    req(id ? `/portfolios/scenarios/${id}` : '/portfolios/scenarios', {
      method: id ? 'PUT' : 'POST',
      body: JSON.stringify(body)
    }).then((r) => r.scenario),
  deleteScenario: (id) => req(`/portfolios/scenarios/${id}`, { method: 'DELETE' }),
  factors: () => req('/portfolios/factors').then((r) => r.factors),

  /** Run a shock. Writes nothing. Pass { scenario_id } | { legs } | { from, to }. */
  stress: (id, body) =>
    req(`/portfolios/${id}/stress`, { method: 'POST', body: JSON.stringify(body) }),

  // ── History (coverage → sync → rebuild, the journal's order) ──
  coverage: (id) => req(`/portfolios/${id}/history/coverage`).then((r) => r.coverage),
  syncHistory: (id) => req(`/portfolios/${id}/history/sync`, { method: 'POST' }).then((r) => r.sync),
  rebuildHistory: (id, from) =>
    req(`/portfolios/${id}/history/rebuild`, {
      method: 'POST',
      body: JSON.stringify({ from: from ?? null })
    }).then((r) => r.rebuild),

  // ── Ledger import ──
  // `analyze` writes nothing and is re-called on every mapping edit; the file rides in
  // the body as base64 each time. `commit` is the only call that writes, and tags every
  // operation with a batch so the whole import can be reverted in one click.
  importAnalyze: (id, body) =>
    req(`/portfolios/${id}/import/analyze`, { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.analysis
    ),
  importCommit: (id, body) =>
    req(`/portfolios/${id}/import/commit`, { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.report
    ),
  listImportBatches: (id) => req(`/portfolios/${id}/import/batches`).then((r) => r.batches),
  revertImportBatch: (batchId) =>
    req(`/portfolios/import/batches/${batchId}/revert`, { method: 'POST' }),
  forgetImportBatch: (batchId) =>
    req(`/portfolios/import/batches/${batchId}`, { method: 'DELETE' }),
  listImportMappings: () => req('/portfolios/import/mappings').then((r) => r.mappings),
  saveImportMapping: (body) =>
    req('/portfolios/import/mappings', { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.mapping
    ),
  deleteImportMapping: (mappingId) =>
    req(`/portfolios/import/mappings/${mappingId}`, { method: 'DELETE' }),

  // ── Broker holdings ──
  // The same two halves over an account instead of a file: `preview` reads what the broker
  // holds and puts it next to the ledger, `commit` writes the operations that align them.
  // The account is read again at commit, so what is written is the broker's number.
  brokerLast: (id) => req(`/portfolios/${id}/import/broker/last`).then((r) => r.last),
  brokerPreview: (id, body) =>
    req(`/portfolios/${id}/import/broker/preview`, {
      method: 'POST',
      body: JSON.stringify(body)
    }),
  brokerCommit: (id, body) =>
    req(`/portfolios/${id}/import/broker/commit`, {
      method: 'POST',
      body: JSON.stringify(body)
    }),
  /** What the venue trades these assets at right now, read with the account's credentials.
   *  Each quote names the market that answered, because an exchange prices BTC in USDT. */
  brokerPrices: (id, body) =>
    req(`/portfolios/${id}/import/broker/prices`, {
      method: 'POST',
      body: JSON.stringify(body)
    })
};

/** Source columns an operations ledger can be mapped onto (mirrors `dict::PORTFOLIO`). */
export const IMPORT_TARGETS = [
  { id: 'op_date', key: 'opDate' },
  { id: 'ticker', key: 'ticker' },
  { id: 'side', key: 'side' },
  { id: 'quantity', key: 'quantity' },
  { id: 'price', key: 'price' },
  { id: 'amount', key: 'amount' },
  { id: 'fee', key: 'fee' },
  { id: 'currency', key: 'currency' },
  { id: 'note', key: 'note' },
  { id: 'external_id', key: 'externalId' }
];

/** Every ledger kind. `trade` moves units and needs a quantity and a unit price; the rest
 *  move cash only and carry an amount. `sign` is what the row does to the cash balance, and
 *  it is what the operations table colors on. */
export const OP_KINDS = [
  { id: 'buy', trade: true, sign: -1 },
  { id: 'sell', trade: true, sign: 1 },
  { id: 'deposit', trade: false, sign: 1 },
  { id: 'withdraw', trade: false, sign: -1 },
  { id: 'dividend', trade: false, sign: 1, income: true, needsAsset: false },
  { id: 'interest', trade: false, sign: 1, income: true },
  { id: 'coupon', trade: false, sign: 1, income: true },
  { id: 'fee', trade: false, sign: -1, cost: true },
  { id: 'tax', trade: false, sign: -1, cost: true }
];

export const opKind = (id) => OP_KINDS.find((k) => k.id === id) ?? OP_KINDS[0];

/** What an asset can be bucketed as. Mirrors `portfolios_api::ASSET_CLASSES`; `cash` is a
 *  drift bucket the ledger produces, never a class an asset can carry. */
export const ASSET_CLASSES = [
  'stock',
  'etf',
  'crypto',
  'bond',
  'commodity',
  'real_estate',
  'alternative'
];

export const DRIFT_BUCKETS = [...ASSET_CLASSES, 'cash'];

/** Currencies offered for a portfolio's display currency (must be FX-covered by the journal job). */
export const CURRENCIES = ['USD', 'EUR', 'GBP', 'JPY', 'CHF', 'CAD', 'AUD', 'CNY'];

/** Price sources selectable per asset class in the reconcile modal. First entry = default provider.
 *  Crypto quotes on the exchanges are USDT/USD ≈ USD; the user types the exact provider ticker. */
export const SPOT_SOURCES = {
  crypto: [
    { id: 'coingecko', label: 'CoinGecko', hint: 'coin id (e.g. bitcoin)' },
    { id: 'binance', label: 'Binance', hint: 'pair (e.g. BTCUSDT)' },
    { id: 'kraken', label: 'Kraken', hint: 'pair (e.g. XBTUSDT)' },
    { id: 'coinbase', label: 'Coinbase', hint: 'product (e.g. BTC-USD)' }
  ],
  stock: [{ id: 'yahoo', label: 'Yahoo', hint: 'ticker (e.g. AAPL)' }],
  etf: [{ id: 'yahoo', label: 'Yahoo', hint: 'ticker (e.g. SPY)' }]
};

// Formatting lives in $lib/format.js. The local SYMBOLS table went with it — it mapped
// JPY and CNY both to "¥", dropped the symbol entirely for any currency it didn't list,
// and forced 2 decimals on JPY, which has no minor unit.
//
// This module's `fmtPct` always printed a sign, so it maps to fmtSignedPct (the shared
// fmtPct does not). `fmtNum` keeps its 4-digit default for quantity columns.
import { fmtNum as fmtNumShared } from '$lib/format.js';

export { fmtMoney, fmtSignedMoney, fmtSignedPct as fmtPct } from '$lib/format.js';

/** Asset quantities want more precision than the shared 2-digit default. */
export const fmtNum = (n, digits = 4) => fmtNumShared(n, digits);

/** Percent gain of market value over cost basis, or null when no basis. */
export function gainPct(marketValue, costBasis) {
  if (!costBasis) return null;
  return ((marketValue - costBasis) / costBasis) * 100;
}
