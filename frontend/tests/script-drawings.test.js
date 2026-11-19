import { test } from 'node:test';
import assert from 'node:assert/strict';
import { drawingSeries, shiftDrawings } from '../src/lib/script/drawings.js';
import { sliceOutput } from '../src/lib/script/host.js';

const RED = { r: 255, g: 0, b: 0, a: 255 };
const cs = { x: 0, y: 0, width: 1000, height: 500 };
/** A fake ECharts api: bar i at x = 10 * i, price p at y = 500 - p. */
const api = { coord: ([x, y]) => [10 * x, 500 - y] };
const opts = {
  x: 0,
  y: 0,
  n: 10,
  high: Array(10).fill(20),
  low: Array(10).fill(10),
  id: 'a',
  xOfMs: (ms) => ms / 1000
};
const render = (s, i = 0) => s.renderItem({ dataIndex: i, coordSys: cs }, api);
const empty = { labels: [], lines: [], boxes: [], tables: [], linefills: [], polylines: [] };

const line = (extra = {}) => ({
  x1: 1,
  y1: 100,
  x2: 3,
  y2: 200,
  xloc: 'bar_index',
  extend: 'none',
  color: RED,
  style: 'solid',
  width: 2,
  ...extra
});
const label = (extra = {}) => ({
  x: 2,
  y: 50,
  xloc: 'bar_index',
  yloc: 'price',
  text: 'hi',
  color: RED,
  style: 'label_down',
  textcolor: null,
  size: 'normal',
  textalign: 'align_center',
  tooltip: '',
  font: 'family_default',
  ...extra
});

test('a line lands on its bars and extends to the pane edge', () => {
  const [s] = drawingSeries({ ...empty, lines: [line()] }, opts);
  assert.equal(s.name, 'script-a-dl');
  const seg = render(s).children[0].shape;
  assert.deepEqual([seg.x1, seg.y1, seg.x2, seg.y2], [10, 400, 30, 300]);
  const [r] = drawingSeries({ ...empty, lines: [line({ extend: 'right' })] }, opts);
  assert.ok(render(r).children[0].shape.x2 > cs.width, 'past the right edge, clipped by the pane');
});

test('labels by price, above the bar, and by time', () => {
  const [s] = drawingSeries(
    { ...empty, labels: [label(), label({ yloc: 'abovebar', x: 4 }), label({ xloc: 'bar_time', x: 5000 })] },
    opts
  );
  const bubble = (g) => g.children[1];
  assert.deepEqual([bubble(render(s, 0)).x, bubble(render(s, 0)).y], [20, 450 - 6]);
  assert.equal(bubble(render(s, 1)).y, 500 - 20 - 6, 'abovebar reads the high');
  assert.equal(bubble(render(s, 2)).x, 50, 'bar_time through xOfMs');
  assert.equal(bubble(render(s, 0)).style.text, 'hi');
});

test('a table sits in its corner whatever the bars', () => {
  const cell = {
    column: 0,
    row: 0,
    text: 'AB',
    width: 0,
    height: 0,
    text_color: RED,
    text_halign: 'align_center',
    text_valign: 'align_center',
    text_size: 'normal',
    bgcolor: null,
    tooltip: '',
    font: 'family_default'
  };
  const t = {
    position: 'top_right',
    columns: 1,
    rows: 1,
    bgcolor: RED,
    frame_color: null,
    frame_width: 0,
    border_color: null,
    border_width: 0,
    cells: [cell],
    merges: []
  };
  const [s] = drawingSeries({ ...empty, tables: [t] }, opts);
  const frame = render(s).children[0].shape;
  assert.ok(frame.x + frame.width <= cs.width && frame.x > cs.width / 2, 'right side');
  assert.equal(frame.y, 6);
});

test('the warmup cut moves bar-index drawings, not time ones', () => {
  const d = {
    ...empty,
    labels: [label({ x: 105 }), label({ xloc: 'bar_time', x: 5000 })],
    lines: [line({ x1: 101, x2: 103 })],
    polylines: [
      {
        points: [
          [100, 1],
          [102, 2]
        ],
        xloc: 'bar_index'
      }
    ],
    boxes: []
  };
  const s = shiftDrawings(d, 100);
  assert.deepEqual(
    [s.labels[0].x, s.labels[1].x, s.lines[0].x1, s.lines[0].x2, s.polylines[0].points[1][0]],
    [5, 5000, 1, 3, 2]
  );
  const out = { plots: [], fills: [], bgcolor: [], barcolor: [], shapes: [], candles: [], hlines: [], drawings: d };
  assert.equal(sliceOutput(out, 100).drawings.labels[0].x, 5);
});
