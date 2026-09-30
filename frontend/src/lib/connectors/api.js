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

  /** payload: { provider, name, modules?, limit?, config?: { field: value } } */
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
    req(`/connectors/${connectorId}/secrets/${encodeURIComponent(name)}`, { method: 'DELETE' }),

  /** Reach the provider with this connector's own settings. Never throws on a failed
   * connection: the answer is `{ ok, detail }` and the detail names what to change. */
  test: (connectorId) => req(`/connectors/${connectorId}/test`, { method: 'POST' })
};

/** Grant value meaning "every data module, present and future". */
export const ALL_MODULES = '*';

/** Whether a connector may be used by `module`. */
export function allows(connector, module) {
  const m = connector?.modules ?? [];
  return m.includes(ALL_MODULES) || m.includes(module);
}

/** A connector is ready when every secret and every required setting it declares is
 * filled in. A keyless provider with no settings is ready the moment it exists. */
export function isReady(c) {
  const secrets = (c.required_secrets ?? []).every((n) => c.set_secrets?.includes(n));
  const settings = (c.config_fields ?? [])
    .filter((f) => f.required)
    .every((f) => `${c.config?.[f.name] ?? ''}`.trim() !== '');
  return secrets && settings;
}
