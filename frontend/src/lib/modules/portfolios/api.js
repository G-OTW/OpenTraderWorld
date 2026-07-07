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
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
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
  deleteOperation: (opId) => req(`/portfolios/operations/${opId}`, { method: 'DELETE' })
};

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

const SYMBOLS = {
  USD: '$',
  EUR: '€',
  GBP: '£',
  JPY: '¥',
  CHF: 'CHF ',
  CAD: 'C$',
  AUD: 'A$',
  CNY: '¥'
};

export function fmtMoney(n, ccy = 'USD', digits = 2) {
  if (n == null || Number.isNaN(n)) return '—';
  const sym = SYMBOLS[ccy] ?? '';
  return `${sym}${n.toLocaleString(undefined, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits
  })}`;
}

export function fmtNum(n, digits = 4) {
  if (n == null || Number.isNaN(n)) return '—';
  return n.toLocaleString(undefined, { maximumFractionDigits: digits });
}

export function fmtPct(n) {
  if (n == null || Number.isNaN(n)) return '—';
  const s = n >= 0 ? '+' : '';
  return `${s}${n.toFixed(2)}%`;
}

/** Percent gain of market value over cost basis, or null when no basis. */
export function gainPct(marketValue, costBasis) {
  if (!costBasis) return null;
  return ((marketValue - costBasis) / costBasis) * 100;
}
