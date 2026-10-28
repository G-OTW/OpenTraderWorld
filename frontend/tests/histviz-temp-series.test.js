import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  TEMP_PROVIDER,
  batchWithTemp,
  isTemp,
  putTemp,
  tempWindow
} from '../src/lib/modules/histviz/tempSeries.js';

const coords = { provider: TEMP_PROVIDER, asset_type: 'index', ticker: 'MYIDX', timeframe: '1d' };
const days = ['2024-01-01', '2024-01-02', '2024-01-03', '2024-01-04', '2024-01-05'].map((d) => `${d}T00:00:00Z`);
putTemp(coords, {
  ts: days,
  o: [1, 2, 3, 4, 5],
  h: [1, 2, 3, 4, 5],
  l: [1, 2, 3, 4, 5],
  c: [1, 2, 3, 4, 5],
  v: [0, 0, 0, 0, 0]
});

test('a temporary series answers the newest bars, then walks back before `to`', () => {
  const first = tempWindow({ ...coords, bars: 2 });
  assert.deepEqual(first.c, [4, 5]);
  assert.equal(first.history_start, false);
  assert.equal(first.stream, false);

  const older = tempWindow({ ...coords, to: first.ts[0], bars: 2 });
  assert.deepEqual(older.c, [2, 3]);
  const oldest = tempWindow({ ...coords, to: older.ts[0], bars: 2 });
  assert.deepEqual(oldest.c, [1]);
  assert.equal(oldest.history_start, true);
});

test('a series this tab does not hold is an error naming the fix, never another series', () => {
  assert.throws(() => tempWindow({ ...coords, timeframe: '1h' }), /import the file again/);
  assert.throws(() => tempWindow({ ...coords, ticker: 'OTHER' }), /import the file again/);
});

test('a batch answers files from the tab and the rest from the server, in order', async () => {
  assert.ok(isTemp(coords));
  const asked = [];
  const remote = async (rest) => {
    asked.push(rest.map((c) => c.ticker));
    return rest.map((c) => ({ ticker: c.ticker }));
  };
  const out = await batchWithTemp(
    [
      { provider: 'binance', ticker: 'BTCUSDT' },
      { ...coords, bars: 1 },
      { ...coords, ticker: 'GONE', bars: 1 },
      { provider: 'yahoo', ticker: 'AAPL' }
    ],
    remote
  );
  assert.deepEqual(asked, [['BTCUSDT', 'AAPL']]);
  assert.equal(out[0].ticker, 'BTCUSDT');
  assert.deepEqual(out[1].c, [5]);
  assert.match(out[2].error, /import the file again/);
  assert.equal(out[3].ticker, 'AAPL');

  await batchWithTemp([{ ...coords, bars: 1 }], remote);
  assert.equal(asked.length, 1, 'no round trip when every pane is a file');
});
