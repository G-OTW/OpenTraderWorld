/**
 * Vault API client — centralized API keys & secrets.
 * Values are write-only: they can be set/replaced but never read back; only item
 * names come out. Requests and rate limits are tracked per vault, not per item.
 */
import { redirectIfUnauthorized } from '$lib/auth.js';
import { shouldRetryAfterReauth } from '$lib/reauth.svelte.js';

/** `retry` is false on the second pass; see the note in $lib/settings/api.js. */
async function req(path, options = {}, retry = true) {
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
    // Writing a vault item needs a recent password (substituting a credential is exactly
    // what a stolen session would do). Prompt once, then retry.
    if (retry && (await shouldRetryAfterReauth(res.status, body))) {
      return req(path, options, false);
    }
    const err = new Error(body?.error ?? `request failed (${res.status})`);
    err.code = body?.code ?? null;
    err.status = res.status;
    throw err;
  }
  return body;
}

export const vaultApi = {
  /** All vaults with item names (never values), reference counts and quota state. */
  list: () => req('/vault').then((r) => r.vaults),
  create: (name) => req('/vault', { method: 'POST', body: JSON.stringify({ name }) }),
  rename: (id, name) =>
    req(`/vault/${id}`, { method: 'PATCH', body: JSON.stringify({ name }) }),
  remove: (id) => req(`/vault/${id}`, { method: 'DELETE' }),
  /** Create or replace an item's value (write-only). */
  setItem: (vaultId, name, value) =>
    req(`/vault/${vaultId}/items`, { method: 'PUT', body: JSON.stringify({ name, value }) }),
  removeItem: (itemId) => req(`/vault/items/${itemId}`, { method: 'DELETE' }),
  /** Vault-wide request limit (observe-and-display, same semantics as source quotas). */
  setQuota: (vaultId, maxRequests, period) =>
    req(`/vault/${vaultId}/quota`, {
      method: 'PUT',
      body: JSON.stringify({ max_requests: maxRequests, period })
    }),
  removeQuota: (vaultId) => req(`/vault/${vaultId}/quota`, { method: 'DELETE' })
};

/** "Vault.item" display label for a vault item, resolved from a vaults list. */
export function itemLabel(vaults, itemId) {
  for (const v of vaults ?? []) {
    const item = (v.items ?? []).find((i) => i.id === itemId);
    if (item) return `${v.name}.${item.name}`;
  }
  return null;
}
