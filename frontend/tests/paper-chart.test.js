import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mergeLiveBar } from '../src/lib/modules/histviz/bars.js';
import { paperChartTrades } from '../src/lib/modules/backtest/paper-dashboard.js';

test('the shared candle merge replaces forming candles and inserts catch-up in timestamp order', () => {
  const bar = (minute, close) => ({ ts: `2026-10-03T12:${minute}:00Z`, o: 10, h: 15, l: 5, c: close, v: 100 });
  let bars = mergeLiveBar(null, bar('00', 10));
  bars = mergeLiveBar(bars, bar('10', 12));
  bars = mergeLiveBar(bars, bar('05', 11));
  bars = mergeLiveBar(bars, bar('10', 14));
  assert.deepEqual(bars.ts, ['00', '05', '10'].map((m) => bar(m, 0).ts));
  assert.deepEqual(bars.c, [10, 11, 14]);
  for (const key of ['o', 'h', 'l', 'c', 'v']) assert.equal(bars[key].length, 3);
});

test('paper chart keeps closed fills and open entries without fabricating end-of-window exits', () => {
  const closed = { ticker: 'AAA', entry_price: 100, entry_ts: '2026-10-03T12:00:00Z', exit_price: 110, exit_ts: '2026-10-03T13:00:00Z' };
  const open = { ticker: 'AAA', avg_price: 105, entry_ts: '2026-10-03T14:00:00Z', exit_price: 112, exit_ts: '2026-10-03T15:00:00Z' };
  const book = { closed: [closed, { ...closed, ticker: 'BBB' }], open: [open], pending: [{ ticker: 'AAA', price: 103 }] };
  const trades = paperChartTrades(book, 'AAA');
  assert.equal(trades.length, 2);
  assert.deepEqual(trades[0], closed);
  assert.equal(trades[1].entry_price, 105);
  assert.equal(trades[1].exit_ts, null);
  assert.equal(trades[1].exit_price, null);
  assert.equal(open.exit_price, 112, 'adapting the book must not mutate it');
  assert.deepEqual(paperChartTrades(book, ''), []);
});
