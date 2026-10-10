/** Community Docs API client. Docs are synced from the website and read in-app/offline. */
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

export const communityDocsApi = {
  list: () => req('/community-docs').then((r) => r.docs),
  favorites: () => req('/community-docs/favorites').then((r) => r.docs),
  get: (slug) => req(`/community-docs/${encodeURIComponent(slug)}`).then((r) => r.doc),
  setFavorite: (slug, favorite) =>
    req(`/community-docs/${encodeURIComponent(slug)}/favorite`, {
      method: 'PUT',
      body: JSON.stringify({ favorite })
    }).then((r) => r.favorited),
  /** Reload docs from upstream sources. Never removes user favorites. */
  refresh: () => req('/community-docs/refresh', { method: 'POST' }).then((r) => r.docs),
  sync: (docs) =>
    req('/community-docs/sync', { method: 'POST', body: JSON.stringify({ docs }) }).then(
      (r) => r.synced
    )
};

/**
 * Group a flat doc list into [{ category, docs }] preserving first-seen order. A doc can
 * carry several categories, so it appears under each of them; docs with none fall under
 * "Uncategorized".
 */
export function groupByCategory(docs) {
  const groups = [];
  const index = new Map();
  const add = (key, d) => {
    let g = index.get(key);
    if (!g) {
      g = { category: key, docs: [] };
      index.set(key, g);
      groups.push(g);
    }
    g.docs.push(d);
  };
  for (const d of docs) {
    const cats = d.categories?.length ? d.categories : ['Uncategorized'];
    for (const c of cats) add(c || 'Uncategorized', d);
  }
  return groups;
}
