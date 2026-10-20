/** Custom-indicator library client — ONE library, shared by every module.
 *
 * The rows live in `backtest_indicators` and the route keeps its `/backtest/…` prefix (it is
 * part of the public API + the MCP catalog), but the library itself is module-agnostic: an
 * indicator saved from the backtest page is the same row the chart module draws, and an edit
 * on either side shows up on the other. Nothing here is backtest-specific.
 */
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
  if (!res.ok) {
    const err = new Error(body?.error ?? `request failed (${res.status})`);
    // A script that does not compile comes back 422 with its diagnostics here.
    err.details = body?.details ?? null;
    throw err;
  }
  return body;
}

export const indicatorLib = {
  /** Saved indicators, newest edit first; `kind` 'dag' | 'script' narrows the list. A DAG row
   *  carries its `definition`, a script row its `source` and server-compiled `meta`. */
  list: (kind) => req(`/backtest/indicators${kind ? `?kind=${kind}` : ''}`).then((r) => r.indicators),
  get: (id) => req(`/backtest/indicators/${id}`).then((r) => r.indicator),
  /** `body` = `{ definition }` (node graph) or `{ kind: 'script', source }`. */
  create: (name, description, body) =>
    req('/backtest/indicators', { method: 'POST', body: JSON.stringify({ name, description, ...body }) }),
  update: (id, name, description, body) =>
    req(`/backtest/indicators/${id}`, { method: 'PUT', body: JSON.stringify({ name, description, ...body }) }),
  remove: (id) => req(`/backtest/indicators/${id}`, { method: 'DELETE' }),
  /** Server-side compile of a script, nothing saved: `{ok, meta, warnings}` | `{ok:false, diagnostics}`. */
  compile: (source) => req('/backtest/indicators/compile', { method: 'POST', body: JSON.stringify({ source }) })
};
