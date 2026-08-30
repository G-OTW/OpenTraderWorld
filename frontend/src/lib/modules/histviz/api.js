/** Historical Data Visualization API client.
 *
 * Bars come back as parallel arrays (ts/o/h/l/c/v) — compact and ready for ECharts.
 *
 * `series` is what the chart uses: one window of an instrument, stored bars plus only the
 * missing edges fetched through a connector, storing nothing. `symbols` finds the
 * instrument in the first place. Bars become permanent only through `savePreview`, which
 * queues the normal download job (consolidating with any bars already stored).
 * `bars`/`preview` remain for dataset-addressed and trailing-window reads. */
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
    req('/histviz/chart-settings', { method: 'PUT', body: JSON.stringify(settings) }),

  /** Chart layout of one dataset — indicators, drawings, plot style. Null when never saved. */
  layout: (datasetId) => req(`/histviz/layouts/${datasetId}`),

  /** Replace that dataset's layout. */
  saveLayout: (datasetId, layout) =>
    req(`/histviz/layouts/${datasetId}`, { method: 'PUT', body: JSON.stringify(layout) }),

  /** Provider capability matrix (used to know which providers have a live WS feed). */
  providers: () => req('/connectors/providers').then((r) => r.providers),

  /** Fetch bars through a connector without storing them.
   *  coords: { connector_id?, provider?, asset_type, ticker, timeframe, bars? } */
  preview: (coords) =>
    req('/histdata/preview', { method: 'POST', body: JSON.stringify(coords) }),

  /** One chart slice: stored bars + only the missing edges fetched from the provider.
   *  coords: { connector_id?, provider?, asset_type, ticker, timeframe, to?, bars? }.
   *  Walk into the past by passing the oldest loaded timestamp as `to`. */
  series: (coords) => req('/histviz/series', { method: 'POST', body: JSON.stringify(coords) }),

  /** Instrument lookup across the connectors granted to the chart.
   *  opts: { q, asset_type, connectors: [id], limit } */
  symbols: (opts = {}) => {
    const qs = new URLSearchParams();
    if (opts.q) qs.set('q', opts.q);
    if (opts.asset_type) qs.set('asset_type', opts.asset_type);
    if (opts.connectors?.length) qs.set('connectors', opts.connectors.join(','));
    if (opts.limit) qs.set('limit', String(opts.limit));
    const q = qs.toString();
    return req(`/histdata/symbols${q ? `?${q}` : ''}`);
  },

  /** Persist a window (queues a download job). coords + { from, to } or { bars }. */
  savePreview: (coords) =>
    req('/histdata/preview/save', { method: 'POST', body: JSON.stringify(coords) }),

  /** SSE URL for an instrument's live bar stream (open with `new EventSource(...)`).
   *  Addressed by coordinates: watching stores nothing, and if the instrument happens to be
   *  stored the server records the same bars into it. */
  streamUrl: ({ provider, asset_type, ticker, timeframe }) => {
    const qs = new URLSearchParams({ provider, asset_type, ticker, timeframe });
    return `/api/histviz/stream?${qs}`;
  }
};
