// Scatter of closed trades: what the axis pickers can plot, and the fit drawn through
// the cloud. Pure derivations over the `points` array of `/api/journal/analytics`, kept
// out of the component so the maths stays readable and testable.

/**
 * One plottable axis.
 *
 *   id    i18n suffix (`journal.analytics.metric.<id>`) and the dropdown value
 *   kind  how the axis, the tooltip and the ticks format it
 *   of    (point, index, ctx) -> number | null; null = not recorded on that trade
 *
 * `ctx` carries the whole-series derivations an individual trade cannot know
 * (running PnL). Points arrive oldest first, so the index is the trade's sequence.
 */
export const METRICS = [
  { id: 'date', kind: 'time', of: (p) => p.at },
  { id: 'index', kind: 'count', of: (_p, i) => i + 1 },
  { id: 'net', kind: 'money', of: (p) => p.net },
  { id: 'cumNet', kind: 'money', of: (_p, i, ctx) => ctx.cum[i] },
  { id: 'returnPct', kind: 'percent', of: (p) => (p.notional > 0 ? (p.net / p.notional) * 100 : null) },
  { id: 'r', kind: 'r', of: (p) => p.r },
  { id: 'riskPct', kind: 'percent', of: (p) => p.risk_pct },
  { id: 'fees', kind: 'money', of: (p) => p.fees },
  { id: 'hold', kind: 'duration', of: (p) => p.hold_min },
  { id: 'size', kind: 'money', of: (p) => (p.notional > 0 ? p.notional : null) },
  { id: 'qty', kind: 'num', of: (p) => (p.qty > 0 ? p.qty : null) },
  { id: 'leverage', kind: 'num', of: (p) => (p.leverage > 0 ? p.leverage : null) },
  { id: 'entryPrice', kind: 'num', of: (p) => p.entry_price },
  { id: 'exitPrice', kind: 'num', of: (p) => p.exit_price },
  // Entry hour and weekday are read in the reader's own timezone, like the calendar.
  { id: 'hour', kind: 'hour', of: (p) => (p.entry_at == null ? null : new Date(p.entry_at).getHours()) },
  {
    id: 'weekday',
    kind: 'weekday',
    // JS weeks start on Sunday; the journal counts from Monday everywhere else.
    of: (p) => (p.entry_at == null ? null : (new Date(p.entry_at).getDay() + 6) % 7)
  }
];

export const metric = (id) => METRICS.find((m) => m.id === id) ?? METRICS[0];

/** Series-wide derivations the metrics read through `ctx`. */
export function scatterCtx(points) {
  let run = 0;
  const cum = points.map((p) => (run += p.net));
  return { cum };
}

/** How the cloud is split into coloured series. */
export const COLOR_MODES = ['result', 'side', 'strategy', 'ticker', 'assetClass'];

/**
 * Group key of a point under one colour mode. Returns `''` for "not filled in", which
 * the caller labels itself (a trade with no strategy is still a trade).
 */
export function colorKey(p, mode) {
  switch (mode) {
    case 'side':
      return p.side || '';
    case 'strategy':
      return p.strategy || '';
    case 'ticker':
      return p.ticker || '';
    case 'assetClass':
      return p.asset_class || '';
    default:
      return p.net > 0 ? 'win' : p.net < 0 ? 'loss' : 'flat';
  }
}

/**
 * Split rows into at most `max` series, biggest first; everything past the cap folds
 * into one `other` group rather than cycling the palette back to its first colour.
 *
 * `rows` are `{ key, ... }`; the return keeps that shape per group.
 */
export function groupRows(rows, max = 8) {
  const by = new Map();
  for (const row of rows) {
    const g = by.get(row.key);
    if (g) g.push(row);
    else by.set(row.key, [row]);
  }
  const groups = [...by.entries()]
    .map(([key, items]) => ({ key, items }))
    .sort((a, b) => b.items.length - a.items.length);
  if (groups.length <= max) return groups;
  const kept = groups.slice(0, max - 1);
  kept.push({
    key: '__other',
    items: groups.slice(max - 1).flatMap((g) => g.items)
  });
  return kept;
}

/**
 * Least-squares fit and Pearson correlation of the cloud. `null` when there is nothing
 * to fit: fewer than three points, or an axis that never varies (a vertical line has no
 * slope, and a correlation would be a division by zero).
 */
export function fit(xs, ys) {
  const n = xs.length;
  if (n < 3) return null;
  const mx = xs.reduce((a, b) => a + b, 0) / n;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  let sxx = 0;
  let syy = 0;
  let sxy = 0;
  for (let i = 0; i < n; i++) {
    const dx = xs[i] - mx;
    const dy = ys[i] - my;
    sxx += dx * dx;
    syy += dy * dy;
    sxy += dx * dy;
  }
  if (sxx <= 0 || syy <= 0) return null;
  const slope = sxy / sxx;
  return {
    n,
    slope,
    intercept: my - slope * mx,
    r: sxy / Math.sqrt(sxx * syy)
  };
}
