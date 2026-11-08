/** MyWealth client-side math: pure functions, no Svelte, tested in tests/wealth-engine.test.js. */

/**
 * Equity and loan-to-value of an asset held against a debt (a house and its mortgage).
 * Both amounts in the same currency; the debt as a positive balance. LTV is null when the
 * asset has no positive value: a ratio over nothing is not a number worth showing.
 */
export function pairMath(assetValue, debtBalance) {
  if (assetValue == null || debtBalance == null) return { equity: null, ltv: null };
  const debt = Math.abs(debtBalance);
  return {
    equity: assetValue - debt,
    ltv: assetValue > 0 ? (debt / assetValue) * 100 : null
  };
}

/**
 * Months of runway: liquid money over what a month costs (expenses plus subscriptions).
 * `{ months: null, reason }` when it cannot be answered: no monthly cost known, or nothing
 * liquid flagged.
 */
export function runway(liquidTotal, monthlyExpenses, subscriptionsMonthly) {
  const cost = (Number(monthlyExpenses) || 0) + (Number(subscriptionsMonthly) || 0);
  if (!(cost > 0)) return { months: null, reason: 'no_cost' };
  if (liquidTotal == null) return { months: null, reason: 'no_liquid' };
  return { months: Math.max(0, liquidTotal) / cost, cost };
}

/** Sum of the categories flagged liquid in a `{ category: value }` map. */
export function liquidTotal(byCategory, liquidNames) {
  const set = new Set(liquidNames);
  if (set.size === 0) return null;
  let sum = 0;
  for (const [cat, v] of Object.entries(byCategory ?? {})) if (set.has(cat)) sum += v;
  return sum;
}

/**
 * The CPI index in force at a date: the latest observation whose period starts on or before
 * it. Observations are `[['YYYY-MM-DD', value], ...]` in ascending order. null before the
 * series starts, never the first value carried backwards.
 */
export function cpiAt(observations, date) {
  let found = null;
  for (const [d, v] of observations) {
    if (d <= date) found = v;
    else break;
  }
  return found;
}

/**
 * Deflate a net-worth curve into today's money: real = nominal x CPI(latest point) / CPI(at).
 * A point the series does not cover becomes null (a gap), not a nominal value in disguise.
 */
export function deflate(points, observations) {
  if (!observations?.length || !points?.length) return [];
  const base = cpiAt(observations, points[points.length - 1].at);
  return points.map((p) => {
    const idx = cpiAt(observations, p.at);
    const ok = base != null && idx != null && idx > 0;
    return {
      ...p,
      net_worth: ok ? (p.net_worth * base) / idx : null,
      estimated: ok ? ((p.estimated ?? 0) * base) / idx : 0
    };
  });
}

/** Side by side category table: every category of either snapshot, then, now and the change. */
export function compareSnapshots(then, now) {
  const a = then?.by_category ?? {};
  const b = now?.by_category ?? {};
  const names = [...new Set([...Object.keys(a), ...Object.keys(b)])];
  const rows = names.map((name) => {
    const was = a[name] ?? null;
    const is = b[name] ?? null;
    const delta = (is ?? 0) - (was ?? 0);
    return { name, then: was, now: is, delta, pct: was ? (delta / Math.abs(was)) * 100 : null };
  });
  rows.sort((x, y) => Math.abs(y.delta) - Math.abs(x.delta) || x.name.localeCompare(y.name));
  const total = { then: then?.net_worth ?? 0, now: now?.net_worth ?? 0 };
  total.delta = total.now - total.then;
  total.pct = total.then ? (total.delta / Math.abs(total.then)) * 100 : null;
  return { rows, total };
}

/** `YYYY-MM-DD` of a date `n` months before `from` (day clamped to the month's length). */
export function monthsBack(from, n) {
  const d = new Date(from + 'T00:00:00');
  const target = new Date(d.getFullYear(), d.getMonth() - n, 1);
  const last = new Date(target.getFullYear(), target.getMonth() + 1, 0).getDate();
  target.setDate(Math.min(d.getDate(), last));
  const pad = (x) => String(x).padStart(2, '0');
  return `${target.getFullYear()}-${pad(target.getMonth() + 1)}-${pad(target.getDate())}`;
}

/** Wallet address format check, the same rules as the server (format only, no checksum). */
export function walletAddressOk(chain, raw) {
  const a = (raw ?? '').trim();
  if (chain === 'eth') return /^0x[0-9a-fA-F]{40}$/.test(a);
  if (chain === 'btc') {
    const lower = a.toLowerCase();
    if (lower.startsWith('bc1')) {
      const single = a === lower || a === a.toUpperCase();
      return single && /^bc1[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{11,71}$/.test(lower);
    }
    return /^[13][1-9A-HJ-NP-Za-km-z]{25,34}$/.test(a);
  }
  return false;
}

/** Monthly payment of a loan (the annuity), for the form's preview. */
export function annuity(principal, ratePct, months) {
  const p = Number(principal);
  const n = Number(months);
  const r = Number(ratePct) / 100 / 12;
  if (!(p > 0) || !(n > 0)) return null;
  if (r === 0) return p / n;
  return (p * r) / (1 - Math.pow(1 + r, -n));
}

/** Net worth by category as `name:value` pairs for the Tax Calculator link, plus the base. */
export function taxLink(byCategory, picked, currency) {
  const base = Object.entries(byCategory ?? {})
    .filter(([cat]) => picked.has(cat))
    .reduce((s, [, v]) => s + v, 0);
  const p = new URLSearchParams();
  p.set('wealth_base', String(Math.round(base * 100) / 100));
  p.set('wealth_currency', currency);
  return { base, href: `/taxcalc?${p.toString()}` };
}

/** Headline periods, in chip order. */
export const PERIODS = ['1M', '3M', '6M', 'YTD', '1Y', 'ALL'];

/**
 * The breakdown query for a headline period. `points_back` counts samples: the month ends
 * plus today, so 1M asks for two (last month end and today). YTD starts on the last day of
 * the previous year. ALL asks for five years of month ends; the caller falls back to year
 * ends when that is not enough to reach the start.
 */
export function periodQuery(id, today) {
  const d = new Date(today + 'T00:00:00');
  switch (id) {
    case '1M':
      return { granularity: 'month', points_back: 2 };
    case '3M':
      return { granularity: 'month', points_back: 4 };
    case '6M':
      return { granularity: 'month', points_back: 7 };
    case 'YTD':
      return { granularity: 'month', points_back: d.getMonth() + 2 };
    case '1Y':
      return { granularity: 'month', points_back: 13 };
    default:
      return { granularity: 'month', points_back: 60 };
  }
}

/**
 * The curve a period shows. ALL drops the leading samples taken before anything was held
 * (a zero), so its change reads from the first valuation rather than from nothing.
 */
export function periodPoints(points, id) {
  const list = points ?? [];
  if (id !== 'ALL') return list;
  const first = list.findIndex((p) => p.net_worth != null && p.net_worth !== 0);
  return first <= 0 ? list : list.slice(first);
}

/**
 * Change over a curve: last measured value minus the first. `pct` is null when the start
 * is zero or unknown (a change from nothing has no percent).
 */
export function curveChange(points) {
  const vals = (points ?? []).filter((p) => p.net_worth != null && Number.isFinite(p.net_worth));
  if (vals.length < 2) return { abs: null, pct: null, from: vals[0]?.at ?? null };
  const a = vals[0].net_worth;
  const b = vals[vals.length - 1].net_worth;
  return { abs: b - a, pct: a ? ((b - a) / Math.abs(a)) * 100 : null, from: vals[0].at };
}

/** Change of an asset between its two latest revisions (newest first), or null. */
export function revisionChange(revisions) {
  const [a, b] = revisions ?? [];
  if (a?.value == null || b?.value == null) return { abs: null, pct: null, since: null };
  const abs = a.value - b.value;
  return { abs, pct: b.value ? (abs / Math.abs(b.value)) * 100 : null, since: b.valued_at };
}
