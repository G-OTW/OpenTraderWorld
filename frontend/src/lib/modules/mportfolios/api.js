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

// Formatting lives in $lib/format.js. The old local fmtValue put the minus inside the
// symbol ("$-263.1B") and rounded away from zero; fmtCompactMoney fixes both.
import { fmtFixed, fmtMoney } from '$lib/format.js';

export { fmtCompactMoney as fmtValue, fmtPct } from '$lib/format.js';

/** Whole share/unit counts — no decimals. */
export const fmtNum = (n, digits = 0) => fmtFixed(n, digits);

/** Quoted prices are USD by construction here. */
export const fmtPrice = (n) => fmtMoney(n, 'USD');
