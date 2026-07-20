/** Watchlists API client.
 *
 * Watchlists → items (pinned provider symbols with a cached quote: USD price, 24h/3d/7d/30d
 * changes, 30-day sparkline). Symbol search resolves crypto via CoinGecko and stocks/ETFs via
 * Yahoo, same scheme as the Portfolio Tracker. Lists can be seeded from a curated template or
 * imported from a portfolio (idempotent — re-importing reconciles). Sync-enabled lists are
 * re-quoted server-side on their own interval; the client only re-reads its own database. */
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

export const watchlistsApi = {
  list: () => req('/watchlists').then((r) => r.watchlists),
  /** body: { name, description?, template? } — template seeds and quotes the list inline. */
  create: (body) =>
    req('/watchlists', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.watchlist),
  detail: (id) => req(`/watchlists/${id}`),
  update: (id, patch) =>
    req(`/watchlists/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.watchlist
    ),
  remove: (id) => req(`/watchlists/${id}`, { method: 'DELETE' }),
  /** Re-quote every item now. Returns the fresh { watchlist, items }. */
  refresh: (id) => req(`/watchlists/${id}/refresh`, { method: 'POST' }),

  templates: () => req('/watchlists/templates').then((r) => r.templates),

  /** Symbol search. kind: 'crypto' | 'stock' (stock also returns ETFs). */
  search: (kind, q) =>
    req(`/watchlists/search?kind=${kind}&q=${encodeURIComponent(q)}`).then((r) => r.results),

  addItem: (watchlistId, body) =>
    req(`/watchlists/${watchlistId}/items`, { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.item
    ),
  updateItem: (itemId, patch) =>
    req(`/watchlists/items/${itemId}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.item
    ),
  removeItem: (itemId) => req(`/watchlists/items/${itemId}`, { method: 'DELETE' }),

  /** Copy a portfolio's assets onto the list (reconciles duplicates). Returns fresh items. */
  importPortfolio: (watchlistId, portfolioId) =>
    req(`/watchlists/${watchlistId}/import`, {
      method: 'POST',
      body: JSON.stringify({ portfolio_id: portfolioId })
    }),

  /** Portfolio picker source for the import flow (Portfolio Tracker's list endpoint). */
  portfolios: () => req('/portfolios').then((r) => r.portfolios),

  /** The module's own provider connectors (scoped; distinct from Historical Data's list),
   * offered as custom quote sources per list or per symbol. */
  connectors: () => req('/histdata/connectors?scope=watchlists').then((r) => r.connectors)
};

/** Auto-sync cadences offered per list. Label keys live in i18n (watchlists.interval.*). */
export const REFRESH_OPTIONS = [
  { secs: 60, key: 'watchlists.interval.1m' },
  { secs: 300, key: 'watchlists.interval.5m' },
  { secs: 900, key: 'watchlists.interval.15m' },
  { secs: 1800, key: 'watchlists.interval.30m' },
  { secs: 3600, key: 'watchlists.interval.1h' },
  { secs: 14400, key: 'watchlists.interval.4h' },
  { secs: 86400, key: 'watchlists.interval.1d' }
];

/** Sub-minute cadences, unlocked only when the list quotes through the user's own connector. */
export const FAST_REFRESH_OPTIONS = [
  { secs: 5, key: 'watchlists.interval.5s' },
  { secs: 10, key: 'watchlists.interval.10s' },
  { secs: 30, key: 'watchlists.interval.30s' }
];

/**
 * Estimated provider requests per minute for a list. Auto-sourced items: one Yahoo call per
 * stock/ETF plus a single batched CoinGecko call for all crypto (the daily-history refetch
 * is amortized to ~4/day and ignored). Connector-sourced items (per `isCustom`) cost one
 * call each per refresh. Drives the "you may get rate-limited" warning.
 */
export function estimatedReqPerMin(items, refreshSecs, isCustom = () => false) {
  if (!items?.length || !refreshSecs) return 0;
  const custom = items.filter(isCustom).length;
  const auto = items.filter((i) => !isCustom(i));
  const yahoo = auto.filter((i) => i.provider === 'yahoo').length;
  const crypto = auto.length - yahoo;
  const perRefresh = custom + yahoo + (crypto > 0 ? 1 : 0);
  return (perRefresh * 60) / refreshSecs;
}

/** Above this estimated req/min the UI warns that free APIs may throttle. */
export const RATE_WARN_PER_MIN = 10;

import { fmtMoney, fmtSignedPct, EM_DASH } from '$lib/format.js';

export { fmtSignedPct, EM_DASH };

/** USD quote with precision that follows the magnitude (sub-dollar coins need decimals). */
export function fmtQuote(n) {
  if (n == null || !isFinite(n)) return EM_DASH;
  const abs = Math.abs(n);
  const digits = abs >= 1 ? 2 : abs >= 0.01 ? 4 : 6;
  return fmtMoney(n, 'USD', digits);
}

/** "3m ago" in the user's locale, or null for a missing timestamp. */
export function agoLabel(iso) {
  if (!iso) return null;
  const secs = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto', style: 'narrow' });
  if (secs < 60) return rtf.format(0, 'minute');
  if (secs < 3600) return rtf.format(-Math.round(secs / 60), 'minute');
  if (secs < 86400) return rtf.format(-Math.round(secs / 3600), 'hour');
  return rtf.format(-Math.round(secs / 86400), 'day');
}
