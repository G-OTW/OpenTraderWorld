/** Chart trading: the order model behind the ticket, the quick Buy/Sell bar and the `+` on the
 *  price scale.
 *
 *  An order is built here, then sent by the page to the chart's account (`legsOf`): it shows
 *  as `sending` until the broker accepts it, then `working` with the broker's id in `ref`.
 *  Dragging its line or cancelling it goes to the broker the same way, and the page drops it
 *  once the broker stops listing it (filled, or cancelled elsewhere). An order left from
 *  before trading was wired stays a local `draft`.
 *
 *  An order belongs to an *instrument* (`instrumentKeyOf`), never to a ticker alone: two
 *  providers can spell one ticker for two different things, and a guessed instrument draws an
 *  order on the wrong chart. */

export const ORDER_TYPES = ['market', 'limit', 'stop', 'stop_limit', 'trailing_stop', 'mit', 'lit', 'oco'];
export const TIFS = ['day', 'gtc', 'gtd', 'ioc', 'fok'];
/** What the ticket offers a broker that states nothing: the plain types and times in force
 *  (it refuses what it does not take), and none of the options. */
const PLAIN_TYPES = ['market', 'limit', 'stop', 'stop_limit', 'oco'];
const PLAIN_TIFS = ['day', 'gtc', 'gtd', 'ioc'];
export const SIZINGS = ['units', 'notional', 'risk'];

const capsOf = (account) => (account?.order_caps && typeof account.order_caps === 'object' ? account.order_caps : null);
const list = (v) => (Array.isArray(v) ? v : []);

/** The order types an account's broker takes, in `ORDER_TYPES` order. `oco` is two orders in
 *  one one-cancels-other group, offered where the broker links them. */
export function typesFor(account) {
  const c = capsOf(account);
  if (!c) return PLAIN_TYPES;
  const types = list(c.types);
  return ORDER_TYPES.filter((t) => (t === 'oco' ? c.oca === true && types.includes('limit') && types.includes('stop') : types.includes(t)));
}

/** The times in force an account's broker takes, in `TIFS` order. */
export function tifsFor(account) {
  const c = capsOf(account);
  if (!c) return PLAIN_TIFS;
  const declared = list(c.tif);
  return declared.length ? TIFS.filter((t) => declared.includes(t)) : PLAIN_TIFS;
}

/** Whether an account's broker takes reduce-only orders: only when it says so. */
export const reduceOnlyFor = (account) => capsOf(account)?.reduce_only === true;

/** What else the broker takes, each only when it says so: the ticket shows no more. */
export function optionsFor(account) {
  const c = capsOf(account) ?? {};
  return {
    exits: capsOf(account) ? c.brackets === true : true,
    reduceOnly: c.reduce_only === true,
    postOnly: c.post_only === true,
    trail: list(c.trail),
    trailStart: c.trail_start === true,
    iceberg: c.iceberg === true,
    outsideRth: c.outside_rth === true,
    triggers: list(c.triggers),
    closePosition: c.close_position === true,
    adaptive: c.adaptive === true
  };
}

/** Times in force an attached exit can take: the broker's, less the immediate ones (an exit
 *  waits for its price) and a dated one (it has no date of its own). GTC first where the
 *  broker has it. */
export function exitTifsFor(tifs) {
  const out = tifs.filter((t) => t === 'day' || t === 'gtc');
  return out.includes('gtc') ? ['gtc', ...out.filter((t) => t !== 'gtc')] : out;
}

/** A size from what the ticket was given: units as they are, an amount of money (`notional`)
 *  over the entry's value, or the money at risk (`risk`) over the distance to the stop loss.
 *  `pointValue` is what one unit moves per point (a future's multiplier, 1 otherwise); `whole`
 *  rounds down to whole units (contracts, shares), `step` to the broker's size step (Binance's
 *  `LOT_SIZE`). Null when it cannot be worked out. */
export function sizeFrom(mode, amount, { entry, stop, pointValue = 1, whole = false, step = null } = {}) {
  const a = Number(amount);
  if (!(a > 0)) return null;
  const pv = Number(pointValue) > 0 ? Number(pointValue) : 1;
  let q;
  if (mode === 'notional') {
    if (!(Number(entry) > 0)) return null;
    q = a / (Number(entry) * pv);
  } else if (mode === 'risk') {
    const d = Math.abs(Number(entry) - Number(stop));
    if (!(Number(entry) > 0) || !(Number(stop) > 0) || !(d > 0)) return null;
    q = a / (d * pv);
  } else {
    q = a;
  }
  if (whole) q = Math.floor(q + 1e-9);
  else if (Number(step) > 0) q = floorToStep(q, Number(step));
  else q = Math.floor(q * 1e8) / 1e8;
  return q > 0 ? q : null;
}

/** Down to a multiple of the broker's size step (never up: rounding adds no risk), written
 *  to the step's own decimals so 0.00124 does not come out as 0.0012399999. */
function floorToStep(q, step) {
  const decimals = Math.max(0, -Math.floor(Math.log10(step) + 1e-9));
  return Number((Math.floor(q / step + 1e-9) * step).toFixed(Math.min(decimals, 12)));
}

/** Quote currencies a crypto pair can end with, longest first so `FDUSD` wins over `USD`. */
const QUOTES = ['FDUSD', 'USDT', 'USDC', 'BUSD', 'TUSD', 'USD', 'EUR', 'GBP', 'TRY', 'BRL', 'JPY', 'BTC', 'ETH', 'BNB'];

/** Base and quote of a crypto pair as the venues spell it (`BTCUSDT`, `BTC-USDT`, `BTC_USDT`,
 *  `ETHUSDT_250926`), or null. Only a label: the order goes out under the chart's ticker. */
export function splitPair(ticker) {
  const parts = String(ticker ?? '').toUpperCase().split(/[-_/]/);
  if (parts.length > 1 && QUOTES.includes(parts[1]) && parts[0]) return { base: parts[0], quote: parts[1] };
  const head = parts[0];
  const quote = QUOTES.find((q) => head.endsWith(q) && head.length > q.length);
  return quote ? { base: head.slice(0, -quote.length), quote } : null;
}

/** Asset kinds that trade in shares, whole ones. */
export const SHARES = ['equity', 'stock', 'etf', 'fund'];

/** What the quick bar's size can be typed in on an instrument: its own unit (`shares`,
 *  `contracts`, or `units` of a crypto base asset, `code` when it is known) and, where an
 *  amount turns into a size by dividing by the price, `notional` in the quote currency.
 *  A listed future or option, or a crypto contract other than Binance USDⓈ-M (whose size is
 *  in the base asset), trades contracts only: its price is not what one contract costs. */
export function tradeUnitsFor({ kind, assetType, broker, ticker, base, quote } = {}) {
  const k = String(kind || assetType || '').toLowerCase();
  const crypto = assetType === 'crypto' || k === 'crypto';
  const b = String(broker ?? '').toLowerCase();
  const derivative = /future|perp|swap|option/.test(k) || (crypto && /futures|deribit/.test(b));
  if (derivative && !(crypto && b === 'binance_futures')) return [{ value: 'units', unit: 'contracts', code: null }];
  const pair = crypto ? splitPair(ticker) : null;
  const units = derivative
    ? { value: 'units', unit: 'contracts', code: null }
    : SHARES.includes(k)
      ? { value: 'units', unit: 'shares', code: null }
      : { value: 'units', unit: 'units', code: base || pair?.base || null };
  return [units, { value: 'notional', unit: 'notional', code: quote || pair?.quote || null }];
}

/** A new order's time in force: day where the broker has it (a 24/7 venue has none), else its
 *  first. */
export const defaultTif = (tifs) => (tifs.includes('day') ? 'day' : (tifs[0] ?? 'day'));

/** Where the order ticket sits: an edge (docked, laid out along it) or the middle of the
 *  charts, laid out in a row or a column. */
export const TICKET_DOCKS = ['bottom', 'top', 'left', 'right', 'center-h', 'center-v'];

/** Whether a ticket at `dock` lays its fields out in a row (true) or a column (false). */
export const dockIsRow = (dock) => dock === 'bottom' || dock === 'top' || dock === 'center-h';

/** The resting type a price implies, the way a broker reads it: buying below the market waits
 *  for it to come down (limit), buying above waits for it to break out (stop), and a sell is
 *  the mirror. With no market to compare against, a limit. */
export function typeAt(side, price, last) {
  if (!(Number(last) > 0) || !(Number(price) > 0)) return 'limit';
  const above = Number(price) > Number(last);
  return side === 'buy' ? (above ? 'stop' : 'limit') : above ? 'limit' : 'stop';
}

/** Which prices a type carries. A trailing stop carries a distance instead, and its stop
 *  field, optional, is the price it starts trailing at; an if-touched order's stop is its
 *  trigger. */
export function pricesOf(type) {
  return {
    limit: type === 'limit' || type === 'stop_limit' || type === 'lit' || type === 'oco',
    stop: type === 'stop' || type === 'stop_limit' || type === 'mit' || type === 'lit' || type === 'oco'
  };
}

/** Whether a type waits for a trigger: what a trigger price source applies to. */
export const triggeredType = (type) => ['stop', 'stop_limit', 'trailing_stop', 'mit', 'lit'].includes(type);

export const flip = (side) => (side === 'buy' ? 'sell' : 'buy');

let seq = 0;
function orderId() {
  seq += 1;
  return `o${Date.now().toString(36)}${seq.toString(36)}`;
}

/** The same orders under new ids, exits following their entry: what a ticket sent again needs,
 *  since a broker account takes each id once, refused or not. */
export function reissue(list) {
  const ids = Object.fromEntries(list.map((o) => [o.id, orderId()]));
  return list.map((o) => ({ ...o, id: ids[o.id], parent: o.parent ? (ids[o.parent] ?? o.parent) : o.parent }));
}

const positive = (v) => Number.isFinite(Number(v)) && Number(v) > 0;

/** Turn what the ticket (or a quick button, or the `+`) asked for into orders.
 *
 *  `req`: { side, type, qty, limit?, stop?, tif?, expires?, tp?, sl?, reduceOnly?, postOnly?,
 *  trail?: { kind: 'amount'|'percent', value }, display?, outsideRth?, trigger?,
 *  closePosition?, adaptive?, exitTif?, exitsReduce? }. `ctx`: { instrument, symbol, account,
 *  journal? }. `expires` (epoch ms) is what a `gtd` order needs, and only it; `journal` is the
 *  journal category a fill is booked into. Returns `{ orders }`, or `{ error }` naming the
 *  field to fix: an order that cannot be placed as written is refused here rather than drawn.
 *  Whether the broker takes each option is its own check, before anything leaves.
 *
 *  - `oco` is two resting legs on the same side in one OCA group (a limit and a stop): the
 *    first to fill cancels the other.
 *  - `tp` / `sl` attach exits to the entry: the opposite side, the same quantity, one OCA
 *    group between them, so the target and the stop never both fill. They take `exitTif`
 *    (GTC by default) and are reduce-only with `exitsReduce`.
 *  - `reduceOnly` flags the entry (or both `oco` legs): it may only shrink the position, for
 *    a broker that declares it (`reduceOnlyFor`).
 *  - `trailing_stop` needs `trail`; its `stop`, when given, is the price it starts at. */
export function buildOrders(req, ctx) {
  const side = req?.side === 'sell' ? 'sell' : 'buy';
  const type = ORDER_TYPES.includes(req?.type) ? req.type : 'market';
  const qty = Number(req?.qty);
  if (!ctx?.instrument) return { error: 'instrument' };
  if (!positive(qty)) return { error: 'qty' };
  const need = pricesOf(type);
  if (need.limit && !positive(req.limit)) return { error: 'limit' };
  if (need.stop && !positive(req.stop)) return { error: 'stop' };
  const trailing = type === 'trailing_stop';
  if (trailing && !positive(req.trail?.value)) return { error: 'trail' };
  if (trailing && req.trail?.kind === 'percent' && !(Number(req.trail.value) < 100)) return { error: 'trail' };
  const display = req.display != null && req.display !== '' ? Number(req.display) : null;
  if (display != null && !(display > 0 && display < qty)) return { error: 'display' };
  if (type === 'oco' && Number(req.limit) === Number(req.stop)) return { error: 'ocoSame' };
  if (req.tp != null && req.tp !== '' && !positive(req.tp)) return { error: 'tp' };
  if (req.sl != null && req.sl !== '' && !positive(req.sl)) return { error: 'sl' };

  const tifs = ctx?.tifs?.length ? ctx.tifs : TIFS;
  const tif = tifs.includes(req?.tif) ? req.tif : defaultTif(tifs);
  const created = Date.now();
  const expires = tif === 'gtd' ? Number(req.expires) : null;
  if (tif === 'gtd' && !(expires > created)) return { error: 'expires' };
  const base = {
    instrument: ctx.instrument,
    symbol: ctx.symbol ?? '',
    account: ctx.account ?? '',
    connector: ctx.connector ?? null,
    journal: ctx.journal ?? null,
    qty,
    tif,
    expires,
    status: 'draft',
    created
  };
  const plain = { reduceOnly: false, postOnly: false, trail: null, display: null, outsideRth: false, trigger: null, closePosition: false, adaptive: null };
  const make = (o) => ({ id: orderId(), limit: null, stop: null, group: null, parent: null, role: 'entry', ...plain, ...base, ...o });
  const reduceOnly = req?.reduceOnly === true;
  // What the entry (or both OCO legs) asks beyond its type.
  const extras = {
    reduceOnly,
    outsideRth: req?.outsideRth === true,
    trigger: req?.trigger && triggeredType(type === 'oco' ? 'stop' : type) ? req.trigger : null
  };

  if (type === 'oco') {
    const group = orderId();
    return {
      orders: [
        make({ side, type: 'limit', limit: Number(req.limit), group, role: 'leg', ...extras, trigger: null }),
        make({ side, type: 'stop', stop: Number(req.stop), group, role: 'leg', ...extras })
      ]
    };
  }

  const entry = make({
    ...extras,
    side,
    type,
    limit: need.limit ? Number(req.limit) : null,
    stop: need.stop || (trailing && positive(req.stop)) ? Number(req.stop) : null,
    postOnly: type === 'limit' && req?.postOnly === true,
    trail: trailing ? { kind: req.trail.kind === 'percent' ? 'percent' : 'amount', value: Number(req.trail.value) } : null,
    display: need.limit ? display : null,
    closePosition: (type === 'stop' || type === 'mit') && req?.closePosition === true,
    adaptive: (type === 'market' || type === 'limit') && req?.adaptive ? req.adaptive : null
  });
  const out = [entry];
  const tp = positive(req.tp) ? Number(req.tp) : null;
  const sl = positive(req.sl) ? Number(req.sl) : null;
  const group = tp != null && sl != null ? orderId() : null;
  const exitTif = exitTifsFor(tifs).includes(req?.exitTif) ? req.exitTif : (exitTifsFor(tifs)[0] ?? 'gtc');
  const exit = { side: flip(side), parent: entry.id, group, tif: exitTif, expires: null, reduceOnly: req?.exitsReduce === true };
  if (tp != null) out.push(make({ ...exit, type: 'limit', limit: tp, role: 'tp' }));
  if (sl != null) out.push(make({ ...exit, type: 'stop', stop: sl, role: 'sl' }));
  return { orders: out };
}

/** Cancel an order: its attached exits go with it (a target with no entry would open a
 *  position instead of closing one), and an OCA group left with one member is no group. */
export function cancelOrder(orders, id) {
  const gone = new Set([id, ...orders.filter((o) => o.parent === id).map((o) => o.id)]);
  const kept = orders.filter((o) => !gone.has(o.id));
  const count = {};
  for (const o of kept) if (o.group) count[o.group] = (count[o.group] ?? 0) + 1;
  return kept.map((o) => (o.group && count[o.group] < 2 ? { ...o, group: null } : o));
}

/** Move one price of an order (a stop-limit has two, `level` says which). */
export function moveOrder(orders, id, level, price) {
  if (!positive(price)) return orders;
  return orders.map((o) => (o.id === id && (level === 'limit' || level === 'stop') ? { ...o, [level]: Number(price) } : o));
}

/** The levels to draw for one instrument: one line per price an order carries. A market
 *  order has none and is listed in the ticket only. */
export function orderLines(orders, instrument) {
  if (!instrument) return [];
  const out = [];
  for (const o of orders ?? []) {
    if (o.instrument !== instrument) continue;
    for (const level of ['limit', 'stop']) {
      if (!positive(o[level])) continue;
      out.push({ key: `${o.id}:${level}`, id: o.id, level, price: Number(o[level]), side: o.side, type: o.type, role: o.role, qty: o.qty, group: o.group, status: o.status, tick: o.tick ?? null, parent: o.parent ?? null });
    }
  }
  return out;
}

/** A number as the decimal string the broker route reads: prices and sizes never travel as
 *  floats. */
export const dec = (v) => (v == null || v === '' ? null : String(Number(v)));

/** The ticket the broker route takes for orders made together (`buildOrders`): the instrument
 *  as the chart's data names it, and one leg per order, exits naming their entry by key. The
 *  chart's provider and connector ride along: a paper account fills on that feed.
 *
 *  `reference` is the chart's last price for the instrument, what a price collar (a risk
 *  limit) measures from, and `referenceAt` when it was seen (epoch ms: a collar refuses a price
 *  too old); `confirmDuplicate` sends an order like one sent seconds ago after the user said so
 *  (the duplicate limit refused it first). */
export function legsOf(list, { reference = null, referenceAt = null, confirmDuplicate = false } = {}) {
  const first = list?.[0];
  if (!first?.instrument) return null;
  const [provider = '', asset_type = '', ...rest] = String(first.instrument).split('|');
  return {
    ticker: rest.join('|'),
    asset_type,
    provider,
    connector: first.connector ?? null,
    reference: positive(reference) ? dec(reference) : null,
    reference_at: positive(reference) && Number.isFinite(referenceAt) ? Math.round(referenceAt) : null,
    confirm_duplicate: confirmDuplicate === true,
    legs: list.map((o) => ({
      key: o.id,
      side: o.side,
      type: o.type,
      qty: dec(o.qty),
      limit: dec(o.limit),
      stop: dec(o.stop),
      tif: o.tif,
      expires: o.expires,
      parent: o.parent,
      group: o.group,
      reduce_only: o.reduceOnly === true,
      post_only: o.postOnly === true,
      trail_amount: o.trail?.kind === 'amount' ? dec(o.trail.value) : null,
      trail_percent: o.trail?.kind === 'percent' ? dec(o.trail.value) : null,
      display_qty: dec(o.display),
      outside_rth: o.outsideRth === true,
      trigger: o.trigger ?? null,
      close_position: o.closePosition === true,
      adaptive: o.adaptive ?? null
    }))
  };
}

/** A dragged price on an order's own tick (the broker's increment for its instrument), or
 *  `fallback` (the chart's snap) when the broker gave none. */
export function snapToTick(price, tick, fallback) {
  const t = Number(tick);
  if (!(t > 0)) return fallback(price);
  const d = Math.max(0, Math.min(10, (String(t).split('.')[1] ?? '').length));
  return roundPrice(Math.round(Number(price) / t) * t, d);
}

/** Round a dragged price to the precision the chart prints. */
export function roundPrice(price, digits) {
  const d = Math.max(0, Math.min(10, Math.round(Number(digits) || 0)));
  const f = 10 ** d;
  return Math.round(Number(price) * f) / f;
}

/** Snap a bar dropped at (x, y) inside a w x h pane to its nearest edge. `at` is the share of
 *  the edge before the bar's centre, so the bar comes back to the same spot after a resize. */
export function snapEdge(x, y, w, h) {
  const W = Math.max(1, w);
  const H = Math.max(1, h);
  const d = { top: y, bottom: H - y, left: x, right: W - x };
  const edge = Object.keys(d).reduce((a, b) => (d[b] < d[a] ? b : a));
  const along = edge === 'top' || edge === 'bottom' ? x / W : y / H;
  return { edge, at: Math.min(1, Math.max(0, along)) };
}

// ── Persistence ──
// This browser's list, like the quick session: not part of the workspace layout. The broker
// holds the truth; the list is what the chart follows.
const ORDERS_KEY = 'otw.histviz.orders.v1';
const MAX_ORDERS = 200;

export function loadOrders() {
  try {
    const list = JSON.parse(localStorage.getItem(ORDERS_KEY) || '[]');
    return Array.isArray(list) ? settle(list.filter((o) => o?.id && o?.instrument)) : [];
  } catch {
    return [];
  }
}

/** A list read back after a reload. An order caught mid-send has no broker id, so whether it
 *  went out is unknown: it is dropped, and the next book sync shows it if it did. One caught
 *  mid-move or mid-cancel is working, and the broker poll settles the rest. */
export function settle(list) {
  return list.filter((o) => o.status !== 'sending' || o.ref).map((o) => (o.status === 'sending' ? { ...o, status: 'working' } : o));
}

/** Orders the broker no longer lists: filled, or cancelled elsewhere. `listed` is the set of
 *  the account's working ids, read at `asked`; an order sent after that moment is left alone,
 *  since the read could not have seen it. */
export function gone(list, account, listed, asked) {
  return list.filter((o) => o.account === account && o.ref && o.status === 'working' && (o.sentAt ?? 0) < asked && !listed.has(String(o.ref)));
}

export function saveOrders(list) {
  try {
    localStorage.setItem(ORDERS_KEY, JSON.stringify(list.slice(-MAX_ORDERS)));
  } catch {
    /* non-fatal */
  }
}
