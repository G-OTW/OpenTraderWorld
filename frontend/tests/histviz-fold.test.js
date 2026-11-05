import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createFold } from '$lib/modules/histviz/fold.js';
import { replaceTail } from '$lib/modules/histviz/bars.js';

const bar = (ts, c, v = 10) => ({ ts, o: c, h: c + 1, l: c - 1, c, v });
const cols = (list) => ({
  ts: list.map((b) => b.ts),
  o: list.map((b) => b.o),
  h: list.map((b) => b.h),
  l: list.map((b) => b.l),
  c: list.map((b) => b.c),
  v: list.map((b) => b.v)
});

test('a 7m fold extends the history candle on the 00:00 UTC grid, repeats replace', () => {
  // 09:55 is 85 x 7 minutes after midnight: the server's candle, its base bars as the tail.
  const tail = cols([bar('2026-10-28T09:55:00Z', 1), bar('2026-10-28T09:56:00Z', 2), bar('2026-10-28T09:57:00Z', 3)]);
  const f = createFold('7m', '1m', tail, '2026-10-28T09:55:00.000Z');
  assert.equal(f.since(), '2026-10-28T09:57:00.000Z');
  // The forming 09:57 bar again (snapshot, catch-up): replaced, its volume not counted twice.
  let out = f.push(bar('2026-10-28T09:57:00Z', 3));
  assert.equal(out.from, null);
  assert.deepEqual(out.bars, [{ ts: '2026-10-28T09:55:00.000Z', o: 1, h: 4, l: 0, c: 3, v: 30 }]);
  out = f.push(bar('2026-10-28T10:01:00Z', 7));
  assert.equal(out.bars[0].ts, '2026-10-28T09:55:00.000Z');
  assert.equal(out.bars[0].c, 7);
  assert.equal(out.bars[0].v, 40);
  // 10:02 opens the next candle.
  out = f.push(bar('2026-10-28T10:02:00Z', 8));
  assert.deepEqual(out.bars, [{ ts: '2026-10-28T10:02:00.000Z', o: 8, h: 9, l: 7, c: 8, v: 10 }]);
  // Older than the history candle the fold started from: not its business.
  assert.equal(f.push(bar('2026-10-28T09:40:00Z', 99)), null);
});

test('a 24/7 market crosses midnight onto the new day grid without a server read', () => {
  const tail = cols([bar('2026-10-28T23:55:00Z', 1), bar('2026-10-28T23:59:00Z', 2)]);
  const f = createFold('7m', '1m', tail, '2026-10-28T23:55:00.000Z');
  const out = f.push(bar('2026-10-29T00:00:00Z', 3));
  assert.equal(out.from, Date.parse('2026-10-29T00:00:00Z'));
  assert.equal(out.resync, null);
  assert.deepEqual(out.bars.map((b) => b.ts), ['2026-10-29T00:00:00.000Z']);
  assert.equal(f.push(bar('2026-10-29T00:07:00Z', 4)).bars[0].ts, '2026-10-29T00:07:00.000Z');
});

test('a session after a night opens on its first bar and asks the server once', () => {
  const f = createFold('2h', '1h', cols([bar('2024-01-02T19:30:00Z', 1)]), '2024-01-02T17:30:00.000Z');
  // Same base bars as the Rust `intraday_consolidation_is_session_anchored` fixture, day two.
  let out = f.push(bar('2024-01-03T13:30:00Z', 4));
  assert.equal(out.resync, Date.parse('2024-01-03T00:00:00Z'));
  out = f.push(bar('2024-01-03T14:30:00Z', 5));
  assert.equal(out.resync, null, 'asked once per day');
  f.push(bar('2024-01-03T15:30:00Z', 6));
  const got = f.candles();
  assert.deepEqual(got.map((b) => b.ts), ['2024-01-03T13:30:00.000Z', '2024-01-03T15:30:00.000Z']);
  assert.deepEqual([got[0].o, got[0].c, got[0].h, got[0].l, got[0].v], [4, 5, 6, 3, 20]);
});

test('a bar older than the day opening it moves the grid back and redraws the day', () => {
  const f = createFold('7m', '1m', null);
  f.push(bar('2026-10-28T14:31:00Z', 2));
  // The server's backfill of a minute the feed missed, arriving after the newer one.
  const out = f.push(bar('2026-10-28T14:30:00Z', 1));
  assert.equal(out.from, Date.parse('2026-10-28T00:00:00Z'));
  assert.deepEqual(out.bars.map((b) => [b.ts, b.o, b.c, b.v]), [['2026-10-28T14:30:00.000Z', 1, 2, 20]]);
});

test('a server read re-seeds the day and keeps the live bars newer than it', () => {
  const f = createFold('7m', '1m', null);
  // Opened late on 14:35: the server says the day started at 13:30 (14:33 is on its grid).
  f.push(bar('2026-10-28T14:35:00Z', 5));
  assert.ok(f.seed(cols([bar('2026-10-28T14:33:00Z', 3), bar('2026-10-28T14:34:00Z', 4)]), '2026-10-28T14:33:00Z'));
  assert.deepEqual(f.candles().map((b) => [b.ts, b.o, b.c, b.v]), [['2026-10-28T14:33:00.000Z', 3, 5, 30]]);
  // An answer older than the day the fold holds changes nothing.
  assert.equal(f.seed(cols([bar('2026-10-27T20:53:00Z', 9)]), '2026-10-27T20:53:00Z'), false);
  assert.equal(f.push(bar('2026-10-28T14:40:00Z', 6)).bars[0].ts, '2026-10-28T14:40:00.000Z');
});

test('weeks fold on the ISO Monday, days on the epoch grid', () => {
  const w = createFold('2w', '1w', null);
  const a = w.push(bar('2026-10-26T00:00:00Z', 1)).bars[0].ts;
  const b = w.push(bar('2026-11-02T00:00:00Z', 2)).bars[0].ts;
  assert.equal(a, b, 'two weeks, one candle');
  assert.equal(new Date(a).getUTCDay(), 1);
  // A session-stamped daily bar (midnight New York) lands on the UTC 2-day grid.
  const d = createFold('2d', '1d', null);
  const out = d.push(bar('2026-10-28T04:00:00Z', 1));
  assert.equal(out.bars[0].ts, new Date(Math.floor(Date.parse('2026-10-28T04:00:00Z') / 172_800_000) * 172_800_000).toISOString());
  assert.equal(out.resync, null);
});

test('replaceTail keeps the bars before the instant and puts the new ones after', () => {
  const chart = cols([bar('2026-10-28T09:00:00Z', 1), bar('2026-10-28T10:00:00Z', 2), bar('2026-10-28T11:00:00Z', 3)]);
  const out = replaceTail(chart, Date.parse('2026-10-28T10:00:00Z'), [bar('2026-10-28T10:30:00Z', 9)]);
  assert.deepEqual(out.ts, ['2026-10-28T09:00:00Z', '2026-10-28T10:30:00Z']);
  assert.deepEqual(out.c, [1, 9]);
  assert.equal(out.count, 2);
});
