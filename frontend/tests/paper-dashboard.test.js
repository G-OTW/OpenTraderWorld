import { test } from 'node:test';
import assert from 'node:assert/strict';
import { projectBook, sessionState } from '../src/lib/modules/backtest/paper-dashboard.js';
const at = '2026-10-03T12:00:00Z';
const fixture = () => ({ session: { status: 'active', last_run_at: at, settings: { instrument: { multiplier: 10 } }, stats: {
  open_positions: [{ ticker: 'AAA', direction: 'long', entry_ts: at, avg_price: 100, qty: 2 }],
  pending_orders: [{ ticker: 'BBB', direction: 'short', kind: 'entry', order: 'market', qty: 3, signal_ts: at }]
} }, instruments: [{ id: 'a', ticker: 'AAA', price: 105, price_at: at }, { id: 'b', ticker: 'BBB', price: 20 }], trades: [], events: [], live_fills: [] });
test('execution state distinguishes scheduled, pending, running, stopped and failed', () => {
  assert.equal(sessionState({ status: 'active' }), 'pending');
  assert.equal(sessionState({ status: 'active', stats: {} }), 'watching');
  assert.equal(sessionState({ status: 'active', running: true }), 'running');
  assert.equal(sessionState({ status: 'paused' }), 'stopped');
  assert.equal(sessionState({ status: 'error' }), 'error');
});
test('marks long and short positions with the contract multiplier', () => {
  const data = fixture();
  assert.equal(projectBook(data).unrealized, 100);
  data.session.stats.open_positions[0].direction = 'short';
  assert.equal(projectBook(data, { a: { price: 95 } }).unrealized, 100);
  assert.equal(projectBook(data).exposure, 2100);
});
test('missing or ambiguous marks remain unknown, not zero', () => {
  const data = fixture();
  data.instruments.push({ id: 'other', ticker: 'AAA', price: 110 });
  assert.equal(projectBook(data).unrealized, null);
  data.instruments = [];
  assert.equal(projectBook(data).unrealized, null);
});
test('pending market signals are not open positions', () => {
  const data = fixture();
  data.trades.push({ open: true, ticker: 'BBB', trade: { pending: true, entry_price: 20 } });
  const result = projectBook(data);
  assert.equal(result.open.length, 1);
  assert.equal(result.pending.length, 1);
});
test('live market fills replace pending orders before the next candle evaluation', () => {
  const data = fixture();
  data.live_fills.push({ kind: 'entry', ticker: 'BBB', direction: 'short', reason: 'market', price: 22, at: '2026-10-03T12:00:01Z', trade_key: 'BBB|short|live:fill' });
  const result = projectBook(data);
  assert.equal(result.pending.length, 0);
  assert.equal(result.open.length, 2);
  assert.equal(result.open[1].avg_price, 22);
  assert.equal(result.open[1].unrealized, 60);
});
test('live exits remove positions and update realized P&L exactly once', () => {
  const data = fixture();
  data.live_fills.push({ kind: 'exit', trade_key: `AAA|long|${at}`, at: '2026-10-03T12:01:00Z' });
  const payload = { ticker: 'AAA', direction: 'long', entry_ts: at, exit_ts: '2026-10-03T12:01:00Z', pnl: 98, fees: 2, live: true };
  data.events.push({ kind: 'close', payload });
  assert.equal(projectBook(data).open.length, 0);
  assert.equal(projectBook(data).realized, 98);
  data.trades.push({ ...payload, open: false, trade: payload });
  assert.equal(projectBook(data).closed.length, 1);
  assert.equal(projectBook(data).realized, 98);
});
test('an older fill cannot consume a new pending order', () => {
  const data = fixture();
  data.live_fills.push({ kind: 'entry', ticker: 'BBB', direction: 'short', reason: 'market', at: '2026-10-03T11:59:59Z' });
  assert.equal(projectBook(data).pending.length, 1);
});
test('history is sorted by close time and accumulates net realized P&L', () => {
  const data = fixture();
  data.trades = [2, 1].map((n) => ({ open: false, ticker: 'AAA', pnl: n === 1 ? 100 : -40, fees: 2, exit_ts: `2026-10-03T12:0${n}:00Z`, trade: {} }));
  const result = projectBook(data);
  assert.deepEqual(result.curve.map((p) => p.value), [100, 60]);
  assert.equal(result.winRate, 50);
  assert.equal(result.fees, 4);
});
test('replayed live round trips are not counted twice when timestamps become candle times', () => {
  const data = fixture();
  data.instruments[0].timeframe = '1m';
  const p = { ticker: 'AAA', direction: 'long', entry_ts: '2026-10-03T12:00:05Z', exit_ts: '2026-10-03T12:01:22Z', exit_price: 110, exit_reason: 'take_profit', qty: 2, pnl: 198, fees: 2, live: true };
  data.events.push({ kind: 'close', payload: p });
  data.trades.push({ ...p, entry_ts: at, exit_ts: '2026-10-03T12:01:00Z', open: false, trade: p });
  assert.equal(projectBook(data).closed.length, 1);
  assert.equal(projectBook(data).realized, 198);
});
test('a failed evaluation preserves a filled order from the previous snapshot', () => {
  const data = fixture();
  data.session.last_run_at = '2026-10-03T12:02:00Z';
  data.session.status = 'error';
  data.live_fills.push({ kind: 'entry', ticker: 'BBB', direction: 'short', reason: 'market', price: 22, at: '2026-10-03T12:00:01Z', trade_key: 'BBB|short|live:fill' });
  assert.equal(projectBook(data).pending.length, 0);
  assert.equal(projectBook(data).open.length, 2);
});
