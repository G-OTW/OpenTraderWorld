/** Trading Robots API client.
 *
 * A robot (stored as a paper session) is a backtest left running forward: the settings frozen at the moment the button
 * was pressed, the datasets they read, a recurrence rule and how the result is reported. The
 * server re-runs the ordinary engine on each tick and diffs the trades, so what a session
 * shows is what the backtest showed, never a second simulator's opinion.
 *
 * The message templates are the user's: `variables()` returns the paths a template may read
 * (generated server-side from the renderer's own context, so the helper list cannot drift)
 * and `previewDraft()` renders what is being typed against a sample trade. */
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
  // `code` says what the caller can do about a refusal (`background_trading_required`: sign
  // it; `code_required` or `trading_locked`: ask for the authenticator code).
  if (!res.ok) throw Object.assign(new Error(body?.error ?? `request failed (${res.status})`), { code: body?.code ?? null });
  return body;
}

const json = (method, body) => ({ method, body: JSON.stringify(body) });

export const robotsApi = {
  /** The list page in one call: every robot with its equity, today's P&L, open count, guard
   *  badge, environment and next tick. */
  fleet: (signal) => req('/robots/fleet', { signal }),
  /** Every session plus the badge counters { active, errored, unseen } and the guard set a
   *  new robot is offered (`guard_defaults`). */
  list: () => req('/robots'),
  /** One session with its open positions and its recent events. */
  get: (id) => req(`/robots/${id}`),
  dashboard: (id, signal) => req(`/robots/${id}/dashboard`, { signal }),
  /** Multiplexed market prices using the backtest module's connector grants. */
  streamsUrl: (instruments) => {
    const items = instruments.map(({ provider, asset_type, ticker, timeframe, connector_id, since }) => ({
      provider, asset_type, ticker, timeframe, ...(connector_id ? { connector: connector_id } : {}), ...(since ? { since } : {})
    }));
    return `/api/robots/streams?instruments=${encodeURIComponent(JSON.stringify(items))}`;
  },
  create: (input) => req('/robots', json('POST', input)),
  update: (id, input) => req(`/robots/${id}`, json('PUT', input)),
  remove: (id) => req(`/robots/${id}`, { method: 'DELETE' }),
  /** Pause or resume. Resuming re-plans from now, so a long pause fires no catch-up burst. */
  setStatus: (id, status) => req(`/robots/${id}/status`, json('POST', { status })),
  /** Tick now, ignoring the freshness short-circuit. */
  runNow: (id) => req(`/robots/${id}/run`, { method: 'POST' }),
  /** The paper book. `open` narrows to the live positions. */
  trades: (id, open = false) => req(`/robots/${id}/trades?open=${open}`),
  /** Template vocabulary + the filters the renderer accepts. */
  /** The vocabulary of one message kind ('open' | 'close' | 'digest'): each resolves a
   *  different context, so the helper list is per kind. */
  variables: (session = null, kind = 'close') => {
    const qs = new URLSearchParams({ kind });
    if (session) qs.set('session', session);
    return req(`/robots/variables?${qs}`);
  },
  /** Render a template that is not saved yet. `event` picks the sample: open, close or
   *  digest (the grouped message, rendered on the period aggregates). */
  previewDraft: (input) => req('/robots/preview', json('POST', input)),
  /** The inbox: events the engine could not deliver are still here on the next login. */
  events: ({ session = null, unseen = false } = {}) => {
    const q = new URLSearchParams();
    if (session) q.set('session', session);
    if (unseen) q.set('unseen', 'true');
    const qs = q.toString();
    return req(`/robots/events${qs ? `?${qs}` : ''}`).then((r) => r.events);
  },
  markSeen: (session = null) =>
    req('/robots/events/seen', json('POST', { session })),
  /** A broker session's execution panel: intended vs broker book, orders, fills simulated vs
   *  real, drift, the account's level and limits. Read from the last sync: no broker call. */
  execution: (id, signal) => req(`/robots/${id}/execution`, { signal }),
  /** Arm: from now on the session may send orders with nobody watching. `code` is the
   *  authenticator code when the trading lock is on and the account is not paper. */
  arm: (id, code = '') => req(`/robots/${id}/arm`, json('PUT', { code })),
  /** Disarm: nothing more is sent; what works at the broker stays there. */
  disarm: (id) => req(`/robots/${id}/arm`, { method: 'DELETE' }),
  /** Stop and flatten: disarm, cancel the session's orders, close what it holds. */
  flatten: (id, code = '') => req(`/robots/${id}/flatten`, json('POST', { code })),
  /** Accept what the account holds outside the strategy, so a degraded session trades again. */
  adopt: (id) => req(`/robots/${id}/adopt`, { method: 'POST' }),
  /** Clear what the guards hold (a halt, a pause, a scale): full size from the next close. */
  resume: (id) => req(`/robots/${id}/resume`, { method: 'POST' }),
  /** Re-freeze on the saved strategy's current version; the book is learned again. */
  refreeze: (id) => req(`/robots/${id}/strategy`, { method: 'POST' }),
  /** The guard timeline, newest first, with what the guards hold now. */
  guardEvents: (id, limit = 100) => req(`/robots/${id}/guards/events?limit=${limit}`)
};

/** A new robot pre-filled from a run or a saved strategy. Fills always leave on the strategy's own
 *  clock; `cadence` is the periodic recap over them, off by default. */
export function defaultSession(name, datasetIds, settings) {
  return {
    name,
    settings,
    dataset_ids: datasetIds,
    window_bars: 5000,
    start_ts: null,
    feed: 'hist',
    kind: 'interval',
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
    every_minutes: 60,
    at_hour: 9,
    at_minute: 0,
    weekdays: 31, // Monday to Friday (Monday is bit 0)
    day_of_month: 1,
    run_at: null,
    notify_when: 'on_change',
    cadence: 'each',
    channel_ids: [],
    entry_title_template: '',
    entry_template: '',
    exit_title_template: '',
    exit_template: '',
    summary_title_template: '',
    summary_template: '',
    notify_open: true,
    notify_close: true,
    notify_tickers: [],
    status: 'active',
    // Where the fills happen: null simulates; { target: 'broker', account_id, exits,
    // max_position_qty } executes on a broker account. Set once, at creation.
    execution: null,
    // The robot's layers: lower runs first; the capital policy; the guard rules.
    priority: 100,
    capital: {},
    guards: [],
    // Periods with no new entry, in the robot's timezone.
    exclusions: {},
    strategy_id: null
  };
}

/** Where the bars come from. `live` is declared but not served yet: the server refuses it,
 *  so the picker shows it as what is coming rather than as a silent no-op. */
export const FEEDS = ['hist', 'live'];
export const SCHEDULE_KINDS = ['interval', 'daily', 'weekly', 'monthly', 'once'];
/** The three messages a session can send, each with its own wording and its own
 *  vocabulary. `event` is what the preview and the variable list are keyed on. */
export const MESSAGE_KINDS = [
  { id: 'entry', event: 'open', title: 'entry_title_template', body: 'entry_template' },
  { id: 'exit', event: 'close', title: 'exit_title_template', body: 'exit_template' },
  { id: 'summary', event: 'digest', title: 'summary_title_template', body: 'summary_template' }
];

export const NOTIFY_WHEN = ['always', 'on_change', 'never'];
/** `each`: the fills are the report. `scheduled`: a status report on the recurrence rule too. */
export const CADENCES = ['each', 'scheduled'];

/** Weekday bitmask helpers — Monday is bit 0, matching the server's rule. */
export const weekdayBit = (i) => 1 << i;
export const hasWeekday = (mask, i) => ((mask ?? 0) & weekdayBit(i)) !== 0;
export const toggleWeekday = (mask, i) => (mask ?? 0) ^ weekdayBit(i);
