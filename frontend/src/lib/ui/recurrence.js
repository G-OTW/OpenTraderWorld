// The recurrence model the shared RecurrencePicker edits. Reminders, routines and
// automator schedules each store their rule in their own payload; a per-module adapter
// (`toModel` / `fromModel`) maps that payload to this one shape and back, so the picker
// knows nothing about any module and no module changes what it saves.
//
// Model fields (each module uses the subset its payload can carry):
//   kind          'once' | 'interval' | 'daily' | 'weekly' | 'monthly' | 'nth_weekday' | 'yearly'
//   every         every N days (daily) or N weeks (weekly)
//   everyMinutes  the period of an `interval` rule
//   weekdays      weekday mask, Mon = 1 … Sun = 64
//   monthDays     days of the month, -1 = the last day
//   nth, weekday  the nth (1..5, -1 = last) weekday (0 = Mon) of the month
//   date          the day of a `once` rule that has its own date (YYYY-MM-DD)
//   runAt         the local date and time of a `once` rule that is an instant (YYYY-MM-DDTHH:MM)
//   time          time of day (HH:MM)
//   start, end    the active window or the start/end dates (YYYY-MM-DD, '' = none)
//   maxCount      how many times at most ('' = no cap)
//   timezone      IANA zone the time of day is read in

/** Weekday toggles in mask order (Mon = 1 … Sun = 64). */
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
export const MASK_WEEKDAYS = 31; // Mon to Fri
export const MASK_WEEKEND = 96; // Sat and Sun

const pad = (n) => String(n).padStart(2, '0');

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

/**
 * The part of a model that says *when* it fires, by kind. Two models with the same key are
 * the same rule, whatever else differs: this is what marks a preset chip as active.
 */
export function ruleKey(m) {
  if (!m) return null;
  switch (m.kind) {
    case 'weekly':
      return ['weekly', m.weekdays ?? 0, m.every ?? 1];
    case 'daily':
      return ['daily', m.every ?? 1];
    case 'monthly':
      return ['monthly', [...(m.monthDays ?? [])].sort((a, b) => a - b)];
    case 'nth_weekday':
      return ['nth_weekday', m.nth, m.weekday];
    case 'once':
      return ['once', m.date ?? '', m.runAt ?? ''];
    case 'interval':
      return ['interval', m.everyMinutes];
    default:
      return [m.kind];
  }
}

export const sameRule = (a, b) => same(ruleKey(a), ruleKey(b));

/**
 * Write back into `prev` only the payload fields whose model value moved between `before`
 * and `after`. An untouched field keeps its stored value as it was (even a legacy or
 * missing one), so opening an editor and saving without a change writes the row back
 * unchanged. `fields` is a list of `[modelKey, (after) => ({ payloadKey: value })]`.
 * Returns `prev` itself when nothing moved.
 */
export function patch(prev, before, after, fields) {
  let out = prev;
  for (const [key, write] of fields) {
    if (!same(before[key], after[key])) out = { ...out, ...write(after) };
  }
  return out;
}

/** An ISO instant as the value of a datetime-local input, in the browser's own zone. */
export function isoToLocalInput(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** A datetime-local value back to an ISO instant, or null when empty or invalid. */
export function localInputToIso(v) {
  if (!v) return null;
  const d = new Date(v);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

/** "HH:MM" from an hour and a minute. */
export const hhmm = (h, m) => `${pad(h ?? 0)}:${pad(m ?? 0)}`;
