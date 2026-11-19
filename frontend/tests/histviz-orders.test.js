import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  typeAt,
  reissue,
  buildOrders,
  cancelOrder,
  moveOrder,
  orderLines,
  roundPrice,
  snapEdge,
  dockIsRow,
  legsOf,
  snapToTick,
  settle,
  gone,
  tifsFor,
  defaultTif,
  typesFor,
  optionsFor,
  exitTifsFor,
  sizeFrom
} from '../src/lib/modules/histviz/orders.js';

const ctx = { instrument: 'ibkr|future|MNQ', symbol: 'MNQ', account: 'a1' };

test('a price on the scale implies limit or stop the way a broker reads it', () => {
  assert.equal(typeAt('buy', 90, 100), 'limit');
  assert.equal(typeAt('buy', 110, 100), 'stop');
  assert.equal(typeAt('sell', 110, 100), 'limit');
  assert.equal(typeAt('sell', 90, 100), 'stop');
  assert.equal(typeAt('buy', 110, null), 'limit', 'no market to compare against');
});

test('an order that cannot be placed as written names the field to fix', () => {
  assert.equal(buildOrders({ side: 'buy', type: 'market', qty: 0 }, ctx).error, 'qty');
  assert.equal(buildOrders({ side: 'buy', type: 'limit', qty: 1 }, ctx).error, 'limit');
  assert.equal(buildOrders({ side: 'buy', type: 'stop_limit', qty: 1, limit: 10 }, ctx).error, 'stop');
  assert.equal(buildOrders({ side: 'buy', type: 'oco', qty: 1, limit: 10, stop: 10 }, ctx).error, 'ocoSame');
  assert.equal(buildOrders({ side: 'buy', type: 'market', qty: 1 }, {}).error, 'instrument');
  assert.equal(buildOrders({ side: 'buy', type: 'market', qty: 1, tp: -1 }, ctx).error, 'tp');
});

test('a bracket attaches opposite-side exits in one OCA group', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'limit', qty: 2, limit: 100, tp: 110, sl: 95 }, ctx);
  assert.equal(orders.length, 3);
  const [entry, tp, sl] = orders;
  assert.equal(entry.limit, 100);
  assert.equal(entry.stop, null);
  for (const x of [tp, sl]) {
    assert.equal(x.side, 'sell');
    assert.equal(x.qty, 2);
    assert.equal(x.parent, entry.id);
  }
  assert.equal(tp.type, 'limit');
  assert.equal(sl.type, 'stop');
  assert.ok(tp.group && tp.group === sl.group);
  assert.equal(entry.group, null);
});

test('a venue without day orders gets its own default time in force', () => {
  const binance = { order_caps: { tif: ['ioc', 'gtc'] } };
  assert.deepEqual(tifsFor(binance), ['gtc', 'ioc'], 'in ticket order');
  assert.deepEqual(tifsFor({}), ['day', 'gtc', 'gtd', 'ioc'], 'a broker stating none is offered all');
  assert.equal(defaultTif(tifsFor(binance)), 'gtc');
  assert.equal(defaultTif(tifsFor({})), 'day');
  const { orders } = buildOrders({ side: 'buy', type: 'market', qty: 1 }, { ...ctx, tifs: tifsFor(binance) });
  assert.equal(orders[0].tif, 'gtc');
  assert.equal(buildOrders({ side: 'buy', type: 'market', qty: 1 }, ctx).orders[0].tif, 'day');
});

test('a single exit is not an OCA group', () => {
  const { orders } = buildOrders({ side: 'sell', type: 'market', qty: 1, sl: 120 }, ctx);
  assert.equal(orders[1].side, 'buy');
  assert.equal(orders[1].group, null);
});

test('a good-till-date order needs a future expiry; its exits stay GTC; every leg keeps the journal', () => {
  const base = { side: 'buy', type: 'limit', qty: 1, limit: 100, tif: 'gtd' };
  assert.equal(buildOrders(base, ctx).error, 'expires');
  assert.equal(buildOrders({ ...base, expires: Date.now() - 1000 }, ctx).error, 'expires');
  const at = Date.now() + 86_400_000;
  const { orders } = buildOrders({ ...base, expires: at, tp: 110, sl: 95 }, { ...ctx, journal: 'j1' });
  assert.deepEqual(orders.map((o) => [o.role, o.tif, o.expires, o.journal]), [
    ['entry', 'gtd', at, 'j1'],
    ['tp', 'gtc', null, 'j1'],
    ['sl', 'gtc', null, 'j1']
  ]);
  const day = buildOrders({ ...base, tif: 'day', expires: at }, ctx).orders[0];
  assert.equal(day.expires, null);
  assert.equal(day.journal, null);
});

test('an OCO is two same-side legs sharing a group', () => {
  const { orders } = buildOrders({ side: 'sell', type: 'oco', qty: 1, limit: 110, stop: 90 }, ctx);
  assert.deepEqual(orders.map((o) => [o.side, o.type]), [['sell', 'limit'], ['sell', 'stop']]);
  assert.equal(orders[0].group, orders[1].group);
});

test('cancelling an entry takes its exits; a lone OCA survivor leaves its group', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'limit', qty: 1, limit: 100, tp: 110, sl: 95 }, ctx);
  assert.deepEqual(cancelOrder(orders, orders[0].id), []);
  const rest = cancelOrder(orders, orders[1].id);
  assert.equal(rest.length, 2);
  assert.equal(rest[1].group, null);
});

test('dragging moves only the level it grabbed; lines are per instrument and per price', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'stop_limit', qty: 1, limit: 101, stop: 100 }, ctx);
  const lines = orderLines(orders, ctx.instrument);
  assert.deepEqual(lines.map((l) => l.level).sort(), ['limit', 'stop']);
  assert.deepEqual(orderLines(orders, 'other|future|MNQ'), []);
  const moved = moveOrder(orders, orders[0].id, 'stop', 99);
  assert.equal(moved[0].stop, 99);
  assert.equal(moved[0].limit, 101);
  assert.equal(moveOrder(orders, orders[0].id, 'stop', 0), orders, 'a non-price is ignored');
  const mkt = buildOrders({ side: 'buy', type: 'market', qty: 1 }, ctx).orders;
  assert.deepEqual(orderLines(mkt, ctx.instrument), [], 'a market order has no line');
});

test('prices round to the printed precision', () => {
  assert.equal(roundPrice(1.23456, 2), 1.23);
  assert.equal(roundPrice(19876.126, 2), 19876.13);
  assert.equal(roundPrice(7.6, 0), 8);
});

test('a dropped bar snaps to its nearest edge and keeps its place along it', () => {
  assert.deepEqual(snapEdge(200, 10, 400, 300), { edge: 'top', at: 0.5 });
  assert.deepEqual(snapEdge(390, 150, 400, 300), { edge: 'right', at: 0.5 });
  assert.deepEqual(snapEdge(5, 60, 400, 300), { edge: 'left', at: 0.2 });
  assert.equal(snapEdge(100, 295, 400, 300).edge, 'bottom');
  assert.equal(snapEdge(-5, -30, 400, 300).at, 0, 'clamped inside the edge');
  assert.equal(dockIsRow('center-h'), true);
  assert.equal(dockIsRow('left'), false);
});

test('a chart trades through its own account only when every condition holds', async () => {
  const { tradeLock, brokerServes } = await import('../src/lib/modules/histviz/book.js');
  const ib = { id: 'a', broker: 'ibkr', modules: ['*', 'trading'] };
  const read = { id: 'b', broker: 'ibkr', modules: ['*'] };
  const mnq = { provider: 'ibkr', asset_type: 'future', ticker: 'MNQZ6' };
  assert.equal(tradeLock(null, mnq), 'noAccount');
  assert.equal(tradeLock(read, mnq), 'noTrading', 'the wildcard never trades');
  assert.equal(tradeLock(ib, { ...mnq, provider: 'alpaca' }), 'notServed');
  assert.equal(tradeLock(ib, mnq), '');
  assert.equal(brokerServes(ib, null), false);
});

test('a range picks the timeframe that reads it, counted back from the last bar', async () => {
  const { pickTimeframe, rangeStart } = await import('../src/lib/modules/histviz/ranges.js');
  assert.equal(pickTimeframe(['1m', '5m', '1h', '1d'], '1h'), '1h');
  assert.equal(pickTimeframe(['1m', '5m', '1h', '1d'], '2h'), '1h', 'finer, never coarser');
  assert.equal(pickTimeframe(['1d', '1w'], '1m'), '1d', 'nothing finer: the finest there is');
  assert.equal(pickTimeframe([], '1d'), null);
  const last = Date.UTC(2026, 9, 16, 20, 0);
  assert.equal(rangeStart({ days: 1 }, last), last - 86_400_000);
  assert.equal(rangeStart({ months: 1 }, last), Date.UTC(2026, 8, 16, 20, 0));
  assert.equal(rangeStart({ ytd: true }, last), Date.UTC(2026, 0, 1));
  assert.equal(rangeStart({ years: 5 }, last), Date.UTC(2021, 9, 16, 20, 0));
  assert.equal(rangeStart({ all: true }, last), null);
});

test('a bracket goes to the broker as one ticket, exits naming their entry', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'limit', qty: 2, limit: 100, tp: 110, sl: 95 }, ctx);
  const t = legsOf(orders);
  assert.equal(t.ticker, 'MNQ');
  assert.equal(t.asset_type, 'future');
  assert.deepEqual(t.legs.map((l) => [l.type, l.side, l.parent === orders[0].id]), [
    ['limit', 'buy', false],
    ['limit', 'sell', true],
    ['stop', 'sell', true]
  ]);
  assert.equal(t.legs[0].key, orders[0].id);
  // Prices and sizes travel as decimal strings, never floats.
  assert.deepEqual([t.legs[0].qty, t.legs[0].limit, t.legs[0].stop, t.legs[2].stop], ['2', '100', null, '95']);
  assert.equal(legsOf([]), null);
});

test('a ticket names the chart feed it was made on, which a paper account fills on', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'market', qty: 1 }, { ...ctx, instrument: 'binance|crypto|BTCUSDT', connector: 'c9' });
  const t = legsOf(orders);
  assert.deepEqual([t.provider, t.asset_type, t.ticker, t.connector], ['binance', 'crypto', 'BTCUSDT', 'c9']);
  // No connector picked on the chart: the server takes the chart's default.
  assert.equal(legsOf(buildOrders({ side: 'buy', type: 'market', qty: 1 }, ctx).orders).connector, null);
});

test('a ticket carries the chart\'s last price for the collar, and a confirmed duplicate says so', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'limit', qty: 1, limit: 100 }, ctx);
  const plain = legsOf(orders);
  assert.equal(plain.reference, null, 'no chart price: the server has nothing to measure from');
  assert.equal(plain.confirm_duplicate, false);
  const t = legsOf(orders, { reference: 101.25, confirmDuplicate: true });
  assert.equal(t.reference, '101.25', 'a decimal string, never a float');
  assert.equal(t.confirm_duplicate, true);
  assert.equal(legsOf(orders, { reference: 0 }).reference, null, 'no price is no reference');
  assert.equal(legsOf(orders, { reference: 'x' }).reference, null);
});

test('a ticket says when the chart saw its price, so a collar can refuse an old one', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'limit', qty: 1, limit: 100 }, ctx);
  assert.equal(legsOf(orders, { reference: 101.25, referenceAt: 1761400000123.4 }).reference_at, 1761400000123);
  assert.equal(legsOf(orders, { reference: 101.25 }).reference_at, null, 'no time: the server refuses the collar');
  assert.equal(legsOf(orders, { referenceAt: 1761400000123 }).reference_at, null, 'no price: no time either');
});

test('a reload drops what was caught mid-send and keeps what the broker had', () => {
  const list = [
    { id: 'a', status: 'sending' },
    { id: 'b', status: 'sending', ref: '7' },
    { id: 'c', status: 'working', ref: '8' },
    { id: 'd', status: 'draft' }
  ];
  assert.deepEqual(settle(list).map((o) => [o.id, o.status]), [
    ['b', 'working'],
    ['c', 'working'],
    ['d', 'draft']
  ]);
});

test('an order the broker stops listing is gone, unless it was sent after the read', () => {
  const list = [
    { id: 'a', account: 'x', ref: '1', status: 'working', sentAt: 10 },
    { id: 'b', account: 'x', ref: '2', status: 'working', sentAt: 10 },
    { id: 'c', account: 'x', ref: '3', status: 'working', sentAt: 50 },
    { id: 'd', account: 'x', ref: '4', status: 'sending', sentAt: 10 },
    { id: 'e', account: 'y', ref: '5', status: 'working', sentAt: 10 }
  ];
  assert.deepEqual(gone(list, 'x', new Set(['2']), 20).map((o) => o.id), ['a']);
});

test('a dragged order lands on the tick its broker stated, else on the chart snap', () => {
  assert.equal(snapToTick(29904.44, 0.25, () => -1), 29904.5);
  assert.equal(snapToTick(29904.1, 0.25, () => -1), 29904);
  assert.equal(snapToTick(1.23456, 0.0001, () => -1), 1.2346);
  assert.equal(snapToTick(29904.44, null, (p) => Math.round(p)), 29904);
});

test('the tick is read from the prices the bars trade on', async () => {
  const { priceTickOf } = await import('../src/lib/modules/histviz/format.js');
  const series = (step, base, n = 40) => {
    const px = Array.from({ length: n }, (_, i) => base + ((i * 7) % 13) * step);
    return { ts: px.map((_, i) => String(i)), o: px, h: px.map((p) => p + 3 * step), l: px.map((p) => p - 2 * step), c: px.map((p) => p + step) };
  };
  assert.equal(priceTickOf(series(0.25, 30029.75)), 0.25);
  assert.equal(priceTickOf(series(0.05, 101.35)), 0.05);
  assert.equal(priceTickOf(series(0.01, 12.37)), 0.01);
  assert.equal(priceTickOf(series(1, 30000)), 1);
  // One price off the grid falls back to the plain precision, never to a wrong grid.
  const off = series(0.25, 30029.75);
  off.c[5] = 30030.49;
  assert.equal(priceTickOf(off), 0.01);
  assert.equal(priceTickOf(series(0.25, 30029.75, 5)), null);
});

test('an order line carries the entry it hangs on, what a hover links', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'limit', qty: 1, limit: 100, tp: 110, sl: 95 }, ctx);
  const lines = orderLines(orders, ctx.instrument);
  assert.deepEqual(lines.map((l) => [l.role, l.parent === orders[0].id]), [
    ['entry', false],
    ['tp', true],
    ['sl', true]
  ]);
});

test('the ticket offers what the broker declares, and the plain set when it declares nothing', () => {
  const spot = { order_caps: { types: ['market', 'limit', 'stop', 'stop_limit', 'trailing_stop', 'mit', 'lit'], tif: ['gtc', 'ioc', 'fok'], brackets: true, oca: true, post_only: true, trail: ['percent'], trail_start: true, iceberg: true } };
  assert.deepEqual(typesFor(spot), ['market', 'limit', 'stop', 'stop_limit', 'trailing_stop', 'mit', 'lit', 'oco']);
  assert.deepEqual(typesFor({ order_caps: { types: ['market', 'limit', 'stop'], oca: false } }), ['market', 'limit', 'stop'], 'no OCO where the broker links none');
  assert.deepEqual(typesFor({}), ['market', 'limit', 'stop', 'stop_limit', 'oco']);
  const o = optionsFor(spot);
  assert.equal(o.postOnly && o.iceberg && o.trailStart && o.exits, true);
  assert.deepEqual(o.trail, ['percent']);
  assert.equal(o.reduceOnly || o.outsideRth || o.closePosition || o.adaptive, false);
  assert.equal(optionsFor({ order_caps: { types: ['market'], brackets: false } }).exits, false, 'no exits where the broker attaches none');
  assert.equal(optionsFor({}).exits, true);
  assert.deepEqual(exitTifsFor(['day', 'gtc', 'gtd', 'ioc', 'fok']), ['gtc', 'day']);
  assert.deepEqual(tifsFor(spot), ['gtc', 'ioc', 'fok']);
});

test('a trailing stop carries its distance and maybe a start price; options ride on the legs', () => {
  assert.equal(buildOrders({ side: 'sell', type: 'trailing_stop', qty: 1 }, ctx).error, 'trail');
  assert.equal(buildOrders({ side: 'sell', type: 'trailing_stop', qty: 1, trail: { kind: 'percent', value: 100 } }, ctx).error, 'trail');
  const { orders } = buildOrders({ side: 'sell', type: 'trailing_stop', qty: 1, trail: { kind: 'percent', value: 1.5 }, stop: 110, trigger: 'mark' }, ctx);
  const [l] = legsOf(orders).legs;
  assert.deepEqual([l.type, l.trail_percent, l.trail_amount, l.stop, l.trigger], ['trailing_stop', '1.5', null, '110', 'mark']);
  const m = legsOf(buildOrders({ side: 'buy', type: 'mit', qty: 1, stop: 95 }, ctx).orders).legs[0];
  assert.deepEqual([m.type, m.stop, m.limit], ['mit', '95', null]);
  const lim = buildOrders({ side: 'buy', type: 'limit', qty: 10, limit: 100, postOnly: true, display: 2, adaptive: 'patient', outsideRth: true }, ctx);
  const k = legsOf(lim.orders).legs[0];
  assert.deepEqual([k.post_only, k.display_qty, k.adaptive, k.outside_rth], [true, '2', 'patient', true]);
  assert.equal(buildOrders({ side: 'buy', type: 'limit', qty: 2, limit: 100, display: 2 }, ctx).error, 'display', 'the shown size is below the whole');
  // Options that the type cannot carry are dropped, not sent.
  const mk = legsOf(buildOrders({ side: 'buy', type: 'market', qty: 1, postOnly: true, closePosition: true, trigger: 'last' }, ctx).orders).legs[0];
  assert.deepEqual([mk.post_only, mk.close_position, mk.trigger], [false, false, null]);
  const st = legsOf(buildOrders({ side: 'sell', type: 'stop', qty: 1, stop: 90, closePosition: true }, ctx).orders).legs[0];
  assert.equal(st.close_position, true);
});

test('exits take the time in force asked and are reduce-only when asked', () => {
  const { orders } = buildOrders({ side: 'buy', type: 'limit', qty: 1, limit: 100, tp: 110, sl: 95, exitTif: 'day', exitsReduce: true }, { ...ctx, tifs: ['day', 'gtc'] });
  assert.deepEqual(orders.slice(1).map((o) => [o.tif, o.reduceOnly]), [['day', true], ['day', true]]);
  const gtc = buildOrders({ side: 'buy', type: 'limit', qty: 1, limit: 100, tp: 110, exitTif: 'ioc' }, { ...ctx, tifs: ['day', 'gtc', 'ioc'] }).orders;
  assert.equal(gtc[1].tif, 'gtc', 'an exit never lives for one instant');
});

test('a size comes from units, an amount of money, or the money at risk to the stop', () => {
  assert.equal(sizeFrom('units', 3), 3);
  assert.equal(sizeFrom('notional', 1000, { entry: 50 }), 20);
  assert.equal(sizeFrom('notional', 1000, { entry: 15000, pointValue: 2, whole: true }), null, 'not one whole contract');
  assert.equal(sizeFrom('risk', 200, { entry: 100, stop: 98 }), 100);
  assert.equal(sizeFrom('risk', 500, { entry: 20000, stop: 19950, pointValue: 2, whole: true }), 5);
  assert.equal(sizeFrom('risk', 500, { entry: 100 }), null, 'no stop, no risk');
  assert.equal(sizeFrom('notional', 100, { entry: 80300, step: 0.00001 }), 0.00124, 'down to the lot step');
  assert.equal(sizeFrom('notional', 100, { entry: 80300, step: 0.01 }), null, 'under one step');
  assert.equal(sizeFrom('notional', 1, { entry: 3, step: 0.1 }), 0.3);
});

test('a ticket sent again goes under new ids, its exits still on their entry', () => {
  const list = buildOrders({ side: 'buy', type: 'limit', qty: 1, limit: 100, tp: 110, sl: 95 }, ctx).orders;
  const again = reissue(list);
  assert.equal(again.length, list.length);
  for (const [i, o] of again.entries()) assert.notEqual(o.id, list[i].id);
  const entry = again.find((o) => !o.parent);
  assert.ok(again.filter((o) => o.parent).every((o) => o.parent === entry.id));
  assert.deepEqual(again.map((o) => o.group), list.map((o) => o.group), 'an OCO group is a link, not an id');
});
