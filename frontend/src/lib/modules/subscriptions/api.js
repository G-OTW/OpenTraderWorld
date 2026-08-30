/** Subscription Tracker API client — CRUD, suggestions, breakdown, settings. Single-user. */
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

function qs(filter = {}) {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(filter)) {
    if (v !== undefined && v !== null && v !== '') p.set(k, v);
  }
  const s = p.toString();
  return s ? `?${s}` : '';
}

export const subsApi = {
  list: (filter = {}) => req(`/subscriptions${qs(filter)}`).then((r) => r.subscriptions),
  get: (id) => req(`/subscriptions/${id}`).then((r) => r.subscription),
  add: (sub) =>
    req('/subscriptions', { method: 'POST', body: JSON.stringify(sub) }).then((r) => r.subscription),
  update: (id, sub) =>
    req(`/subscriptions/${id}`, { method: 'PATCH', body: JSON.stringify(sub) }),
  remove: (id) => req(`/subscriptions/${id}`, { method: 'DELETE' }),

  suggestions: () => req('/subscriptions/suggestions'), // { platforms, categories }
  breakdown: (filter = {}) => req(`/subscriptions/breakdown${qs(filter)}`).then((r) => r.breakdown),

  getSettings: () => req('/subscriptions/settings').then((r) => r.settings),
  updateSettings: (patch) =>
    req('/subscriptions/settings', { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.settings
    )
};

/** Currencies (mirror of the backend whitelist; shared with the journal). */
export const CURRENCIES = [
  'USD',
  'EUR',
  'GBP',
  'JPY',
  'CNY',
  'CHF',
  'CAD',
  'AUD',
  'HKD',
  'SEK',
  'NOK',
  'DKK'
];

export const FREQUENCIES = [
  { id: 'weekly', label: 'Weekly' },
  { id: 'monthly', label: 'Monthly' },
  { id: 'quarterly', label: 'Quarterly' },
  { id: 'yearly', label: 'Yearly' }
];

/** Monthly-equivalent factor (mirrors the backend normalisation). */
export function monthlyFactor(freq) {
  switch (freq) {
    case 'weekly':
      return 52 / 12;
    case 'quarterly':
      return 1 / 3;
    case 'yearly':
      return 1 / 12;
    default:
      return 1;
  }
}

// Formatting lives in $lib/format.js. fmtDate there accepts a Date or an ISO string,
// which is what this module passes; fmtMonth is the old monthLabel.
export { fmtMoney, fmtMonth as monthLabel, fmtDate } from '$lib/format.js';

/** Add `n` periods of `frequency` to a Date (mutates a copy), returning a new Date. */
function addPeriod(d, frequency) {
  const out = new Date(d);
  switch (frequency) {
    case 'weekly':
      out.setDate(out.getDate() + 7);
      break;
    case 'quarterly':
      out.setMonth(out.getMonth() + 3);
      break;
    case 'yearly':
      out.setFullYear(out.getFullYear() + 1);
      break;
    default:
      out.setMonth(out.getMonth() + 1);
  }
  return out;
}

/**
 * Next billing date for a subscription: the first occurrence of its cadence (anchored on
 * `started_on`) that is today or later. Returns a Date, or null if there's no start anchor.
 */
export function nextBillingDate(sub) {
  if (!sub.started_on) return null;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  let d = new Date(sub.started_on + 'T00:00:00');
  // Walk forward in whole periods until we reach/pass today (cap iterations as a safeguard).
  let guard = 0;
  while (d < today && guard < 5000) {
    d = addPeriod(d, sub.frequency);
    guard++;
  }
  return d;
}

