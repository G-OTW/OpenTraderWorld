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

  /** Layout of an *instrument*, stored or not. Coordinates rather than a dataset id: a
   *  symbol you are only looking at keeps its indicators and drawings too. */
  layoutOf: ({ provider, asset_type, ticker, timeframe }) =>
    req(
      `/histviz/layout?${new URLSearchParams({ provider, asset_type, ticker, timeframe })}`
    ),

  /** Replace it. */
  saveLayoutOf: ({ provider, asset_type, ticker, timeframe }, layout) =>
    req('/histviz/layout', {
      method: 'PUT',
      body: JSON.stringify({ provider, asset_type, ticker, timeframe, layout })
    }),

  // ── Workspaces: the grid of panes ──
  workspaces: () => req('/histviz/workspaces').then((r) => r.workspaces),
  workspace: (id) => req(`/histviz/workspaces/${id}`),
  createWorkspace: (w) => req('/histviz/workspaces', { method: 'POST', body: JSON.stringify(w) }),
  saveWorkspace: (id, w) =>
    req(`/histviz/workspaces/${id}`, { method: 'PUT', body: JSON.stringify(w) }),
  deleteWorkspace: (id) => req(`/histviz/workspaces/${id}`, { method: 'DELETE' }),

  // ── Chart lists: the rail's own source, next to the Watchlists module's ──
  lists: () => req('/histviz/lists').then((r) => r.lists),
  createList: (l) => req('/histviz/lists', { method: 'POST', body: JSON.stringify(l) }),
  saveList: (id, l) => req(`/histviz/lists/${id}`, { method: 'PUT', body: JSON.stringify(l) }),
  deleteList: (id) => req(`/histviz/lists/${id}`, { method: 'DELETE' }),
  /** Copy a chart list into the Watchlists module. Explicit: charting never writes there. */
  promoteList: (id, name) =>
    req(`/histviz/lists/${id}/promote`, { method: 'POST', body: JSON.stringify({ name }) }),

  // ── Alerts: a level the server watches on closed bars, with no browser open ──
  /** Every alert, or one instrument's when the coordinates are given. */
  alerts: (instrument = null) => {
    const qs = instrument
      ? `?${new URLSearchParams({
          provider: instrument.provider,
          asset_type: instrument.asset_type,
          ticker: instrument.ticker,
          timeframe: instrument.timeframe
        })}`
      : '';
    return req(`/histviz/alerts${qs}`).then((r) => r.alerts);
  },
  createAlert: (a) => req('/histviz/alerts', { method: 'POST', body: JSON.stringify(a) }),
  saveAlert: (id, a) =>
    req(`/histviz/alerts/${id}`, { method: 'PUT', body: JSON.stringify(a) }),
  deleteAlert: (id) => req(`/histviz/alerts/${id}`, { method: 'DELETE' }),

  /** Provider capability matrix (used to know which providers have a live WS feed). */
  providers: () => req('/connectors/providers').then((r) => r.providers),

  /** Fetch bars through a connector without storing them.
   *  coords: { connector_id?, provider?, asset_type, ticker, timeframe, bars? } */
  preview: (coords) =>
    req('/histdata/preview', { method: 'POST', body: JSON.stringify(coords) }),

  /** One window per instrument in a single round trip, what a workspace opens with.
   *  Answers in the order asked; a pane that fails carries `error` and the others their
   *  bars. Read one at a time server-side, because panes share connectors and a connector's
   *  quota and rate limit are counted per account. */
  seriesBatch: (items) =>
    req('/histviz/series/batch', { method: 'POST', body: JSON.stringify({ items }) }).then(
      (r) => r.results
    ),

  /** The same batch, answered on **one clock**: `clock` is the caller's own bar timestamps
   *  ({provider, ts}) and each result comes back with a `map`, row → that instrument's bar
   *  index, `null` where it has no bar in the period.
   *
   *  Two providers stamp the same trading day at two different instants (a crypto exchange
   *  at 00:00 UTC, a US broker at the session open), so pairing bars by raw timestamp slides
   *  one series a full day against the other. Bucketing is the server's `align::merge`, the
   *  same one the quant and backtest sides use. Answers `{results, clock}`; `clock.refused`
   *  says why there is no map (intraday across providers has no honest bucket). */
  seriesBatchOn: (items, clock, opts = {}) =>
    req('/histviz/series/batch', {
      method: 'POST',
      body: JSON.stringify({ items, align: true, clock }),
      ...opts
    }),

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
   *  stored the server records the same bars into it.
   *
   *  `connector` picks which of the user's accounts for that provider to stream on. Two
   *  keys are two entitlements and two connection seats, so it is part of the address and
   *  not a detail: omit it and the server falls back to the oldest one the chart is granted.
   *
   *  Events: `status` ({state, code, message}), `snapshot` (the forming bar) and `bar`. */
  streamUrl: ({ provider, asset_type, ticker, timeframe, connector_id, since }) => {
    const qs = new URLSearchParams({ provider, asset_type, ticker, timeframe });
    if (connector_id) qs.set('connector', connector_id);
    // The newest bar already on the chart. The server sends what closed between it and the
    // first live event, so the seam between the downloaded window and the stream has no
    // hole: the series call, the WS connect and (for a minute-bar provider) the wait for the
    // next publication all take time the chart would otherwise skip over.
    if (since) qs.set('since', since);
    return `/api/histviz/stream?${qs}`;
  },

  /** SSE URL for a whole workspace: one connection carrying every pane's instrument.
   *
   *  Each event gains `i`, the instrument's index in the array, so the page routes it to the
   *  pane that asked. The list rides as URL-encoded JSON because a ticker carries `:`, `/`
   *  and `@` (`SAN:EUR`, `BTC/USD`, `7203@TSEJ:JPY`) and any separator we invented would
   *  eventually meet one of them. */
  streamsUrl: (instruments) => {
    const items = instruments.map(
      ({ provider, asset_type, ticker, timeframe, connector_id, since }) => ({
        provider,
        asset_type,
        ticker,
        timeframe,
        ...(connector_id ? { connector: connector_id } : {}),
        ...(since ? { since } : {})
      })
    );
    return `/api/histviz/streams?instruments=${encodeURIComponent(JSON.stringify(items))}`;
  }
};
