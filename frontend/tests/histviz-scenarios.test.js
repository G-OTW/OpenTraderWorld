// Scenario tests for the pure chart logic that had no coverage: timeframe helpers, range
// framing, live-bar merging, price precision / tick detection, and the quick-backtest replay.
// Every expected number is computed by hand in the comment next to it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { tfSeconds, consolidationBase } from '$lib/modules/histviz/pane.js';
import { pickTimeframe, rangeStart, isoSecond } from '$lib/modules/histviz/ranges.js';
import { mergeLiveBar, fillBars } from '$lib/modules/histviz/bars.js';
import {
  decimalsOf,
  priceDigitsOf,
  priceTickOf,
  priceDigitsOfValues,
  compactPrice
} from '$lib/modules/histviz/format.js';
import {
  replay,
  makeFill,
  resolveQty,
  lotFor,
  formatSize,
  unrealized,
  stats,
  equityCurve,
  streaks,
  sideStats
} from '$lib/modules/histviz/quicktest.js';

// ── timeframes ──────────────────────────────────────────────────────────────

const BINANCE = ['1m', '5m', '15m', '1h', '4h', '1d', '1w'];

test('tfSeconds reads canonical and custom labels, nothing else', () => {
  assert.equal(tfSeconds('1m'), 60);
  assert.equal(tfSeconds('7m'), 420);
  assert.equal(tfSeconds('3h'), 10800);
  assert.equal(tfSeconds('2w'), 1209600);
  assert.equal(tfSeconds('0m'), null);
  assert.equal(tfSeconds('1M'), null, 'a month has no fixed length here');
  assert.equal(tfSeconds('abc'), null);
  assert.equal(tfSeconds(undefined), null);
});

test('a custom timeframe consolidates from the served one with the fewest bars per bucket', () => {
  // 10m = 2 x 5m (ratio 2) beats 10 x 1m.
  assert.equal(consolidationBase(BINANCE, '10m'), '5m');
  // 7m is only a multiple of 1m.
  assert.equal(consolidationBase(BINANCE, '7m'), '1m');
  // 3h = 3 x 1h; 4h does not divide it.
  assert.equal(consolidationBase(BINANCE, '3h'), '1h');
  // 2h = 2 x 1h.
  assert.equal(consolidationBase(BINANCE, '2h'), '1h');
  // 90m = 6 x 15m (ratio 6) beats 18 x 5m and 90 x 1m.
  assert.equal(consolidationBase(BINANCE, '90m'), '15m');
  assert.equal(consolidationBase(BINANCE, '3d'), '1d');
  assert.equal(consolidationBase(BINANCE, '2w'), '1w');
});

test('a served timeframe, or one nothing divides, is not consolidated', () => {
  assert.equal(consolidationBase(BINANCE, '1h'), null, 'served as is');
  assert.equal(consolidationBase(['1h', '1d'], '30m'), null, 'finer than anything served');
  assert.equal(consolidationBase(['5m', '1h'], '7m'), null, '7m is no multiple of 5m');
  assert.equal(consolidationBase([], '10m'), null);
  assert.equal(consolidationBase(BINANCE, 'nope'), null);
});

// ── range bar ───────────────────────────────────────────────────────────────

test('a range reads at the timeframe it wants, else the coarsest finer one, else the finest', () => {
  const have = ['1m', '5m', '1h', '1d'];
  assert.equal(pickTimeframe(have, '1h'), '1h');
  assert.equal(pickTimeframe(have, '15m'), '5m', '15m is not offered: the coarsest finer one is 5m');
  assert.equal(pickTimeframe(have, '2h'), '1h');
  assert.equal(pickTimeframe(['1h', '1d'], '1m'), '1h', 'nothing finer: the finest there is');
  assert.equal(pickTimeframe([], '1d'), null);
  assert.equal(pickTimeframe(['junk'], '1d'), null);
});

test('a range counts back from the last bar, in UTC calendar terms', () => {
  const last = Date.UTC(2024, 5, 14, 15, 30); // 2024-06-14T15:30Z
  assert.equal(rangeStart({ days: 1 }, last), last - 86_400_000);
  assert.equal(rangeStart({ days: 7 }, last), Date.UTC(2024, 5, 7, 15, 30));
  assert.equal(rangeStart({ months: 1 }, last), Date.UTC(2024, 4, 14, 15, 30));
  assert.equal(rangeStart({ months: 3 }, last), Date.UTC(2024, 2, 14, 15, 30));
  assert.equal(rangeStart({ years: 1 }, last), Date.UTC(2023, 5, 14, 15, 30));
  assert.equal(rangeStart({ ytd: true }, last), Date.UTC(2024, 0, 1));
  assert.equal(rangeStart({ all: true }, last), null);
});

test('one month back from the 31st lands on the previous month, not two days into this one', () => {
  // 2024-03-31 minus one calendar month is 2024-02-29 (the month's last day), as every
  // calendar library clamps it. Date overflow gives 2024-03-02 and a "1M" range that is
  // short by two sessions.
  const last = Date.UTC(2024, 2, 31, 0, 0);
  const got = new Date(rangeStart({ months: 1 }, last));
  assert.equal(got.getUTCMonth(), 1, `month back from Mar 31 is ${got.toISOString()}`);
});

test('isoSecond drops the milliseconds so stamps compare as text', () => {
  assert.equal(isoSecond(Date.UTC(2024, 0, 2, 3, 4, 5, 678)), '2024-01-02T03:04:05Z');
});

// ── live bars ───────────────────────────────────────────────────────────────

const mk = (stamps) => ({
  ts: stamps,
  o: stamps.map((_, i) => 10 + i),
  h: stamps.map((_, i) => 20 + i),
  l: stamps.map((_, i) => 5 + i),
  c: stamps.map((_, i) => 15 + i),
  v: stamps.map(() => 1)
});
const tick = (ts, v = 9) => ({ ts, o: 1, h: 2, l: 0.5, c: 1.5, v });

test('a live candle with a known stamp replaces that bar and nothing else', () => {
  const b = mk(['2024-01-01T00:00:00Z', '2024-01-01T00:01:00Z', '2024-01-01T00:02:00Z']);
  const out = mergeLiveBar(b, tick('2024-01-01T00:01:00Z'));
  assert.equal(out.ts.length, 3);
  assert.deepEqual([out.o[1], out.h[1], out.l[1], out.c[1], out.v[1]], [1, 2, 0.5, 1.5, 9]);
  assert.deepEqual([out.o[0], out.c[2]], [10, 17], 'neighbours untouched');
});

test('a new live candle is appended, a late one is inserted in order', () => {
  const b = mk(['2024-01-01T00:00:00Z', '2024-01-01T00:02:00Z']);
  mergeLiveBar(b, tick('2024-01-01T00:03:00Z'));
  mergeLiveBar(b, tick('2024-01-01T00:01:00Z'));
  assert.deepEqual(b.ts, [
    '2024-01-01T00:00:00Z',
    '2024-01-01T00:01:00Z',
    '2024-01-01T00:02:00Z',
    '2024-01-01T00:03:00Z'
  ]);
  // The columns stay parallel after the splice.
  for (const k of ['o', 'h', 'l', 'c', 'v']) assert.equal(b[k].length, 4, k);
  assert.equal(b.c[1], 1.5, 'the late candle carries its own values at its own index');
});

test('the first live candle of an empty chart starts the series', () => {
  const out = mergeLiveBar({ ts: [] }, tick('2024-01-01T00:00:00Z'));
  assert.deepEqual(out.ts, ['2024-01-01T00:00:00Z']);
  assert.deepEqual(out.c, [1.5]);
});

test('a read-back fills strictly inside the hole: both ends of it are excluded', () => {
  const chart = mk(['2024-01-01T00:00:00Z', '2024-01-01T00:05:00Z']);
  const got = mk([
    '2024-01-01T00:00:00Z', // the hole's left end: not added (already there anyway)
    '2024-01-01T00:02:00Z',
    '2024-01-01T00:05:00Z' // the hole's right end
  ]);
  const out = fillBars(chart, got, Date.parse('2024-01-01T00:00:00Z'), Date.parse('2024-01-01T00:05:00Z'));
  assert.deepEqual(out.ts, ['2024-01-01T00:00:00Z', '2024-01-01T00:02:00Z', '2024-01-01T00:05:00Z']);
  assert.equal(out.count, 3);
});

// ── price precision ─────────────────────────────────────────────────────────

test('decimalsOf ignores binary float tails', () => {
  assert.equal(decimalsOf(0.1 + 0.2), 1, '0.30000000000000004 is written 0.3');
  assert.equal(decimalsOf(42283.58), 2);
  assert.equal(decimalsOf(100), 0);
  assert.equal(decimalsOf(1e-7), 7);
  assert.equal(decimalsOf(1.5e-7), 8);
  assert.equal(decimalsOf(0), 0);
  assert.equal(decimalsOf(NaN), 0);
});

test('a price sent as a 32-bit float is read at the decimals it stands for', () => {
  assert.equal(decimalsOf(211.8699951171875), 2, 'Yahoo writes 211.87 this way');
  assert.equal(decimalsOf(211.869995117188), 2, 'and the database gives it back cut to 15 digits');
  assert.equal(decimalsOf(Math.fround(0.6734)), 4);
  assert.equal(decimalsOf(81649.25), 2, 'a price that happens to be a 32-bit float keeps its decimals');
  assert.equal(decimalsOf(100000.125), 3);
  assert.equal(decimalsOf(81649.26), 2, 'any other price is unchanged');
  const view = { ts: ['a', 'b'], o: [211.8699951171875, 2], h: [Math.fround(212.38), 2], l: [1, 2], c: [1, 2] };
  assert.equal(priceDigitsOf(view), 2);
});

test('the chart prints the deepest precision any bar uses, capped at 10', () => {
  const view = { ts: ['a', 'b'], o: [1, 2], h: [1.25, 2], l: [1, 2.5], c: [1, 2.125] };
  assert.equal(priceDigitsOf(view), 3, '2.125 has three decimals');
  assert.equal(priceDigitsOf({ ts: [] }), 2, 'no bars: cents');
  assert.equal(priceDigitsOfValues([1, 2.5, 0.12345678912]), 10, 'capped');
});

test('a future on a 0.25 grid reads its tick from the prices', () => {
  const n = 30;
  const px = (i) => 5000 + 0.25 * i;
  const view = {
    ts: Array.from({ length: n }, (_, i) => `t${i}`),
    o: Array.from({ length: n }, (_, i) => px(i)),
    h: Array.from({ length: n }, (_, i) => px(i) + 0.5),
    l: Array.from({ length: n }, (_, i) => px(i) - 0.25),
    c: Array.from({ length: n }, (_, i) => px(i) + 0.25)
  };
  assert.equal(priceTickOf(view), 0.25);
  // One price off the grid (adjusted data) brings the tick down to the precision, never to a wrong grid.
  const off = { ...view, c: view.c.map((v, i) => (i === 3 ? v + 0.01 : v)) };
  assert.equal(priceTickOf(off), 0.01);
  // Too few bars could all sit on a coarser grid by chance.
  assert.equal(priceTickOf({ ts: ['a', 'b'], o: [1, 2], h: [1, 2], l: [1, 2], c: [1, 2] }), null);
});

test('very small prices are written with a zero-count subscript', () => {
  assert.equal(compactPrice(0.000004549), '0.0₅4549');
  assert.equal(compactPrice(-0.000004549), '-0.0₅4549');
  assert.equal(compactPrice(1.3e-7), '0.0₆13', 'no invented trailing zeros');
  assert.equal(compactPrice(0.001), null, 'ordinary prices are written in full');
  assert.equal(compactPrice(0.0005), null, 'three zeros is still written in full');
  assert.equal(compactPrice(0), null);
  // 9.9999e-6 rounds up to 1.000e-5 before the zeros are counted: 4 zeros, not 5.
  assert.equal(compactPrice(9.9999e-6), '0.0₄1');
});

// ── quick backtest replay (hand-computed) ───────────────────────────────────

const f = (id, side, x, y, qty) => ({ id, side, x, y, qty, lot: 1 });

test('scale in, partial exit, final exit: weighted average cost and realized PnL', () => {
  // long 2@100, long 2@110 -> avg (200+220)/4 = 105. Sell 1@120 realizes (120-105)*1 = 15.
  // Sell 3@130 realizes (130-105)*3 = 75. Trade PnL 90, exit average (120 + 3*130)/4 = 127.5.
  const { marks, trades, open } = replay([
    f('a', 'long', 1, 100, 2),
    f('b', 'long', 2, 110, 2),
    f('c', 'short', 3, 120, 1),
    f('d', 'short', 4, 130, 3)
  ]);
  assert.equal(open, null);
  assert.equal(trades.length, 1);
  const t = trades[0];
  assert.equal(t.dir, 1);
  assert.equal(t.qty, 4);
  assert.equal(t.entryAvg, 105);
  assert.equal(t.exitAvg, 127.5);
  assert.equal(t.pnl, 90);
  assert.ok(Math.abs(t.pnlPct - (90 / (105 * 4)) * 100) < 1e-9, `pnlPct ${t.pnlPct}`);
  assert.deepEqual(marks.map((m) => m.role), ['entry', 'add', 'reduce', 'exit']);
  assert.deepEqual(marks.map((m) => m.realized), [0, 0, 15, 75]);
});

test('a fill larger than the position reverses it: one trade closes, the other opens', () => {
  // long 3@100, short 5@110 closes the 3 (+30) and opens short 2@110; long 2@105 closes it
  // for (110-105)*2 = +10.
  const { marks, trades, open } = replay([
    f('a', 'long', 1, 100, 3),
    f('b', 'short', 2, 110, 5),
    f('c', 'long', 3, 105, 2)
  ]);
  assert.equal(open, null);
  assert.deepEqual(trades.map((t) => [t.dir, t.qty, t.pnl]), [
    [1, 3, 30],
    [-1, 2, 10]
  ]);
  assert.deepEqual(marks.map((m) => m.role), ['entry', 'reverse', 'exit']);
  assert.equal(marks[1].realized, 30);
  // The reversing fill belongs to both trades (deleting either must remove it).
  assert.ok(trades[0].fillIds.includes('b') && trades[1].fillIds.includes('b'));
});

test('a short that moves against you loses (105 - 100) x 1', () => {
  const { trades } = replay([f('a', 'short', 1, 100, 1), f('b', 'long', 2, 105, 1)]);
  assert.equal(trades[0].pnl, -5);
  assert.equal(trades[0].dir, -1);
  assert.ok(trades[0].pnlPct < 0);
});

test('fills replay in time order, ties in insertion order', () => {
  const ordered = [f('a', 'long', 1, 100, 1), f('b', 'short', 2, 110, 1)];
  const shuffled = [ordered[1], ordered[0]];
  assert.equal(replay(shuffled).trades[0].pnl, 10);
  // Two clicks on the same bar: the entry first, then the exit.
  const same = [f('a', 'long', 5, 100, 1), f('b', 'short', 5, 103, 1)];
  assert.equal(replay(same).trades[0].pnl, 3);
});

test('an open position stays open with its average and unrealized PnL', () => {
  const { trades, open } = replay([f('a', 'long', 1, 100, 2), f('b', 'long', 2, 110, 2)]);
  assert.equal(trades.length, 0);
  assert.equal(open.qty, 4);
  assert.equal(open.avg, 105);
  assert.equal(unrealized(open, 110), 20, '(110-105) x 4');
  const short = replay([f('a', 'short', 1, 100, 2)]).open;
  assert.equal(unrealized(short, 90), 20, '(90-100) x 2 x -1');
  assert.equal(unrealized(null, 90), 0);
});

test('MAE, run-up and R come from the bars held, not the bars around them', () => {
  // long 2@100 at t=1000, closed 2@120 at t=3000. Bars inside: lows 99, 95, 101; highs 101, 110, 125.
  // Heat = 100 - 95 = 5 per unit, run-up = 125 - 100 = 25 per unit. Two units: risk 10, run-up 50.
  // PnL (120-100) x 2 = 40, R = 40 / 10 = 4. The bar at t=500 and t=4000 lie outside the trade.
  const bars = {
    ms: [500, 1000, 2000, 3000, 4000],
    l: [50, 99, 95, 101, 10],
    h: [500, 101, 110, 125, 999]
  };
  const { trades } = replay([f('a', 'long', 1000, 100, 2), f('b', 'short', 3000, 120, 2)], bars);
  const t = trades[0];
  assert.equal(t.pnl, 40);
  assert.equal(t.risk, 10);
  assert.equal(t.mae, -10);
  assert.equal(t.runup, 50);
  assert.equal(t.r, 4);
});

test('no excursion data means no R, not an infinite one', () => {
  const { trades } = replay([f('a', 'long', 1, 100, 1), f('b', 'short', 2, 110, 1)]);
  assert.equal(trades[0].r, null);
});

test('a size is read in units, contracts or notional and turned into units at the fill price', () => {
  assert.equal(resolveQty('units', 0.5, 100), 0.5);
  assert.equal(resolveQty('contracts', 2, 100, 50), 100, '2 contracts x 50 points');
  assert.equal(resolveQty('notional', 5000, 250), 20, '$5000 at 250');
  assert.equal(resolveQty('units', 0, 100), 1, 'a blank size is the default quantity');
  assert.equal(resolveQty('notional', 5000, 0), 1, 'no price: the default quantity, not a division by zero');
  // lotFor is its inverse: qty / lot gives back what was typed.
  assert.equal(100 / lotFor('contracts', 100, 50), 2);
  assert.equal(20 / lotFor('notional', 250), 5000);
  assert.equal(lotFor('units', 100), 1);
  assert.equal(makeFill('long', 1, 100, 0).qty > 0, true, 'a zero quantity becomes the default, never a zero-size fill');
});

test('sizes abbreviate past a thousand and keep six decimals below', () => {
  assert.equal(formatSize(12500), '12.5k');
  assert.equal(formatSize(3_400_000), '3.4M');
  assert.equal(formatSize(999.1234567), '999.123457');
  assert.equal(formatSize(0.5), '0.5');
  assert.equal(formatSize(NaN), '');
});

// ── session statistics (hand-computed) ──────────────────────────────────────

const tr = (pnl, closeTs, dir = 1, extra = {}) => ({ pnl, closeTs, openTs: closeTs - 1000, dir, qty: 1, pnlPct: pnl, ...extra });

test('session statistics over five closed trades', () => {
  // pnl 10, -4, -6, 20, -2 in close order.
  const trades = [tr(10, 1), tr(-4, 2), tr(-6, 3), tr(20, 4), tr(-2, 5)];
  const s = stats(trades);
  assert.equal(s.trades, 5);
  assert.equal(s.pnl, 18);
  assert.equal(s.wins, 2);
  assert.equal(s.losses, 3);
  assert.equal(s.winRate, 40);
  assert.equal(s.grossWin, 30);
  assert.equal(s.grossLoss, 12);
  assert.equal(s.profitFactor, 2.5);
  assert.equal(s.expectancy, 3.6);
  assert.equal(s.best, 20);
  assert.equal(s.worst, -6);
  assert.equal(s.avgWin, 15);
  assert.equal(s.avgLoss, -4);
  // Equity 10, 6, 0, 20, 18 against a peak of 10, 10, 10, 20, 20: the worst dip is 10.
  assert.equal(s.maxDrawdown, 10);
  assert.deepEqual(equityCurve(trades).points.map((p) => p.equity), [10, 6, 0, 20, 18]);
  assert.deepEqual(streaks(trades), { maxWins: 1, maxLosses: 2, current: -1 });
});

test('no losing trade means no profit factor, and no trade means no averages', () => {
  assert.equal(stats([tr(5, 1)]).profitFactor, null);
  const none = stats([]);
  assert.equal(none.trades, 0);
  assert.equal(none.winRate, 0);
  assert.equal(none.expectancy, null);
  assert.equal(none.best, null);
});

test('a break-even trade ends a run instead of extending it', () => {
  assert.deepEqual(streaks([tr(1, 1), tr(1, 2), tr(0, 3), tr(1, 4)]), { maxWins: 2, maxLosses: 0, current: 1 });
});

test('long and short books are summed apart', () => {
  const s = sideStats([tr(10, 1, 1), tr(-4, 2, -1), tr(6, 3, -1)]);
  assert.equal(s.long.pnl, 10);
  assert.equal(s.short.pnl, 2);
  assert.equal(s.short.winRate, 50);
  assert.equal(s.all.pnl, 12);
});
