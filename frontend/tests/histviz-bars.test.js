import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fillBars } from '$lib/modules/histviz/bars.js';

const series = (stamps, base = 1) => ({
  ts: stamps,
  o: stamps.map((_, i) => base + i),
  h: stamps.map((_, i) => base + i),
  l: stamps.map((_, i) => base + i),
  c: stamps.map((_, i) => base + i),
  v: stamps.map(() => 1)
});
const ms = (s) => Date.parse(s);

test('a read-back fills the hole between the two bars, in order', () => {
  const chart = series(['2026-10-22T07:05:00Z', '2026-10-22T07:06:00Z', '2026-10-22T10:58:00Z']);
  const got = series(['2026-10-22T07:06:00Z', '2026-10-22T07:07:00Z', '2026-10-22T07:08:00Z'], 100);
  const out = fillBars(chart, got, ms('2026-10-22T07:06:00Z'), ms('2026-10-22T10:58:00Z'));
  assert.deepEqual(out.ts, [
    '2026-10-22T07:05:00Z',
    '2026-10-22T07:06:00Z',
    '2026-10-22T07:07:00Z',
    '2026-10-22T07:08:00Z',
    '2026-10-22T10:58:00Z'
  ]);
  assert.equal(out.count, 5);
  // The bar the chart already held is the live feed's and is not overwritten.
  assert.equal(out.c[1], 2);
  assert.equal(out.c[2], 101);
});

test('bars outside the asked stretch are not added', () => {
  const chart = series(['2026-10-22T07:06:00Z', '2026-10-22T10:58:00Z']);
  const got = series(['2026-10-22T06:00:00Z', '2026-10-22T11:00:00Z']);
  assert.equal(fillBars(chart, got, ms('2026-10-22T07:06:00Z'), ms('2026-10-22T10:58:00Z')), chart);
});

test('an empty answer leaves the series as it was', () => {
  const chart = series(['2026-10-22T20:59:00Z', '2026-10-22T22:00:00Z']);
  assert.equal(fillBars(chart, { ts: [] }, ms('2026-10-22T20:59:00Z'), ms('2026-10-22T22:00:00Z')), chart);
});
