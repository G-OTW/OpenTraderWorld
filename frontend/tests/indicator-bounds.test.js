import { test } from 'node:test';
import assert from 'node:assert/strict';
import { defBounds, paneScale } from '../src/lib/indicators/bounds.js';

test('a DAG keeps the range of its bounded built-ins through arithmetic', () => {
  const def = {
    nodes: [
      { op: 'indicator', indicator: 'rsi', period: 14 },
      { op: 'const', value: 50 },
      { op: 'sub', a: 0, b: 1 },
      { op: 'indicator', indicator: 'cci', period: 20 },
      { op: 'price', field: 'close' },
      { op: 'indicator', indicator: 'sma', src: 0, period: 9 },
      { op: 'clamp', a: 3, lo: -200, hi: 200 },
      { op: 'div', a: 4, b: 0 }
    ],
    outputs: [0, 2, 3, 4, 5, 6, 7]
  };
  assert.deepEqual(defBounds(def), [[0, 100], [-50, 50], null, null, [0, 100], [-200, 200], null]);
});

test('a pane with no bounded study or level keeps fitting the data', () => {
  assert.equal(paneScale([null, null], []), null);
});

test('a bounded pane frames the whole range with a margin, and never clips the data', () => {
  const s = paneScale([[0, 100], null], [70, 30]);
  assert.equal(s.min({ min: 35, max: 65 }), -5);
  assert.equal(s.max({ min: 35, max: 65 }), 105);
  // Data past the declared range is joined, not cut.
  assert.equal(s.min({ min: -20, max: 60 }), -26);
  // Levels alone (a CCI drawing ±100) frame too.
  const l = paneScale([], [100, -100]);
  assert.equal(l.max({ min: -50, max: 250 }), 267.5);
  assert.equal(l.min({ min: NaN, max: NaN }), -110);
});
