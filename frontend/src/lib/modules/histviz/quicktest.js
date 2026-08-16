/** Quick backtest — the position engine behind clicking entries and exits on the chart.
 *
 * The user only ever posts **fills**: a side, a price and an instant.
 *
 *   { id, side: 'long' | 'short', x: epoch-ms, y: price, qty }
 *
 * Everything else — the open position, its weighted average cost, the closed trades and their
 * PnL — is *replayed* from that list, never stored. That is what makes the two gestures the
 * spec asks for cheap: flipping a marker long↔short or deleting one is a one-field edit, and
 * the whole session re-derives itself around it.
 *
 * Like drawings, a fill is anchored in time+price, so it survives pan/zoom, a timeframe switch
 * and older history being loaded in front of it.
 *
 * Position rules (the ones a discretionary trader expects):
 *  - same side as the open position  → pyramiding, weighted-average entry price
 *  - opposite side, smaller quantity → partial close, PnL realized on that slice
 *  - opposite side, larger quantity  → the position closes and reverses with the remainder
 */

const EPS = 1e-9;

export const DEFAULT_QTY = 1;

/** Marker ink. Fixed on purpose: blue long / red short is the trading convention, and keeping
 *  it out of the candle palette means a user who recolours their candles green-on-magenta
 *  still reads their own entries at a glance. */
export const QUICK_COLORS = { long: '#3b82f6', short: '#ef4444' };

let seq = 0;
export const newFillId = () => `q${Date.now().toString(36)}${(seq++).toString(36)}`;

/** A fill ready to push into the session list. */
export function makeFill(side, x, y, qty = DEFAULT_QTY) {
  return { id: newFillId(), side, x, y, qty: Math.max(EPS, Number(qty) || DEFAULT_QTY) };
}

export const flipSide = (side) => (side === 'long' ? 'short' : 'long');

/** Fills in execution order: by instant, then by insertion (two clicks on the same bar). */
function ordered(fills) {
  return fills
    .map((f, i) => ({ f, i }))
    .sort((a, b) => a.f.x - b.f.x || a.i - b.i)
    .map((e) => e.f);
}

/** How far a trade went each way while it was open, in price units: `adverse` is the worst
 *  heat it took (the risk denominator for R — with no stop-loss to quote, the honest proxy)
 *  and `favorable` the best unrealized move it gave back or kept (its run-up). */
function excursions(bars, dir, entry, fromMs, toMs) {
  if (!bars?.ms?.length) return { adverse: 0, favorable: 0 };
  let adverse = 0;
  let favorable = 0;
  for (let i = 0; i < bars.ms.length; i++) {
    const t = bars.ms[i];
    if (t < fromMs) continue;
    if (t > toMs) break;
    const down = dir > 0 ? entry - bars.l[i] : bars.h[i] - entry;
    const up = dir > 0 ? bars.h[i] - entry : entry - bars.l[i];
    if (down > adverse) adverse = down;
    if (up > favorable) favorable = up;
  }
  return { adverse, favorable };
}

/**
 * Replay a fill list.
 *
 * @param fills the session's fills, in any order
 * @param bars  optional `{ ms: number[], h: number[], l: number[] }` used for MAE/R
 * @returns { marks, trades, open } where `marks` mirrors `fills` (chart markers, with the role
 *          the fill played and the PnL it realized) and `open` is the live position or null.
 */
export function replay(fills, bars = null) {
  const marks = [];
  const trades = [];
  let pos = null;

  const close = (exitTs) => {
    const t = {
      id: pos.id,
      dir: pos.dir,
      qty: pos.entryQty,
      entryAvg: pos.entryNotional / pos.entryQty,
      exitAvg: pos.exitNotional / pos.exitQty,
      openTs: pos.openTs,
      closeTs: exitTs,
      pnl: pos.realized,
      // Every fill that built this trade — what "delete this trade" has to remove. A
      // reversing fill closes one trade and opens the next, so it belongs to both.
      fillIds: pos.fillIds
    };
    const ex = excursions(bars, t.dir, t.entryAvg, t.openTs, t.closeTs);
    const risk = ex.adverse * t.qty;
    t.risk = risk;
    // Signed, the way a strategy tester plots them: run-up above the line, heat below.
    t.runup = ex.favorable * t.qty;
    t.mae = -risk;
    t.r = risk > EPS ? t.pnl / risk : null;
    t.pnlPct = t.entryAvg > 0 ? (t.pnl / (t.entryAvg * t.qty)) * 100 : 0;
    trades.push(t);
    pos = null;
  };

  for (const f of ordered(fills)) {
    const dir = f.side === 'long' ? 1 : -1;
    let left = Math.max(0, Number(f.qty) || 0);
    let role = 'entry';
    let realized = 0;

    if (pos && pos.dir !== dir && left > EPS) {
      pos.fillIds.push(f.id);
      const q = Math.min(left, pos.qty);
      realized = (f.y - pos.avg) * q * pos.dir;
      pos.realized += realized;
      pos.qty -= q;
      pos.exitQty += q;
      pos.exitNotional += f.y * q;
      left -= q;
      role = pos.qty > EPS ? 'reduce' : 'exit';
      if (pos.qty <= EPS) close(f.x);
    }

    if (left > EPS) {
      if (!pos) {
        pos = {
          id: f.id,
          dir,
          qty: left,
          avg: f.y,
          openTs: f.x,
          entryQty: left,
          entryNotional: f.y * left,
          exitQty: 0,
          exitNotional: 0,
          realized: 0,
          fillIds: [f.id]
        };
        role = role === 'exit' ? 'reverse' : 'entry';
      } else {
        pos.avg = (pos.avg * pos.qty + f.y * left) / (pos.qty + left);
        pos.qty += left;
        pos.entryQty += left;
        pos.entryNotional += f.y * left;
        pos.fillIds.push(f.id);
        role = 'add';
      }
    }

    marks.push({ id: f.id, x: f.x, y: f.y, side: f.side, qty: f.qty, role, realized });
  }

  const open = pos
    ? {
        dir: pos.dir,
        qty: pos.qty,
        avg: pos.avg,
        openTs: pos.openTs,
        realized: pos.realized,
        fillIds: pos.fillIds
      }
    : null;
  return { marks, trades, open };
}

/**
 * How the size box is read. The engine only ever counts **units** (what the position holds);
 * a mode is just the arithmetic that turns what the user typed into units, at the price they
 * clicked. Sizing in contracts or in dollars is otherwise the same session.
 *
 *  units     — the quantity itself (0.5 BTC)
 *  contracts — contracts × contract size / point value (2 × 50 index points)
 *  notional  — an amount of quote currency, converted at the fill price ($5 000 worth)
 */
export const SIZE_MODES = ['units', 'contracts', 'notional'];

export function resolveQty(mode, size, price, contractSize = 1) {
  const s = Number(size);
  if (!Number.isFinite(s) || s <= 0) return DEFAULT_QTY;
  if (mode === 'contracts') {
    const m = Number(contractSize);
    return s * (Number.isFinite(m) && m > 0 ? m : 1);
  }
  if (mode === 'notional') {
    const p = Number(price);
    return p > 0 ? s / p : DEFAULT_QTY;
  }
  return s;
}

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
    best: n ? Math.max(...trades.map((t) => t.pnl)) : null,
    worst: n ? Math.min(...trades.map((t) => t.pnl)) : null,
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
  const lo = Math.min(...pcts);
  const hi = Math.max(...pcts);
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

/** Unrealized PnL of the open position at `price`. */
export function unrealized(open, price) {
  if (!open || !Number.isFinite(price)) return 0;
  return (price - open.avg) * open.qty * open.dir;
}
