import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fmtFrac, fmtSplit } from '../src/lib/modules/backtest/format.js';

// The engine stores spread, slippage, stops and the in-sample split as fractions. They used to
// be printed raw with a "%" after them, so a 70% in-sample split read "0.7%".
test('fractions print as percents', () => {
  assert.equal(fmtFrac(0.7), '70%');
  assert.equal(fmtFrac(0.0005), '0.05%');
  assert.equal(fmtFrac(0.02), '2%');
  assert.equal(fmtFrac(null), '–');
});

test('the split names the in-sample share first, then the out-of-sample tail', () => {
  assert.equal(fmtSplit(0.7), '70% / 30%');
});
