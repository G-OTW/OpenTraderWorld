import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyEvent } from '$lib/modules/histviz/accountEvents.js';

const position = {
  symbol: 'MNQZ6',
  side: 'long',
  qty: 1,
  avg_price: 30063.93,
  currency: 'USD',
  asset_class: 'future',
  multiplier: 2,
  venue: 'CME',
  account: 'DU1',
  native_id: '770561201',
  market_price: 30080,
  unrealized_pnl: 32.14,
  opened_at: '2026-10-22T05:57:51Z',
  entry_fee: '0.62',
  fee_currency: 'USD'
};
const order = { id: '17', symbol: 'MNQZ6', side: 'buy', order_type: 'LMT', qty: 1, filled_qty: 0, limit_price: 29950, stop_price: null, account: 'DU1' };
const book = { positions: [position], orders: [order], warnings: [] };
/** A position as the feed sends it: no mark, no venue, no entry, no result. */
const fed = (patch) => ({ ...position, venue: '', market_price: null, unrealized_pnl: null, opened_at: null, entry_fee: null, fee_currency: '', ...patch });

test('a new working order lands on the book', () => {
  const next = applyEvent(book, { kind: 'order', key: 'k1', order: { ...order, id: '56', limit_price: 29900 } });
  assert.deepEqual(next.orders.map((o) => o.id), ['17', '56']);
  assert.equal(book.orders.length, 1, 'the book read is left as it was');
});

test('a moved order replaces its row', () => {
  const next = applyEvent(book, { kind: 'order', order: { ...order, limit_price: 29900 } });
  assert.equal(next.orders.length, 1);
  assert.equal(next.orders[0].limit_price, 29900);
});

test('a partial fill updates what has filled', () => {
  assert.equal(applyEvent(book, { kind: 'filled', id: '17', filled_qty: 1 }).orders[0].filled_qty, 1);
  assert.equal(applyEvent(book, { kind: 'filled', id: '99', filled_qty: 1 }), book, 'an order the book does not list changes nothing');
});

test('an order that stopped working leaves the book', () => {
  assert.deepEqual(applyEvent(book, { kind: 'done', id: '17', status: 'filled' }).orders, []);
});

test('a new position lands on the book, in symbol order', () => {
  const next = applyEvent(book, { kind: 'position', position: fed({ symbol: 'AAPL', native_id: '265598', qty: 10, avg_price: 230 }) });
  assert.deepEqual(next.positions.map((p) => p.symbol), ['AAPL', 'MNQZ6']);
});

test('a position scaled into keeps its mark and entry, not the result of the old size', () => {
  const [p] = applyEvent(book, { kind: 'position', position: fed({ qty: 2, avg_price: 30070 }) }).positions;
  assert.equal(p.qty, 2);
  assert.equal(p.avg_price, 30070);
  assert.equal(p.market_price, 30080);
  assert.equal(p.venue, 'CME');
  assert.equal(p.opened_at, position.opened_at);
  assert.equal(p.unrealized_pnl, null);
});

test('a position that flipped side keeps no entry of the old one', () => {
  const [p] = applyEvent(book, { kind: 'position', position: fed({ side: 'short', qty: 1 }) }).positions;
  assert.equal(p.side, 'short');
  assert.equal(p.opened_at, null);
  assert.equal(p.entry_fee, null);
});

test('a flat position leaves the book', () => {
  assert.deepEqual(applyEvent(book, { kind: 'position', position: fed({ qty: 0 }) }).positions, []);
});

test('a position of another account is its own row', () => {
  const next = applyEvent(book, { kind: 'position', position: fed({ account: 'DU2' }) });
  assert.equal(next.positions.length, 2);
});

test('an event with nothing to apply leaves the book as it is', () => {
  assert.equal(applyEvent(book, { kind: 'resync' }), book);
  assert.equal(applyEvent(null, { kind: 'done', id: '17' }), null);
});
