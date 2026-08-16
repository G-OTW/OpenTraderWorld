/** Data broker API client — market-data connectors shared by every data module.
 *
 * A connector is a named provider account: credentials (plugged from the vault), an
 * optional request quota, and the list of modules allowed to use it (`modules`, where
 * `['*']` means all). Credentials are write-only — the server returns which secret names
 * are set, never a value. */
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

export const connectorsApi = {
  /** Capability matrix per supported provider (drives the create form). */
  providers: () => req('/connectors/providers').then((r) => r.providers),

  /** Module ids a connector can be granted to, in display order. */
  modules: () => req('/connectors/modules').then((r) => r.modules),

  /** Connectors with capabilities, credential status and quota usage.
   * `module` restricts to what that module is allowed to use; omit for the whole list. */
  list: (module = null) =>
    req(`/connectors${module ? `?module=${encodeURIComponent(module)}` : ''}`).then(
      (r) => r.connectors
    ),

  /** payload: { provider, name, modules?: string[], limit?: { enabled, max_requests, period } } */
  create: (payload) =>
    req('/connectors', { method: 'POST', body: JSON.stringify(payload) }).then((r) => r.connector),
  update: (id, patch) =>
    req(`/connectors/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  remove: (id) => req(`/connectors/${id}`, { method: 'DELETE' }),

  setSecret: (connectorId, name, value, vaultItemId = null) =>
    req(`/connectors/${connectorId}/secrets`, {
      method: 'POST',
      body: JSON.stringify({ name, value, vault_item_id: vaultItemId })
    }),
  deleteSecret: (connectorId, name) =>
    req(`/connectors/${connectorId}/secrets/${encodeURIComponent(name)}`, { method: 'DELETE' })
};

/** Grant value meaning "every data module, present and future". */
export const ALL_MODULES = '*';

/** Whether a connector may be used by `module`. */
export function allows(connector, module) {
  const m = connector?.modules ?? [];
  return m.includes(ALL_MODULES) || m.includes(module);
}

/** A connector is ready when it is keyless or all its required secrets are set. */
export function isReady(c) {
  return !c.required_secrets?.length || c.required_secrets.every((n) => c.set_secrets.includes(n));
}
