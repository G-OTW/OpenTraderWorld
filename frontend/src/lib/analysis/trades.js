/** Trade analytics: the derivations behind the statistics and performance views.
 *
 * Every function here takes the same **normalised closed trade**, whatever produced it (the
 * chart's quick backtest replays clicks, the backtest engine simulates a strategy server-side):
 *
 *   { dir: 1 | -1, qty, entryAvg, exitAvg, openTs, closeTs,   // epoch ms
 *     pnl, pnlPct, runup, mae, r }                            // currency, %, currency, ≤ 0, ratio
 *
 * `runup` and `mae` are the excursion pair in account currency (run-up ≥ 0 above the line, heat
 * ≤ 0 below it) and `r` is the result over the heat it took, or null when there was none.
 * Nothing here reads a store or touches the DOM, so both callers and a test can use it as is.
 */

const EPS = 1e-9;

/** Min/max over a list. `Math.min(...xs)` passes every element as an argument, which blows the
 *  call stack somewhere past ~100k values, and a backtest can close far more trades than that. */
const minOf = (xs) => xs.reduce((m, x) => (x < m ? x : m), Infinity);
const maxOf = (xs) => xs.reduce((m, x) => (x > m ? x : m), -Infinity);

/**
 * Equity of the session after each closed trade, plus the drawdown that goes with it —
 * the two series the P&L tab draws.
 *
 * @returns { points: [{ ts, equity, dd, pnl }], maxDrawdown } where `dd` is ≤ 0 (distance
 *          below the running peak, in currency) and `maxDrawdown` its worst value, positive.
 */
export function equityCurve(trades) {
  const sorted = [...trades].sort((a, b) => a.closeTs - b.closeTs);
  const points = [];
  let equity = 0;
  let peak = 0;
  let maxDrawdown = 0;
  for (const t of sorted) {
    equity += t.pnl;
    if (equity > peak) peak = equity;
    const dd = equity - peak;
    if (-dd > maxDrawdown) maxDrawdown = -dd;
    points.push({
      ts: t.closeTs,
      equity,
      dd,
      pnl: t.pnl,
      // Carried along so the performance chart can plot a trade's excursions on the same
      // timeline without going back to the trade list.
      n: points.length + 1,
      dir: t.dir,
      runup: t.runup ?? 0,
      mae: t.mae ?? 0
    });
  }
  return { points, maxDrawdown };
}

/** Session statistics over closed trades. `r` is only averaged over trades that had heat. */
export function stats(trades) {
  const n = trades.length;
  const winners = trades.filter((t) => t.pnl > 0);
  const losers = trades.filter((t) => t.pnl < 0);
  const wins = winners.length;
  const pnl = trades.reduce((s, t) => s + t.pnl, 0);
  const rs = trades.map((t) => t.r).filter((r) => r != null && Number.isFinite(r));
  const grossWin = winners.reduce((s, t) => s + t.pnl, 0);
  const grossLoss = losers.reduce((s, t) => s - t.pnl, 0);
  const longs = trades.filter((t) => t.dir > 0);
  const shorts = trades.filter((t) => t.dir < 0);
  const held = trades.map((t) => t.closeTs - t.openTs).filter((d) => Number.isFinite(d) && d >= 0);
  const mean = (xs) => (xs.length ? xs.reduce((s, x) => s + x, 0) / xs.length : null);
  return {
    trades: n,
    pnl,
    wins,
    losses: losers.length,
    winRate: n ? (wins / n) * 100 : 0,
    avgR: rs.length ? rs.reduce((s, r) => s + r, 0) / rs.length : null,
    profitFactor: grossLoss > EPS ? grossWin / grossLoss : null,
    grossWin,
    grossLoss,
    avgWin: mean(winners.map((t) => t.pnl)),
    avgLoss: mean(losers.map((t) => t.pnl)),
    best: n ? maxOf(trades.map((t) => t.pnl)) : null,
    worst: n ? minOf(trades.map((t) => t.pnl)) : null,
    // Expectancy: what one more trade of this session is worth, on average.
    expectancy: n ? pnl / n : null,
    avgPnlPct: mean(trades.map((t) => t.pnlPct)),
    avgHoldMs: mean(held),
    breakevens: trades.filter((t) => t.pnl === 0).length,
    longs: longs.length,
    shorts: shorts.length,
    longWins: longs.filter((t) => t.pnl > 0).length,
    shortWins: shorts.filter((t) => t.pnl > 0).length,
    maxDrawdown: equityCurve(trades).maxDrawdown,
    // Run-up and heat the average trade went through — the excursion pair, averaged.
    avgRunup: mean(trades.map((t) => t.runup).filter((v) => v != null)),
    avgMae: mean(trades.map((t) => t.mae).filter((v) => v != null))
  };
}

/** The same numbers split by direction — a strategy that only loses on shorts says so here.
 *  Each bucket carries what a profits-and-losses bar needs: the two gross sides and the net. */
export function sideStats(trades) {
  const of = (list) => {
    const grossWin = list.filter((t) => t.pnl > 0).reduce((s, t) => s + t.pnl, 0);
    const grossLoss = list.filter((t) => t.pnl < 0).reduce((s, t) => s - t.pnl, 0);
    return {
      trades: list.length,
      pnl: grossWin - grossLoss,
      grossWin,
      grossLoss,
      winRate: list.length ? (list.filter((t) => t.pnl > 0).length / list.length) * 100 : 0
    };
  };
  return {
    all: of(trades),
    long: of(trades.filter((t) => t.dir > 0)),
    short: of(trades.filter((t) => t.dir < 0))
  };
}

/** Longest runs of winners and losers, and the run in progress (signed: + wins, − losses). */
export function streaks(trades) {
  const sorted = [...trades].sort((a, b) => a.closeTs - b.closeTs);
  let maxWins = 0;
  let maxLosses = 0;
  let run = 0;
  for (const t of sorted) {
    if (t.pnl > 0) run = run > 0 ? run + 1 : 1;
    else if (t.pnl < 0) run = run < 0 ? run - 1 : -1;
    else run = 0;
    if (run > maxWins) maxWins = run;
    if (-run > maxLosses) maxLosses = -run;
  }
  return { maxWins, maxLosses, current: run };
}

/**
 * Returns histogram: trades bucketed by their percentage result, winners and losers counted
 * apart so the two colours can be stacked on one bar. `step` is the bucket width in percent;
 * null lets it follow the spread (a scalping session must not land in one bar).
 */
export function returnBuckets(trades, step = null) {
  if (!trades.length) return { buckets: [], step: 1, avgWinPct: null, avgLossPct: null };
  const pcts = trades.map((t) => t.pnlPct);
  const lo = minOf(pcts);
  const hi = maxOf(pcts);
  const span = Math.max(hi - lo, 1e-6);
  // ~16 buckets, snapped to a readable width (…, 0.5, 1, 2, 5, 10, …).
  const raw = step ?? span / 16;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const w = step ?? [1, 2, 5, 10].map((k) => k * mag).find((k) => k >= raw) ?? mag * 10;
  const idx = (v) => Math.floor(v / w);
  const from = idx(lo);
  const to = idx(hi);
  const buckets = [];
  for (let i = from; i <= to; i++) buckets.push({ from: i * w, to: (i + 1) * w, winners: 0, losers: 0 });
  for (const t of trades) {
    const b = buckets[idx(t.pnlPct) - from];
    if (!b) continue;
    if (t.pnl >= 0) b.winners++;
    else b.losers++;
  }
  const mean = (xs) => (xs.length ? xs.reduce((s, x) => s + x, 0) / xs.length : null);
  return {
    buckets,
    step: w,
    avgWinPct: mean(trades.filter((t) => t.pnl > 0).map((t) => t.pnlPct)),
    avgLossPct: mean(trades.filter((t) => t.pnl < 0).map((t) => t.pnlPct))
  };
}
