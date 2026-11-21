/** Ticker resolution and window storage shared by the download form and the pickers that
 *  need a stored dataset (Backtest, Quant).
 *
 * `resolveTicker` asks the connector's own symbol lookup (the one the chart searches with)
 * whether a symbol exists before anything is queued. A symbol that does not come back is an
 * error naming the fix, never the closest hit.
 *
 * `storeWindow` keeps a window of a live instrument through `POST /histdata/preview/save`,
 * the same download job the chart's "save" queues: closed bars only, upserted on the
 * dataset key, gaps recorded. It waits on that job, not on the catalog, so a dataset that
 * already held other bars is not taken as done before the new window is in. */
import { histdataApi } from './api.js';
import { histvizApi } from '../histviz/api.js';
import { tfSeconds } from '../histviz/pane.js';

/** Statuses a download job can still leave on its own (mirrors the store's LIVE_STATUSES). */
const LIVE = new Set(['queued', 'running', 'waiting', 'cancelling']);

/**
 * Check one symbol at one connector.
 *
 * Answers `{ status, symbol?, suggestions?, message? }`:
 * - `ok`: the provider lists it; `symbol` is its own spelling (a case-only difference is
 *   the same instrument, the provider's spelling is what the job must send);
 * - `missing`: no listing carries it; `suggestions` are a few hits to choose from, never
 *   picked for the user;
 * - `unchecked`: the provider has no symbol search, so the first fetch is the only check
 *   there is (what the form always did for these providers);
 * - `failed`: the lookup itself failed; `message` is the provider's reason.
 */
export async function resolveTicker({ connectorId, assetType, ticker, module = 'histdata' }) {
  const r = await histdataApi.symbols({
    q: ticker,
    asset_type: assetType,
    connectors: [connectorId],
    module,
    limit: 50
  });
  const notes = (r.notes ?? []).filter((n) => !n.connector_id || n.connector_id === connectorId);
  const hits = (r.results ?? []).filter((h) => h.connector_id === connectorId);
  const exact = hits.find((h) => h.symbol === ticker);
  if (exact) return { status: 'ok', symbol: exact.symbol };
  const folded = [...new Set(hits.filter((h) => h.symbol.toUpperCase() === ticker.toUpperCase()).map((h) => h.symbol))];
  if (folded.length === 1) return { status: 'ok', symbol: folded[0] };
  if (notes.some((n) => n.code === 'unsupported')) return { status: 'unchecked' };
  if (!hits.length && notes.length) return { status: 'failed', message: notes.map((n) => n.message).join('; ') };
  return { status: 'missing', suggestions: [...new Set(hits.map((h) => h.symbol))].slice(0, 5) };
}

/** A first window worth a run: five years of daily-or-longer bars, thirty days of intraday
 *  (a year of minute bars is a long download most providers do not even serve). */
export function defaultFrom(timeframe, now = new Date()) {
  const secs = tfSeconds(timeframe) ?? 86400;
  const d = new Date(now);
  if (secs >= 86400) d.setUTCFullYear(d.getUTCFullYear() - 5);
  else d.setUTCDate(d.getUTCDate() - 30);
  return d.toISOString().slice(0, 10);
}

const sleep = (ms, signal) =>
  new Promise((resolve, reject) => {
    const id = setTimeout(resolve, ms);
    signal?.addEventListener('abort', () => (clearTimeout(id), reject(signal.reason)), { once: true });
  });

/** Poll the queue until job `id` leaves the live statuses. `onjob` sees every reading. */
export async function waitForJob(id, { onjob, signal, every = 1000 } = {}) {
  let unseen = 0;
  for (;;) {
    await sleep(every, signal);
    const job = (await histdataApi.jobs()).find((j) => j.id === id);
    if (!job) {
      // The queue lists recent jobs; one that vanished from it is not running any more.
      if (++unseen >= 5) return null;
      continue;
    }
    unseen = 0;
    onjob?.(job);
    if (!LIVE.has(job.status)) return job;
  }
}

/**
 * Store `[from, to)` of an instrument and wait for the dataset.
 *
 * `coords`: `{ connector_id, provider, asset_type, ticker, timeframe }` (a symbol-search hit
 * plus the chosen timeframe). Resolves `{ dataset, datasets, job }` once the job is over and
 * the dataset holds bars; throws the job's own error otherwise.
 */
export async function storeWindow(coords, { from, to }, { onjob, signal } = {}) {
  const { connector_id, provider, asset_type, ticker, timeframe } = coords;
  const queued = await histvizApi.savePreview({ connector_id, provider, asset_type, ticker, timeframe, from, to });
  const job = await waitForJob(queued.job_id, { onjob, signal });
  const datasets = await histdataApi.datasets();
  const dataset = datasets.find((d) => d.id === queued.dataset_id) ?? null;
  if (job && !['done', 'partial'].includes(job.status)) {
    throw new Error(job.error || `download ${job.status}`);
  }
  if (!dataset?.bar_count) {
    throw new Error(job?.error || `no bars stored for ${ticker} ${timeframe}`);
  }
  return { dataset, datasets, job };
}
