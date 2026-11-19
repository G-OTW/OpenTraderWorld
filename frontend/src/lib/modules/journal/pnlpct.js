// PnL read as a percentage of capital, shared by the calendar and the breakdown chart.
//
// The base is the capital the account carried: invested capital plus the PnL realized
// before. A book with no capital recorded has no such base, so its first point stands in
// for it (the size of the first result), which keeps the shape of the curve readable.

/**
 * Starting equity of each traded day, in date order.
 * @param {Array<{date: string, net_pnl: number}>} days
 * @param {number} capital invested capital
 * @returns {Map<string, number>} 'YYYY-MM-DD' -> equity carried into that day
 */
export function dayBases(days, capital) {
  const sorted = [...days].sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : 0));
  let equity = capital > 0 ? capital : Math.abs(sorted.find((d) => d.net_pnl)?.net_pnl ?? 0);
  const out = new Map();
  for (const d of sorted) {
    out.set(d.date, equity);
    equity += d.net_pnl;
  }
  return out;
}

/** `net` as a percentage of `base`, or null when there is no positive base. */
export const pctOf = (net, base) => (base > 0 ? (net / base) * 100 : null);

/**
 * Equity points rebased to percent change from the first point. The first point is the
 * base when positive, else the first non-zero point (a book with no capital).
 * @param {number[]} ys
 * @returns {number[]}
 */
export function rebasePct(ys) {
  if (!ys.length) return [];
  const start = ys[0];
  const base = start > 0 ? start : Math.abs(ys.find((y) => y) ?? 0);
  return ys.map((y) => (base > 0 ? ((y - start) / base) * 100 : null));
}

/** Signed percentage with a true minus sign, '—' when there is none. */
export const fmtSignedPct = (n, dp = 2) =>
  n == null ? '—' : `${n > 0 ? '+' : n < 0 ? '−' : ''}${Math.abs(n).toFixed(dp)}%`;
