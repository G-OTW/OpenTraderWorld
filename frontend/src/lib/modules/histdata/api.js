/** Historical Data API client.
 *
 * Connectors themselves live in the data broker ($lib/connectors/api.js) — this client
 * spends them. Downloads run as background jobs; the page polls /jobs for progress.
 * Datasets are the stored catalog. `preview` fetches bars without storing anything. */
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

/** Fetch a file and hand it to the browser's downloader, honouring the server's
 *  Content-Disposition name. Returns the name it saved under. */
async function downloadFile(path, fallbackName) {
  const res = await fetch(`/api${path}`);
  redirectIfUnauthorized(res);
  if (!res.ok) {
    let body = null;
    try {
      body = await res.json();
    } catch {
      /* not JSON */
    }
    throw new Error(body?.error ?? `request failed (${res.status})`);
  }
  const blob = await res.blob();
  const name =
    res.headers.get('content-disposition')?.match(/filename="([^"]+)"/)?.[1] ?? fallbackName;
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
  return name;
}

export const histdataApi = {
  /** Queue a download. `from`/`to` are RFC3339 strings. */
  startDownload: (payload) =>
    req('/histdata/downloads', { method: 'POST', body: JSON.stringify(payload) }),

  /** Same payload, queues nothing: what the batch would cost in provider requests. */
  estimateDownload: (payload) =>
    req('/histdata/downloads', {
      method: 'POST',
      body: JSON.stringify({ ...payload, estimate_only: true })
    }).then((r) => r.estimate),

  jobs: () => req('/histdata/jobs').then((r) => r.jobs),
  /** Stop a job. A running one stops at its next chunk, keeping the bars already stored. */
  cancelJob: (id) => req(`/histdata/jobs/${id}`, { method: 'DELETE' }),
  /** Stop every job of a batch that has not finished. */
  cancelBatch: (id) => req(`/histdata/jobs/batch/${id}`, { method: 'DELETE' }),

  datasets: () => req('/histdata/datasets').then((r) => r.datasets),

  /** Read a file and report the bars it would write. Stores nothing. */
  importAnalyze: (payload) =>
    req('/histdata/import/analyze', { method: 'POST', body: JSON.stringify(payload) }).then(
      (r) => r.analysis
    ),
  /** Write them. Idempotent: the same file twice leaves the same dataset. */
  importCommit: (payload) =>
    req('/histdata/import/commit', { method: 'POST', body: JSON.stringify(payload) }).then(
      (r) => r.report
    ),

  append: (id) => req(`/histdata/datasets/${id}/append`, { method: 'POST' }),
  remove: (id) => req(`/histdata/datasets/${id}`, { method: 'DELETE' }),
  /** Export a dataset to a file. `format` is 'csv' or 'parquet'. Fetched rather than
   *  linked: the server builds the whole file before it answers, and a bare <a download>
   *  leaves the page looking idle for a million-bar Parquet. The caller shows a spinner
   *  for as long as this promise is pending. */
  exportDataset: (id, format = 'csv') =>
    downloadFile(
      `/histdata/datasets/${id}/export${format === 'csv' ? '' : `?format=${format}`}`,
      `dataset.${format}`
    )
};

/** The columns an OHLCV file can be mapped onto, in the order the mapping step offers
 *  them. `close` first: a file with one price column is a close series. */
export const IMPORT_TARGETS = [
  { id: 'ts', key: 'ts' },
  { id: 'open', key: 'open' },
  { id: 'high', key: 'high' },
  { id: 'low', key: 'low' },
  { id: 'close', key: 'close' },
  { id: 'volume', key: 'volume' },
  { id: 'adj_open', key: 'adjOpen' },
  { id: 'adj_high', key: 'adjHigh' },
  { id: 'adj_low', key: 'adjLow' },
  { id: 'adj_close', key: 'adjClose' }
];

/** The reserved provider id every imported dataset is filed under. */
export const IMPORT_PROVIDER = 'import';

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
