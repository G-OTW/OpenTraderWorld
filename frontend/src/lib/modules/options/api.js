/** Options API client: expiries and chain snapshots read from a provider (Interactive
 * Brokers, Massive), the pure strategy evaluation, saved strategies, and the IV surface and
 * IV history the Quant module computes, granted here to `options`. */
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

const send = (method, body) => ({ method, body: JSON.stringify(body) });

/** Start a provider task, report its progress, resolve with its result. Reads both the
 * flat task shape of the Quant helper (`status`, `done`, `total`, `step`) and the nested one
 * (`state`, `progress: {done, total, what}`), so either server side answers the same page. */
async function runTask(path, body, onProgress = () => {}) {
  const { task } = await req(path, send('POST', body));
  for (;;) {
    await new Promise((r) => setTimeout(r, 1000));
    const s = await req(`/options/tasks/${task}`);
    const state = s.state ?? s.status;
    if (state === 'done') return s.result;
    if (state === 'error') throw new Error(s.error ?? 'task failed');
    const p = s.progress ?? s;
    onProgress({ done: p.done ?? 0, total: p.total ?? 0, step: p.what ?? p.step ?? '' });
  }
}

export const optionsApi = {
  /** Listed expiries of an underlying, with strikes where the provider gives them. */
  expiries: ({ connector_id, underlying, asset_type }) => {
    const qs = new URLSearchParams({ connector_id, underlying, asset_type });
    return req(`/options/expiries?${qs}`);
  },

  /** Chain snapshot: one underlying, up to 8 expiries, a strike window. */
  chain: (body, onProgress) => runTask('/options/chain', body, onProgress),

  /** Cash dividends coming on an underlying: `{ dividends: [{ ex_date, amount, projected }],
   * provider, notice }`. An empty list with a notice when nothing can be read. */
  dividends: ({ underlying, asset_type }) => req(`/options/dividends?${new URLSearchParams({ underlying, asset_type })}`),

  /** Payoff, time path, breakevens, extremes, greeks, POP, margin and assignment warnings of a
   * set of legs. Pure, no provider. */
  evaluate: (body) => req('/options/strategy/evaluate', send('POST', body)),

  strategies: {
    list: () => req('/options/strategies').then((r) => (Array.isArray(r) ? r : (r?.strategies ?? []))),
    create: (s) => req('/options/strategies', send('POST', s)),
    update: (id, s) => req(`/options/strategies/${id}`, send('PUT', s)),
    remove: (id) => req(`/options/strategies/${id}`, { method: 'DELETE' })
  },

  /** Same bodies and answers as Quant's market endpoints, spent on the `options` grant. */
  surface: (body, onProgress) => runTask('/options/surface', body, onProgress),
  ivHistory: (body, onProgress) => runTask('/options/ivhistory', body, onProgress),

  /** Instrument lookup on one connector: the underlying is picked from these results. */
  symbols: ({ q, asset_type, connector_id }) => {
    const qs = new URLSearchParams({ module: 'options' });
    if (q) qs.set('q', q);
    if (asset_type) qs.set('asset_type', asset_type);
    if (connector_id) qs.set('connectors', connector_id);
    return req(`/histdata/symbols?${qs}`);
  }
};
