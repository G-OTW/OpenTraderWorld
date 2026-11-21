import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cssColor, scriptSeries, scriptBarColors, scriptLegend } from '../src/lib/script/render.js';

const RED = { r: 255, g: 0, b: 0, a: 255 };
const BLUE = { r: 0, g: 0, b: 255, a: 255 };

const plot = (values, colors, extra = {}) => ({
  index: 0,
  values,
  colors: colors ?? values.map(() => RED),
  linewidth: 1,
  offset: 0,
  histbase: 0,
  hidden: false,
  ...extra
});
const meta = (styles) => ({ title: 't', overlay: false, inputs: [], plots: styles.map((style, index) => ({ index, title: `p${index}`, style, causal: true })) });
const out = (o) => ({ plots: [], hlines: [], fills: [], bgcolor: [], barcolor: [], shapes: [], candles: [], ...o });
const opts = (n, extra = {}) => ({ x: 0, y: 0, n, high: Array(n).fill(2), low: Array(n).fill(1), hidden: new Set(), id: 'a', priceAxis: true, ...extra });
/** y values of a series, whatever the item form ([x, y] or {value:[x, y]}). */
const ys = (s) => s.data.map((d) => (Array.isArray(d) ? d[1] : Array.isArray(d?.value) ? d.value[1] : d));
const xs = (s) => s.data.map((d) => (Array.isArray(d) ? d[0] : d?.value?.[0]));

test('cssColor maps Pine Rgba to CSS, null stays null', () => {
  assert.equal(cssColor({ r: 1, g: 2, b: 3, a: 128 }), 'rgba(1,2,3,0.502)');
  assert.equal(cssColor(null), null);
});

test('line bridges na: only valid points, one series for one color', () => {
  const s = scriptSeries(out({ plots: [plot([1, null, 3])] }), meta(['Line']), opts(3));
  assert.equal(s.length, 1);
  assert.deepEqual(s[0].data, [[0, 1], [2, 3]]);
  assert.ok(s[0].name.startsWith('script-a-'));
});

test('linebr breaks on na: dense data with the gap kept', () => {
  const s = scriptSeries(out({ plots: [plot([1, null, 3])] }), meta(['LineBr']), opts(3));
  assert.equal(s.length, 1);
  assert.deepEqual(ys(s[0]), [1, null, 3]);
});

// A multicolor line is one series whatever its colors: one item per segment, `[x, y, prevX,
// prevY, color]`, painted with the color of the bar it ends on.
const fakeApi = { coord: ([x, y]) => [x * 10, 100 - y], value: null };
const paint = (s, i) => s.renderItem({}, { ...fakeApi, value: (d) => s.data[i][d] });

test('per-bar colors keep a line in one series: a segment takes the color of the bar it ends on', () => {
  const s = scriptSeries(out({ plots: [plot([1, 2, 3], [RED, RED, BLUE])] }), meta(['Line']), opts(3));
  assert.equal(s.length, 1);
  assert.equal(s[0].type, 'custom');
  assert.deepEqual(s[0].encode, { x: 0, y: [1, 3] });
  assert.deepEqual(s[0].data.map((d) => d.slice(0, 4)), [[1, 2, 0, 1], [2, 3, 1, 2]]);
  const seg = paint(s[0], 1);
  assert.equal(seg.style.stroke, 'rgba(0,0,255,1)');
  assert.deepEqual(seg.shape.points, [[10, 98], [20, 97]]);
});

test('a multicolor line bridges na values and skips a bar whose color is na', () => {
  const s = scriptSeries(out({ plots: [plot([1, null, 3, 4, 5], [RED, RED, null, RED, BLUE])] }), meta(['Line']), opts(5));
  assert.deepEqual(s[0].data.map((d) => d.slice(0, 4)), [[3, 4, 2, 3], [4, 5, 3, 4]], 'bar 2 (na color) hides the 0 to 2 segment');
});

test('a colored stepline draws its own steps, one series', () => {
  const s = scriptSeries(out({ plots: [plot([1, 2, 5, 3], [RED, RED, BLUE, RED])] }), meta(['StepLine']), opts(4));
  assert.equal(s.length, 1);
  assert.equal(s[0].step, undefined);
  assert.deepEqual(paint(s[0], 1).shape.points, [[10, 98], [20, 98], [20, 95]], 'flat at the previous value, then up');
  assert.equal(paint(s[0], 1).style.stroke, 'rgba(0,0,255,1)');
});

test('a one-color line stays a plain line series', () => {
  const s = scriptSeries(out({ plots: [plot([1, 2, 5])] }), meta(['StepLine']), opts(3));
  assert.equal(s.length, 1);
  assert.equal(s[0].type, 'line');
  assert.deepEqual(s[0].data, [[0, 1], [1, 1], [1, 2], [2, 2], [2, 5]]);
});

test('offset shifts points and drops what leaves the chart', () => {
  const right = scriptSeries(out({ plots: [plot([1, 2, 3], null, { offset: 1 })] }), meta(['Line']), opts(3));
  assert.deepEqual(xs(right[0]), [1, 2]);
  const gone = scriptSeries(out({ plots: [plot([1, 2, 3], null, { offset: -5 })] }), meta(['Line']), opts(3));
  assert.deepEqual(gone, []);
});

test('histogram with histbase stacks a transparent base and the delta', () => {
  const s = scriptSeries(out({ plots: [plot([3, 1, 2], null, { histbase: 2 })] }), meta(['Histogram']), opts(3));
  assert.equal(s.length, 2);
  assert.ok(s.every((x) => x.type === 'bar' && x.stack === s[0].stack && x.stackStrategy === 'all'));
  assert.deepEqual(ys(s[0]), [2, 2, 2]);
  assert.deepEqual(ys(s[1]), [1, -1, 0]);
});

test('fill skips a quad with a na corner', () => {
  const o = out({
    plots: [plot([1, 2, 3, 4]), { ...plot([2, null, 4, 5]), index: 1 }],
    fills: [{ a: { Plot: 0 }, b: { Plot: 1 }, colors: [RED, RED, RED, RED] }]
  });
  const fill = scriptSeries(o, meta(['Line', 'Line']), opts(4)).find((x) => x.type === 'custom');
  // quads 0→1 and 1→2 touch the na at bar 1; only 2→3 remains.
  assert.equal(fill.data.length, 1);
});

test('barcolor: the later call wins per bar, null when none colors it', () => {
  const o = out({ barcolor: [{ colors: [RED, RED, null] }, { colors: [null, BLUE, null] }] });
  assert.deepEqual(scriptBarColors(o, 3), ['rgba(255,0,0,1)', 'rgba(0,0,255,1)', null]);
  assert.equal(scriptBarColors(out({}), 3), null);
});

test('a script with only an hline draws exactly one markLine series', () => {
  const o = out({ hlines: [{ price: 50, title: 'mid', color: RED, style: 'style_dashed', linewidth: 1 }] });
  const s = scriptSeries(o, meta([]), opts(3));
  assert.equal(s.length, 1);
  assert.equal(s[0].markLine.data[0].yAxis, 50);
  assert.equal(s[0].markLine.lineStyle.type, 'dashed');
});

test('hidden plots draw nothing and leave no legend entry; fills and hlines stay', () => {
  const o = out({
    plots: [plot([1, 2], null, { hidden: true }), { ...plot([1, 2]), index: 1 }],
    hlines: [{ price: 1, title: '', color: null, style: 'style_solid', linewidth: 1 }]
  });
  const m = meta(['Line', 'Line']);
  assert.deepEqual(scriptLegend(o, m, new Set([1])), []);
  const s = scriptSeries(o, m, opts(2, { hidden: new Set([1]) }));
  assert.equal(s.length, 1);
  assert.ok(s[0].markLine);
});

test('legend entries are offset and carry the last color', () => {
  const o = out({ plots: [plot([1, 2, 3], [RED, BLUE, null], { offset: 1 })] });
  const [e] = scriptLegend(o, meta(['Line']), new Set());
  assert.equal(e.name, 'p0');
  assert.deepEqual(e.data, [null, 1, 2]);
  assert.equal(e.color, 'rgba(0,0,255,1)');
});

test('zero bars draws nothing', () => {
  assert.deepEqual(scriptSeries(out({ plots: [plot([])] }), meta(['Line']), opts(0)), []);
});

test('shapes: abovebar sits on the high, a false condition draws nothing', () => {
  const o = out({ shapes: [{ kind: 'plotshape', when: [1, 0, null], style: 'triangledown', location: 'abovebar', color: [RED, RED, RED], text: '' }] });
  const s = scriptSeries(o, meta([]), opts(3));
  assert.equal(s.length, 1);
  assert.deepEqual(ys(s[0]), [2]);
});

test('plotarrow: up below the bar for positive, down above for negative, nothing for 0', () => {
  const o = out({ shapes: [{ kind: 'plotarrow', when: [1, -1, 0], style: '', location: '', color: [RED, BLUE, RED], text: '' }] });
  const s = scriptSeries(o, meta([]), opts(3));
  assert.deepEqual(ys(s[0]), [1, 2]);
});
