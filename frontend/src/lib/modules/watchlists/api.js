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

  /** Price alerts. Evaluated server-side on every refresh — they fire with no page open. */
  addAlert: (itemId, alert) =>
    req(`/watchlists/items/${itemId}/alerts`, {
      method: 'POST',
      body: JSON.stringify(alert)
    }).then((r) => r.alert),
  updateAlert: (alertId, alert) =>
    req(`/watchlists/alerts/${alertId}`, { method: 'PATCH', body: JSON.stringify(alert) }).then(
      (r) => r.alert
    ),
  removeAlert: (alertId) => req(`/watchlists/alerts/${alertId}`, { method: 'DELETE' }),

  /** Copy a portfolio's assets onto the list (reconciles duplicates). Returns fresh items. */
  importPortfolio: (watchlistId, portfolioId) =>
    req(`/watchlists/${watchlistId}/import`, {
      method: 'POST',
      body: JSON.stringify({ portfolio_id: portfolioId })
    }),

  /** Portfolio picker source for the import flow (Portfolio Tracker's list endpoint). */
  portfolios: () => req('/portfolios').then((r) => r.portfolios),

  /** Data-broker connectors granted to Watchlists, offered as custom quote sources per
   * list or per symbol. Managing them lives in $lib/connectors. */
  connectors: () => req('/connectors?module=watchlists').then((r) => r.connectors)
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

/** "3m ago" in the app locale (not the browser's), or null for a missing timestamp. */
export function agoLabel(iso, locale) {
  if (!iso) return null;
  const secs = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  const rtf = new Intl.RelativeTimeFormat(locale || 'en', { numeric: 'auto', style: 'narrow' });
  if (secs < 60) return rtf.format(0, 'minute');
  if (secs < 3600) return rtf.format(-Math.round(secs / 60), 'minute');
  if (secs < 86400) return rtf.format(-Math.round(secs / 3600), 'hour');
  return rtf.format(-Math.round(secs / 86400), 'day');
}

// ── Alerts ────────────────────────────────────────────────────────────────────

/** What an alert watches. Mirrors the `metric` CHECK in migration 0087. */
export const ALERT_METRICS = [
  { id: 'price', key: 'watchlists.alerts.metric.price' },
  { id: 'pct', key: 'watchlists.alerts.metric.pct' },
  { id: 'usd', key: 'watchlists.alerts.metric.usd' }
];

/** Directions valid per metric — a price is crossed, a variation can also go either way. */
export const ALERT_DIRECTIONS = {
  price: [
    { id: 'above', key: 'watchlists.alerts.dir.above' },
    { id: 'below', key: 'watchlists.alerts.dir.below' }
  ],
  move: [
    { id: 'up', key: 'watchlists.alerts.dir.up' },
    { id: 'down', key: 'watchlists.alerts.dir.down' },
    { id: 'move', key: 'watchlists.alerts.dir.move' }
  ]
};

export const directionsFor = (metric) =>
  metric === 'price' ? ALERT_DIRECTIONS.price : ALERT_DIRECTIONS.move;

/** Window presets offered for both bases. `0` = no deadline (anchor only). */
export const ALERT_WINDOWS = [
  { secs: 3600, key: 'watchlists.alerts.window.1h' },
  { secs: 14400, key: 'watchlists.alerts.window.4h' },
  { secs: 43200, key: 'watchlists.alerts.window.12h' },
  { secs: 86400, key: 'watchlists.alerts.window.1d' },
  { secs: 259200, key: 'watchlists.alerts.window.3d' },
  { secs: 604800, key: 'watchlists.alerts.window.7d' },
  { secs: 2592000, key: 'watchlists.alerts.window.30d' }
];

/** Re-arm delay after a repeating alert fires, so one move can't spam every refresh. */
export const ALERT_COOLDOWNS = [
  { secs: 300, key: 'watchlists.interval.5m' },
  { secs: 900, key: 'watchlists.interval.15m' },
  { secs: 3600, key: 'watchlists.interval.1h' },
  { secs: 14400, key: 'watchlists.interval.4h' },
  { secs: 86400, key: 'watchlists.interval.1d' }
];

/** A blank alert, defaulted to the question people actually ask most: "+5% from now". */
export function newAlert() {
  return {
    metric: 'pct',
    direction: 'move',
    threshold: 5,
    basis: 'anchor',
    window_secs: 0,
    repeat: false,
    cooldown_secs: 3600,
    enabled: true,
    channel_ids: [],
    note: ''
  };
}

/** Only the fields the API accepts, with the numbers coerced. */
export function alertPayload(a) {
  return {
    metric: a.metric,
    direction: a.direction,
    threshold: Number(a.threshold),
    // A price threshold has nothing to measure *from*, so it is always an anchor.
    basis: a.metric === 'price' ? 'anchor' : a.basis,
    window_secs: Number(a.window_secs) || 0,
    repeat: !!a.repeat,
    cooldown_secs: Number(a.cooldown_secs) || 0,
    enabled: !!a.enabled,
    channel_ids: [...(a.channel_ids ?? [])],
    note: (a.note ?? '').trim()
  };
}

/**
 * Client-side mirror of the server's `validate_alert`, so the form can say what is wrong
 * before a round-trip. Returns an i18n key, or null when the alert is sendable.
 */
export function alertError(a) {
  const n = Number(a.threshold);
  if (!isFinite(n) || n <= 0) return 'watchlists.alerts.err.threshold';
  if (a.metric !== 'price' && a.basis === 'rolling' && !(Number(a.window_secs) > 0))
    return 'watchlists.alerts.err.window';
  return null;
}

/** "Above $120" / "±5% over 24h" — one line describing what an alert waits for. */
export function alertSummary(a, tr) {
  const amount =
    a.metric === 'pct' ? `${trimNum(a.threshold)}%` : fmtQuote(a.threshold);
  const dir = tr(`watchlists.alerts.dir.${a.direction}`);
  const head = `${dir} ${amount}`;
  if (a.metric === 'price') return head;
  const win = windowLabel(a.window_secs, tr);
  if (a.basis === 'rolling') return tr('watchlists.alerts.sum.rolling', { cond: head, window: win });
  return a.window_secs > 0
    ? tr('watchlists.alerts.sum.anchorWindow', { cond: head, window: win })
    : tr('watchlists.alerts.sum.anchor', { cond: head });
}

/** A duration in seconds as the preset's own label, falling back to a plain hour/day count. */
export function windowLabel(secs, tr) {
  const n = Number(secs) || 0;
  const preset = ALERT_WINDOWS.find((w) => w.secs === n) ?? ALERT_COOLDOWNS.find((w) => w.secs === n);
  if (preset) return tr(preset.key);
  if (n % 86400 === 0) return `${n / 86400}d`;
  if (n % 3600 === 0) return `${n / 3600}h`;
  return `${Math.round(n / 60)}m`;
}

/** Two decimals at most, without trailing zeros — matches the Rust `trim_num`. */
export function trimNum(n) {
  return String(Number(Number(n).toFixed(2)));
}
