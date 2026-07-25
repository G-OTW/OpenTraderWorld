/** Resources API client — master categories plus the bookmarks they hold. Single-user. */
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

export const resourcesApi = {
  // Categories
  listCategories: () => req('/resources/categories').then((r) => r.categories),
  addCategory: (c) =>
    req('/resources/categories', { method: 'POST', body: JSON.stringify(c) }).then((r) => r.category),
  updateCategory: (id, c) =>
    req(`/resources/categories/${id}`, { method: 'PATCH', body: JSON.stringify(c) }),
  removeCategory: (id) => req(`/resources/categories/${id}`, { method: 'DELETE' }),

  // Resources
  list: () => req('/resources').then((r) => r.resources),
  add: (r) => req('/resources', { method: 'POST', body: JSON.stringify(r) }).then((x) => x.resource),
  update: (id, r) => req(`/resources/${id}`, { method: 'PATCH', body: JSON.stringify(r) }),
  remove: (id) => req(`/resources/${id}`, { method: 'DELETE' })
};

/** Hostname of a link for compact display, or null if blank/unparseable. */
export function linkHost(link) {
  if (!link) return null;
  try {
    return new URL(link).hostname.replace(/^www\./, '');
  } catch {
    return link;
  }
}
