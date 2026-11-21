import test from 'node:test';
import assert from 'node:assert/strict';
import { chainSmiles, sameRates } from '../src/lib/modules/options/chain.js';

const row = (strike, call, iv_mid, expiry = '2026-12-18') => ({ strike, call, iv_mid, expiry });

test('a smile reads the out-of-the-money side, sorted by moneyness', () => {
  const rows = [row(110, true, 0.2), row(90, false, 0.3), row(90, true, 0.9), row(110, false, 0.9), row(100, true, 0.25), row(100, false, 0.9)];
  assert.deepEqual(chainSmiles(rows, 100, ['2026-12-18']), [
    { expiry: '2026-12-18', points: [[0.9, 0.3], [1, 0.25], [1.1, 0.2]] }
  ]);
});

test('an expiry without two usable points has no smile', () => {
  assert.deepEqual(chainSmiles([row(110, true, 0.2), row(90, false, null)], 100, ['2026-12-18']), []);
  assert.deepEqual(chainSmiles([row(110, true, 0.2)], null, ['2026-12-18']), []);
  assert.deepEqual(chainSmiles(undefined, 100, ['2026-12-18']), []);
});

test('changed cash dividends make the loaded chain stale', () => {
  const a = { rate: 0.04, dividend: 0, cash: '[]' };
  assert.equal(sameRates(a, { ...a }), true);
  assert.equal(sameRates(a, { ...a, cash: '[{"ex_date":"2026-12-01","amount":1}]' }), false);
});
