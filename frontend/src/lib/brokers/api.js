/** Account broker API client: broker accounts shared by every module that reads a book.
 *
 * A broker account is a named instance of a broker: read-only credentials (plugged from
 * the vault), non-secret settings, and the list of modules allowed to use it (`modules`,
 * where `['*']` means all). Credentials are write-only: the server returns which secret
 * names are set, never a value.
 *
 * Deliberately a different list from `$lib/connectors`: that one buys market data, this one
 * reads a portfolio, and the keys are not interchangeable. Everything here reads, except the
 * three order calls, which the server answers only for an account with trading on. */
import { redirectIfUnauthorized } from '$lib/auth.js';
import { shouldRetryAfterReauth } from '$lib/reauth.svelte.js';

/** `retry` is false on the second pass: turning trading on raises the password prompt once,
 *  and a refusal after that is real. The same goes for an order refused by the trading lock,
 *  which raises the authenticator-code prompt once. */
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
    if (retry && (await shouldRetryAfterReauth(res.status, body))) return req(path, options, false);
    // `code` and `details` say what the caller can do about it (a paper future with no
    // margin: `margin_missing`, naming the instrument).
    throw Object.assign(new Error(body?.error ?? `request failed (${res.status})`), { code: body?.code ?? null, details: body?.details ?? null });
  }
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
    req(`/brokers/${accountId}/book${module ? `?module=${encodeURIComponent(module)}` : ''}`),

  /** URL of the SSE stream following these accounts' books live: `ready`, then `account`
   *  events (see `histviz/accountEvents.js`), `off` when a broker stops pushing. */
  eventsUrl: (accountIds, module = null) =>
    `/api/brokers/events?accounts=${encodeURIComponent(accountIds.join(','))}${module ? `&module=${encodeURIComponent(module)}` : ''}`,

  /** Live marks of held instruments, by the broker's id (`native_id`): `{ [id]: price }`.
   *  Trading accounts only; a broker that streams none answers `{}`. */
  marks: (accountId, ids, module = null) =>
    req(
      `/brokers/${accountId}/marks?ids=${encodeURIComponent(ids.join(','))}${module ? `&module=${encodeURIComponent(module)}` : ''}`
    ).then((r) => r.marks ?? {}),

  /** Send a ticket `{ ticker, asset_type, legs }` (see histviz `legsOf`). Answers once the
   * broker accepted every leg: `[{ key, id }]`, the chart's key and the broker's order id. */
  placeOrders: (accountId, ticket, module = null) =>
    req(`/brokers/${accountId}/orders${module ? `?module=${encodeURIComponent(module)}` : ''}`, {
      method: 'POST',
      body: JSON.stringify(ticket)
    }).then((r) => r.placed),

  /** Move a working order: `{ limit }` or `{ stop }`. */
  moveOrder: (accountId, orderId, prices, module = null) =>
    req(`/brokers/${accountId}/orders/${encodeURIComponent(orderId)}${module ? `?module=${encodeURIComponent(module)}` : ''}`, {
      method: 'PATCH',
      body: JSON.stringify(prices)
    }),

  /** How the account spells a chart's instrument (`MNQZ6` for `MNQ.20261218@CME` at IB), and
   *  the size step its orders must be a multiple of: `{ symbol, step }`, each null when the
   *  broker cannot tell. */
  symbolFor: (accountId, ticker, assetType) =>
    req(`/brokers/${accountId}/symbol-for?ticker=${encodeURIComponent(ticker)}&asset_type=${encodeURIComponent(assetType)}`).then((r) => ({
      symbol: r.symbol ?? null,
      step: Number(r.step) > 0 ? Number(r.step) : null,
      leverage: Number(r.leverage) > 0 ? Number(r.leverage) : null,
      marginMode: r.margin_mode ?? null
    })),

  /** The account's fills on a chart's instrument over `[from, to]` (ISO), the server cutting
   *  the window to the last 30 days: `[{ at, side, qty, price, ... }]`. */
  fills: (accountId, ticker, assetType, from, to, module = null) =>
    req(
      `/brokers/${accountId}/fills?ticker=${encodeURIComponent(ticker)}&asset_type=${encodeURIComponent(assetType ?? '')}&from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}${module ? `&module=${encodeURIComponent(module)}` : ''}`
    ).then((r) => r.fills ?? []),

  /** Close a position at the market: `{ native_id, account, key }` as the position was
   *  reported. Answers `{ placed }`. */
  closePosition: (accountId, body, module = null) =>
    req(`/brokers/${accountId}/positions/close${module ? `?module=${encodeURIComponent(module)}` : ''}`, {
      method: 'POST',
      body: JSON.stringify(body)
    }),

  /** Open trading on every account for this session with an authenticator code (trading
   *  lock on). Once per sign-in; cancelling never needs it. */
  unlock: (code) => req('/brokers/unlock', { method: 'POST', body: JSON.stringify({ code }) }, false),

  /** Turn live trading on or off for an account that trades real money. On asks for the
   *  authenticator code (the wrapper raises the prompt and retries once). `{ ok, account }`. */
  setLive: (accountId, enabled) => req(`/brokers/${accountId}/live`, { method: 'POST', body: JSON.stringify({ enabled }) }),

  /** Kill switch: cancel every working order of the account, at the broker. `{ cancelled }`. */
  cancelAll: (accountId) => req(`/brokers/${accountId}/orders/cancel-all`, { method: 'POST' }),
  /** Orders the broker never received (`book.unsent`): send them again as one ticket under new
   *  ids (`{ placed }`), or drop them (`{ dismissed }`). */
  resendUnsent: (accountId, clientIds, module = null) =>
    req(`/brokers/${accountId}/orders/unsent/resend${module ? `?module=${encodeURIComponent(module)}` : ''}`, {
      method: 'POST',
      body: JSON.stringify({ client_ids: clientIds })
    }),
  dismissUnsent: (accountId, clientIds, module = null) =>
    req(`/brokers/${accountId}/orders/unsent/dismiss${module ? `?module=${encodeURIComponent(module)}` : ''}`, {
      method: 'POST',
      body: JSON.stringify({ client_ids: clientIds })
    }),

  /** Risk: `{ policy: { reset, password_lift, timezone }, halts: [{ id, account_id, reason,
   *  detail, created_at, until }], limits: { [accountId]: Limits }, period: { start, next } }`.
   *  A halt whose `account_id` is null covers every account; `until` null lasts until lifted. */
  risk: () => req('/brokers/risk'),

  /** How halts end: `{ reset: hour|day|week|manual, password_lift, timezone }`. Asks for the
   *  password (the wrapper raises the prompt and retries once). */
  setRiskPolicy: (policy) => req('/brokers/risk/policy', { method: 'PUT', body: JSON.stringify(policy) }).then((r) => r.policy),

  /** One account's limits, every field a decimal string or null (off). Asks for the password. */
  setRiskLimits: (accountId, limits) =>
    req(`/brokers/${accountId}/risk`, { method: 'PUT', body: JSON.stringify(limits) }).then((r) => r.limits),

  /** Global kill switch: every working order on every trading account cancelled, then every
   *  account halted. `{ halt, accounts: { [id]: { name, cancelled } | { name, error } } }`. */
  killSwitch: () => req('/brokers/kill-switch', { method: 'POST' }),

  /** Lift one halt, or every one in force without an id. Asks for the password; refused when
   *  the policy keeps halts until they end on their own. `{ lifted }`. */
  liftHalt: (id = null) => req('/brokers/halts/lift', { method: 'POST', body: JSON.stringify(id == null ? {} : { id }) }),

  /** The order audit log, newest first; `account` narrows it to one. */
  orderLog: (account = null, limit = 200) =>
    req(`/brokers/order-log?limit=${limit}${account ? `&account=${encodeURIComponent(account)}` : ''}`).then((r) => r.entries),

  /** Per-account order metrics: `[{ account_id, calls, placed, refused, latency_p50_ms, latency_p99_ms, last_at }]`. */
  orderStats: () => req('/brokers/order-stats').then((r) => r.stats),

  /** The account's money, the same figures for every broker that states them: one line per
   *  account the connection reaches, `{ account, currency, equity, cash, unrealized, realized,
   *  fees, margin_used, maintenance_margin, available, excess_liquidity, buying_power,
   *  gross_position_value, assets }`, a figure the broker does not state being null. `symbol`,
   *  the pair a chart trades, adds its base and quote to `assets` (`[{ asset, free, locked }]`)
   *  where the broker holds assets by name. */
  account: (accountId, module = null, symbol = null) => {
    const q = new URLSearchParams();
    if (module) q.set('module', module);
    if (symbol) q.set('symbol', symbol);
    const qs = q.toString();
    return req(`/brokers/${accountId}/account${qs ? `?${qs}` : ''}`).then((r) => r.accounts ?? []);
  },

  /** A paper account's money: `{ summary, cash, history }` (equity, cash, buying power; the
   *  cash moves; the orders that stopped working, with why). */
  paper: (accountId) => req(`/brokers/${accountId}/paper`),

  /** Move a paper account's cash: `deposit` | `withdraw` an amount, or `reset` the account
   *  to an amount (its starting balance when null). Answers `{ summary }`. */
  paperCash: (accountId, kind, amount = null) =>
    req(`/brokers/${accountId}/paper/cash`, { method: 'POST', body: JSON.stringify({ kind, amount }) }),

  cancelOrder: (accountId, orderId, module = null) =>
    req(`/brokers/${accountId}/orders/${encodeURIComponent(orderId)}${module ? `?module=${encodeURIComponent(module)}` : ''}`, {
      method: 'DELETE'
    })
};

/** Grant value meaning "every account module, present and future". */
export const ALL_MODULES = '*';

/** Grant that lets orders be sent to the account. Never implied by `ALL_MODULES`, and
 *  turning it on asks for the password every time. */
export const TRADING = 'trading';

/** The broker OpenTraderWorld runs itself: orders fill on the chart's live price. */
export const PAPER = 'paper';

/** Whether trading was switched on for an account. */
export const trades = (account) => (account?.modules ?? []).includes(TRADING);

/** The halts in force on an account (its own and the ones on every account), from a risk
 *  read. One whose end has passed is left out: the server stopped counting it. */
export function haltsOn(risk, accountId, now = Date.now()) {
  return (risk?.halts ?? []).filter(
    (h) => (h.account_id == null || h.account_id === accountId) && (h.until == null || Date.parse(h.until) > now)
  );
}

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
