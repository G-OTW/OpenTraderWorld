/** Historical Data API client.
 *
 * Providers expose a capability matrix (asset types, timeframes, required secrets) that the
 * download form uses to grey out impossible combinations. Credentials are project-wide and
 * write-only — the server only ever returns which secret names are set. Downloads run as
 * background jobs; the page polls /jobs for progress. Datasets are the stored catalog. */
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

export const histdataApi = {
  /** Capability matrix per supported provider (drives the "add connector" form). */
  providers: () => req('/histdata/providers').then((r) => r.providers),

  /** Named connectors: capability facts + set credentials + quota usage, per instance. */
  connectors: () => req('/histdata/connectors').then((r) => r.connectors),
  /** payload: { provider, name, limit?: { enabled, max_requests, period } } */
  createConnector: (payload) =>
    req('/histdata/connectors', { method: 'POST', body: JSON.stringify(payload) }).then(
      (r) => r.connector
    ),
  updateConnector: (id, patch) =>
    req(`/histdata/connectors/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteConnector: (id) => req(`/histdata/connectors/${id}`, { method: 'DELETE' }),

  setSecret: (connectorId, name, value) =>
    req(`/histdata/connectors/${connectorId}/secrets`, {
      method: 'POST',
      body: JSON.stringify({ name, value })
    }),
  deleteSecret: (connectorId, name) =>
    req(`/histdata/connectors/${connectorId}/secrets/${encodeURIComponent(name)}`, {
      method: 'DELETE'
    }),

  /** Queue a download. `from`/`to` are RFC3339 strings. */
  startDownload: (payload) =>
    req('/histdata/downloads', { method: 'POST', body: JSON.stringify(payload) }),

  jobs: () => req('/histdata/jobs').then((r) => r.jobs),

  datasets: () => req('/histdata/datasets').then((r) => r.datasets),
  append: (id) => req(`/histdata/datasets/${id}/append`, { method: 'POST' }),
  remove: (id) => req(`/histdata/datasets/${id}`, { method: 'DELETE' }),
  /** Export URL (CSV) — used directly as a download link. */
  exportUrl: (id) => `/api/histdata/datasets/${id}/export`
};

// Formatting lives in $lib/format.js. The local copy stopped at GB, so a 5 TB dataset
// read as "5120.0 GB", and it printed "0 B" for a null size as well as a zero one.
export { fmtBytes } from '$lib/format.js';

/** Group datasets by asset_type → ticker for the management tree. */
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
