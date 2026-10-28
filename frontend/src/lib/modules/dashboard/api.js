/** Dashboard API client — load/save the user's arrangeable grid layout. */
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

export const dashboardApi = {
  /** Saved layout, or `null` if the user has never customized it. */
  getLayout: () => req('/dashboard/layout'),
  /** Persist the whole layout document. */
  saveLayout: (layout) =>
    req('/dashboard/layout', { method: 'PUT', body: JSON.stringify(layout) })
};
