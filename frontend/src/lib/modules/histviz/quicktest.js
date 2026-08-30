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

// The analytics are shared with the backtest module; a quick session reads them from here.
export { equityCurve, stats, sideStats, streaks, returnBuckets } from '$lib/analysis/trades.js';

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

/** Unrealized PnL of the open position at `price`. */
export function unrealized(open, price) {
  if (!open || !Number.isFinite(price)) return 0;
  return (price - open.avg) * open.qty * open.dir;
}
