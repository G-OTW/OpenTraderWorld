// Reminder <-> shared recurrence model. A reminder stores `frequency`, a weekday mask for
// weekly ones, a start date and time (the anchor monthly and yearly repeat from), and an
// optional end date and fire cap.
import { patch } from '$lib/ui/recurrence.js';

/** reminder (row or form draft) -> model. */
export function toModel(r) {
  return {
    kind: r.frequency ?? 'once',
    weekdays: r.weekdays ?? 0,
    start: r.start_date ?? '',
    time: r.start_time ?? '',
    end: r.end_date ?? '',
    maxCount: r.max_count ?? ''
  };
}

/** model -> reminder: `prev` with only the moved fields replaced. */
export function fromModel(m, prev) {
  return patch(prev, toModel(prev), m, [
    ['kind', (x) => ({ frequency: x.kind })],
    ['weekdays', (x) => ({ weekdays: x.weekdays })],
    ['start', (x) => ({ start_date: x.start })],
    ['time', (x) => ({ start_time: x.time })],
    ['end', (x) => ({ end_date: x.end })],
    ['maxCount', (x) => ({ max_count: x.maxCount })]
  ]);
}

/**
 * The next few fires of a weekly reminder, walked the same way the server walks the mask,
 * so the user sees what "Mon, Thu" means before saving. Empty for any other frequency.
 */
export function nextFires(r, now = new Date(), count = 4) {
  const mask = r.frequency === 'weekly' ? (r.weekdays ?? 0) : 0;
  if (!mask || !r.start_date) return [];
  const [h, min] = (r.start_time || '00:00').split(':').map(Number);
  const cursor = new Date(`${r.start_date}T00:00:00`);
  if (Number.isNaN(cursor.getTime())) return [];
  cursor.setHours(h || 0, min || 0, 0, 0);
  const end = r.end_date ? new Date(`${r.end_date}T23:59:59`) : null;
  const out = [];
  for (let i = 0; i < 400 && out.length < count; i++) {
    const bit = 1 << ((cursor.getDay() + 6) % 7);
    if (mask & bit && cursor >= now) {
      if (end && cursor > end) break;
      out.push(new Date(cursor));
    }
    cursor.setDate(cursor.getDate() + 1);
  }
  return out;
}
