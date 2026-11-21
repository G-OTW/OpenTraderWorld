import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as echarts from 'echarts';
import { registerCrispCandles, snapLine } from '../src/lib/modules/histviz/crispCandles.js';

registerCrispCandles();

// Every candle laid out by a real ECharts instance, after the snapping pass.
function layouts(dpr, count) {
  const chart = echarts.init(null, null, { renderer: 'svg', ssr: true, width: 777, height: 333 });
  // The SVG painter has no pixel ratio of its own; give it the one a canvas would report.
  chart.getZr().painter.dpr = dpr;
  const rows = Array.from({ length: count }, (_, i) => {
    const o = 100 + Math.sin(i * 0.7) * 3.137;
    const c = o + Math.cos(i * 1.3) * 2.071;
    return [o, c, Math.min(o, c) - 1.013, Math.max(o, c) + 0.917];
  });
  chart.setOption({
    animation: false,
    grid: { left: 13.3, right: 7.7, top: 9.1, bottom: 11.9 },
    xAxis: { type: 'category', data: rows.map((_, i) => i) },
    yAxis: { scale: true },
    series: [{ type: 'candlestick', data: rows, large: true, largeThreshold: 400, progressive: 0, itemStyle: { borderWidth: 1 } }]
  });
  const data = chart.getModel().getSeriesByIndex(0).getData();
  const out = { large: data.getLayout('largePoints'), ends: [] };
  for (let i = 0; i < data.count(); i++) out.ends.push(data.getItemLayout(i)?.ends);
  chart.dispose();
  return out;
}

const onGrid = (v, dpr, half) => {
  const d = v * dpr - (half ? 0.5 : 0);
  return Math.abs(d - Math.round(d)) < 1e-6;
};

for (const dpr of [1, 1.5, 2]) {
  test(`candles land on the device pixel grid at dpr ${dpr}`, () => {
    const w = Math.max(1, Math.round(dpr));
    const odd = w % 2 === 1;
    const { ends } = layouts(dpr, 60);
    const widths = new Set();
    for (const e of ends) {
      // Stroke centres (body edges, wick x) on the crisp position for this stroke width.
      for (const k of [0, 1, 2, 3]) {
        assert.ok(onGrid(e[k][0], dpr, odd), `body x ${e[k][0]}`);
        assert.ok(onGrid(e[k][1], dpr, odd), `body y ${e[k][1]}`);
      }
      assert.ok(onGrid(e[4][0], dpr, odd));
      // Wick tips on whole device pixels.
      assert.ok(onGrid(e[4][1], dpr, false));
      assert.ok(onGrid(e[6][1], dpr, false));
      // Wick centred in the body, body width the same for every candle.
      assert.ok(Math.abs(e[4][0] - (e[0][0] + e[1][0]) / 2) < 1e-9);
      widths.add(Math.round((e[1][0] - e[0][0]) * dpr));
    }
    assert.equal(widths.size, 1);
  });
}

test('large mode snaps the high-low lines too', () => {
  const dpr = 2;
  const { large } = layouts(dpr, 500);
  assert.ok(large?.length >= 2000);
  for (let i = 0; i < large.length; i += 4) {
    assert.ok(onGrid(large[i + 1], dpr, false));
    assert.ok(onGrid(large[i + 2], dpr, false));
    assert.ok(onGrid(large[i + 3], dpr, false));
  }
});

test('a stroke centre sits on a half pixel for odd widths, a whole one for even', () => {
  assert.equal(snapLine(10.2, 1, 1), 10.5);
  assert.equal(snapLine(10.2, 2, 2), 10);
  assert.equal(snapLine(10.3, 2, 1), 10.25);
});
