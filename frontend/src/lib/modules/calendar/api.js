/** Calendar API client — personal-event CRUD. Single-user.
 *
 * Economics & Earnings tabs are embedded investing.com widgets and need no backend;
 * only personal events round-trip through here. Timestamps are RFC3339 strings. */
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

export const calendarApi = {
  /** List events; optionally bound to an RFC3339 [from, to) window. */
  list: (from, to) => {
    const qs = from && to ? `?from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}` : '';
    return req(`/calendar/events${qs}`).then((r) => r.events);
  },
  get: (id) => req(`/calendar/events/${id}`).then((r) => r.event),
  add: (ev) =>
    req('/calendar/events', { method: 'POST', body: JSON.stringify(ev) }).then((r) => r.event),
  update: (id, ev) =>
    req(`/calendar/events/${id}`, { method: 'PATCH', body: JSON.stringify(ev) }),
  remove: (id) => req(`/calendar/events/${id}`, { method: 'DELETE' })
};

/** Map a stored event row to a FullCalendar event object. */
export function toFcEvent(e) {
  return {
    id: e.id,
    title: e.title,
    start: e.start_at,
    end: e.end_at ?? undefined,
    allDay: e.all_day,
    backgroundColor: e.color || undefined,
    borderColor: e.color || undefined,
    extendedProps: { source: 'event', category: e.category, location: e.location, notes: e.notes }
  };
}

/* ── Overlay sources (reminders / todos / goals) ──────────────────────────────
   Dated items from other modules, shown as read-only, color-coded events. They
   carry `extendedProps.source` so the page can route clicks to the right module
   instead of opening the personal-event form. `editable:false` blocks drag/resize. */

const OVERLAY_COLORS = {
  reminder: '#f59e0b', // amber
  todo: '#22c55e', // green
  goal: '#a855f7' // violet
};

/** Reminders with a scheduled fire time -> FC events (timed). */
export function reminderToFcEvent(r) {
  if (!r.next_fire_at) return null;
  return {
    id: `reminder:${r.id}`,
    title: `⏰ ${r.name}`,
    start: r.next_fire_at,
    allDay: false,
    backgroundColor: OVERLAY_COLORS.reminder,
    borderColor: OVERLAY_COLORS.reminder,
    editable: false,
    extendedProps: { source: 'reminder', refId: r.id }
  };
}

/** ToDos with a due date -> FC events (all-day, or timed if due_time set). */
export function todoToFcEvent(t) {
  if (!t.due_date) return null;
  const timed = !!t.due_time;
  return {
    id: `todo:${t.id}`,
    title: `${t.done ? '✓ ' : '☐ '}${t.name}`,
    start: timed ? `${t.due_date}T${t.due_time}` : t.due_date,
    allDay: !timed,
    backgroundColor: OVERLAY_COLORS.todo,
    borderColor: OVERLAY_COLORS.todo,
    editable: false,
    classNames: t.done ? ['otw-overlay-done'] : [],
    extendedProps: { source: 'todo', refId: t.id }
  };
}

/** Goals with a deadline -> FC events (all-day). */
export function goalToFcEvent(g) {
  if (!g.deadline) return null;
  return {
    id: `goal:${g.id}`,
    title: `🎯 ${g.name}`,
    start: g.deadline,
    allDay: true,
    backgroundColor: OVERLAY_COLORS.goal,
    borderColor: OVERLAY_COLORS.goal,
    editable: false,
    extendedProps: { source: 'goal', refId: g.id }
  };
}

/** "<input type=datetime-local>" value (local, no tz) -> RFC3339 with local offset. */
export function localToRfc3339(local) {
  if (!local) return '';
  return new Date(local).toISOString();
}

/** RFC3339 -> "<input type=datetime-local>" value in local time. */
export function rfc3339ToLocal(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** RFC3339 -> "<input type=date>" value (local). */
export function rfc3339ToDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** "<input type=date>" value -> RFC3339 at local midnight. */
export function dateToRfc3339(date) {
  if (!date) return '';
  return new Date(date + 'T00:00:00').toISOString();
}
