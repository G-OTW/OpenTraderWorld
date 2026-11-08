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
  /** The row for `{provider, asset_type, ticker}` as scripts read it, or null: what is stored,
   *  with a future's empty tick size, point value and currency from its contract specs. */
  get: (coords) => instrumentMeta.info(coords).then((r) => r.meta),
  /** The same read with where it came from: `{ meta, filled: [field], source?, spec_error? }`. */
  info: (coords) => {
    const qs = new URLSearchParams({ provider: coords.provider ?? '', asset_type: coords.asset_type ?? '', ticker: coords.ticker ?? '' });
    return req(`/histdata/instrument-meta?${qs}`);
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
