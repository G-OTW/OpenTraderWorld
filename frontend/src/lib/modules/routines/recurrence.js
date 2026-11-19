// Routines <-> shared recurrence model. A routine stores a `schedule` object (one shape per
// kind) plus an optional active window (`start_date`, `end_date`); see RecurrencePicker.
import { patch } from '$lib/ui/recurrence.js';
import { MASK_WEEKDAYS, defaultSchedule, todayStr } from './api.js';

const sortDays = (days) => [...days].sort((a, b) => a - b);

/** The fields each kind owns in the stored schedule, as model key -> payload writer. */
const FIELDS = {
  weekly: [
    ['weekdays', (m) => ({ weekdays: m.weekdays })],
    ['every', (m) => ({ interval: m.every })]
  ],
  daily: [['every', (m) => ({ interval: m.every })]],
  monthly: [['monthDays', (m) => ({ days: sortDays(m.monthDays) })]],
  nth_weekday: [
    ['nth', (m) => ({ nth: m.nth })],
    ['weekday', (m) => ({ weekday: m.weekday })]
  ],
  once: [['date', (m) => ({ date: m.date })]]
};

/** A fresh schedule of the model's kind, holding only that kind's fields. */
function build(m) {
  switch (m.kind) {
    case 'weekly':
      return { kind: 'weekly', weekdays: m.weekdays, interval: m.every };
    case 'daily':
      return { kind: 'daily', interval: m.every };
    case 'monthly':
      return { kind: 'monthly', days: sortDays(m.monthDays) };
    case 'nth_weekday':
      return { kind: 'nth_weekday', weekday: m.weekday, nth: m.nth };
    case 'once':
      return { kind: 'once', date: m.date };
    default:
      return { kind: m.kind };
  }
}

/** The rule part of the model for a stored schedule (no window). */
export function ruleOf(schedule) {
  const s = schedule ?? defaultSchedule();
  return {
    kind: s.kind,
    every: s.interval ?? 1,
    weekdays: s.weekdays ?? 0,
    monthDays: s.days ?? [],
    nth: s.nth ?? 1,
    weekday: s.weekday ?? 0,
    // The picker always showed today for a once rule with no date yet.
    date: s.date ?? todayStr()
  };
}

/** `{ schedule, start_date, end_date }` -> model. */
export function toModel(p) {
  return { ...ruleOf(p.schedule), start: p.start_date ?? '', end: p.end_date ?? '' };
}

/** model -> `{ schedule, start_date, end_date }`, keeping every field the user left alone. */
export function fromModel(m, prev) {
  const before = toModel(prev);
  const s = prev.schedule ?? defaultSchedule();
  let schedule = prev.schedule;
  if (m.kind !== s.kind) schedule = build(m);
  else {
    const patched = patch(s, before, m, FIELDS[m.kind] ?? []);
    if (patched !== s) schedule = patched;
  }
  const win = patch({ start_date: prev.start_date, end_date: prev.end_date }, before, m, [
    ['start', (x) => ({ start_date: x.start })],
    ['end', (x) => ({ end_date: x.end })]
  ]);
  return { schedule, start_date: win.start_date, end_date: win.end_date };
}

/** Switching kind builds a sensible rule of that kind, so the preview never goes blank. */
export function kindDefaults(kind, m) {
  switch (kind) {
    case 'weekly':
      return { weekdays: MASK_WEEKDAYS, every: 1 };
    case 'daily':
      return { every: 1 };
    case 'monthly':
      return { monthDays: [1] };
    case 'nth_weekday':
      return { weekday: 0, nth: 1 };
    case 'once':
      return { date: m.start || todayStr() };
    default:
      return {};
  }
}
