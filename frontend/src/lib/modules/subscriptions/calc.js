/**
 * Pure Subscription Tracker math, shared by the page and the tests. No imports, no DOM.
 *
 * Billing dates mirror the backend (`otw-store/src/subscriptions.rs`): anchored on the trial
 * end when there is one, else on the start date; months are added from the anchor with the
 * day clamped to the month's length (Jan 31 then Feb 28, then Mar 31), so nothing drifts;
 * a paused subscription bills nothing and a cancelled one stops at its end date.
 */

/** Monthly-equivalent factor (mirrors the backend normalisation). */
export function monthlyFactor(freq) {
  switch (freq) {
    case 'weekly':
      return 52 / 12;
    case 'quarterly':
      return 1 / 3;
    case 'yearly':
      return 1 / 12;
    default:
      return 1;
  }
}

/** `my_share_pct` as a factor; anything out of (0, 100] reads as the whole price. */
export function shareFactor(pct) {
  const n = Number(pct);
  return Number.isFinite(n) && n > 0 ? Math.min(n, 100) / 100 : 1;
}

/** The user's monthly-equivalent cost, in the subscription's own currency. */
export function monthlyOf(sub) {
  return (sub.price ?? 0) * monthlyFactor(sub.frequency) * shareFactor(sub.my_share_pct);
}

/** The user's part of one billing, in the subscription's own currency. */
export function myPrice(sub) {
  return (sub.price ?? 0) * shareFactor(sub.my_share_pct);
}

/** `YYYY-MM-DD` to a local Date at midnight, or null. */
export function parseDay(s) {
  if (!s || typeof s !== 'string') return null;
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(s);
  if (!m) return null;
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
}

/** Local midnight of a Date (a copy). */
export function startOfDay(d = new Date()) {
  const out = new Date(d);
  out.setHours(0, 0, 0, 0);
  return out;
}

/** `d` moved by `n` months, the day clamped to the target month's length. */
export function addMonthsClamped(d, n) {
  const total = d.getFullYear() * 12 + d.getMonth() + n;
  const year = Math.floor(total / 12);
  const month = total - year * 12;
  const last = new Date(year, month + 1, 0).getDate();
  return new Date(year, month, Math.min(d.getDate(), last));
}

function nth(anchor, frequency, k) {
  switch (frequency) {
    case 'weekly':
      return new Date(anchor.getFullYear(), anchor.getMonth(), anchor.getDate() + 7 * k);
    case 'quarterly':
      return addMonthsClamped(anchor, 3 * k);
    case 'yearly':
      return addMonthsClamped(anchor, 12 * k);
    default:
      return addMonthsClamped(anchor, k);
  }
}

/** The date billing repeats from: the trial end when set, else the start date. */
export function anchorOf(sub) {
  return parseDay(sub.trial_end) ?? parseDay(sub.started_on);
}

/** Cancelled on or before `today`. */
export function isEnded(sub, today = new Date()) {
  const e = parseDay(sub.ended_at);
  return !!e && e <= startOfDay(today);
}

/**
 * Billing dates of `sub` in `from..=to` (Dates). None while paused, none from the end date,
 * none for a subscription with no start date (it has no billing day).
 */
export function billingDates(sub, from, to) {
  const out = [];
  if (!sub.active) return out;
  const anchor = anchorOf(sub);
  if (!anchor) return out;
  const lo = startOfDay(from);
  const hi = startOfDay(to);
  if (hi < lo || hi < anchor) return out;
  const end = parseDay(sub.ended_at);
  // Skip whole periods before `from` instead of walking from an old anchor.
  let k = 0;
  if (lo > anchor) {
    const months = (lo.getFullYear() - anchor.getFullYear()) * 12 + lo.getMonth() - anchor.getMonth();
    const rough =
      sub.frequency === 'weekly'
        ? Math.floor((lo - anchor) / (7 * 86400000))
        : sub.frequency === 'quarterly'
          ? Math.floor(months / 3)
          : sub.frequency === 'yearly'
            ? Math.floor(months / 12)
            : months;
    k = Math.max(0, rough - 1);
  }
  for (let guard = 0; guard < 1000; guard++, k++) {
    const d = nth(anchor, sub.frequency, k);
    if (d > hi) break;
    if (end && d >= end) break;
    if (d >= lo) out.push(d);
  }
  return out;
}

/** Next billing date on or after `today` within a year, or null. */
export function nextBillingDate(sub, today = new Date()) {
  const from = startOfDay(today);
  const to = new Date(from.getFullYear(), from.getMonth(), from.getDate() + 400);
  return billingDates(sub, from, to)[0] ?? null;
}

/**
 * What switching to the yearly plan saves a year, at the user's share: the cost at the
 * current cadence over 12 months minus the yearly price. Null when the plan is already
 * yearly or the yearly price is unknown. Negative means the yearly plan costs more.
 */
export function switchSavings(sub) {
  if (sub.frequency === 'yearly') return null;
  const yearly = sub.yearly_price;
  if (yearly === null || yearly === undefined || yearly === '' || !Number.isFinite(Number(yearly))) {
    return null;
  }
  const current = (sub.price ?? 0) * monthlyFactor(sub.frequency) * 12;
  return (current - Number(yearly)) * shareFactor(sub.my_share_pct);
}

/**
 * Card expiry state for a `YYYY-MM` value: 'expired' once the month is over, 'soon' when the
 * card's last day is within `days` (60 by default), else null.
 */
export function cardExpiryState(cardExpiry, today = new Date(), days = 60) {
  const m = /^(\d{4})-(\d{2})$/.exec(cardExpiry ?? '');
  if (!m) return null;
  const lastDay = new Date(Number(m[1]), Number(m[2]), 0);
  const t = startOfDay(today);
  if (lastDay < t) return 'expired';
  const left = Math.round((lastDay - t) / 86400000);
  return left <= days ? 'soon' : null;
}

/** Days until a running trial converts (0 = today), or null when there is no trial ahead. */
export function trialDaysLeft(sub, today = new Date()) {
  const end = parseDay(sub.trial_end);
  if (!end || !sub.active || isEnded(sub, today)) return null;
  const t = startOfDay(today);
  if (end < t) return null;
  return Math.round((end - t) / 86400000);
}

/** Rows with a trial ahead first (soonest conversion first), the rest in their order. */
export function trialsFirst(rows, today = new Date()) {
  const trials = [];
  const rest = [];
  for (const r of rows) {
    const left = trialDaysLeft(r, today);
    if (left === null) rest.push(r);
    else trials.push({ r, left });
  }
  trials.sort((a, b) => a.left - b.left);
  return [...trials.map((x) => x.r), ...rest];
}

/** A running subscription whose latest rating is missing or older than 90 days. */
export function needsRating(sub, today = new Date()) {
  if (!sub.active || isEnded(sub, today)) return false;
  const last = parseDay(sub.last_rated_at);
  if (!last) return true;
  return (startOfDay(today) - last) / 86400000 > 90;
}

/**
 * "Worth it?" ranking of running, rated subscriptions: monthly cost (display currency, via
 * `toDisplay(amount, currency)`; null = no rate) divided by the latest score, highest first.
 * The expensive, low-use ones lead. Unrated or unconvertible rows are left out.
 */
export function worthRanking(subs, toDisplay = (a) => a, today = new Date()) {
  const rows = [];
  for (const s of subs) {
    if (!s.active || isEnded(s, today) || !s.last_score) continue;
    const monthly = toDisplay(monthlyOf(s), s.currency);
    if (monthly === null || monthly === undefined || !Number.isFinite(monthly)) continue;
    rows.push({ sub: s, monthly, score: s.last_score, costPerPoint: monthly / s.last_score });
  }
  return rows.sort((a, b) => b.costPerPoint - a.costPerPoint || a.sub.name.localeCompare(b.sub.name));
}

/** `YYYY-MM-DD` of a local Date. */
export function dayKey(d) {
  const p = (x) => String(x).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/**
 * The month grid for `year`/`month0` (0-based): weeks starting Monday, each a list of 7
 * Dates (days outside the month included, so the grid is rectangular).
 */
export function monthGrid(year, month0) {
  const first = new Date(year, month0, 1);
  const offset = (first.getDay() + 6) % 7; // Monday = 0
  const start = new Date(year, month0, 1 - offset);
  const weeks = [];
  for (let w = 0; w < 6; w++) {
    const week = [];
    for (let i = 0; i < 7; i++) {
      week.push(new Date(start.getFullYear(), start.getMonth(), start.getDate() + w * 7 + i));
    }
    // A sixth row entirely in the next month is dropped.
    if (w > 3 && week[0].getMonth() !== month0) break;
    weeks.push(week);
  }
  return weeks;
}

/**
 * Billing events of `subs` in the month `year`/`month0`, keyed by `YYYY-MM-DD`:
 * `{ [day]: [{ sub, amount, currency }] }`, amount at the user's share in the sub's currency.
 */
export function monthEvents(subs, year, month0) {
  const from = new Date(year, month0, 1);
  const to = new Date(year, month0 + 1, 0);
  const out = {};
  for (const s of subs) {
    for (const d of billingDates(s, from, to)) {
      const k = dayKey(d);
      (out[k] ??= []).push({ sub: s, amount: myPrice(s), currency: s.currency });
    }
  }
  return out;
}

/** Category name as the backend groups it (`category_of` in otw-store). */
export const UNCATEGORIZED = 'Uncategorized';
export function categoryOf(sub) {
  return sub?.category ? sub.category : UNCATEGORIZED;
}

/**
 * Billed amounts per period split by category, from each period's `by_sub`. The first
 * `keep` names of `ranking` get their own row (in that order, so a category keeps its
 * colour slot), every other category is summed into `other` (null when empty).
 * `points` are breakdown months or years: [{ by_sub: { [subId]: amount } }].
 */
export function stackByCategory(points = [], subs = [], ranking = [], keep = 4) {
  const catOf = new Map(subs.map((s) => [String(s.id), categoryOf(s)]));
  const named = ranking.slice(0, keep);
  const slot = new Map(named.map((c, i) => [c, i]));
  const rows = named.map(() => points.map(() => 0));
  const other = points.map(() => 0);
  let anyOther = false;
  points.forEach((p, j) => {
    for (const [id, v] of Object.entries(p?.by_sub ?? {})) {
      const amount = Number(v) || 0;
      const k = slot.get(catOf.get(id) ?? UNCATEGORIZED);
      if (k === undefined) {
        other[j] += amount;
        if (amount) anyOther = true;
      } else rows[k][j] += amount;
    }
  });
  return { named, rows, other: anyOther ? other : null };
}

/**
 * Billings of `subs` from today through the next `days` days (today included):
 * [{ sub, date, amount, currency }], amount at the user's share in the sub's currency.
 */
export function upcomingBillings(subs = [], days = 30, today = new Date()) {
  const from = startOfDay(today);
  const to = new Date(from.getFullYear(), from.getMonth(), from.getDate() + days - 1);
  const out = [];
  for (const s of subs) {
    for (const date of billingDates(s, from, to)) {
      out.push({ sub: s, date, amount: myPrice(s), currency: s.currency });
    }
  }
  return out.sort((a, b) => a.date - b.date);
}

/**
 * Rows of a `worthRanking` worth a second look: rated 2 or less, or costing at least
 * twice the median cost per point. Returns the set of subscription ids.
 */
export function reviewIds(rows = []) {
  const out = new Set();
  if (!rows.length) return out;
  const sorted = rows.map((r) => r.costPerPoint).sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const median = sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
  for (const r of rows) {
    if (r.score <= 2 || (rows.length > 2 && median > 0 && r.costPerPoint >= 2 * median)) out.add(r.sub.id);
  }
  return out;
}
