/** Subscription Tracker API client: CRUD, suggestions, breakdown, prices, ratings, forecast,
 * iCal export and settings. Single-user. */
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

  prices: (id) => req(`/subscriptions/${id}/prices`).then((r) => r.prices),
  rate: (id, score) =>
    req(`/subscriptions/${id}/ratings`, { method: 'POST', body: JSON.stringify({ score }) }),
  forecast: () => req('/subscriptions/forecast'),

  /** Download the next 12 months of billing dates as an .ics file. */
  async downloadIcs() {
    const res = await fetch('/api/subscriptions/calendar.ics');
    redirectIfUnauthorized(res);
    if (!res.ok) {
      let body = null;
      try {
        body = await res.json();
      } catch {
        /* empty */
      }
      throw new Error(body?.message ?? body?.error ?? `request failed (${res.status})`);
    }
    const url = URL.createObjectURL(await res.blob());
    const a = document.createElement('a');
    a.href = url;
    a.download = 'subscriptions.ics';
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  },

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

// Billing math lives in calc.js (pure, unit-tested); re-exported for existing callers.
export { monthlyFactor, nextBillingDate } from './calc.js';

// Formatting lives in $lib/format.js. fmtDate there accepts a Date or an ISO string,
// which is what this module passes; fmtMonth is the old monthLabel.
export { fmtMoney, fmtMonth as monthLabel, fmtDate } from '$lib/format.js';
