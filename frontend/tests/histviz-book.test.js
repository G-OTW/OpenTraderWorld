import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bookLines, bookFor, brokerServes, tradeLock } from '$lib/modules/histviz/book.js';

const book = {
  positions: [{ symbol: 'MNQZ6', side: 'long', qty: 1, avg_price: 30063.93, currency: 'USD', multiplier: 2, account: 'DU1', native_id: '770561201' }],
  orders: [{ id: 17, symbol: 'MNQZ6', side: 'buy', order_type: 'LMT', qty: 1, filled_qty: 0, limit_price: 29950, stop_price: null }]
};

test('a position line carries what closing it and pricing its result need', () => {
  const [p] = bookLines(bookFor(book, 'MNQZ6')).filter((l) => l.kind === 'position');
  assert.equal(p.nativeId, '770561201');
  assert.equal(p.multiplier, 2);
  assert.equal(p.account, 'DU1');
  assert.equal(p.price, 30063.93);
});

test('a broker order line carries its id, what a double-click edits', () => {
  const [o] = bookLines(bookFor(book, 'MNQZ6')).filter((l) => l.kind === 'order');
  assert.equal(o.id, '17');
  assert.equal(o.level, 'limit');
});

test('a position the broker gives no id for cannot be closed from the chart', () => {
  const [p] = bookLines({ positions: [{ ...book.positions[0], native_id: undefined }], orders: [] });
  assert.equal(p.nativeId, null);
});

test('a bracket exit names its entry and says whether it takes profit or stops loss', () => {
  const b = {
    positions: [],
    orders: [
      { id: 14, symbol: 'X', side: 'buy', order_type: 'LMT', qty: 1, limit_price: 100, stop_price: null },
      { id: 15, symbol: 'X', side: 'sell', order_type: 'LMT', qty: 1, limit_price: 110, stop_price: null, parent_id: '14' },
      { id: 16, symbol: 'X', side: 'sell', order_type: 'STP', qty: 1, limit_price: null, stop_price: 95, parent_id: '14' }
    ]
  };
  const lines = bookLines(b);
  assert.deepEqual(lines.map((l) => [l.id, l.parent, l.role]), [
    ['14', null, null],
    ['15', '14', 'tp'],
    ['16', '14', 'sl']
  ]);
});

test('a paper account trades whatever the chart shows; a venue only its own feed', () => {
  const coords = { provider: 'binance', asset_type: 'crypto', ticker: 'BTCUSDT' };
  const paper = { broker: 'paper', modules: ['*', 'trading'] };
  assert.equal(brokerServes(paper, coords), true);
  assert.equal(tradeLock(paper, coords), '');
  assert.equal(brokerServes({ broker: 'ibkr', modules: ['*', 'trading'] }, coords), false);
  assert.equal(brokerServes(paper, null), false);
});

test('a trailing stop the chart cannot work out draws the broker level and its start, linked to it', () => {
  const lines = bookLines({ orders: [{ id: 'BTCUSDT:k3', symbol: 'BTCUSDT', side: 'sell', order_type: 'TRAILING_STOP_MARKET', qty: 0.002, filled_qty: 0, stop_price: 79930.98, trail_start: 80020 }] });
  assert.deepEqual(lines.map((l) => [l.level, l.price, l.parent]), [['stop', 79930.98, null], ['start', 80020, 'BTCUSDT:k3']]);
  const same = bookLines({ orders: [{ id: 'x', symbol: 'BTCUSDT', side: 'sell', qty: 1, stop_price: 80020, trail_start: 80020 }] });
  assert.equal(same.length, 1);
});

test('a trailing stop draws where it trails now, from the bars since it was placed', () => {
  const ms = (m) => Date.parse(`2026-10-26T19:${String(m).padStart(2, '0')}:00Z`);
  // The order goes in during the 19:30 bar: that bar's high (before it) does not count.
  const bars = { ms: [30, 31, 32, 33].map(ms), h: [80200, 80000, 80100, 80050], l: [79900, 79900, 79950, 79990], c: [79950, 79990, 80040, 80010] };
  const order = { id: 'BTCUSDT:algo:k', symbol: 'BTCUSDT', side: 'sell', qty: 0.002, filled_qty: 0, trail_rate: 0.1, stop_price: 79950 * 0.999, placed_at: '2026-10-26T19:30:33Z' };
  const seen = new Map();
  const [line] = bookLines({ orders: [order] }, bars, seen);
  assert.equal(line.level, 'stop');
  assert.ok(Math.abs(line.price - 80100 * 0.999) < 1e-6);
  // A later read with fewer bars never draws it lower than it went.
  const [again] = bookLines({ orders: [order] }, { ms: bars.ms.slice(0, 2), h: bars.h.slice(0, 2), l: bars.l.slice(0, 2), c: bars.c.slice(0, 2) }, seen);
  assert.ok(Math.abs(again.price - 80100 * 0.999) < 1e-6);
});

test('a trailing stop with a start price draws no stop until the price reaches it', () => {
  const ms = (m) => Date.parse(`2026-10-26T19:${String(m).padStart(2, '0')}:00Z`);
  const order = { id: 'k', symbol: 'BTCUSDT', side: 'sell', qty: 1, trail_rate: 0.1, trail_start: 80020, stop_price: 79930.98, placed_at: '2026-10-26T19:10:20Z' };
  const before = bookLines({ orders: [order] }, { ms: [10, 11].map(ms), h: [80015, 80018], l: [80000, 80000], c: [80011, 80010] }, new Map());
  assert.deepEqual(before.map((l) => l.level), ['start']);
  const after = bookLines({ orders: [order] }, { ms: [10, 11].map(ms), h: [80015, 80090], l: [80000, 80000], c: [80011, 80050] }, new Map());
  assert.deepEqual(after.map((l) => [l.level, Math.round(l.price * 100) / 100]), [['stop', 80009.91]]);
});

test('a buy trailing stop trails over the lowest price', () => {
  const ms = (m) => Date.parse(`2026-10-26T19:${String(m).padStart(2, '0')}:00Z`);
  const order = { id: 'b', symbol: 'BTCUSDT', side: 'buy', qty: 1, trail_rate: 1, stop_price: 80800, placed_at: '2026-10-26T19:10:20Z' };
  const [line] = bookLines({ orders: [order] }, { ms: [10, 11].map(ms), h: [80100, 80000], l: [79000, 79500], c: [80000, 79800] }, new Map());
  assert.ok(Math.abs(line.price - 79500 * 1.01) < 1e-6);
});
