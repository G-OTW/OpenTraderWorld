/** A trading account's book followed live: the broker's pushes applied to the book the page
 *  last read (`GET /api/brokers/events`, `brokers::AccountEvent` on the server).
 *
 *  An event names a row as it now stands and replaces it: `order` (a working order, whole),
 *  `filled` (how much of one has filled), `done` (it stopped working), `position` (a position,
 *  zero when flat). `resync` is the caller's to handle: it reads the book again.
 *
 *  A position from the feed carries quantity and cost only. The figures a full read adds
 *  (mark, venue, entry, result) are kept from the row it replaces while they still describe
 *  it, and the next read brings them up to date. */

const sameOrder = (a, id) => String(a.id) === String(id);
const samePosition = (a, b) => String(a.native_id ?? a.symbol) === String(b.native_id ?? b.symbol) && (a.account ?? '') === (b.account ?? '');
const blank = (v) => v == null || v === '';

/** The feed's row, completed from the one it replaces. The entry belongs to a position on the
 *  same side; the broker's result only to the same quantity at the same cost. */
function mergePosition(old, p) {
  if (!old) return p;
  const out = { ...p };
  for (const k of ['market_price', 'venue', 'symbol', 'currency', 'asset_class']) if (blank(out[k])) out[k] = old[k];
  if (old.side === p.side) {
    for (const k of ['opened_at', 'entry_fee', 'fee_currency']) if (blank(out[k])) out[k] = old[k];
    if (blank(out.unrealized_pnl) && old.qty === p.qty && old.avg_price === p.avg_price) out.unrealized_pnl = old.unrealized_pnl;
  }
  return out;
}

/** The book after one event, or the same object when the event changes nothing in it. */
export function applyEvent(book, ev) {
  if (!book || !ev) return book;
  const orders = book.orders ?? [];
  const positions = book.positions ?? [];
  switch (ev.kind) {
    case 'order': {
      const i = orders.findIndex((o) => sameOrder(o, ev.order.id));
      if (i < 0) return { ...book, orders: [...orders, ev.order] };
      return { ...book, orders: orders.map((o, j) => (j === i ? { ...o, ...ev.order, placed_at: ev.order.placed_at ?? o.placed_at } : o)) };
    }
    case 'filled': {
      if (!orders.some((o) => sameOrder(o, ev.id))) return book;
      return { ...book, orders: orders.map((o) => (sameOrder(o, ev.id) ? { ...o, filled_qty: ev.filled_qty } : o)) };
    }
    case 'done': {
      if (!orders.some((o) => sameOrder(o, ev.id))) return book;
      return { ...book, orders: orders.filter((o) => !sameOrder(o, ev.id)) };
    }
    case 'position': {
      const p = ev.position;
      const i = positions.findIndex((r) => samePosition(r, p));
      if (!(p.qty > 0)) return i < 0 ? book : { ...book, positions: positions.filter((_, j) => j !== i) };
      const row = mergePosition(i < 0 ? null : positions[i], p);
      const next = i < 0 ? [...positions, row] : positions.map((r, j) => (j === i ? row : r));
      return { ...book, positions: next.sort((a, b) => String(a.symbol).localeCompare(String(b.symbol))) };
    }
    default:
      return book;
  }
}
