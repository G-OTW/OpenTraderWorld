/** The range bar under a chart, TradingView style: one click frames a period (a day, a month,
 *  five years) at the timeframe that reads it best, instead of a slider eating the chart's
 *  height.
 *
 *  A period is counted back from the **last bar**, not from now: on a Sunday "1D" is
 *  Friday's session, not an empty day. */

import { tfSeconds } from './pane.js';

/** Each range: its key (the label), the timeframe it wants, how far back it reaches. */
export const RANGES = [
  { key: '1D', tf: '1m', back: { days: 1 } },
  { key: '1W', tf: '15m', back: { days: 7 } },
  { key: '1M', tf: '30m', back: { months: 1 } },
  { key: '3M', tf: '1h', back: { months: 3 } },
  { key: '6M', tf: '2h', back: { months: 6 } },
  { key: 'YTD', tf: '1d', back: { ytd: true } },
  { key: '1Y', tf: '1d', back: { years: 1 } },
  { key: '5Y', tf: '1w', back: { years: 5 } },
  { key: 'All', tf: '1w', back: { all: true } }
];

/** The timeframe a range uses among the ones the chart can show: the one it wants, else the
 *  coarsest that is still finer (more detail, never less), else the finest there is. */
export function pickTimeframe(available, wanted) {
  const list = (available ?? []).filter((tf) => tfSeconds(tf));
  if (!list.length) return null;
  if (list.includes(wanted)) return wanted;
  const w = tfSeconds(wanted) ?? 0;
  const finer = list.filter((tf) => tfSeconds(tf) <= w).sort((a, b) => tfSeconds(b) - tfSeconds(a));
  if (finer.length) return finer[0];
  return [...list].sort((a, b) => tfSeconds(a) - tfSeconds(b))[0];
}

/** Where a range starts, in ms, for a series whose last bar is at `lastMs`. `null` for
 *  "All": everything loaded. Calendar arithmetic in UTC, so "1M" is a month, not 30 days. */
export function rangeStart(back, lastMs) {
  if (back?.all) return null;
  const d = new Date(lastMs);
  if (back.ytd) return Date.UTC(d.getUTCFullYear(), 0, 1);
  if (back.days) return lastMs - back.days * 86_400_000;
  const months = (back.months ?? 0) + 12 * (back.years ?? 0);
  if (!months) return d.getTime();
  // Calendar months back, the day clamped to the target month's last: one month before
  // March 31 is February 29, not March 2 (`setUTCMonth` overflows into the next month).
  const m = d.getUTCMonth() - months;
  const y = d.getUTCFullYear() + Math.floor(m / 12);
  const mo = ((m % 12) + 12) % 12;
  const last = new Date(Date.UTC(y, mo + 1, 0)).getUTCDate();
  return Date.UTC(y, mo, Math.min(d.getUTCDate(), last), d.getUTCHours(), d.getUTCMinutes(), d.getUTCSeconds(), d.getUTCMilliseconds());
}

/** An instant as the bars stamp theirs (RFC3339, no milliseconds), so it compares as text. */
export const isoSecond = (ms) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
