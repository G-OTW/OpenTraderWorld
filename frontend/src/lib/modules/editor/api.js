/** Editor module API client — document tree (folders + pages). Single-user. */
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

export const docsApi = {
  list: () => req('/documents').then((r) => r.documents),
  get: (id) => req(`/documents/${id}`).then((r) => r.document),
  create: (parent_id, kind, title) =>
    req('/documents', {
      method: 'POST',
      body: JSON.stringify({ parent_id, kind, title })
    }).then((r) => r.document),
  update: (id, patch) =>
    req(`/documents/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  move: (id, parent_id, position) =>
    req(`/documents/${id}/move`, {
      method: 'POST',
      body: JSON.stringify({ parent_id, position })
    }),
  remove: (id) => req(`/documents/${id}`, { method: 'DELETE' })
};
