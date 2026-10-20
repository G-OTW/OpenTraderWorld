/** Broker book overlay: what a synced account holds and what it has working, matched to the
 *  instrument a chart is showing.
 *
 *  The book is **pulled on demand**, never followed: the user picks an account and presses
 *  sync, and what comes back is a snapshot stamped with the moment it was read. Nothing here
 *  places, changes or cancels anything; the chart only draws.
 */

/** The module id this overlay proves its grant with. */
export const MODULE = 'histviz';

/** The account the workspace last synced, so the choice survives a reload. The rows never
 *  are: a stale book drawn as if it were live is worse than an empty chart. */
const ACCOUNT_KEY = 'otw.histviz.book.account';

export function loadBookAccount() {
  try {
    return localStorage.getItem(ACCOUNT_KEY) || '';
  } catch {
    return '';
  }
}

export function saveBookAccount(id) {
  try {
    if (id) localStorage.setItem(ACCOUNT_KEY, id);
    else localStorage.removeItem(ACCOUNT_KEY);
  } catch {
    /* empty */
  }
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

/** Price levels to draw, from a slice.
 *
 *  A position is one line at its average cost; without one there is no price to draw at, so
 *  it is counted and left off the chart rather than parked at zero. An order is one line per
 *  price it carries: a stop-limit works at two levels and hiding either would misdraw it. */
export function bookLines(slice) {
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
      account: p.account ?? ''
    });
  }
  for (const o of slice?.orders ?? []) {
    const rest = Math.max(0, (Number(o.qty) || 0) - (Number(o.filled_qty) || 0));
    const limit = Number(o.limit_price);
    const stop = Number(o.stop_price);
    const base = {
      kind: 'order',
      side: o.side === 'sell' ? 'sell' : 'buy',
      qty: rest,
      type: o.order_type ?? '',
      currency: o.currency ?? '',
      venue: o.venue ?? '',
      account: o.account ?? ''
    };
    if (limit > 0) out.push({ ...base, key: `o:${o.id}:limit`, level: 'limit', price: limit });
    if (stop > 0 && stop !== limit) {
      out.push({ ...base, key: `o:${o.id}:stop`, level: 'stop', price: stop });
    }
  }
  return out;
}

/** Positions in the slice the chart cannot draw, because the broker reports no average
 *  cost for them. Shown as a count, so an empty overlay is never a mystery. */
export function unpricedPositions(slice) {
  return (slice?.positions ?? []).filter((p) => !(Number(p.avg_price) > 0)).length;
}
