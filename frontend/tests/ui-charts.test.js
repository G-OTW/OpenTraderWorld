import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  identityColor,
  otherColor,
  toneColor,
  toneOf,
  foldOther,
  partsOf,
  fade
} from '$lib/ui/charts/palette.js';
import {
  niceTicks,
  indexTo100,
  toSeconds,
  alignSeries,
  splitEstimated,
  extent,
  barPath,
  changePct
} from '$lib/ui/charts/scale.js';

// A stand-in for chartColors(): slot names instead of hexes, so the order is legible.
const colors = {
  series: ['c1', 'c2', 'c3', 'c4', 'c5', 'c6', 'c7', 'c8'],
  green: 'green',
  red: 'red',
  muted: 'muted'
};

test('identity order is chart-1, chart-5, chart-6, chart-2, then the fold colour, never red', () => {
  assert.deepEqual([0, 1, 2, 3].map((i) => identityColor(i, colors)), ['c1', 'c5', 'c6', 'c2']);
  for (const i of [4, 5, 9, -1, 1.5, null]) assert.equal(identityColor(i, colors), 'c4');
  assert.equal(otherColor(colors), 'c4');
  // The extension reaches teal and olive only on request, and still never red.
  assert.deepEqual([4, 5, 6].map((i) => identityColor(i, colors, 6)), ['c7', 'c8', 'c4']);
  const used = new Set(Array.from({ length: 10 }, (_, i) => identityColor(i, colors, 6)));
  assert.ok(!used.has('c3'), 'red is never an identity slot');
});

test('tone follows the sign, zero and unknown read muted, invert flips it', () => {
  assert.equal(toneColor(3, colors), 'green');
  assert.equal(toneColor(-0.1, colors), 'red');
  for (const v of [0, null, undefined, NaN]) assert.equal(toneColor(v, colors), 'muted');
  assert.equal(toneColor(5, colors, true), 'red');
  assert.equal(toneOf(-2), 'down');
  assert.equal(toneOf(-2, true), 'up');
  assert.equal(toneOf(0), 'flat');
});

test('foldOther ranks, keeps the head and sums the tail without mutating the input', () => {
  const items = [
    { label: 'A', value: 5 },
    { label: 'B', value: 50 },
    { label: 'C', value: 20 },
    { label: 'D', value: 1 },
    { label: 'E', value: 'x' },
    { label: 'F', value: 30 }
  ];
  const snapshot = JSON.stringify(items);
  const out = foldOther(items, 3, 'label', 'value', 'Other');
  assert.deepEqual(out.map((o) => o.label), ['B', 'F', 'C', 'Other']);
  assert.equal(out[3].value, 6);
  assert.equal(out[3].other, true);
  assert.deepEqual(out[3].items.map((o) => o.label), ['A', 'D', 'E']);
  assert.equal(out[0], items[1], 'kept items are the caller objects');
  assert.equal(JSON.stringify(items), snapshot);
  // Nothing folds when the list fits; equal values keep their input order.
  assert.deepEqual(foldOther([{ label: 'x', value: 1 }, { label: 'y', value: 1 }], 4).map((o) => o.label), ['x', 'y']);
  // Custom keys.
  const custom = foldOther([{ n: 'a', v: 1 }, { n: 'b', v: 2 }, { n: 'c', v: 3 }], 1, 'n', 'v', 'Rest');
  assert.deepEqual(custom, [{ n: 'c', v: 3 }, { n: 'Rest', v: 3, other: true, items: [{ n: 'b', v: 2 }, { n: 'a', v: 1 }] }]);
});

test('partsOf drops non-positive parts, shares sum to one, entity colours survive', () => {
  const { total, parts } = partsOf(
    [
      { label: 'Cash', value: 60, color: 'mine' },
      { label: 'Debt', value: -10 },
      { label: 'Zero', value: 0 },
      { label: 'Stocks', value: 40, colorIndex: 2 }
    ],
    { colors }
  );
  assert.equal(total, 100);
  assert.deepEqual(parts.map((p) => [p.label, p.share, p.color]), [
    ['Cash', 0.6, 'mine'],
    ['Stocks', 0.4, 'c6']
  ]);
  const many = partsOf(
    Array.from({ length: 7 }, (_, i) => ({ label: `p${i}`, value: 10 - i })),
    { colors, otherLabel: 'Other' }
  ).parts;
  assert.equal(many.length, 5);
  assert.deepEqual(many.map((p) => p.color), ['c1', 'c5', 'c6', 'c2', 'c4']);
  assert.equal(many[4].key, '__other');
  assert.ok(Math.abs(many.reduce((s, p) => s + p.share, 0) - 1) < 1e-12);
});

test('fade turns any palette colour into rgba and leaves unknown input alone', () => {
  assert.equal(fade('#356fd6', 0.1), 'rgba(53, 111, 214, 0.1)');
  assert.equal(fade('#fff', 0.5), 'rgba(255, 255, 255, 0.5)');
  assert.equal(fade('#356fd6cc', 0.2), 'rgba(53, 111, 214, 0.2)');
  assert.equal(fade('rgb(1, 2, 3)', 2), 'rgba(1, 2, 3, 1)');
  assert.equal(fade('var(--accent)', 0.1), 'var(--accent)');
});

test('niceTicks gives 3 to 5 round ticks that cover the range', () => {
  assert.deepEqual(niceTicks(0, 87), [0, 25, 50, 75, 100]);
  assert.deepEqual(niceTicks(-12, 30), [-20, 0, 20, 40]);
  assert.deepEqual(niceTicks(-0.3, 0.1), [-0.3, -0.2, -0.1, 0, 0.1]);
  assert.deepEqual(niceTicks(30, -12), [-20, 0, 20, 40], 'reversed bounds');
  assert.deepEqual(niceTicks(100, 100), [90, 95, 100, 105, 110], 'a flat range is widened');
  assert.deepEqual(niceTicks(NaN, 3), []);
  for (const [lo, hi] of [[0, 1], [3, 7.5], [12345, 98765], [-5e6, 2e6], [0.0012, 0.0049], [1, 1000]]) {
    const t = niceTicks(lo, hi);
    assert.ok(t.length >= 3 && t.length <= 5, `${lo}..${hi} gave ${t.length} ticks`);
    assert.ok(t[0] <= lo && t[t.length - 1] >= hi, `${lo}..${hi} covered by ${t}`);
    const step = t[1] - t[0];
    for (let i = 1; i < t.length; i++) assert.ok(Math.abs(t[i] - t[i - 1] - step) < step * 1e-9);
  }
  assert.ok(!Object.is(niceTicks(-1, 1).find((v) => v === 0), -0), 'no negative zero tick');
});

test('indexTo100 rebases at the first positive value, in both shapes', () => {
  assert.deepEqual(indexTo100([50, 55, 45]), [100, 110, 90]);
  assert.deepEqual(indexTo100([null, 0, 20, 30]), [null, 0, 100, 150]);
  assert.deepEqual(indexTo100([[1, 200, true], [2, 300], [3, null]]), [[1, 100, true], [2, 150], [3, null]]);
  assert.deepEqual(indexTo100([-5, 10]), [null, null], 'a negative start cannot be indexed');
  assert.deepEqual(indexTo100([]), []);
});

test('toSeconds reads dates, seconds and milliseconds', () => {
  const local = new Date(2026, 0, 31).getTime() / 1000;
  assert.equal(toSeconds('2026-01-31'), local, 'a plain date is local midnight');
  assert.equal(toSeconds(1767225600), 1767225600);
  assert.equal(toSeconds(1767225600000), 1767225600);
  assert.equal(toSeconds(new Date(1767225600000)), 1767225600);
  assert.equal(toSeconds('2026-01-01T00:00:00Z'), 1767225600);
  for (const v of [null, '', 'nope', NaN]) assert.equal(toSeconds(v), null);
});

test('alignSeries joins on one axis: null is a gap, a missing point is undefined', () => {
  const { xs, ys, notes } = alignSeries([
    [[30, 3], [10, 1], [20, null]],
    [[20, 7, 'est'], [40, 9]]
  ]);
  assert.deepEqual(xs, [10, 20, 30, 40]);
  assert.deepEqual(ys[0], [1, null, 3, undefined]);
  assert.deepEqual(ys[1], [undefined, 7, undefined, 9]);
  assert.deepEqual(notes[1], [null, 'est', null, null]);
});

test('splitEstimated draws estimates dashed from the last measured point', () => {
  const r = splitEstimated([1, 2, 3, 4, 5], [0, 0, 0, 1, 1]);
  assert.deepEqual(r.solid, [1, 2, 3, null, null]);
  assert.deepEqual(r.dashed, [null, null, 3, 4, 5]);
  assert.equal(r.any, true);
  // The join skips alignment artifacts to find the next real point.
  const g = splitEstimated([1, 2, undefined, 4], [0, 0, 0, 1]);
  assert.deepEqual(g.dashed, [null, 2, undefined, 4]);
  assert.equal(splitEstimated([1, 2], []).any, false);
});

test('extent, changePct and barPath geometry', () => {
  assert.deepEqual(extent([[3, null, 9], [undefined, -2]], [0]), [-2, 9]);
  assert.equal(extent([[null]]), null);
  assert.equal(changePct(110, 100), 10);
  assert.equal(changePct(-50, -100), 50);
  assert.equal(changePct(5, 0), null);
  assert.equal(barPath(0, 0, 10, 20, 4, 'none'), 'M0,0h10v20h-10Z');
  assert.ok(barPath(0, 0, 10, 20, 4, 'top').startsWith('M0,20V4Q0,0 4,0'));
  assert.ok(barPath(0, 0, 10, 20, 4, 'bottom').includes('Q10,20 6,20'));
  assert.equal(barPath(0, 0, 0, 10), '', 'an empty bar draws nothing');
  // A short bar shrinks the radius instead of overshooting its own height.
  assert.ok(barPath(0, 0, 10, 2, 4, 'top').startsWith('M0,2V2Q0,0 2,0'));
});
