import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  treemap,
  normalizeSeries,
  rangePosition,
  distancePct,
  virtualPnl,
  groupBySection,
  moveRow,
  fmtBig,
  heatAlpha,
  shortPrice,
  divergeColor,
  luminance,
  inkOn,
  FLAGS
} from '../src/lib/modules/watchlists/views.js';

const area = (r) => r.w * r.h;
const close = (a, b, eps = 1e-6) => Math.abs(a - b) < eps;

test('treemap fills the box and sizes tiles by weight', () => {
  const tiles = [
    { id: 'a', weight: 6 },
    { id: 'b', weight: 3 },
    { id: 'c', weight: 1 }
  ];
  const out = treemap(tiles, 100, 50);
  assert.equal(out.length, 3);
  const total = out.reduce((s, r) => s + area(r), 0);
  assert.ok(close(total, 5000));
  const by = Object.fromEntries(out.map((r) => [r.id, area(r)]));
  assert.ok(close(by.a, 3000));
  assert.ok(close(by.b, 1500));
  assert.ok(close(by.c, 500));
  for (const r of out) {
    assert.ok(r.x >= -1e-9 && r.y >= -1e-9 && r.x + r.w <= 100 + 1e-6 && r.y + r.h <= 50 + 1e-6);
  }
});

test('treemap gives unknown sizes the median weight, and equal tiles when none is known', () => {
  const out = treemap(
    [
      { id: 'a', weight: 10 },
      { id: 'b', weight: null },
      { id: 'c', weight: 0 }
    ],
    90,
    10
  );
  for (const r of out) assert.ok(close(area(r), 300));
  const eq = treemap([{ id: 'x' }, { id: 'y' }], 20, 10);
  assert.ok(close(area(eq[0]), area(eq[1])));
  assert.deepEqual(treemap([], 10, 10), []);
});

test('tiles do not overlap', () => {
  const out = treemap(
    [5, 4, 3, 2, 2, 1, 1, 0.5].map((w, i) => ({ id: String(i), weight: w })),
    300,
    200
  );
  for (let i = 0; i < out.length; i++)
    for (let j = i + 1; j < out.length; j++) {
      const a = out[i];
      const b = out[j];
      const ox = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
      const oy = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
      assert.ok(ox <= 1e-6 || oy <= 1e-6, `${a.id} overlaps ${b.id}`);
    }
});

test('normalizeSeries rebases to 100 at the first point', () => {
  assert.deepEqual(normalizeSeries([[1, 50], [2, 75], [3, 25]]), [[1, 100], [2, 150], [3, 50]]);
  assert.deepEqual(normalizeSeries([]), []);
  assert.deepEqual(normalizeSeries([[1, 0], [2, 5]]), []);
});

test('52-week position at the low, the high and the middle', () => {
  assert.equal(rangePosition(10, 10, 20), 0);
  assert.equal(rangePosition(20, 10, 20), 1);
  assert.equal(rangePosition(15, 10, 20), 0.5);
  // Outside the stored range is clamped; a missing or empty range draws nothing.
  assert.equal(rangePosition(25, 10, 20), 1);
  assert.equal(rangePosition(15, null, 20), null);
  assert.equal(rangePosition(15, 20, 20), null);
});

test('target distance is positive above the price and negative below', () => {
  assert.ok(close(distancePct(100, 110), 10));
  assert.ok(close(distancePct(100, 90), -10));
  assert.equal(distancePct(null, 90), null);
  assert.equal(distancePct(100, null), null);
});

test('virtual position P/L', () => {
  const r = virtualPnl(10, 50, 60);
  assert.ok(close(r.pnl, 100));
  assert.ok(close(r.pct, 20));
  const loss = virtualPnl(2, 100, 75);
  assert.ok(close(loss.pnl, -50));
  assert.ok(close(loss.pct, -25));
  assert.equal(virtualPnl(null, 50, 60), null);
});

test('sections group in display order with ungrouped rows first', () => {
  const items = [
    { id: '1', section: 'core' },
    { id: '2', section: null },
    { id: '3', section: 'exits' },
    { id: '4', section: 'core' }
  ];
  const g = groupBySection(items);
  assert.deepEqual(
    g.map((x) => [x.section, x.items.map((i) => i.id)]),
    [
      [null, ['2']],
      ['core', ['1', '4']],
      ['exits', ['3']]
    ]
  );
});

test('moving a row between sections returns the full new order', () => {
  const items = [
    { id: '1', section: 'core' },
    { id: '2', section: 'core' },
    { id: '3', section: 'exits' }
  ];
  assert.deepEqual(moveRow(items, '1', 'exits'), [
    { id: '2', section: 'core' },
    { id: '3', section: 'exits' },
    { id: '1', section: 'exits' }
  ]);
  assert.deepEqual(moveRow(items, '3', 'core', '1'), [
    { id: '3', section: 'core' },
    { id: '1', section: 'core' },
    { id: '2', section: 'core' }
  ]);
});

test('big numbers are abbreviated and missing stays null', () => {
  assert.equal(fmtBig(1.53e12), '1.53T');
  assert.equal(fmtBig(4.5e7), '45.00M');
  assert.equal(fmtBig(null), null);
});

test('heat tint grows with the move and stops at a wash', () => {
  assert.equal(heatAlpha(null), null);
  assert.equal(heatAlpha(0), 0);
  assert.ok(close(heatAlpha(1, 4, 0.1, 0.5), 0.2));
  assert.equal(heatAlpha(-40, 4, 0.1, 0.5), 0.5);
  assert.equal(heatAlpha(-2, 4), heatAlpha(2, 4));
});

test('short prices compact from 10,000 and keep the formatter below', () => {
  assert.equal(shortPrice(108400), '$108.4K');
  assert.equal(shortPrice(2.5e6), '$2.5M');
  assert.equal(shortPrice(236.8, (v) => `$${v.toFixed(2)}`), '$236.80');
  assert.equal(shortPrice(null), null);
});

test('diverging fill: neutral midpoint at 0, poles at the ends, capped by reach', () => {
  const stops = ['#ff0000', '#808080', '#00ff00'];
  assert.equal(divergeColor(0, stops), 'rgb(128, 128, 128)');
  assert.equal(divergeColor(1, stops, 1), 'rgb(0, 255, 0)');
  assert.equal(divergeColor(-1, stops, 1), 'rgb(255, 0, 0)');
  assert.equal(divergeColor(null, stops), null);
  // reach keeps the strongest value short of the saturated pole
  assert.notEqual(divergeColor(1, stops), 'rgb(0, 255, 0)');
});

test('ink on a fill picks the higher-contrast text colour', () => {
  assert.ok(luminance('#ffffff') > 0.99);
  assert.ok(luminance('rgb(0, 0, 0)') < 0.01);
  assert.equal(inkOn('rgb(250, 250, 250)', '#111111', '#eeeeee'), '#111111');
  assert.equal(inkOn('#101010', '#111111', '#eeeeee'), '#eeeeee');
});

test('flag colours are theme tokens, never literals', () => {
  for (const f of FLAGS) assert.match(f.color, /var\(--/);
});
