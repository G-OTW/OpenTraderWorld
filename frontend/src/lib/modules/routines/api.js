/** Trading Routines API client.
 *
 * One board call per day: routines due that date (with per-item tick state) and the trailing
 * consistency strip. Routines are recurring checklists bound to a session (pre-market /
 * in session / post-market / anytime) and a weekday mask (Mon=1 … Sun=64). */
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

export const traderApi = {
  board: (date) => req(`/trader/board${date ? `?date=${date}` : ''}`),

  listRoutines: () => req('/trader/routines').then((r) => r.routines),
  routineDetail: (id) => req(`/trader/routines/${id}`),
  createRoutine: (body) =>
    req('/trader/routines', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.routine),
  updateRoutine: (id, patch) =>
    req(`/trader/routines/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.routine
    ),
  deleteRoutine: (id) => req(`/trader/routines/${id}`, { method: 'DELETE' }),

  checkItem: (itemId, date, checked) =>
    req(`/trader/items/${itemId}/check`, {
      method: 'POST',
      body: JSON.stringify({ date, checked })
    })
};

/** Session buckets, in trading-day order. */
export const SESSIONS = [
  { key: 'pre', label: 'Pre-market', icon: '☀️' },
  { key: 'live', label: 'In session', icon: '📈' },
  { key: 'post', label: 'Post-market', icon: '🌙' },
  { key: 'any', label: 'Anytime', icon: '📌' }
];

/** Weekday toggles in mask order (Mon=1 … Sun=64). */
export const WEEKDAYS = [
  { bit: 1, label: 'M' },
  { bit: 2, label: 'T' },
  { bit: 4, label: 'W' },
  { bit: 8, label: 'T' },
  { bit: 16, label: 'F' },
  { bit: 32, label: 'S' },
  { bit: 64, label: 'S' }
];

/** Starter routines offered on an empty board. */
export const STARTER_ROUTINES = [
  {
    name: 'Pre-market prep',
    session: 'pre',
    weekdays: 31,
    items: [
      'Review overnight news & futures',
      'Check the economic calendar for releases',
      'Mark key levels on your watchlist',
      'Define max loss for the day',
      'Write your plan: setups you will (and will not) take'
    ]
  },
  {
    name: 'In-session discipline',
    session: 'live',
    weekdays: 31,
    items: [
      'Only take planned setups',
      'Respect position size rules',
      'Log every trade as you take it',
      'Step away after 2 consecutive losses'
    ]
  },
  {
    name: 'Post-market review',
    session: 'post',
    weekdays: 31,
    items: [
      'Journal every trade (screenshots + reasoning)',
      'Grade your execution, not the outcome',
      'Note one thing to improve tomorrow',
      'Close the platform — no revenge trading'
    ]
  }
];
