import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  hexLayout,
  fitRadius,
  hexPoints,
  colorScale,
  scaleKind,
  inkOn,
  mix,
  cssColor,
  concreteColor,
  nextSlot,
  itemColor,
  rowMetrics,
  metricValue,
  normalized,
  sortRows,
  normalizeLayout,
  reorder,
  liveSet,
  LIVE_CAP
} from '../src/lib/modules/scanner/board.js';

const COLORS = {
  neg: '#c94b54',
  mid: '#efede7',
  pos: '#15866a',
  low: '#efede7',
  high: '#356fd6',
  lit: '#356fd6',
  off: '#e3e0d9',
  none: '#f3f4f6'
};

test('honeycomb cells never overlap and odd rows shift half a cell', () => {
  const r = 20;
  const { cells, cols, rows } = hexLayout(23, 300, r, 2);
  assert.equal(cells.length, 23);
  assert.equal(rows, Math.ceil(23 / cols));
  const w = Math.sqrt(3) * r;
  for (let i = 0; i < cells.length; i++) {
    for (let j = i + 1; j < cells.length; j++) {
      const d = Math.hypot(cells[i].x - cells[j].x, cells[i].y - cells[j].y);
      assert.ok(d >= w - 1e-9, `cells ${i} and ${j} overlap (${d})`);
    }
  }
  assert.ok(cells[cols].x > cells[0].x, 'second row is offset');
  assert.equal(cells[2 * cols].x, cells[0].x, 'third row is aligned with the first');
});

test('honeycomb fits its width and a narrow box still gets one column', () => {
  const l = hexLayout(10, 400, 24);
  assert.ok(l.width <= 400 + 1e-9, `${l.width}`);
  assert.equal(hexLayout(5, 10, 24).cols, 1);
  assert.deepEqual(hexLayout(0, 400, 24), { cols: hexLayout(0, 400, 24).cols, rows: 0, width: 0, height: 0, cells: [] });
});

test('fitRadius picks the largest hexagon that fits the height', () => {
  const r = fitRadius(200, 800, 400);
  assert.ok(hexLayout(200, 800, r).height <= 400);
  if (r < 56) assert.ok(hexLayout(200, 800, r + 1).height > 400);
  assert.equal(fitRadius(3, 800, 400), 56);
  assert.equal(fitRadius(5000, 100, 50), 14);
});

test('a hexagon has six points around its center', () => {
  const pts = hexPoints(50, 50, 10).split(' ');
  assert.equal(pts.length, 6);
  assert.equal(pts[0], '50.00,40.00');
});

test('the change scale diverges around zero and is symmetric', () => {
  const f = colorScale('change_pct', [-1, 0.5, 4], COLORS);
  assert.equal(scaleKind('change_pct'), 'diverging');
  assert.equal(f(0), COLORS.mid);
  assert.equal(f(4), COLORS.pos);
  assert.equal(f(-4), COLORS.neg);
  // Beyond the span: clamped to the pole.
  assert.equal(f(-40), COLORS.neg);
  assert.equal(f(null), COLORS.none);
  assert.deepEqual(f.domain, [-4, 4]);
  // Equal moves either way are equally far from the midpoint.
  assert.equal(f(2), mix(COLORS.mid, COLORS.pos, 0.5));
  assert.equal(f(-2), mix(COLORS.mid, COLORS.neg, 0.5));
});

test('the volume scale is sequential from zero and a signal is lit or not', () => {
  const f = colorScale('volume_ratio', [0.5, 2, 4], COLORS);
  assert.equal(scaleKind('volume_ratio'), 'sequential');
  assert.equal(f(0), COLORS.low);
  assert.equal(f(4), COLORS.high);
  const rec = colorScale('hit_recency', [1, 30], COLORS);
  assert.equal(rec(0), COLORS.high);
  assert.equal(rec(48), COLORS.low);
  const s = colorScale('signal:abc', [true, false], COLORS);
  assert.equal(scaleKind('signal:abc'), 'binary');
  assert.equal(s(true), COLORS.lit);
  assert.equal(s(false), COLORS.off);
  assert.equal(s(null), COLORS.none);
});

test('tile ink reads on its fill', () => {
  assert.equal(inkOn('#ffffff'), '#17191c');
  assert.equal(inkOn('#0d366b'), '#ffffff');
});

test('assigned colors: palette slots in fixed order, items inherit their hive', () => {
  assert.equal(cssColor('chart-3'), 'var(--chart-3)');
  assert.equal(cssColor('#123456'), '#123456');
  assert.equal(cssColor(''), '');
  assert.equal(concreteColor('chart-2', ['#a', '#b']), '#b');
  assert.equal(nextSlot([]), 'chart-1');
  assert.equal(nextSlot(['chart-1', 'chart-2']), 'chart-3');
  assert.equal(nextSlot(['chart-1', 'chart-2', 'chart-3', 'chart-4', 'chart-5', 'chart-6', 'chart-7', 'chart-8']), '');
  const hives = [{ id: 'h', color: 'chart-5', item_ids: ['i'] }];
  assert.equal(itemColor({ id: 'i', color: '' }, hives), 'chart-5');
  assert.equal(itemColor({ id: 'i', color: '#ff0000' }, hives), '#ff0000');
  assert.equal(itemColor({ id: 'x', color: '' }, hives), '');
});

const row = (over = {}) => ({
  id: 'a',
  ticker: 'AAA',
  last_close: 110,
  prev_close: 100,
  day_open: 105,
  last_volume: 300,
  avg_volume: 100,
  last_hit_at: null,
  signal_state: { s1: true },
  ...over
});

test('row metrics: change on the bar, from the open, volume ratio, live close wins', () => {
  const m = rowMetrics(row());
  assert.ok(Math.abs(m.change_pct - 10) < 1e-9);
  assert.ok(Math.abs(m.change_from_open - (110 / 105 - 1) * 100) < 1e-9);
  assert.equal(m.volume_ratio, 3);
  assert.equal(m.hit_recency, null);
  assert.ok(Math.abs(rowMetrics(row(), { close: 90 }).change_pct + 10) < 1e-9);
  assert.equal(rowMetrics(row({ prev_close: null })).change_pct, null);
  assert.equal(metricValue(row(), 'signal:s1'), true);
  assert.equal(metricValue(row(), 'signal:s2'), null);
  const now = Date.parse('2026-09-07T12:00:00Z');
  assert.equal(rowMetrics(row({ last_hit_at: '2026-09-07T10:00:00Z' }), null, now).hit_recency, 2);
});

test('lines are normalized to their first close', () => {
  assert.deepEqual(normalized([[1, 50], [2, 100]]), [[1000, 100], [2000, 200]]);
  assert.deepEqual(normalized([]), []);
});

test('rows sort by metric with missing values last', () => {
  const rows = [row({ id: 'a', ticker: 'A', prev_close: 100 }), row({ id: 'b', ticker: 'B', prev_close: 50 }), row({ id: 'c', ticker: 'C', prev_close: null })];
  assert.deepEqual(sortRows(rows, 'change_pct').map((r) => r.ticker), ['B', 'A', 'C']);
  assert.deepEqual(sortRows(rows, 'ticker').map((r) => r.ticker), ['A', 'B', 'C']);
});

test('board layout: defaults survive anything stored, reorder rewrites sort', () => {
  assert.deepEqual(normalizeLayout(null), { size: 'm', live: false, sort: 'ticker', hitsOnly: false, lines: 'close' });
  assert.deepEqual(normalizeLayout({ size: 'xl', live: 'yes', sort: 'change_pct', hitsOnly: true }), {
    size: 'm',
    live: false,
    sort: 'change_pct',
    hitsOnly: true,
    lines: 'close'
  });
  const views = [{ id: 'a' }, { id: 'b' }, { id: 'c' }];
  const moved = reorder(views, 0, 2);
  assert.deepEqual(moved.map((v) => v.id), ['b', 'c', 'a']);
  assert.deepEqual(moved.map((v) => v.sort), [0, 1, 2]);
  // A persisted board read back in its stored order is the same board.
  const back = [...moved].sort((x, y) => x.sort - y.sort);
  assert.deepEqual(back.map((v) => v.id), ['b', 'c', 'a']);
});

test('a live board subscribes each visible item once and stays on bar close over the cap', () => {
  const items = (n, p) => Array.from({ length: n }, (_, i) => ({ id: `${p}${i}` }));
  const boards = { h1: { items: items(60, 'x') }, h2: { items: [...items(10, 'x'), ...items(20, 'y')] } };
  const views = [
    { hive_id: 'h1', layout: { live: true } },
    { hive_id: 'h2', layout: { live: true } },
    { hive_id: 'h2', layout: { live: false } }
  ];
  const s = liveSet(views, boards);
  assert.equal(s.total, 80);
  assert.equal(s.items.length, 80);
  assert.equal(s.over, false);
  const big = { h1: { items: items(LIVE_CAP + 1, 'z') } };
  const o = liveSet([{ hive_id: 'h1', layout: { live: true } }], big);
  assert.equal(o.over, true);
  assert.equal(o.items.length, 0);
});
