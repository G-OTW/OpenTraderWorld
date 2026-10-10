/** Account broker API client: broker accounts shared by every module that reads a book.
 *
 * A broker account is a named instance of a broker: read-only credentials (plugged from
 * the vault), non-secret settings, and the list of modules allowed to use it (`modules`,
 * where `['*']` means all). Credentials are write-only: the server returns which secret
 * names are set, never a value.
 *
 * Deliberately a different list from `$lib/connectors`: that one buys market data, this one
 * reads a portfolio, and the keys are not interchangeable. Everything here reads; no call
 * places, changes or cancels an order. */
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

export const brokersApi = {
  /** Capability matrix per supported broker (drives the create form). */
  providers: () => req('/brokers/providers').then((r) => r.providers),

  /** Module ids an account can be granted to, in display order. */
  modules: () => req('/brokers/modules').then((r) => r.modules),

  /** Accounts with their broker's capabilities and credential status.
   * `module` restricts to what that module is allowed to use; omit for the whole list. */
  list: (module = null) =>
    req(`/brokers${module ? `?module=${encodeURIComponent(module)}` : ''}`).then(
      (r) => r.accounts
    ),

  /** payload: { broker, name, modules?, config?: { field: value } } */
  create: (payload) =>
    req('/brokers', { method: 'POST', body: JSON.stringify(payload) }).then((r) => r.account),
  update: (id, patch) => req(`/brokers/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  remove: (id) => req(`/brokers/${id}`, { method: 'DELETE' }),

  setSecret: (accountId, name, value, vaultItemId = null) =>
    req(`/brokers/${accountId}/secrets`, {
      method: 'POST',
      body: JSON.stringify({ name, value, vault_item_id: vaultItemId })
    }),
  deleteSecret: (accountId, name) =>
    req(`/brokers/${accountId}/secrets/${encodeURIComponent(name)}`, { method: 'DELETE' }),

  /** Reach the broker with this account's own credentials. Never throws on a refused
   * connection: the answer is `{ ok, detail }` and the detail names what to change. */
  test: (accountId) => req(`/brokers/${accountId}/test`, { method: 'POST' }),

  /** Instruments worth offering in a picker: what this account holds or has worked.
   * A suggestion list, never a filter. */
  symbols: (accountId) => req(`/brokers/${accountId}/symbols`).then((r) => r.symbols),

  positions: (accountId) => req(`/brokers/${accountId}/positions`).then((r) => r.positions),
  orders: (accountId) => req(`/brokers/${accountId}/orders`).then((r) => r.orders),

  /** Positions and working orders in one read: `{ positions, orders, warnings }`. Only what
   * the broker declares is asked for, and a side that fails lands in `warnings` instead of
   * sinking the other. */
  book: (accountId, module = null) =>
    req(`/brokers/${accountId}/book${module ? `?module=${encodeURIComponent(module)}` : ''}`)
};

/** Grant value meaning "every account module, present and future". */
export const ALL_MODULES = '*';

/** Whether an account may be used by `module`. */
export function allows(account, module) {
  const m = account?.modules ?? [];
  return m.includes(ALL_MODULES) || m.includes(module);
}

/** An account is ready when every credential and every required setting it declares is
 * filled in. */
export function isReady(a) {
  const cap = a?.capability ?? {};
  const secrets = (cap.required_secrets ?? []).every((n) => a.set_secrets?.includes(n));
  const settings = (cap.config_fields ?? [])
    .filter((f) => f.required)
    .every((f) => `${a.config?.[f.name] ?? ''}`.trim() !== '');
  return secrets && settings;
}
