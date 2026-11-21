/** Pure helpers over the chain: which expiries a filter picks, the strike window a fetch asks
 * for, the client-side filters on a loaded chain, grouping and the at-the-money strike. */

/** The provider refuses more than this many expiries per chain call. */
export const MAX_EXPIRIES = 8;

const DAY = 86400000;

/** Local calendar date as `YYYY-MM-DD`. */
export function isoDate(d = new Date()) {
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** Whole calendar days from today to a `YYYY-MM-DD` date. */
export function daysTo(date) {
  const [y, m, d] = date.split('-').map(Number);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return Math.round((new Date(y, m - 1, d) - today) / DAY);
}

/** Expiry dates a filter selects, before the cap. `f` is `{ mode, from, to, dates }` with
 * mode `week` | `month` | `quarter` | `range` | `specific`. */
export function pickExpiries(list, f) {
  const dates = (list ?? []).map((e) => e.date);
  const today = new Date();
  if (f.mode === 'week') {
    // Through the coming Sunday.
    const end = isoDate(new Date(today.getTime() + ((7 - today.getDay()) % 7) * DAY));
    return dates.filter((d) => d <= end);
  }
  if (f.mode === 'month') {
    const ym = isoDate(today).slice(0, 7);
    return dates.filter((d) => d.slice(0, 7) === ym);
  }
  if (f.mode === 'quarter') {
    const end = isoDate(new Date(today.getTime() + 91 * DAY));
    return dates.filter((d) => d <= end);
  }
  if (f.mode === 'range') return dates.filter((d) => (!f.from || d >= f.from) && (!f.to || d <= f.to));
  return dates.filter((d) => f.dates.includes(d));
}

/** Parse a comma or space separated strike list. */
export function parseStrikes(text) {
  return String(text ?? '')
    .split(/[\s,;]+/)
    .map(Number)
    .filter((n) => Number.isFinite(n) && n > 0);
}

/** The N strikes each side of the one nearest spot, from a sorted unique list. */
export function strikesAround(strikes, spot, n) {
  if (!strikes.length || spot == null) return strikes;
  const i = nearestIndex(strikes, spot);
  return strikes.slice(Math.max(0, i - n), i + n + 1);
}

export function nearestIndex(sorted, x) {
  let best = 0;
  for (let i = 1; i < sorted.length; i++) if (Math.abs(sorted[i] - x) < Math.abs(sorted[best] - x)) best = i;
  return best;
}

/** Strike window `[lo, hi]` a chain fetch asks for, from the strike filter
 * `{ mode: near|pct5|range|specific, n, lo, hi, list }`, never with lo above hi. `near` takes
 * the N strikes each side of spot from the strikes each picked expiry lists; an expiry listed
 * without strikes borrows those of the nearest expiry that has them, and when none has any the
 * window falls back to 5 % each side of spot. */
export function fetchWindow(f, spot, expiries, picked) {
  return ordered(rawWindow(f, spot, expiries, picked));
}

function rawWindow(f, spot, expiries, picked) {
  if (f.mode === 'range') return [num(f.lo), num(f.hi)];
  if (f.mode === 'specific') {
    const s = parseStrikes(f.list);
    return s.length ? [Math.min(...s), Math.max(...s)] : [null, null];
  }
  if (spot == null) return [null, null];
  const pct5 = [round(spot * 0.95), round(spot * 1.05)];
  if (f.mode === 'pct5') return pct5;
  const listed = (expiries ?? []).filter((e) => Array.isArray(e.strikes) && e.strikes.length);
  if (!listed.length || !picked.length) return pct5;
  let lo = Infinity;
  let hi = -Infinity;
  for (const date of picked) {
    const e = nearestListed(listed, date);
    const near = strikesAround([...e.strikes].sort((a, b) => a - b), spot, f.n);
    lo = Math.min(lo, near[0]);
    hi = Math.max(hi, near.at(-1));
  }
  return [lo, hi];
}

/** The listed expiry with strikes closest in time to `date` (itself when it lists them). */
function nearestListed(listed, date) {
  const t = Date.parse(date);
  let best = listed[0];
  for (const e of listed) {
    if (e.date === date) return e;
    if (Math.abs(Date.parse(e.date) - t) < Math.abs(Date.parse(best.date) - t)) best = e;
  }
  return best;
}

const ordered = ([lo, hi]) => (lo != null && hi != null && lo > hi ? [hi, lo] : [lo, hi]);
const num = (v) => (v === '' || v == null || !Number.isFinite(Number(v)) ? null : Number(v));
const round = (v) => Math.round(v * 100) / 100;

/** Stable key of a strike filter, to tell whether the loaded chain was asked with it. */
export const strikeKey = (f) => JSON.stringify([f.mode, f.n, f.lo, f.hi, f.list]);

/** Whether the loaded chain already answers what the page asks: else a fetch is needed.
 * `want` is `{ rate, dividend, strike }` (`strike` from `strikeKey`). With the same strike
 * filter the asked window stands, even when the spot it was computed from has since moved. */
export function covers(chain, picked, win, want) {
  if (!chain?.asked) return false;
  const a = chain.asked;
  if (!picked.every((d) => a.expiries.includes(d))) return false;
  if (!sameRates(a, want)) return false;
  if (a.strike === want.strike) return true;
  const [lo, hi] = win;
  if (lo != null && (a.lo == null || lo < a.lo)) return false;
  if (hi != null && (a.hi == null || hi > a.hi)) return false;
  return true;
}

/** Whether the loaded chain was computed at the rate and dividend now set. */
export const sameRates = (a, want) => a.rate === want.rate && a.dividend === want.dividend && a.cash === want.cash;

/** Rows grouped by expiry, filtered client-side. Each group:
 * `{ expiry, dte, atmIv, strikes: [{ strike, call, put }] }` sorted by strike. */
export function groupChain(rows, spot, { expiries, strike, spread }) {
  const maxSpread = spread.mode === 'any' ? null : spread.mode === 'manual' ? num(spread.value) / 100 : SPREADS[spread.mode];
  const specific = strike.mode === 'specific' ? parseStrikes(strike.list) : null;
  const byExpiry = new Map();
  for (const r of rows ?? []) {
    if (expiries.length && !expiries.includes(r.expiry)) continue;
    if (!byExpiry.has(r.expiry)) byExpiry.set(r.expiry, new Map());
    const m = byExpiry.get(r.expiry);
    if (!m.has(r.strike)) m.set(r.strike, { strike: r.strike, call: null, put: null });
    m.get(r.strike)[r.call ? 'call' : 'put'] = r;
  }
  const keepSpread = (r) => maxSpread == null || (r?.spread_pct != null && r.spread_pct <= maxSpread);
  const groups = [];
  for (const [expiry, m] of [...byExpiry].sort((a, b) => (a[0] < b[0] ? -1 : 1))) {
    let list = [...m.values()].sort((a, b) => a.strike - b.strike);
    const atm = spot != null && list.length ? list[nearestIndex(list.map((s) => s.strike), spot)] : null;
    const atmIv = mean([atm?.call?.iv_mid, atm?.put?.iv_mid]);
    if (strike.mode === 'near') list = strikesAround(list.map((s) => s.strike), spot, strike.n).map((k) => m.get(k));
    else if (strike.mode === 'pct5' && spot) list = list.filter((s) => Math.abs(s.strike / spot - 1) <= 0.05);
    else if (strike.mode === 'range') list = list.filter((s) => (num(strike.lo) == null || s.strike >= num(strike.lo)) && (num(strike.hi) == null || s.strike <= num(strike.hi)));
    else if (specific) list = list.filter((s) => specific.some((k) => Math.abs(k - s.strike) < 1e-6));
    if (maxSpread != null) {
      list = list
        .map((s) => ({ ...s, call: keepSpread(s.call) ? s.call : null, put: keepSpread(s.put) ? s.put : null }))
        .filter((s) => s.call || s.put);
    }
    const dte = list.find((s) => s.call?.dte != null || s.put?.dte != null);
    groups.push({ expiry, dte: dte ? (dte.call?.dte ?? dte.put?.dte) : daysTo(expiry), atmIv, atm: atm?.strike ?? null, strikes: list });
  }
  return groups;
}

export const SPREADS = { tight: 0.05, medium: 0.15, wide: 0.4 };

function mean(xs) {
  const v = xs.filter((x) => typeof x === 'number' && Number.isFinite(x));
  return v.length ? v.reduce((a, b) => a + b, 0) / v.length : null;
}

/** Sorted unique strikes of one expiry in the loaded chain. */
export function chainStrikes(rows, expiry) {
  return [...new Set((rows ?? []).filter((r) => !expiry || r.expiry === expiry).map((r) => r.strike))].sort((a, b) => a - b);
}

/** Sorted unique expiries of the loaded chain. */
export function chainExpiries(rows) {
  return [...new Set((rows ?? []).map((r) => r.expiry))].sort();
}

/** The chain row for one contract. */
export function findRow(rows, expiry, strike, call) {
  return (rows ?? []).find((r) => r.expiry === expiry && r.call === call && Math.abs(r.strike - strike) < 1e-6) ?? null;
}

/** One smile per expiry, as POST /options/strategy/evaluate takes it: strike over spot against
 * the mid IV of the out-of-the-money side, the liquid one. Expiries with under two points are
 * left out. */
export function chainSmiles(rows, spot, expiries) {
  if (!spot || !rows?.length) return [];
  const out = [];
  for (const expiry of expiries) {
    const points = rows
      .filter((r) => r.expiry === expiry && (r.call ? r.strike >= spot : r.strike < spot) && Number.isFinite(r.iv_mid) && r.iv_mid > 0 && r.iv_mid <= 5)
      .map((r) => [r.strike / spot, r.iv_mid])
      .sort((a, b) => a[0] - b[0]);
    if (points.length >= 2) out.push({ expiry, points });
  }
  return out;
}
