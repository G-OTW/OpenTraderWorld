/** Custom-indicator library client — ONE library, shared by every module.
 *
 * The rows live in `backtest_indicators` and the route keeps its `/backtest/…` prefix (it is
 * part of the public API + the MCP catalog), but the library itself is module-agnostic: an
 * indicator saved from the backtest expert page is the same row the chart module draws, and
 * an edit on either side shows up on the other. Nothing here is backtest-specific.
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
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

export const indicatorLib = {
  /** Every saved indicator, newest edit first. Rows carry their full `definition`. */
  list: () => req('/backtest/indicators').then((r) => r.indicators),
  get: (id) => req(`/backtest/indicators/${id}`).then((r) => r.indicator),
  create: (name, description, definition) =>
    req('/backtest/indicators', { method: 'POST', body: JSON.stringify({ name, description, definition }) }),
  update: (id, name, description, definition) =>
    req(`/backtest/indicators/${id}`, { method: 'PUT', body: JSON.stringify({ name, description, definition }) }),
  remove: (id) => req(`/backtest/indicators/${id}`, { method: 'DELETE' })
};
