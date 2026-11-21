// The reminder being edited, shared by every place a reminder is created: the RemindMe
// form, the bell button on other pages, the dashboard widget and the calendar event form.
// One set of defaults, one validation, one payload builder, so a reminder made from any of
// them is the same reminder.
import { dateKey } from '$lib/format.js';

export const DEFAULT_TIME = '09:00';

/** Local day the default time still lies ahead: today before 09:00, else tomorrow. */
export function defaultStartDate(now = new Date()) {
  const [h, m] = DEFAULT_TIME.split(':').map(Number);
  const at = new Date(now);
  at.setHours(h, m, 0, 0);
  if (at > now) return dateKey(now);
  const tomorrow = new Date(now);
  tomorrow.setDate(tomorrow.getDate() + 1);
  return dateKey(tomorrow);
}

/** The form state for a new reminder, or for `initial` (a row, or seed fields). */
export function blankReminder(initial = null) {
  // Local day, not UTC: a reminder created at 23:30 must default to today, not tomorrow.
  const today = dateKey();
  const base = {
    name: '',
    kind: 'custom',
    linked_id: '',
    details: '',
    url: '',
    link_label: '',
    frequency: 'once',
    weekdays: null,
    every_minutes: 60,
    repeat_every_minutes: null,
    repeat_count: '',
    start_date: defaultStartDate(),
    start_time: DEFAULT_TIME,
    end_date: '',
    max_count: '',
    active: true
  };
  if (initial) {
    return {
      ...base,
      ...initial,
      linked_id: initial.linked_id ?? '',
      details: initial.details ?? '',
      url: initial.url ?? '',
      link_label: initial.link_label ?? '',
      weekdays: initial.weekdays ?? null,
      every_minutes: initial.every_minutes ?? 60,
      repeat_every_minutes: initial.repeat_every_minutes ?? null,
      repeat_count: initial.repeat_count ?? '',
      start_date: initial.start_date ?? today,
      start_time: initial.start_time ?? DEFAULT_TIME,
      end_date: initial.end_date ?? '',
      max_count: initial.max_count ?? ''
    };
  }
  return base;
}

/** The browser accepts a bare "example.com" in a text input; https:// is what the server
 *  requires and what the user meant, so add it rather than rejecting the input. */
export function normalizeUrl(u) {
  const s = (u ?? '').trim();
  if (!s || /^https?:\/\//i.test(s)) return s;
  return `https://${s}`;
}

/** A link was typed that the server would refuse. */
export function linkInvalid(r) {
  return !!(r.url ?? '').trim() && !/^https?:\/\/\S+$/i.test(normalizeUrl(r.url));
}

/** A `once` reminder whose start already passed fires immediately on the next tick. */
export function startsInPast(r, now = new Date()) {
  if (r.frequency !== 'once' || !r.active || !r.start_date) return false;
  const at = new Date(`${r.start_date}T${r.start_time || '00:00'}`);
  return !Number.isNaN(at.getTime()) && at < now;
}

/** Browser UTC offset in minutes (getTimezoneOffset is inverted, so negate it), so the
 *  chosen local time fires at the right instant server-side. */
export const tzOffsetMinutes = () => -new Date().getTimezoneOffset();

/** The run of fires within a day, both set or both null (never on an interval reminder). */
function dayRun(r) {
  const every = Number(r.repeat_every_minutes) || 0;
  const count = Number(r.repeat_count) || 0;
  return r.frequency !== 'interval' && every > 0 && count > 1
    ? { repeat_every_minutes: every, repeat_count: count }
    : { repeat_every_minutes: null, repeat_count: null };
}

/** A run within the day whose last fire would land a day or more after the first. */
export function dayRunTooLong(r) {
  const { repeat_every_minutes: every, repeat_count: count } = dayRun(r);
  return !!every && (count - 1) * every >= 1440;
}

/** Form state -> the body POST/PATCH /api/reminders takes. */
export function reminderPayload(r) {
  return {
    name: r.name,
    kind: r.kind,
    linked_id: r.kind === 'custom' ? null : r.linked_id || null,
    details: r.details,
    url: normalizeUrl(r.url),
    link_label: (r.link_label ?? '').trim(),
    frequency: r.frequency,
    weekdays: r.frequency === 'weekly' ? (r.weekdays || null) : null,
    every_minutes: r.frequency === 'interval' ? Number(r.every_minutes) || 60 : null,
    ...dayRun(r),
    start_date: r.start_date || null,
    start_time: r.start_time || '00:00',
    tz_offset_minutes: tzOffsetMinutes(),
    end_date: r.end_date || null,
    max_count: r.max_count === '' || r.max_count === null ? null : Number(r.max_count),
    active: !!r.active
  };
}

/**
 * The reminder a calendar event carries: once, at the event's start (09:00 for an all-day
 * event, which has no clock time), named after it. `ev` holds `title`, `start_at` (RFC3339),
 * `all_day` and `notes`. Mirrors what the calendar page sends, so the form shows exactly
 * the reminder that will be created and kept in sync.
 */
export function reminderFromEvent(ev, fallbackName = '') {
  const d = new Date(ev.start_at);
  const pad = (n) => String(n).padStart(2, '0');
  return {
    name: ev.title || fallbackName,
    kind: 'custom',
    frequency: 'once',
    start_date: dateKey(d),
    start_time: ev.all_day ? DEFAULT_TIME : `${pad(d.getHours())}:${pad(d.getMinutes())}`,
    tz_offset_minutes: tzOffsetMinutes(),
    details: ev.notes || ''
  };
}
