/** Paper trading API client.
 *
 * A session is a backtest left running forward: the settings frozen at the moment the button
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
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

const json = (method, body) => ({ method, body: JSON.stringify(body) });

export const paperApi = {
  /** Every session plus the badge counters { active, errored, unseen }. */
  list: () => req('/backtest/paper'),
  /** One session with its open positions and its recent events. */
  get: (id) => req(`/backtest/paper/${id}`),
  create: (input) => req('/backtest/paper', json('POST', input)),
  update: (id, input) => req(`/backtest/paper/${id}`, json('PUT', input)),
  remove: (id) => req(`/backtest/paper/${id}`, { method: 'DELETE' }),
  /** Pause or resume. Resuming re-plans from now, so a long pause fires no catch-up burst. */
  setStatus: (id, status) => req(`/backtest/paper/${id}/status`, json('POST', { status })),
  /** Tick now, ignoring the freshness short-circuit. */
  runNow: (id) => req(`/backtest/paper/${id}/run`, { method: 'POST' }),
  /** The paper book. `open` narrows to the live positions. */
  trades: (id, open = false) => req(`/backtest/paper/${id}/trades?open=${open}`),
  /** Template vocabulary + the filters the renderer accepts. */
  /** The vocabulary of one message kind ('open' | 'close' | 'digest'): each resolves a
   *  different context, so the helper list is per kind. */
  variables: (session = null, kind = 'close') => {
    const qs = new URLSearchParams({ kind });
    if (session) qs.set('session', session);
    return req(`/backtest/paper/variables?${qs}`);
  },
  /** Render a template that is not saved yet. `event` picks the sample: open, close or
   *  digest (the grouped message, rendered on the period aggregates). */
  previewDraft: (input) => req('/backtest/paper/preview', json('POST', input)),
  /** The inbox: events the engine could not deliver are still here on the next login. */
  events: ({ session = null, unseen = false } = {}) => {
    const q = new URLSearchParams();
    if (session) q.set('session', session);
    if (unseen) q.set('unseen', 'true');
    const qs = q.toString();
    return req(`/backtest/paper/events${qs ? `?${qs}` : ''}`).then((r) => r.events);
  },
  markSeen: (session = null) =>
    req('/backtest/paper/events/seen', json('POST', { session }))
};

/** A new session pre-filled from the run on screen. Fills always leave on the strategy's own
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
    status: 'active'
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
export const CADENCES = ['each', 'hourly', 'daily', 'weekly'];

/** Weekday bitmask helpers — Monday is bit 0, matching the server's rule. */
export const weekdayBit = (i) => 1 << i;
export const hasWeekday = (mask, i) => ((mask ?? 0) & weekdayBit(i)) !== 0;
export const toggleWeekday = (mask, i) => (mask ?? 0) ^ weekdayBit(i);
