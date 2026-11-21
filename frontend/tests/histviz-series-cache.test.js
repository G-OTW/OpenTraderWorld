import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  keepSeries,
  takeSeries,
  clearSeries,
  joinFresh,
  seriesKey,
  heldSeries,
  toRecord,
  fromRecord,
  RETENTIONS
} from '$lib/modules/histviz/seriesCache.js';

const day = (d) => `2026-11-${String(d).padStart(2, '0')}T00:00:00Z`;
const series = (days, base = 100, extra = {}) => ({
  ...extra,
  ts: days.map(day),
  o: days.map((d) => base + d),
  h: days.map((d) => base + d),
  l: days.map((d) => base + d),
  c: days.map((d) => base + d),
  v: days.map(() => 1),
  count: days.length
});

test('a kept series comes back, the least recently used goes first', () => {
  clearSeries();
  for (let i = 0; i < 21; i++) keepSeries(`k${i}`, series([1, 2]), false);
  assert.equal(heldSeries().series, 20);
  assert.equal(takeSeries('k0'), null);
  assert.ok(takeSeries('k1'));
  keepSeries('k21', series([1]), true);
  // k1 was used last, so k2 is the one pushed out.
  assert.ok(takeSeries('k1'));
  assert.equal(takeSeries('k2'), null);
  assert.equal(takeSeries('k21').atHistoryStart, true);
});

test('keeping a key again replaces it and an empty series is not kept', () => {
  clearSeries();
  keepSeries('a', series([1, 2, 3]), false);
  keepSeries('a', series([1, 2]), false);
  keepSeries('b', series([]), false);
  assert.deepEqual(heldSeries(), { series: 1, bars: 2 });
  clearSeries();
  assert.deepEqual(heldSeries(), { series: 0, bars: 0 });
});

test('a record round-trips to the same series answer, metadata included', () => {
  const s = series([1, 2, 3], 100, { provider: 'ibkr', continuous: { ticker: 'MNQ@CME' }, notice: null });
  s.ts[1] = '2026-11-02T13:45:30.250Z';
  const rec = toRecord('k', s, true);
  assert.ok(rec.t instanceof Float64Array && rec.c instanceof Float64Array);
  const { bars, atHistoryStart } = fromRecord(rec);
  assert.equal(atHistoryStart, true);
  assert.deepEqual(bars.ts, [day(1), '2026-11-02T13:45:30.250Z', day(3)]);
  assert.deepEqual(bars.c, s.c);
  assert.equal(bars.count, 3);
  assert.equal(bars.provider, 'ibkr');
  assert.deepEqual(bars.continuous, { ticker: 'MNQ@CME' });
});

test('the retention choices run from the tab to seven days', () => {
  assert.deepEqual(Object.keys(RETENTIONS), ['tab', '1h', '6h', '1d', '3d', '7d']);
  assert.equal(RETENTIONS.tab, null);
  assert.equal(RETENTIONS['7d'], 7 * 86_400_000);
});

test('the key names the connector, the instrument and the timeframe', () => {
  const c = { connector_id: 'x', provider: 'ibkr', asset_type: 'future', ticker: 'MNQ@CME', timeframe: '1d' };
  assert.equal(seriesKey(c), 'x|ibkr|future|MNQ@CME|1d');
  assert.notEqual(seriesKey(c), seriesKey({ ...c, timeframe: '1h' }));
  assert.equal(seriesKey(null), '');
});

test('a fresh read replaces the kept tail and keeps the older bars', () => {
  const kept = series([1, 2, 3, 4], 100);
  const fresh = series([3, 4, 5], 200, { notice: null, stream: true });
  const out = joinFresh(kept, fresh);
  assert.deepEqual(out.ts, [1, 2, 3, 4, 5].map(day));
  assert.deepEqual(out.c, [101, 102, 203, 204, 205]);
  assert.equal(out.count, 5);
  assert.equal(out.stream, true);
});

test('a fresh read that does not meet the kept bars is no join', () => {
  assert.equal(joinFresh(series([1, 2]), series([5, 6])), null);
  assert.equal(joinFresh(series([]), series([1])), null);
});

test('a fresh read with no bars keeps the kept ones under its notice', () => {
  const out = joinFresh(series([1, 2]), { ts: [], notice: { code: 'provider', message: 'down' } });
  assert.deepEqual(out.ts, [1, 2].map(day));
  assert.equal(out.notice.code, 'provider');
});
