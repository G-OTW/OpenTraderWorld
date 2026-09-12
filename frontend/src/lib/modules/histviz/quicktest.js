/** Quick backtest — the position engine behind clicking entries and exits on the chart.
 *
 * The user only ever posts **fills**: a side, a price and an instant.
 *
 *   { id, side: 'long' | 'short', x: epoch-ms, y: price, qty, lot }
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

let seq = 0;
export const newFillId = () => `q${Date.now().toString(36)}${(seq++).toString(36)}`;

/** A fill ready to push into the session list.
 *
 * `qty` is always in **units**, the only thing the engine counts. `lot` is how many units one
 * of the user's own units is worth (`lotFor`): it is carried so a marker reads as what was
 * typed, 1 contract, not the 2 units it bought. Nothing computes with it. */
export function makeFill(side, x, y, qty = DEFAULT_QTY, lot = 1) {
  const l = Number(lot);
  return {
    id: newFillId(),
    side,
    x,
    y,
    qty: Math.max(EPS, Number(qty) || DEFAULT_QTY),
    lot: Number.isFinite(l) && l > 0 ? l : 1
  };
}

export const flipSide = (side) => (side === 'long' ? 'short' : 'long');

/** A size in the user's own unit: `qty / lot`, abbreviated past a thousand so a marker label
 *  stays a label (12.5k, 3.4M). Fractions keep six decimals, the engine's own precision. */
export function formatSize(v) {
  const n = Number(v);
  if (!Number.isFinite(n)) return '';
  const a = Math.abs(n);
  if (a >= 1e6) return `${Number((n / 1e6).toFixed(2))}M`;
  if (a >= 1e3) return `${Number((n / 1e3).toFixed(2))}k`;
  return Number(n.toFixed(6)).toString();
}

/** What one of the user's own units is worth in engine units, the inverse of `resolveQty`:
 *  divide a quantity by it and you get back what was typed. A contract is its point value; a
 *  dollar of notional buys `1 / price` units, so a fill posted in notional reads in currency,
 *  each marker at its own price (an exit is worth what it was worth on exit, not on entry). */
export function lotFor(mode, price, contractSize = 1) {
  if (mode === 'contracts') {
    const m = Number(contractSize);
    return Number.isFinite(m) && m > 0 ? m : 1;
  }
  if (mode === 'notional') {
    const p = Number(price);
    return p > 0 ? 1 / p : 1;
  }
  return 1;
}

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
      // The same trade in the unit it was clicked in: contracts entered, or the currency the
      // entries and the exits were each worth at their own price.
      size: pos.entrySize,
      exitSize: pos.exitSize,
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
    // What one unit of this fill was typed as. Absent on a session from before sizes were
    // carried: those fills are already in units.
    const lot = Number(f.lot) > 0 ? Number(f.lot) : 1;
    let left = Math.max(0, Number(f.qty) || 0);
    let role = 'entry';
    let realized = 0;

    if (pos && pos.dir !== dir && left > EPS) {
      pos.fillIds.push(f.id);
      const q = Math.min(left, pos.qty);
      realized = (f.y - pos.avg) * q * pos.dir;
      pos.realized += realized;
      // A partial close takes its share of the size still open, so the chip keeps reading in
      // the unit the position was opened in.
      pos.size -= pos.size * (q / pos.qty);
      pos.qty -= q;
      pos.exitQty += q;
      pos.exitSize += q / lot;
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
          size: left / lot,
          avg: f.y,
          openTs: f.x,
          entryQty: left,
          entrySize: left / lot,
          entryNotional: f.y * left,
          exitQty: 0,
          exitSize: 0,
          exitNotional: 0,
          realized: 0,
          fillIds: [f.id]
        };
        role = role === 'exit' ? 'reverse' : 'entry';
      } else {
        pos.avg = (pos.avg * pos.qty + f.y * left) / (pos.qty + left);
        pos.qty += left;
        pos.size += left / lot;
        pos.entryQty += left;
        pos.entrySize += left / lot;
        pos.entryNotional += f.y * left;
        pos.fillIds.push(f.id);
        role = 'add';
      }
    }

    marks.push({ id: f.id, x: f.x, y: f.y, side: f.side, qty: f.qty, lot: f.lot, role, realized });
  }

  const open = pos
    ? {
        dir: pos.dir,
        qty: pos.qty,
        size: pos.size,
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
