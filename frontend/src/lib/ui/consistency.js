/** Shared vocabulary for the day-mark consistency widgets.
 *
 * Two modules keep a per-day verdict — the routines board and the mindset check-in — each
 * in its own table, but with the same two states and the same week/month/year arithmetic.
 * ConsistencyModal and CategoryManager read from here so neither belongs to a module. */

/** The two verdicts a day can carry, plus the empty state. Clicking a day cycles through
 *  them in this order — one click claims the best outcome, a second downgrades it, a
 *  third clears the day. */
export const MARK_CYCLE = ['full', 'action', null];

export function nextMark(current) {
  const i = MARK_CYCLE.indexOf(current ?? null);
  return MARK_CYCLE[(i + 1) % MARK_CYCLE.length];
}

export const MARK_COLORS = {
  full: 'var(--green)',
  action: 'var(--amber)'
};

/** Palette offered for a category accent — the theme's chart tokens, so a category colour
 *  stays consistent with every chart in the app and survives a theme switch. */
export const CATEGORY_COLORS = [
  'var(--chart-1)',
  'var(--chart-2)',
  'var(--chart-3)',
  'var(--chart-4)',
  'var(--chart-5)',
  'var(--chart-6)',
  'var(--green)',
  'var(--red)',
  'var(--amber)'
];

/** Weekday order used by every grid here (Mon first, matching the mask bit order). */
export const WEEKDAYS = [
  { bit: 1, index: 0, key: 'mon' },
  { bit: 2, index: 1, key: 'tue' },
  { bit: 4, index: 2, key: 'wed' },
  { bit: 8, index: 3, key: 'thu' },
  { bit: 16, index: 4, key: 'fri' },
  { bit: 32, index: 5, key: 'sat' },
  { bit: 64, index: 6, key: 'sun' }
];

// ── Local-date helpers ───────────────────────────────────────────────────────
// toISOString() is UTC and lands on the wrong day for any timezone offset from it, so
// every date these widgets format goes through fmtLocal.

export function fmtLocal(d) {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate()
  ).padStart(2, '0')}`;
}

export function todayStr() {
  return fmtLocal(new Date());
}

/** Parse a YYYY-MM-DD as a local midnight Date (never a UTC one). */
export function parseLocal(s) {
  return new Date(`${s}T00:00:00`);
}

/** Mon=0 … Sun=6 (JS getDay() has Sunday at 0). */
export function weekdayIndex(d) {
  return (d.getDay() + 6) % 7;
}

export function startOfDay(d) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

/** Inclusive [from, to] bounds of the period containing `d`. Weeks start Monday. */
export function periodRange(period, d = new Date()) {
  const y = d.getFullYear();
  const m = d.getMonth();
  if (period === 'week') {
    const start = new Date(y, m, d.getDate() - weekdayIndex(d));
    return [start, new Date(y, m, start.getDate() + 6)];
  }
  if (period === 'month') return [new Date(y, m, 1), new Date(y, m + 1, 0)];
  return [new Date(y, 0, 1), new Date(y, 11, 31)];
}

/** Marks kept, days counted and days elapsed for one period — the "12/31 days" readout.
 *  `total` stops at today inside a running period: a month is not 30 days of failure on
 *  the 3rd. */
export function periodStats(marks, period, d = new Date()) {
  const [from, to] = periodRange(period, d);
  const fromKey = fmtLocal(from);
  const toKey = fmtLocal(to);
  const inRange = (marks ?? []).filter((m) => m.day >= fromKey && m.day <= toKey);
  const full = inRange.filter((m) => m.mark === 'full').length;
  const action = inRange.filter((m) => m.mark === 'action').length;
  const end = to < d ? to : d;
  const elapsed = Math.round((startOfDay(end) - startOfDay(from)) / 86400000) + 1;
  return { full, action, done: full + action, total: Math.max(elapsed, 1) };
}

/** The period tabs both modules offer. */
export const PERIODS = ['week', 'month', 'year'];
