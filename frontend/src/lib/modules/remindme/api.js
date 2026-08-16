/** RemindMe API client — reminder CRUD plus the notifications surface. Single-user. */
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

export const remindApi = {
  // Reminders
  list: () => req('/reminders').then((r) => r.reminders),
  get: (id) => req(`/reminders/${id}`).then((r) => r.reminder),
  add: (r) => req('/reminders', { method: 'POST', body: JSON.stringify(r) }).then((x) => x.reminder),
  update: (id, r) => req(`/reminders/${id}`, { method: 'PATCH', body: JSON.stringify(r) }),
  remove: (id) => req(`/reminders/${id}`, { method: 'DELETE' }),

  // Notifications
  notifications: (limit = 200) => req(`/notifications?limit=${limit}`), // { notifications, unread }
  unreadSince: (since) =>
    req(`/notifications/unread${since ? `?since=${encodeURIComponent(since)}` : ''}`), // { notifications, unread }
  ackAll: () => req('/notifications/ack-all', { method: 'POST' }),
  markRead: (id) => req(`/notifications/${id}/read`, { method: 'POST' }),
  removeNotif: (id) => req(`/notifications/${id}`, { method: 'DELETE' })
};

// External notification channels are not RemindMe's: they are the shared broker in
// $lib/notifications (configured in Settings → Notifications, granted per module).

export const KINDS = [
  { id: 'custom', label: 'Custom' },
  { id: 'goal', label: 'Goal' },
  { id: 'todo', label: 'ToDo' }
];

export const FREQUENCIES = [
  { id: 'once', label: 'Once' },
  { id: 'daily', label: 'Daily' },
  { id: 'weekly', label: 'Weekly' },
  { id: 'monthly', label: 'Monthly' },
  { id: 'yearly', label: 'Yearly' }
];

/** Weekly day toggles, in mask order (Mon = 1 … Sun = 64) — the routines convention. */
export const WEEKDAYS = [
  { bit: 1, key: 'mon' },
  { bit: 2, key: 'tue' },
  { bit: 4, key: 'wed' },
  { bit: 8, key: 'thu' },
  { bit: 16, key: 'fri' },
  { bit: 32, key: 'sat' },
  { bit: 64, key: 'sun' }
];

export const MASK_EVERY_DAY = 127;
export const MASK_WEEKDAYS = 31; // Mon–Fri
export const MASK_WEEKEND = 96; // Sat–Sun

/** Mask of the weekday a `YYYY-MM-DD` local date falls on (the default for a new weekly). */
export function maskForDate(iso) {
  const d = iso ? new Date(`${iso}T00:00:00`) : new Date();
  if (Number.isNaN(d.getTime())) return 1;
  return 1 << ((d.getDay() + 6) % 7); // JS Sunday=0 → Monday-first index
}

/** "Mon, Wed, Fri" / "Weekdays" — the day list of a weekly reminder, or '' if it has none. */
export function weekdaysLabel(mask, tr) {
  const m = Number(mask) & MASK_EVERY_DAY;
  if (!m) return '';
  if (m === MASK_EVERY_DAY) return tr('remindme.form.everyDay');
  if (m === MASK_WEEKDAYS) return tr('remindme.form.weekdays');
  if (m === MASK_WEEKEND) return tr('remindme.form.weekend');
  return WEEKDAYS.filter((d) => m & d.bit)
    .map((d) => tr(`routines.weekdayShort.${d.key}`))
    .join(', ');
}

export const freqLabel = (id) => FREQUENCIES.find((f) => f.id === id)?.label ?? id;
export const kindLabel = (id) => KINDS.find((k) => k.id === id)?.label ?? id;

/** The in-app route a notification's linked item points at (or the reminders list). */
export function linkFor(n) {
  if (n.kind === 'goal' && n.linked_id) return '/goals';
  if (n.kind === 'todo' && n.linked_id) return '/todos';
  // Watchlist price alerts carry the list they fired from in `url`.
  if (n.kind === 'watchlist') return n.url || '/watchlists';
  return '/remindme';
}

// fmtDate lives in $lib/format.js. The one call site guards with `?? '—'`, which the
// shared em-dash return makes redundant but not wrong.
export { fmtDate } from '$lib/format.js';

/**
 * "Mar 14, 14:32" — day and time, no year. Deliberately not the shared fmtDateTime
 * (which includes the year) nor fmtTime (which omits the day): a reminder list is
 * scanned within the current year, and both parts have to fit one narrow column.
 */
export function fmtTime(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  return d.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit'
  });
}
