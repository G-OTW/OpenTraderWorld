/**
 * Versioning client: the two global switches (Settings) and the version history of editor
 * documents and saved strategies. Both histories share one route shape, so one factory
 * serves them: `docVersions` under /documents, `strategyVersions` under /backtest/strategies.
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

/** `scope`: 'editor' | 'strategies'. `purge` (switching off only) deletes that history. */
export const versioningApi = {
  settings: () => req('/settings/versioning'),
  setEnabled: (scope, enabled, purge = false) =>
    req('/settings/versioning', { method: 'POST', body: JSON.stringify({ scope, enabled, purge }) })
};

function historyApi(base) {
  return {
    list: (id) => req(`${base}/${id}/versions`).then((r) => r.versions),
    /** Versions plus `current`: the latest one's id while the item still matches it, else null. */
    listWithCurrent: (id) =>
      req(`${base}/${id}/versions`).then((r) => ({ versions: r.versions, current: r.current ?? null })),
    get: (id, vid) => req(`${base}/${id}/versions/${vid}`).then((r) => r.version),
    create: (id, note) =>
      req(`${base}/${id}/versions`, { method: 'POST', body: JSON.stringify({ note }) }).then(
        (r) => r.version
      ),
    remove: (id, vid) => req(`${base}/${id}/versions/${vid}`, { method: 'DELETE' }),
    purge: (id) => req(`${base}/${id}/versions`, { method: 'DELETE' }),
    /** `note` labels the version the restore creates. */
    restore: (id, vid, note) =>
      req(`${base}/${id}/versions/${vid}/restore`, { method: 'POST', body: JSON.stringify({ note }) }),
    toggle: (id, enabled, purge = false) =>
      req(`${base}/${id}/versioning`, { method: 'POST', body: JSON.stringify({ enabled, purge }) }),
    /** Histories whose item was deleted (strategies only): [{ id, title, kind, versions, last_at }]. */
    deleted: () => req(`${base}/deleted-versions`).then((r) => r.items)
  };
}

export const docVersions = historyApi('/documents');
export const strategyVersions = historyApi('/backtest/strategies');
