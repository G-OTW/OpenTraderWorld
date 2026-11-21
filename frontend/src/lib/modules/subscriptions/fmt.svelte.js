// Figures for the Subscription Tracker screens: money masked under the privacy screen,
// and a word instead of a figure when the value cannot be measured (no FX rate, no data).
import { fmtMoney } from '$lib/format.js';
import { privacy } from '$lib/theme/privacy.svelte.js';
import { MASK } from '$lib/ui/charts/palette.js';

/** Money, or `na` when the amount is unknown, or the mask while the screen is on. */
export function money(v, ccy, na = 'n/a') {
  if (v === null || v === undefined || !Number.isFinite(Number(v))) return na;
  if (privacy.hidden) return MASK;
  return fmtMoney(Number(v), ccy);
}

/** Whole days from today (local midnight) to `d`; null without a date. */
export function daysUntil(d, today = new Date()) {
  if (!d) return null;
  const a = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  const b = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  return Math.round((b - a) / 86400000);
}

/** First letter of a name for the monogram tile. */
export function initial(name) {
  const ch = String(name ?? '').trim().charAt(0);
  return ch ? ch.toUpperCase() : '?';
}
