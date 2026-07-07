/** Historical Data Visualization API client.
 *
 * Reuses the histdata catalog (datasets) and adds a bars fetch for charting. Bars come
 * back as parallel arrays (ts/o/h/l/c/v) — compact and ready for ECharts. The viz module
 * is read-only: downloading/managing data lives in the Historical Data module. */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, opts = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...opts
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

export const histvizApi = {
  /** Stored datasets catalog (same source as the download module). */
  datasets: () => req('/histdata/datasets').then((r) => r.datasets),

  /** OHLCV for a dataset. opts: { from, to } RFC3339, limit (default server-side). */
  bars: (id, opts = {}) => {
    const qs = new URLSearchParams();
    if (opts.from) qs.set('from', opts.from);
    if (opts.to) qs.set('to', opts.to);
    if (opts.limit) qs.set('limit', String(opts.limit));
    const q = qs.toString();
    return req(`/histdata/datasets/${id}/bars${q ? `?${q}` : ''}`);
  },

  /** Global chart display settings (server-persisted). Returns the stored blob or null. */
  chartSettings: () => req('/histviz/chart-settings'),

  /** Replace the global chart settings with `settings` (plain object). */
  saveChartSettings: (settings) =>
    req('/histviz/chart-settings', { method: 'PUT', body: JSON.stringify(settings) })
};

/** Group datasets by asset_type → ticker for the picker. */
export function groupDatasets(rows) {
  const byType = new Map();
  for (const d of rows) {
    if (!byType.has(d.asset_type)) byType.set(d.asset_type, new Map());
    const byTicker = byType.get(d.asset_type);
    if (!byTicker.has(d.ticker)) byTicker.set(d.ticker, []);
    byTicker.get(d.ticker).push(d);
  }
  return [...byType.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([asset_type, byTicker]) => ({
      asset_type,
      tickers: [...byTicker.entries()]
        .sort((a, b) => a[0].localeCompare(b[0]))
        .map(([ticker, sets]) => ({ ticker, sets }))
    }));
}
