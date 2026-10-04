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
  // A backtest trade carries the engine's own return; a journal trade derives it.
  {
    id: 'returnPct',
    kind: 'percent',
    of: (p) => p.return_pct ?? (p.notional > 0 ? (p.net / p.notional) * 100 : null)
  },
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
  },
  // Market-data axes. They exist only on trades the enrichment pass has measured
  // against candles; on every other trade they read `null`, which the scatter already
  // treats as "not recorded" and drops from the cloud with a count.
  { id: 'mae', kind: 'money', of: (p) => p.mae ?? null },
  { id: 'mfe', kind: 'money', of: (p) => p.mfe ?? null },
  { id: 'maeR', kind: 'r', of: (p) => p.mae_r ?? null },
  { id: 'mfeR', kind: 'r', of: (p) => p.mfe_r ?? null },
  { id: 'efficiency', kind: 'percent', of: (p) => p.exit_efficiency ?? null },
  { id: 'giveback', kind: 'money', of: (p) => p.giveback ?? null },
  { id: 'volPct', kind: 'percent', of: (p) => p.vol_pct ?? null },
  { id: 'stopAtr', kind: 'num', of: (p) => p.stop_distance_atr ?? null }
];

export const metric = (id) => METRICS.find((m) => m.id === id) ?? METRICS[0];

/** Series-wide derivations the metrics read through `ctx`. */
export function scatterCtx(points) {
  let run = 0;
  const cum = points.map((p) => (run += p.net));
  return { cum };
}

/** How the cloud is split into coloured series. */
export const COLOR_MODES = [
  'result',
  'side',
  'strategy',
  'ticker',
  'assetClass',
  // Only meaningful once the market-data pass has run; an unmeasured trade groups under
  // the empty key, which the chart labels "not recorded" like any other missing value.
  'regime',
  'trend'
];

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
    case 'regime':
      return p.regime || '';
    case 'trend':
      return p.trend || '';
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

// ── Market session of a trade (3D scatter) ──

const WEEKDAY_IDX = { Mon: 0, Tue: 1, Wed: 2, Thu: 3, Fri: 4, Sat: 5, Sun: 6 };
const clocks = new Map();

/** Weekday (Monday = 0) and minute of day of `ms` on the wall clock of `tz`; null on a bad zone. */
function wallClock(ms, tz) {
  let f = clocks.get(tz);
  if (f === undefined) {
    try {
      f = new Intl.DateTimeFormat('en-US', {
        timeZone: tz,
        weekday: 'short',
        hour: '2-digit',
        minute: '2-digit',
        hourCycle: 'h23'
      });
    } catch {
      f = null;
    }
    clocks.set(tz, f);
  }
  if (!f) return null;
  const parts = Object.fromEntries(f.formatToParts(ms).map((x) => [x.type, x.value]));
  return { wd: WEEKDAY_IDX[parts.weekday], min: Number(parts.hour) * 60 + Number(parts.minute) };
}

/**
 * Whether `ms` falls inside session `s`. Same rule as the server (`market_sessions_api.rs`):
 * a window belongs to the local day it opens on, so the overnight tail of a Friday-evening
 * session still counts on Saturday morning; `end <= start` runs into the next day.
 */
function inSession(s, ms) {
  const c = wallClock(ms, s.timezone);
  if (!c) return false;
  const opens = (wd) => ((s.weekdays >> wd) & 1) === 1;
  if (s.end_minute > s.start_minute) {
    return c.min >= s.start_minute && c.min < s.end_minute && opens(c.wd);
  }
  if (c.min >= s.start_minute) return opens(c.wd);
  if (c.min < s.end_minute) return opens((c.wd + 6) % 7);
  return false;
}

/**
 * The `session` axis over the user's market sessions (Settings, in their order). A trade
 * scores the index of the first session its entry falls in, so an overlap (London / New
 * York) goes to the one listed first; outside every window it scores `sessions.length`,
 * a real "off session" value rather than a missing one. `null` sessions (list not
 * loaded) leave the axis unrecorded.
 */
export function sessionMetric(sessions) {
  return {
    id: 'session',
    kind: 'session',
    of: (p) => {
      if (!sessions || p.entry_at == null) return null;
      const ms = new Date(p.entry_at).getTime();
      if (!Number.isFinite(ms)) return null;
      const i = sessions.findIndex((s) => inSession(s, ms));
      return i < 0 ? sessions.length : i;
    }
  };
}
