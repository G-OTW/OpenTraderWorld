/** Instrument facts scripts read through `syminfo.*` (tick size, exchange timezone, …). */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, { headers: { 'content-type': 'application/json' }, ...options });
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

export const instrumentMeta = {
  /** The stored row for `{provider, asset_type, ticker}`, or null. */
  get: (coords) => {
    const qs = new URLSearchParams({ provider: coords.provider ?? '', asset_type: coords.asset_type ?? '', ticker: coords.ticker ?? '' });
    return req(`/histdata/instrument-meta?${qs}`).then((r) => r.meta);
  },
  /** Replace the whole row (a null field = unknown). */
  put: (meta) => req('/histdata/instrument-meta', { method: 'PUT', body: JSON.stringify(meta) })
};

/** A chart's scripts run on the server over the stored warmup and the bars sent
 *  (`POST /api/histviz/scripts/run`): `[{ key, answer }]`, each answer shaped like the
 *  worker's run answer, the output already cut to the bars sent. */
export const scriptServer = {
  run: (body) => req('/histviz/scripts/run', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.results)
};
