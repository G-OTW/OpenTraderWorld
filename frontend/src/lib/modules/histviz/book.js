/** Broker book overlay: what a synced account holds and what it has working, matched to the
 *  instrument a chart is showing.
 *
 *  The book is **pulled on demand**, never followed: the user picks an account and presses
 *  sync, and what comes back is a snapshot stamped with the moment it was read. Nothing here
 *  places, changes or cancels anything; the chart only draws.
 */

/** The module id this overlay proves its grant with. */
export const MODULE = 'histviz';

/** Grant that lets orders go to an account (see `$lib/brokers/api.js`). */
const TRADING = 'trading';

/** The broker OpenTraderWorld runs itself: it fills on the chart's own live price. */
export const PAPER = 'paper';

/** Whether a broker serves the instrument a chart shows: the chart's data comes from that
 *  broker's own feed (`ibkr` data for an `ibkr` account). Exact, never a lookalike: an
 *  instrument the broker might list under another name is not one it was asked about. A
 *  paper account trades whatever the chart shows, on the chart's feed. */
export function brokerServes(account, coords) {
  if (!account || !coords?.provider) return false;
  return account.broker === PAPER || coords.provider === account.broker;
}

/** Why a chart cannot be traded through its account, or '' when it can: no account picked,
 *  trading not switched on for it, or an instrument the broker does not serve. */
export function tradeLock(account, coords) {
  if (!account) return 'noAccount';
  if (!(account.modules ?? []).includes(TRADING)) return 'noTrading';
  if (!brokerServes(account, coords)) return 'notServed';
  return '';
}

/** Punctuation, the only thing two spellings of one ticker are allowed to differ by. */
const PUNCT = /[^A-Z0-9]/g;

/** Whether a broker symbol and a chart ticker name the same instrument.
 *
 *  Exact first, then punctuation-insensitive: `BTC-USD`, `BTC/USD` and `BTCUSD` are one
 *  instrument spelled three ways by three venues. That is where it stops. An alias is a
 *  guess (Kraken's `XBT` is not `BTC` by arithmetic, only by convention), and a guessed
 *  instrument draws someone else's position on your chart. */
export function sameInstrument(a, b) {
  const x = String(a ?? '')
    .trim()
    .toUpperCase();
  const y = String(b ?? '')
    .trim()
    .toUpperCase();
  if (!x || !y) return false;
  return x === y || x.replace(PUNCT, '') === y.replace(PUNCT, '');
}

/** The slice of a book that belongs to one ticker. */
export function bookFor(book, ticker) {
  if (!book || !ticker) return { positions: [], orders: [] };
  return {
    positions: (book.positions ?? []).filter((p) => sameInstrument(p.symbol, ticker)),
    orders: (book.orders ?? []).filter((o) => sameInstrument(o.symbol, ticker))
  };
}

/** Every instrument the book mentions, once, in order: what the account is in, so a chart
 *  showing none of it can say which symbol to open instead of drawing nothing in silence. */
export function bookSymbols(book) {
  // Keyed on the same normalization the chart matches with, so an account that spells one
  // instrument two ways lists it once instead of twice.
  const seen = new Map();
  for (const r of [...(book?.positions ?? []), ...(book?.orders ?? [])]) {
    if (!r.symbol) continue;
    const key = String(r.symbol).trim().toUpperCase().replace(PUNCT, '');
    if (!seen.has(key)) seen.set(key, r.symbol);
  }
  return [...seen.values()].sort();
}

/** Where a trailing stop trails now, worked out from the bars since it was placed: Binance
 *  only reports the level it was placed at. A sell trails `rate` percent under the highest
 *  price since it started, a buy over the lowest. With a start price it starts once the price
 *  reaches it, and draws no stop before (`active: false`).
 *
 *  `bars` is `{ ms, h, l, c }`, the chart's own: the bar the order was placed in counts its
 *  close only, the part of its range before the order is not the order's. `seen` keeps the
 *  furthest price met per order across reads, so a stop never draws lower than it went.
 *  Null when there is not enough to work it out: the broker's level is drawn instead. */
export function trailLevel(o, bars, seen) {
  const rate = Number(o.trail_rate) / 100;
  const placed = Date.parse(o.placed_at ?? '');
  if (!(rate > 0) || !Number.isFinite(placed) || !bars?.ms?.length) return null;
  const sell = o.side === 'sell';
  const start = Number(o.trail_start) > 0 ? Number(o.trail_start) : null;
  // Without a start price it trails from the price it was placed at, which its first level
  // says: Binance places it `rate` away from that price.
  const stop = Number(o.stop_price);
  const from = start ?? (stop > 0 ? (sell ? stop / (1 - rate) : stop / (1 + rate)) : null);
  const key = String(o.id);
  const prev = seen?.get(key);
  let active = start == null || prev != null;
  let far = prev ?? from;
  let i = bars.ms.findLastIndex((ms) => ms <= placed);
  const first = Math.max(0, i);
  for (i = first; i < bars.ms.length; i++) {
    const p = i === first && bars.ms[i] <= placed ? bars.c[i] : sell ? bars.h[i] : bars.l[i];
    if (!(p > 0)) continue;
    if (!active && (sell ? p >= start : p <= start)) active = true;
    if (active) far = far == null ? p : sell ? Math.max(far, p) : Math.min(far, p);
  }
  if (!active) return { active: false };
  if (far == null) return null;
  seen?.set(key, far);
  return { active: true, price: sell ? far * (1 - rate) : far * (1 + rate) };
}

/** Price levels to draw, from a slice.
 *
 *  A position is one line at its average cost; without one there is no price to draw at, so
 *  it is counted and left off the chart rather than parked at zero. An order is one line per
 *  price it carries: a stop-limit works at two levels and hiding either would misdraw it. A
 *  trailing stop draws where it trails now (`trailLevel`, from `bars` and `seen`). */
export function bookLines(slice, bars = null, seen = null) {
  const out = [];
  for (const p of slice?.positions ?? []) {
    const price = Number(p.avg_price);
    if (!(price > 0)) continue;
    out.push({
      key: `p:${p.symbol}:${p.side}:${p.account ?? ''}`,
      kind: 'position',
      side: p.side === 'short' ? 'short' : 'long',
      price,
      qty: Number(p.qty) || 0,
      currency: p.currency ?? '',
      venue: p.venue ?? '',
      account: p.account ?? '',
      symbol: p.symbol ?? '',
      multiplier: Number(p.multiplier) > 0 ? Number(p.multiplier) : 1,
      // The broker's id for the instrument, what a close is sent on (null: it cannot close).
      nativeId: p.native_id ?? null
    });
  }
  for (const o of slice?.orders ?? []) {
    const rest = Math.max(0, (Number(o.qty) || 0) - (Number(o.filled_qty) || 0));
    const limit = Number(o.limit_price);
    const trail = trailLevel(o, bars, seen);
    const stop = trail ? (trail.active ? trail.price : NaN) : Number(o.stop_price);
    const parent = o.parent_id ? String(o.parent_id) : null;
    const base = {
      kind: 'order',
      id: String(o.id),
      // An exit of a bracket names its entry; a limit exit is the take-profit, a stop the
      // stop-loss.
      parent,
      role: parent ? (limit > 0 && !(stop > 0) ? 'tp' : 'sl') : null,
      side: o.side === 'sell' ? 'sell' : 'buy',
      qty: rest,
      type: o.order_type ?? '',
      currency: o.currency ?? '',
      venue: o.venue ?? '',
      account: o.account ?? ''
    };
    if (limit > 0) out.push({ ...base, key: `o:${o.id}:limit`, level: 'limit', price: limit });
    if (stop > 0 && stop !== limit) {
      out.push({ ...base, key: `o:${o.id}:stop`, level: 'stop', price: stop, trailing: !!trail });
    }
    // A trailing stop shows its start price until the market reaches it, linked to its
    // stop; once it trails, the start says nothing more (Binance fills one in when none was
    // given: the price at placing).
    const start = Number(o.trail_start);
    if (start > 0 && start !== stop && !trail?.active) {
      out.push({ ...base, key: `o:${o.id}:start`, level: 'start', price: start, parent: String(o.id) });
    }
  }
  return out;
}

/** Positions in the slice the chart cannot draw, because the broker reports no average
 *  cost for them. Shown as a count, so an empty overlay is never a mystery. */
export function unpricedPositions(slice) {
  return (slice?.positions ?? []).filter((p) => !(Number(p.avg_price) > 0)).length;
}
