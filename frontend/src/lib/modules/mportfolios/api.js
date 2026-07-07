/** Managers' Portfolios API client.
 *
 * Reads the cache populated on a schedule from Dataroma's public superinvestor pages. The list
 * endpoint takes optional `q` (name search) and `ticker` (keep only portfolios holding it)
 * filters; the detail endpoint returns one portfolio's holdings. Refresh triggers a fresh scrape
 * server-side. Distinct from the future user "portfolios" module. Attribution: data provided by
 * Dataroma. */
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

export const mportfoliosApi = {
  /** List portfolios; `q` filters by name, `ticker` keeps only holders of that ticker. */
  list: ({ q = '', ticker = '' } = {}) => {
    const p = new URLSearchParams();
    if (q.trim()) p.set('q', q.trim());
    if (ticker.trim()) p.set('ticker', ticker.trim());
    const qs = p.toString();
    return req(`/mportfolios${qs ? `?${qs}` : ''}`);
  },

  /** One portfolio + its holdings. */
  detail: (slug) => req(`/mportfolios/${encodeURIComponent(slug)}`),

  /** Trigger a fresh Dataroma scrape (runs to completion before resolving). */
  refresh: () => req('/mportfolios/refresh', { method: 'POST' }),

  /** Take a snapshot of the live portfolio `slug`. Returns { id }. */
  snapshot: (slug) =>
    req('/mportfolios/snapshots', { method: 'POST', body: JSON.stringify({ slug }) }),

  /** List saved snapshots (newest first), optionally filtered by name. */
  listSnapshots: ({ q = '' } = {}) => {
    const p = new URLSearchParams();
    if (q.trim()) p.set('q', q.trim());
    const qs = p.toString();
    return req(`/mportfolios/snapshots${qs ? `?${qs}` : ''}`);
  },

  /** One snapshot + its frozen holdings. */
  snapshotDetail: (id) => req(`/mportfolios/snapshots/${encodeURIComponent(id)}`),

  /** Delete one snapshot. */
  deleteSnapshot: (id) =>
    req(`/mportfolios/snapshots/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  /** Delete every snapshot for a source portfolio slug. */
  deleteSnapshotsBySlug: (slug) =>
    req(`/mportfolios/snapshots/by-slug/${encodeURIComponent(slug)}`, { method: 'DELETE' })
};

/** Compact USD formatting: $263.1B, $172M. */
export function fmtValue(n, text) {
  if (n == null) return text ?? '—';
  const abs = Math.abs(n);
  if (abs >= 1e9) return `$${(n / 1e9).toFixed(1)}B`;
  if (abs >= 1e6) return `$${(n / 1e6).toFixed(0)}M`;
  if (abs >= 1e3) return `$${(n / 1e3).toFixed(0)}K`;
  return `$${n.toFixed(0)}`;
}

/** Plain number with thousands separators, or — when null. */
export function fmtNum(n, digits = 0) {
  if (n == null) return '—';
  return n.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

export function fmtPrice(n) {
  if (n == null) return '—';
  return `$${n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

export function fmtPct(n) {
  if (n == null) return '—';
  return `${n.toFixed(2)}%`;
}
