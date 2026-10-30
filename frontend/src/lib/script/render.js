/** A script's `RunOutput` as ECharts series. Pure: the chart passes its axes and the bars'
 *  high/low (for shapes), gets series back. Index `i` of every output array is bar `i` of the
 *  bars the script ran on; a plot's `offset` moves its points, and points pushed outside the
 *  chart are dropped.
 *
 *  Pine rules kept here: `line`/`stepline`/`area` bridge `na` gaps, the `br` styles break on
 *  them; a line segment takes the color of the bar it ends on; a `na` color hides that bar. */

import { drawingSeries } from './drawings.js';

/** Pine `Rgba` (alpha 0..255) to a CSS color; null stays null. */
export function cssColor(c) {
  if (!c) return null;
  return `rgba(${c.r},${c.g},${c.b},${Math.round((c.a / 255) * 1000) / 1000})`;
}

/** Distinct colors drawn per plot before the rest fold onto the first one. */
const MAX_COLOR_SERIES = 32;

const LINE_TYPES = { style_solid: 'solid', style_dashed: 'dashed', style_dotted: 'dotted' };

const CROSS = 'path://M4 0h2v4h4v2H6v4H4V6H0V4h4z';
const XCROSS = 'path://M1 0l4 4 4-4 1 1-4 4 4 4-1 1-4-4-4 4-1-1 4-4-4-4z';
/** `shape.*` → ECharts symbol (+ rotation). */
const SHAPES = {
  circle: ['circle', 0],
  square: ['rect', 0],
  diamond: ['diamond', 0],
  triangleup: ['triangle', 0],
  triangledown: ['triangle', 180],
  arrowup: ['arrow', 0],
  arrowdown: ['arrow', 180],
  flag: ['pin', 0],
  labelup: ['roundRect', 0],
  labeldown: ['roundRect', 0],
  cross: [CROSS, 0],
  xcross: [XCROSS, 0]
};
/** Glyphs for shapes pinned to the pane's top or bottom edge. */
const GLYPHS = { circle: '●', square: '■', diamond: '◆', triangleup: '▲', triangledown: '▼', arrowup: '↑', arrowdown: '↓', flag: '⚑', labelup: '▲', labeldown: '▼', cross: '+', xcross: '✕' };

const ok = (v) => v != null && Number.isFinite(v);

/** Values moved by `offset` into a dense array of `n` (what lands outside is dropped). */
function shifted(values, offset, n) {
  const out = new Array(n).fill(null);
  values.forEach((v, i) => {
    const x = i + offset;
    if (x >= 0 && x < n && ok(v)) out[x] = v;
  });
  return out;
}

/** Same, for the per-bar colors. */
function shiftedColors(colors, offset, n) {
  const out = new Array(n).fill(null);
  colors.forEach((c, i) => {
    const x = i + offset;
    if (x >= 0 && x < n) out[x] = cssColor(c);
  });
  return out;
}

/** Color → key, folding past MAX_COLOR_SERIES onto the first one. */
function colorKeys() {
  const keys = [];
  return (c) => {
    if (c == null) return null;
    if (keys.includes(c)) return c;
    if (keys.length >= MAX_COLOR_SERIES) return keys[0];
    keys.push(c);
    return c;
  };
}

function base(name, o, extra) {
  return { name, xAxisIndex: o.x, yAxisIndex: o.y, clip: true, animation: false, silent: true, ...extra };
}

function line(name, data, color, width, o, extra = {}) {
  return base(name, o, {
    type: 'line',
    data,
    symbol: 'none',
    showSymbol: false,
    connectNulls: false,
    lineStyle: { color, width },
    ...extra
  });
}

/** Bridged styles: a series per color over the valid points only; runs of one color that
 *  are not adjacent are separated by a null so they do not join. `stepped` adds the corner of
 *  each step itself: ECharts' `step` draws one toward the null too, across the whole gap. */
function bridged(name, ys, cs, width, o, extra, stepped = false) {
  const pts = [];
  ys.forEach((v, x) => v != null && pts.push({ x, v, c: cs[x] }));
  const key = colorKeys();
  const runs = new Map();
  for (let k = 1; k < pts.length; k++) {
    const c = key(pts[k].c);
    if (c == null) continue;
    let d = runs.get(c);
    if (!d) runs.set(c, (d = []));
    const last = d[d.length - 1];
    if (last && last[0] !== pts[k - 1].x) d.push([pts[k - 1].x, null]);
    if (!last || last[0] !== pts[k - 1].x) d.push([pts[k - 1].x, pts[k - 1].v]);
    if (stepped) d.push([pts[k].x, pts[k - 1].v]);
    d.push([pts[k].x, pts[k].v]);
  }
  // One valid point: nothing to join, still visible as a line of one point.
  if (pts.length === 1 && pts[0].c != null) runs.set(pts[0].c, [[pts[0].x, pts[0].v]]);
  return [...runs].map(([c, data], k) => line(`${name}-${k}`, data, c, width, o, extra(c)));
}

/** A bridged line whose bars carry several colors, as **one** series: a series per color cost
 *  ECharts a full pass each on every render (a gradient made 23 of them). One custom item per
 *  segment, `[x, y, prevX, prevY, color]`, placed on the bar it ends on so the window filter
 *  keeps exactly the segments a plain line would draw; painted with that bar's color. */
function segmented(name, ys, cs, width, o, stepped) {
  const colors = [];
  const slot = new Map();
  const data = [];
  let prev = null;
  ys.forEach((v, x) => {
    if (v == null) return;
    const c = cs[x];
    if (prev && c != null) {
      let k = slot.get(c);
      if (k == null) slot.set(c, (k = colors.push(c) - 1));
      data.push([x, v, prev.x, prev.v, k]);
    }
    prev = { x, v };
  });
  return [
    base(name, o, {
      type: 'custom',
      data,
      encode: { x: 0, y: [1, 3] },
      progressive: 0,
      renderItem: (_params, api) => {
        const [x1, y1] = api.coord([api.value(0), api.value(1)]);
        const [x0, y0] = api.coord([api.value(2), api.value(3)]);
        const points = stepped ? [[x0, y0], [x1, y0], [x1, y1]] : [[x0, y0], [x1, y1]];
        return { type: 'polyline', shape: { points }, style: { stroke: colors[api.value(4)], lineWidth: width, fill: null } };
      }
    })
  ];
}

/** Breaking styles: dense arrays per color; a bar shows when its color or the next bar's
 *  (the segment it starts) is that color. */
function broken(name, ys, cs, width, o, extra) {
  const key = colorKeys();
  const colors = [...new Set(cs.map(key).filter(Boolean))];
  return colors.map((c, k) => {
    const data = ys.map((v, i) => (v != null && (key(cs[i]) === c || (ys[i + 1] != null && key(cs[i + 1]) === c)) ? v : null));
    return line(`${name}-${k}`, data, c, width, o, extra(c));
  });
}

function bars(name, ys, cs, p, o, style) {
  const width = style === 'Columns' ? '60%' : Math.max(1, p.linewidth);
  const items = (f) =>
    ys.flatMap((v, x) => (v != null && cs[x] != null ? [{ value: [x, f(v)], itemStyle: { color: cs[x] } }] : []));
  const bar = (suffix, data, extra = {}) => base(`${name}${suffix}`, o, { type: 'bar', data, barWidth: width, ...extra });
  if (!p.histbase) return [bar('', items((v) => v))];
  const stack = `${name}-stack`;
  return [
    bar('-base', items(() => p.histbase).map((d) => ({ ...d, itemStyle: { color: 'transparent' } })), { stack, stackStrategy: 'all' }),
    bar('', items((v) => v - p.histbase), { stack, stackStrategy: 'all' })
  ];
}

function dots(name, ys, cs, p, o, symbol) {
  const data = ys.flatMap((v, x) => (v != null && cs[x] != null ? [{ value: [x, v], itemStyle: { color: cs[x] } }] : []));
  return [base(name, o, { type: 'scatter', data, symbol, symbolSize: p.linewidth * 2 + 2 })];
}

/** More than one distinct color among the drawn bars. */
function manyColors(cs) {
  let first = null;
  for (const c of cs) {
    if (c == null) continue;
    if (first == null) first = c;
    else if (c !== first) return true;
  }
  return false;
}

function plotSeries(p, style, o) {
  const name = `script-${o.id}-p${p.index}`;
  const ys = shifted(p.values, p.offset, o.n);
  const cs = shiftedColors(p.colors, p.offset, o.n);
  const width = Math.max(1, p.linewidth);
  const area = (c) => ({ areaStyle: { color: c, opacity: 0.3, origin: p.histbase } });
  switch (style) {
    case 'Line':
    case 'StepLine': {
      const stepped = style === 'StepLine';
      return manyColors(cs) ? segmented(name, ys, cs, width, o, stepped) : bridged(name, ys, cs, width, o, () => ({}), stepped);
    }
    case 'Area':
      return bridged(name, ys, cs, width, o, area);
    case 'LineBr':
      return broken(name, ys, cs, width, o, () => ({}));
    case 'AreaBr':
      return broken(name, ys, cs, width, o, area);
    case 'Histogram':
    case 'Columns':
      return bars(name, ys, cs, p, o, style);
    case 'Circles':
      return dots(name, ys, cs, p, o, 'circle');
    case 'Cross':
      return dots(name, ys, cs, p, o, CROSS);
    default:
      return [];
  }
}

function hlineSeries(h, k, o) {
  if (!ok(h.price)) return [];
  return [
    base(`script-${o.id}-h${k}`, o, {
      type: 'line',
      data: [],
      markLine: {
        silent: true,
        symbol: 'none',
        label: { show: false },
        lineStyle: { color: cssColor(h.color) ?? 'rgba(120,123,134,1)', type: LINE_TYPES[h.style] ?? 'solid', width: Math.max(1, h.linewidth) },
        data: [{ yAxis: h.price }]
      }
    })
  ];
}

/** A fill side as a dense array: a plot (shifted) or an hline's constant price. */
function refValues(ref, out, n) {
  if (ref.Plot != null) {
    const p = out.plots.find((q) => q.index === ref.Plot);
    return p ? shifted(p.values, p.offset, n) : new Array(n).fill(null);
  }
  const h = out.hlines[ref.HLine];
  return new Array(n).fill(h && ok(h.price) ? h.price : null);
}

function fillSeries(f, k, out, o) {
  const a = refValues(f.a, out, o.n);
  const b = refValues(f.b, out, o.n);
  const colors = [];
  const data = [];
  for (let x = 1; x < o.n; x++) {
    const c = cssColor(f.colors[x]);
    if (c == null || [a[x], b[x], a[x - 1], b[x - 1]].some((v) => v == null)) continue;
    data.push([x, a[x], b[x], a[x - 1], b[x - 1]]);
    colors.push(c);
  }
  if (!data.length) return [];
  return [
    base(`script-${o.id}-f${k}`, o, {
      type: 'custom',
      data,
      z: 1,
      renderItem: (params, api) => {
        const x = api.value(0);
        const pts = [
          api.coord([x - 1, api.value(3)]),
          api.coord([x, api.value(1)]),
          api.coord([x, api.value(2)]),
          api.coord([x - 1, api.value(4)])
        ];
        return { type: 'polygon', shape: { points: pts }, style: { fill: colors[params.dataIndex] } };
      }
    })
  ];
}

function bgSeries(bg, k, o) {
  const colors = [];
  const data = [];
  bg.colors.forEach((c, x) => {
    const css = cssColor(c);
    if (css != null && x < o.n) {
      data.push([x]);
      colors.push(css);
    }
  });
  if (!data.length) return [];
  return [
    base(`script-${o.id}-bg${k}`, o, {
      type: 'custom',
      data,
      z: 0,
      renderItem: (params, api) => {
        const cs = params.coordSys;
        const w = api.size([1, 0])[0];
        const cx = api.coord([api.value(0), 0])[0];
        return { type: 'rect', shape: { x: cx - w / 2, y: cs.y, width: w, height: cs.height }, style: { fill: colors[params.dataIndex] } };
      }
    })
  ];
}

function shapeSeries(s, k, o) {
  const name = `script-${o.id}-s${k}`;
  const items = [];
  const edge = [];
  s.when.forEach((w, x) => {
    if (x >= o.n || w == null || w === 0) return;
    const color = cssColor(s.color[x]);
    if (color == null) return;
    if (s.kind === 'plotarrow') {
      const up = w > 0;
      items.push({ value: [x, up ? o.low[x] : o.high[x]], symbol: 'arrow', symbolRotate: up ? 0 : 180, symbolOffset: [0, up ? 10 : -10], itemStyle: { color } });
      return;
    }
    if (s.location === 'top' || s.location === 'bottom') {
      edge.push({ x, color });
      return;
    }
    const y = s.location === 'abovebar' ? o.high[x] : s.location === 'belowbar' ? o.low[x] : w;
    if (!ok(y)) return;
    const dy = s.location === 'abovebar' ? -10 : s.location === 'belowbar' ? 10 : 0;
    const label = s.kind === 'plotchar' || s.text ? { show: true, formatter: s.kind === 'plotchar' ? s.style : s.text, color, position: dy < 0 ? 'top' : 'bottom' } : undefined;
    const [symbol, rotate] = s.kind === 'plotchar' ? ['circle', 0] : (SHAPES[s.style] ?? SHAPES.xcross);
    items.push({ value: [x, y], symbol, symbolRotate: rotate, symbolSize: s.kind === 'plotchar' ? 0 : 8, symbolOffset: [0, dy], itemStyle: { color }, label });
  });
  const out = [];
  if (items.length) out.push(base(name, o, { type: 'scatter', data: items }));
  if (edge.length) {
    const glyph = s.kind === 'plotchar' ? s.style : (GLYPHS[s.style] ?? '✕');
    out.push(
      base(`${name}-edge`, o, {
        type: 'custom',
        data: edge.map((e) => [e.x]),
        renderItem: (params, api) => {
          const cs = params.coordSys;
          const cx = api.coord([api.value(0), 0])[0];
          const y = s.location === 'top' ? cs.y + 8 : cs.y + cs.height - 8;
          return { type: 'text', style: { text: glyph, x: cx, y, align: 'center', verticalAlign: 'middle', fill: edge[params.dataIndex].color, fontSize: 10 } };
        }
      })
    );
  }
  return out;
}

function candleSeries(c, k, o) {
  const name = `script-${o.id}-c${k}`;
  const rows = [];
  for (let x = 0; x < o.n; x++) {
    const v = [c.o[x], c.c[x], c.l[x], c.h[x]];
    const color = cssColor(c.colors[x]);
    rows.push(v.every(ok) && color ? { value: v, itemStyle: { color, color0: color, borderColor: color, borderColor0: color } } : '-');
  }
  if (c.kind === 'plotcandle') return [base(name, o, { type: 'candlestick', data: rows })];
  // plotbar: OHLC ticks (vertical high-low, open tick left, close tick right).
  const data = [];
  const colors = [];
  rows.forEach((r, x) => {
    if (r === '-') return;
    data.push([x, ...r.value]);
    colors.push(r.itemStyle.color);
  });
  return [
    base(name, o, {
      type: 'custom',
      data,
      renderItem: (params, api) => {
        const x = api.value(0);
        const w = api.size([1, 0])[0] * 0.3;
        const [cx, yo] = api.coord([x, api.value(1)]);
        const yc = api.coord([x, api.value(2)])[1];
        const yl = api.coord([x, api.value(3)])[1];
        const yh = api.coord([x, api.value(4)])[1];
        const stroke = { stroke: colors[params.dataIndex], lineWidth: 1 };
        return {
          type: 'group',
          children: [
            { type: 'line', shape: { x1: cx, y1: yl, x2: cx, y2: yh }, style: stroke },
            { type: 'line', shape: { x1: cx - w, y1: yo, x2: cx, y2: yo }, style: stroke },
            { type: 'line', shape: { x1: cx, y1: yc, x2: cx + w, y2: yc }, style: stroke }
          ]
        };
      }
    })
  ];
}

/** Every series the script draws. `opts = { x, y, n, high, low, hidden:Set<plot index>, id,
 *  xOfMs }` (`xOfMs`: bar index of an instant, for drawings placed by time). */
export function scriptSeries(output, meta, opts) {
  if (!output || !opts.n) return [];
  const styles = new Map((meta?.plots ?? []).map((p) => [p.index, p.style]));
  const series = [];
  output.bgcolor.forEach((bg, k) => series.push(...bgSeries(bg, k, opts)));
  output.fills.forEach((f, k) => series.push(...fillSeries(f, k, output, opts)));
  output.hlines.forEach((h, k) => series.push(...hlineSeries(h, k, opts)));
  for (const p of output.plots) {
    if (p.hidden || opts.hidden?.has(p.index)) continue;
    series.push(...plotSeries(p, styles.get(p.index) ?? 'Line', opts));
  }
  output.candles.forEach((c, k) => series.push(...candleSeries(c, k, opts)));
  output.shapes.forEach((s, k) => series.push(...shapeSeries(s, k, opts)));
  series.push(...drawingSeries(output.drawings, opts));
  return series;
}

/** Candle colors from `barcolor`: per bar, the last call that colors it wins. */
export function scriptBarColors(output, n) {
  if (!output?.barcolor?.length) return null;
  const out = new Array(n).fill(null);
  for (const b of output.barcolor) b.colors.forEach((c, i) => i < n && c && (out[i] = cssColor(c)));
  return out;
}

const KIND = { Histogram: 'bar', Columns: 'bar', Circles: 'scatter', Cross: 'scatter' };

/** Visible plots in the chart's generic output shape (legend, crosshair tags, sources for
 *  other studies): `{name, kind, data, color}`, data already offset. */
export function scriptLegend(output, meta, hidden) {
  if (!output) return [];
  const byIndex = new Map((meta?.plots ?? []).map((p) => [p.index, p]));
  return output.plots
    .filter((p) => !p.hidden && !hidden?.has(p.index))
    .map((p) => {
      const m = byIndex.get(p.index);
      const last = [...p.colors].reverse().find(Boolean) ?? null;
      return { name: m?.title ?? `plot ${p.index}`, kind: KIND[m?.style] ?? 'line', data: shifted(p.values, p.offset, p.values.length), color: cssColor(last) };
    });
}
