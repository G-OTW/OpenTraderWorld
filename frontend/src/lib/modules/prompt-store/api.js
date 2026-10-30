/**
 * PromptStore API client — a prompt library with version history, quick voting
 * (thumbs up/down), duplicate, and rollback. Single-user.
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

export const promptsApi = {
  list: () => req('/prompts').then((r) => r.prompts),
  tags: () => req('/prompts/tags').then((r) => r.tags),
  get: (id) => req(`/prompts/${id}`).then((r) => r.prompt),
  add: (prompt) =>
    req('/prompts', { method: 'POST', body: JSON.stringify(prompt) }).then((r) => r.prompt),
  update: (id, prompt) =>
    req(`/prompts/${id}`, { method: 'PATCH', body: JSON.stringify(prompt) }),
  setVote: (id, vote) =>
    req(`/prompts/${id}/vote`, { method: 'PATCH', body: JSON.stringify({ vote }) }),
  duplicate: (id) => req(`/prompts/${id}/duplicate`, { method: 'POST' }).then((r) => r.prompt),
  versions: (id) => req(`/prompts/${id}/versions`).then((r) => r.versions),
  rollback: (id, version) =>
    req(`/prompts/${id}/rollback`, {
      method: 'POST',
      body: JSON.stringify({ version })
    }).then((r) => r.prompt),
  remove: (id) => req(`/prompts/${id}`, { method: 'DELETE' })
};

/** RFC3339 timestamp -> "Mar 14, 2026" (date only), null-safe. */
export function fmtDate(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (isNaN(d)) return null;
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

/** RFC3339 timestamp -> "Mar 14, 2026, 3:07 PM", null-safe. */
export function fmtDateTime(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (isNaN(d)) return null;
  return d.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: 'numeric',
    minute: '2-digit'
  });
}
