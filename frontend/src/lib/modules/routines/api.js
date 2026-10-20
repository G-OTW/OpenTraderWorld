/** Trading Routines API client.
 *
 * The module has two pages over one dataset. **Board** (`/routines`) is the day view: one
 * call returns the templates due that date with per-item tick state, plus the consistency
 * strip. **Templates** (`/routines/templates`) is the authoring view: categories and full
 * CRUD on a template's name, description, notes, recurrence and checklist.
 *
 * Recurrence is a `schedule` object, not a weekday mask — see SCHEDULE_KINDS and
 * describeSchedule() for the shapes and their human labels. */
import { redirectIfUnauthorized } from '$lib/auth.js';
import { fmtLocal, weekdayIndex, startOfDay, parseLocal, todayStr } from '$lib/ui/consistency.js';

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

  // Templates. `listRoutines` returns [{ routine, items }] so the list can show step
  // counts and a preview without a detail call per row.
  listRoutines: () => req('/trader/routines'),
  routineDetail: (id) => req(`/trader/routines/${id}`),
  createRoutine: (body) =>
    req('/trader/routines', { method: 'POST', body: JSON.stringify(body) }).then((r) => r.routine),
  updateRoutine: (id, patch) =>
    req(`/trader/routines/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.routine
    ),
  deleteRoutine: (id) => req(`/trader/routines/${id}`, { method: 'DELETE' }),
  duplicateRoutine: (id, name) =>
    req(`/trader/routines/${id}/duplicate`, {
      method: 'POST',
      body: JSON.stringify({ name })
    }).then((r) => r.routine),
  reorderRoutines: (ids) =>
    req('/trader/routines/reorder', { method: 'POST', body: JSON.stringify({ ids }) }),

  // Categories.
  listCategories: () => req('/trader/categories').then((r) => r.categories),
  createCategory: (name, color) =>
    req('/trader/categories', { method: 'POST', body: JSON.stringify({ name, color }) }).then(
      (r) => r.category
    ),
  updateCategory: (id, patch) =>
    req(`/trader/categories/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.category
    ),
  deleteCategory: (id) => req(`/trader/categories/${id}`, { method: 'DELETE' }),

  checkItem: (itemId, date, checked) =>
    req(`/trader/items/${itemId}/check`, {
      method: 'POST',
      body: JSON.stringify({ date, checked })
    }),

  // Consistency day marks.
  listMarks: (from, to) =>
    req(`/trader/marks?from=${from}&to=${to}`).then((r) => r.marks),
  setMark: (date, mark) =>
    req('/trader/marks', { method: 'PUT', body: JSON.stringify({ date, mark }) })
};

/** Legacy session buckets. Categories replaced them in the UI; the field is still written
 *  so the dashboard widget and older clients keep grouping a template. */
export const SESSIONS = [
  { key: 'pre', label: 'Pre-market', icon: '☀️' },
  { key: 'live', label: 'In session', icon: '📈' },
  { key: 'post', label: 'Post-market', icon: '🌙' },
  { key: 'any', label: 'Anytime', icon: '📌' }
];

/** Weekday toggles in mask order (Mon=1 … Sun=64). Local to this module: the mask bits are
 *  a scheduling concept, not part of the shared consistency vocabulary. */
export const WEEKDAYS = [
  { bit: 1, index: 0, key: 'mon' },
  { bit: 2, index: 1, key: 'tue' },
  { bit: 4, index: 2, key: 'wed' },
  { bit: 8, index: 3, key: 'thu' },
  { bit: 16, index: 4, key: 'fri' },
  { bit: 32, index: 5, key: 'sat' },
  { bit: 64, index: 6, key: 'sun' }
];

export const MASK_EVERY_DAY = 127;
export const MASK_WEEKDAYS = 31; // Mon–Fri
export const MASK_WEEKEND = 96; // Sat–Sun

/** The recurrence kinds the picker offers, in order of how often they're wanted. */
export const SCHEDULE_KINDS = ['weekly', 'daily', 'monthly', 'nth_weekday', 'once'];

/** A fresh weekly-on-weekdays rule — the default a new template opens with. */
export function defaultSchedule() {
  return { kind: 'weekly', weekdays: MASK_WEEKDAYS, interval: 1 };
}

/** Read a routine's rule, tolerating rows written before schedules existed. */
export function scheduleOf(routine) {
  return routine?.schedule ?? { kind: 'weekly', weekdays: routine?.weekdays ?? 127, interval: 1 };
}

// ── Consistency marks ────────────────────────────────────────────────────────
// The day-mark vocabulary and the local-date helpers are shared with the mindset module
// (same two verdicts, same week/month/year maths), so they live in $lib/ui/consistency.js
// and are re-exported here for this module's own components.
export {
  MARK_CYCLE,
  nextMark,
  MARK_COLORS,
  CATEGORY_COLORS,
  fmtLocal,
  todayStr,
  parseLocal,
  weekdayIndex,
  periodRange,
  periodStats,
  PERIODS
} from '$lib/ui/consistency.js';

// ── Recurrence evaluation ────────────────────────────────────────────────────
// A mirror of `Schedule::occurs_on` in otw-store. It exists so the template editor can
// draw a live preview calendar without a round trip per keystroke; the server stays the
// authority for what the board actually shows.

/** Whether `schedule` fires on the local Date `d`. `anchor` is the template's start date. */
export function occursOn(schedule, d, anchor = null) {
  if (!schedule) return false;
  const a = anchor ? parseLocal(anchor) : new Date(1970, 0, 5); // epoch Monday
  const dayMs = 86400000;
  const wholeDays = (x, y) => Math.round((startOfDay(x) - startOfDay(y)) / dayMs);

  switch (schedule.kind) {
    case 'daily': {
      const n = Math.max(1, schedule.interval ?? 1);
      return mod(wholeDays(d, a), n) === 0;
    }
    case 'weekly': {
      const bit = 1 << weekdayIndex(d);
      if (((schedule.weekdays ?? 0) & bit) === 0) return false;
      const n = Math.max(1, schedule.interval ?? 1);
      if (n === 1) return true;
      const weeks = Math.round(wholeDays(mondayOf(d), mondayOf(a)) / 7);
      return mod(weeks, n) === 0;
    }
    case 'monthly': {
      const dom = d.getDate();
      const last = daysInMonth(d);
      return (schedule.days ?? []).some((x) => (x === -1 ? dom === last : x === dom));
    }
    case 'nth_weekday': {
      if (weekdayIndex(d) !== schedule.weekday) return false;
      const dom = d.getDate();
      return schedule.nth === -1
        ? dom + 7 > daysInMonth(d)
        : Math.floor((dom - 1) / 7) + 1 === schedule.nth;
    }
    case 'once':
      return schedule.date === fmtLocal(d);
    default:
      return false;
  }
}

function mondayOf(d) {
  const m = startOfDay(d);
  m.setDate(m.getDate() - weekdayIndex(d));
  return m;
}
function mod(a, n) {
  return ((a % n) + n) % n;
}

/** Whether a template is due on `d`: inside its window and matching its rule. */
export function isDue(routine, d) {
  const day = fmtLocal(d);
  if (routine.start_date && day < routine.start_date) return false;
  if (routine.end_date && day > routine.end_date) return false;
  return occursOn(scheduleOf(routine), d, routine.start_date);
}

/** The next `count` dates a rule fires on, starting from `from` — the editor's preview. */
export function nextOccurrences(routine, from = new Date(), count = 5, horizonDays = 400) {
  const out = [];
  const d = startOfDay(from);
  for (let i = 0; i < horizonDays && out.length < count; i++) {
    if (isDue(routine, d)) out.push(fmtLocal(d));
    d.setDate(d.getDate() + 1);
  }
  return out;
}

/** Human summary of a rule ("Weekdays", "Every 2 weeks on Mon, Wed", "1st Thursday").
 *  `tr` is the $t translator; the caller passes it so this stays a pure function. */
export function describeSchedule(routine, tr) {
  const s = scheduleOf(routine);
  const dayName = (i) => tr(`routines.weekday.${WEEKDAYS[i].key}`);

  switch (s.kind) {
    case 'daily':
      return (s.interval ?? 1) === 1
        ? tr('routines.schedule.everyDay')
        : tr('routines.schedule.everyNDays', { n: s.interval });
    case 'weekly': {
      const mask = s.weekdays ?? 0;
      let days;
      if (mask === MASK_EVERY_DAY) days = tr('routines.schedule.everyDay');
      else if (mask === MASK_WEEKDAYS) days = tr('routines.schedule.weekdays');
      else if (mask === MASK_WEEKEND) days = tr('routines.schedule.weekend');
      else days = WEEKDAYS.filter((w) => mask & w.bit).map((w) => dayName(w.index)).join(', ');
      return (s.interval ?? 1) === 1
        ? days
        : tr('routines.schedule.everyNWeeksOn', { n: s.interval, days });
    }
    case 'monthly': {
      const days = (s.days ?? [])
        .map((d) => (d === -1 ? tr('routines.schedule.lastDay') : d))
        .join(', ');
      return tr('routines.schedule.monthlyOn', { days });
    }
    case 'nth_weekday': {
      const ord =
        s.nth === -1 ? tr('routines.schedule.ordLast') : tr(`routines.schedule.ord${s.nth}`);
      return tr('routines.schedule.nthWeekday', { ord, day: dayName(s.weekday) });
    }
    case 'once':
      return tr('routines.schedule.onceOn', { date: s.date });
    default:
      return '';
  }
}

/** The active-window suffix shown next to a schedule ("until 12 Sep", "1 Sep – 30 Nov"). */
export function describeWindow(routine, tr) {
  const { start_date: a, end_date: b } = routine;
  if (a && b) return tr('routines.schedule.windowBetween', { from: a, to: b });
  if (a) return tr('routines.schedule.windowFrom', { from: a });
  if (b) return tr('routines.schedule.windowUntil', { to: b });
  return '';
}

/** Ready-made rules the picker offers as one click, before any fine-tuning. */
export function schedulePresets(tr) {
  const today = todayStr();
  return [
    { id: 'weekdays', label: tr('routines.preset.weekdays'), make: () => ({ kind: 'weekly', weekdays: MASK_WEEKDAYS, interval: 1 }) },
    { id: 'everyday', label: tr('routines.preset.everyDay'), make: () => ({ kind: 'weekly', weekdays: MASK_EVERY_DAY, interval: 1 }) },
    { id: 'weekend', label: tr('routines.preset.weekend'), make: () => ({ kind: 'weekly', weekdays: MASK_WEEKEND, interval: 1 }) },
    { id: 'friday', label: tr('routines.preset.everyFriday'), make: () => ({ kind: 'weekly', weekdays: 16, interval: 1 }) },
    { id: 'biweekly', label: tr('routines.preset.everyOtherWeek'), make: () => ({ kind: 'weekly', weekdays: MASK_WEEKDAYS, interval: 2 }) },
    { id: 'monthstart', label: tr('routines.preset.firstOfMonth'), make: () => ({ kind: 'monthly', days: [1] }) },
    { id: 'monthend', label: tr('routines.preset.lastOfMonth'), make: () => ({ kind: 'monthly', days: [-1] }) },
    { id: 'once', label: tr('routines.preset.onceOnly'), make: () => ({ kind: 'once', date: today }) }
  ];
}

/** Ready-made active windows ("one week", "one month", "one year", open-ended). */
export function windowPresets(tr) {
  const shift = (days) => {
    const d = new Date();
    d.setDate(d.getDate() + days);
    return fmtLocal(d);
  };
  return [
    { id: 'always', label: tr('routines.window.always'), make: () => ({ start_date: null, end_date: null }) },
    { id: 'week', label: tr('routines.window.oneWeek'), make: () => ({ start_date: todayStr(), end_date: shift(7) }) },
    { id: 'month', label: tr('routines.window.oneMonth'), make: () => ({ start_date: todayStr(), end_date: shift(30) }) },
    { id: 'quarter', label: tr('routines.window.oneQuarter'), make: () => ({ start_date: todayStr(), end_date: shift(90) }) },
    { id: 'year', label: tr('routines.window.oneYear'), make: () => ({ start_date: todayStr(), end_date: shift(365) }) }
  ];
}
