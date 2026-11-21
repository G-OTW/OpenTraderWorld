// Automator schedule rule <-> shared recurrence model. The rule is a flat row (kind,
// every_minutes, at_hour, at_minute, weekdays, day_of_month, run_at, timezone, catch_up)
// that robots reuse for their sessions, so only the fields the user moved are written back.
import { patch, hhmm, isoToLocalInput, localInputToIso } from '$lib/ui/recurrence.js';

/** rule -> model. The time of day defaults to 08:00, as the picker always did. */
export function toModel(r) {
  return {
    kind: r.kind,
    everyMinutes: r.every_minutes ?? 60,
    time: hhmm(r.at_hour ?? 8, r.at_minute ?? 0),
    weekdays: r.weekdays ?? 0,
    monthDays: [r.day_of_month ?? 1],
    runAt: isoToLocalInput(r.run_at),
    timezone: r.timezone ?? ''
  };
}

/** model -> rule: `prev` with only the moved fields replaced. */
export function fromModel(m, prev) {
  return patch(prev, toModel(prev), m, [
    ['kind', (x) => ({ kind: x.kind })],
    ['everyMinutes', (x) => ({ every_minutes: x.everyMinutes })],
    [
      'time',
      (x) => {
        const [h, min] = (x.time || '').split(':');
        return { at_hour: Number(h) || 0, at_minute: Number(min) || 0 };
      }
    ],
    ['weekdays', (x) => ({ weekdays: x.weekdays })],
    ['monthDays', (x) => ({ day_of_month: x.monthDays[0] ?? 1 })],
    ['runAt', (x) => ({ run_at: localInputToIso(x.runAt) })],
    ['timezone', (x) => ({ timezone: x.timezone })]
  ]);
}
