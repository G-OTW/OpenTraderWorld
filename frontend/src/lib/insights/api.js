/** Insights API client: one compute route over the journal, a backtest run or a paper session. */
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

export const insightsApi = {
  /** body: { source: {journal: {filters}} | {backtest: {run_id}} | {paper: {session_id}}, researchers?, tz_offset? } */
  run: (body) => req('/insights', { method: 'POST', body: JSON.stringify(body) })
};
