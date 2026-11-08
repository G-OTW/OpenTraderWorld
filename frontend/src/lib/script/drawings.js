/** A script's drawing objects (`RunOutput.drawings`: labels, lines, boxes, tables, linefills,
 *  polylines) as ECharts custom series on the script's axes. Pure like `render.js`: x is a
 *  bar index of the drawn bars (`xloc = bar_index`, already shifted past the warmup by
 *  `sliceOutput`) or an open time in ms (`bar_time`, placed with `o.xOfMs`). A drawing past
 *  the last bar lands in the right margin; the series clip to their pane. */

import { cssColor } from './render.js';

const FONT = { tiny: 9, small: 10, normal: 12, auto: 12, large: 15, huge: 20 };
const SYMBOL = { tiny: 6, small: 9, normal: 12, auto: 12, large: 17, huge: 24 };
const DASH = { dashed: [6, 4], dotted: [2, 3] };
const FAMILY = { family_monospace: 'monospace' };

const ok = (v) => v != null && Number.isFinite(v);

/** The drawing's x as a bar index of the drawn bars. */
function xOf(x, xloc, o) {
  if (xloc === 'bar_time') return o.xOfMs ? o.xOfMs(x) : null;
  return x;
}

/** The price a label stands at: its `y`, or the bar's high/low for `abovebar`/`belowbar`. */
function labelY(l, x, o) {
  if (l.yloc === 'abovebar') return o.high?.[Math.round(x)];
  if (l.yloc === 'belowbar') return o.low?.[Math.round(x)];
  return l.y;
}

function stroke(color, width, style) {
  return { stroke: cssColor(color) ?? 'transparent', lineWidth: width, lineDash: DASH[style] ?? null, fill: null };
}

/** `[x1, y1, x2, y2]` in pixels, extended to the pane's edges per `extend`. */
function extended(p1, p2, extend, cs) {
  const [dx, dy] = [p2[0] - p1[0], p2[1] - p1[1]];
  const far = ((cs.width + cs.height) * 4) / Math.max(Math.hypot(dx, dy), 1e-9);
  const a = extend === 'left' || extend === 'both' ? [p2[0] - dx * far, p2[1] - dy * far] : p1;
  const b = extend === 'right' || extend === 'both' ? [p1[0] + dx * far, p1[1] + dy * far] : p2;
  return [a, b];
}

function arrowHead(tip, from, color, width) {
  const ang = Math.atan2(tip[1] - from[1], tip[0] - from[0]);
  const len = 6 + width * 2;
  const pt = (d) => [tip[0] - len * Math.cos(ang + d), tip[1] - len * Math.sin(ang + d)];
  return { type: 'polygon', shape: { points: [tip, pt(0.45), pt(-0.45)] }, style: { fill: cssColor(color) } };
}

function lineShape(l, o, cs, api) {
  const [x1, x2] = [xOf(l.x1, l.xloc, o), xOf(l.x2, l.xloc, o)];
  if (![x1, x2, l.y1, l.y2].every(ok)) return null;
  const [a, b] = extended(api.coord([x1, l.y1]), api.coord([x2, l.y2]), l.extend, cs);
  const children = [
    { type: 'line', shape: { x1: a[0], y1: a[1], x2: b[0], y2: b[1] }, style: stroke(l.color, l.width, l.style) }
  ];
  if (l.style === 'arrow_right' || l.style === 'arrow_both') children.push(arrowHead(b, a, l.color, l.width));
  if (l.style === 'arrow_left' || l.style === 'arrow_both') children.push(arrowHead(a, b, l.color, l.width));
  return { type: 'group', children };
}

/** A label: its text in a bubble pointing at (x, y), or a symbol with the text beside it. */
function labelShape(l, o, api) {
  const x = xOf(l.x, l.xloc, o);
  const y = ok(x) ? labelY(l, x, o) : null;
  if (!ok(x) || !ok(y)) return null;
  const [px, py] = api.coord([x, y]);
  const fill = cssColor(l.color);
  const font = { fontSize: FONT[l.size] ?? 12, fontFamily: FAMILY[l.font] ?? 'sans-serif' };
  const textFill = cssColor(l.textcolor) ?? '#fff';
  const align = l.textalign === 'align_left' ? 'left' : l.textalign === 'align_right' ? 'right' : 'center';
  const bubble = (dx, dy, textAlign, verticalAlign) => ({
    type: 'text',
    x: px + dx,
    y: py + dy,
    style: {
      text: l.text,
      ...font,
      fill: textFill,
      align: textAlign,
      verticalAlign,
      backgroundColor: fill,
      padding: [3, 6],
      borderRadius: 3,
      textAlign: align
    }
  });
  const pointer = (pts) => ({ type: 'polygon', shape: { points: pts }, style: { fill } });
  switch (l.style) {
    case 'label_down':
      return {
        type: 'group',
        children: [
          pointer([
            [px - 5, py - 6],
            [px + 5, py - 6],
            [px, py]
          ]),
          bubble(0, -6, 'center', 'bottom')
        ]
      };
    case 'label_up':
      return {
        type: 'group',
        children: [
          pointer([
            [px - 5, py + 6],
            [px + 5, py + 6],
            [px, py]
          ]),
          bubble(0, 6, 'center', 'top')
        ]
      };
    case 'label_left':
      return {
        type: 'group',
        children: [
          pointer([
            [px + 6, py - 5],
            [px + 6, py + 5],
            [px, py]
          ]),
          bubble(6, 0, 'left', 'middle')
        ]
      };
    case 'label_right':
      return {
        type: 'group',
        children: [
          pointer([
            [px - 6, py - 5],
            [px - 6, py + 5],
            [px, py]
          ]),
          bubble(-6, 0, 'right', 'middle')
        ]
      };
    case 'label_lower_left':
      return bubble(0, 0, 'left', 'top');
    case 'label_lower_right':
      return bubble(0, 0, 'right', 'top');
    case 'label_upper_left':
      return bubble(0, 0, 'left', 'bottom');
    case 'label_upper_right':
      return bubble(0, 0, 'right', 'bottom');
    case 'label_center':
      return bubble(0, 0, 'center', 'middle');
    case 'none':
    case 'text_outline':
      return {
        type: 'text',
        x: px,
        y: py,
        style: {
          text: l.text,
          ...font,
          fill: textFill,
          align: 'center',
          verticalAlign: 'middle',
          textAlign: align,
          stroke: l.style === 'text_outline' ? fill : null,
          lineWidth: 2
        }
      };
    default: {
      // A shape style: the symbol on the point, the text above it.
      const size = SYMBOL[l.size] ?? 12;
      const h = size / 2;
      const shapes = {
        circle: { type: 'circle', shape: { cx: px, cy: py, r: h } },
        square: { type: 'rect', shape: { x: px - h, y: py - h, width: size, height: size } },
        diamond: {
          type: 'polygon',
          shape: {
            points: [
              [px, py - h],
              [px + h, py],
              [px, py + h],
              [px - h, py]
            ]
          }
        },
        triangleup: {
          type: 'polygon',
          shape: {
            points: [
              [px, py - h],
              [px + h, py + h],
              [px - h, py + h]
            ]
          }
        },
        triangledown: {
          type: 'polygon',
          shape: {
            points: [
              [px - h, py - h],
              [px + h, py - h],
              [px, py + h]
            ]
          }
        },
        arrowup: {
          type: 'polygon',
          shape: {
            points: [
              [px, py - h],
              [px + h, py],
              [px + h / 3, py],
              [px + h / 3, py + h],
              [px - h / 3, py + h],
              [px - h / 3, py],
              [px - h, py]
            ]
          }
        },
        arrowdown: {
          type: 'polygon',
          shape: {
            points: [
              [px, py + h],
              [px + h, py],
              [px + h / 3, py],
              [px + h / 3, py - h],
              [px - h / 3, py - h],
              [px - h / 3, py],
              [px - h, py]
            ]
          }
        },
        flag: {
          type: 'polygon',
          shape: {
            points: [
              [px - h, py + h],
              [px - h, py - h],
              [px + h, py - h / 2],
              [px - h, py]
            ]
          }
        }
      };
      const cross = l.style === 'cross' || l.style === 'xcross';
      const sym = cross
        ? {
            type: 'group',
            rotation: l.style === 'xcross' ? Math.PI / 4 : 0,
            originX: px,
            originY: py,
            children: [
              {
                type: 'line',
                shape: { x1: px - h, y1: py, x2: px + h, y2: py },
                style: { stroke: fill, lineWidth: 2 }
              },
              { type: 'line', shape: { x1: px, y1: py - h, x2: px, y2: py + h }, style: { stroke: fill, lineWidth: 2 } }
            ]
          }
        : { ...(shapes[l.style] ?? shapes.circle), style: { fill } };
      const children = [sym];
      if (l.text)
        children.push({
          type: 'text',
          x: px,
          y: py - h - 3,
          style: {
            text: l.text,
            ...font,
            fill: textFill === '#fff' ? fill : textFill,
            align: 'center',
            verticalAlign: 'bottom',
            textAlign: align
          }
        });
      return { type: 'group', children };
    }
  }
}

function boxShape(b, o, cs, api) {
  const [x1, x2] = [xOf(b.left, b.xloc, o), xOf(b.right, b.xloc, o)];
  if (![x1, x2, b.top, b.bottom].every(ok)) return null;
  let [l, t] = api.coord([x1, b.top]);
  let [r, btm] = api.coord([x2, b.bottom]);
  if (b.extend === 'left' || b.extend === 'both') l = Math.min(l, cs.x);
  if (b.extend === 'right' || b.extend === 'both') r = Math.max(r, cs.x + cs.width);
  const [x, y, w, h] = [Math.min(l, r), Math.min(t, btm), Math.abs(r - l), Math.abs(btm - t)];
  const children = [
    {
      type: 'rect',
      shape: { x, y, width: w, height: h },
      style: {
        fill: cssColor(b.bgcolor) ?? 'transparent',
        stroke: b.border_width > 0 ? (cssColor(b.border_color) ?? 'transparent') : null,
        lineWidth: b.border_width,
        lineDash: DASH[b.border_style] ?? null
      }
    }
  ];
  if (b.text) {
    const ha = b.text_halign === 'align_left' ? 'left' : b.text_halign === 'align_right' ? 'right' : 'center';
    const va = b.text_valign === 'align_top' ? 'top' : b.text_valign === 'align_bottom' ? 'bottom' : 'middle';
    const tx = ha === 'left' ? x + 4 : ha === 'right' ? x + w - 4 : x + w / 2;
    const ty = va === 'top' ? y + 3 : va === 'bottom' ? y + h - 3 : y + h / 2;
    children.push({
      type: 'text',
      x: tx,
      y: ty,
      style: {
        text: b.text,
        fontSize: FONT[b.text_size] ?? 12,
        fontFamily: FAMILY[b.font] ?? 'sans-serif',
        fill: cssColor(b.text_color) ?? '#000',
        align: ha,
        verticalAlign: va,
        width: b.text_wrap === 'wrap_auto' ? w - 8 : undefined,
        overflow: b.text_wrap === 'wrap_auto' ? 'break' : undefined
      }
    });
  }
  return { type: 'group', children };
}

function polylineShape(p, o, api) {
  const pts = p.points
    .map(([x, y]) => [xOf(x, p.xloc, o), y])
    .filter(([x, y]) => ok(x) && ok(y))
    .map((pt) => api.coord(pt));
  if (pts.length < 2) return null;
  const st = { ...stroke(p.line_color, p.line_width, p.line_style), fill: p.closed ? cssColor(p.fill_color) : null };
  return { type: p.closed ? 'polygon' : 'polyline', shape: { points: pts, smooth: p.curved ? 0.4 : 0 }, style: st };
}

function linefillShape(f, lines, o, cs, api) {
  const ends = [lines[f.line1], lines[f.line2]].map((l) => {
    const [x1, x2] = [xOf(l.x1, l.xloc, o), xOf(l.x2, l.xloc, o)];
    if (![x1, x2, l.y1, l.y2].every(ok)) return null;
    return extended(api.coord([x1, l.y1]), api.coord([x2, l.y2]), l.extend, cs);
  });
  if (ends.some((e) => !e)) return null;
  return {
    type: 'polygon',
    shape: { points: [ends[0][0], ends[0][1], ends[1][1], ends[1][0]] },
    style: { fill: cssColor(f.color) }
  };
}

/** A table pinned to its corner of the pane: cells sized to their text unless a width or
 *  height (percent of the pane) is set; merged ranges draw as one cell. */
function tableShape(t, cs) {
  const cellAt = new Map(t.cells.map((c) => [`${c.column},${c.row}`, c]));
  const covered = new Set();
  const span = new Map();
  for (const [c0, r0, c1, r1] of t.merges) {
    span.set(`${c0},${r0}`, [c1, r1]);
    for (let c = c0; c <= c1; c++) for (let r = r0; r <= r1; r++) if (c !== c0 || r !== r0) covered.add(`${c},${r}`);
  }
  const fontOf = (c) => FONT[c?.text_size] ?? 12;
  const textW = (c) => {
    const lines = (c?.text ?? '').split('\n');
    return Math.max(...lines.map((s) => s.length)) * fontOf(c) * 0.6;
  };
  const colW = Array.from({ length: t.columns }, (_, col) =>
    Math.max(
      0,
      ...Array.from({ length: t.rows }, (_, row) => {
        const c = cellAt.get(`${col},${row}`);
        if (!c || covered.has(`${col},${row}`) || span.has(`${col},${row}`)) return 0;
        return c.width > 0 ? (cs.width * c.width) / 100 : textW(c) + 12;
      })
    )
  );
  const rowH = Array.from({ length: t.rows }, (_, row) =>
    Math.max(
      0,
      ...Array.from({ length: t.columns }, (_, col) => {
        const c = cellAt.get(`${col},${row}`);
        if (!c || covered.has(`${col},${row}`)) return 0;
        const lines = (c.text ?? '').split('\n').length;
        return c.height > 0 ? (cs.height * c.height) / 100 : lines * fontOf(c) * 1.3 + 6;
      })
    )
  );
  const W = colW.reduce((a, b) => a + b, 0);
  const H = rowH.reduce((a, b) => a + b, 0);
  if (!W || !H) return null;
  const [v, h] = t.position.split('_');
  const pad = 6;
  const x0 = h === 'left' ? cs.x + pad : h === 'right' ? cs.x + cs.width - W - pad : cs.x + (cs.width - W) / 2;
  const y0 = v === 'top' ? cs.y + pad : v === 'bottom' ? cs.y + cs.height - H - pad : cs.y + (cs.height - H) / 2;
  const colX = colW.map((_, i) => x0 + colW.slice(0, i).reduce((a, b) => a + b, 0));
  const rowY = rowH.map((_, i) => y0 + rowH.slice(0, i).reduce((a, b) => a + b, 0));
  const children = [
    {
      type: 'rect',
      shape: { x: x0, y: y0, width: W, height: H },
      style: {
        fill: cssColor(t.bgcolor) ?? 'transparent',
        stroke: t.frame_width > 0 ? cssColor(t.frame_color) : null,
        lineWidth: t.frame_width
      }
    }
  ];
  for (const c of t.cells) {
    const key = `${c.column},${c.row}`;
    if (covered.has(key) || c.column >= t.columns || c.row >= t.rows) continue;
    const [c1, r1] = span.get(key) ?? [c.column, c.row];
    const x = colX[c.column];
    const y = rowY[c.row];
    const w = colW.slice(c.column, Math.min(c1, t.columns - 1) + 1).reduce((a, b) => a + b, 0);
    const hh = rowH.slice(c.row, Math.min(r1, t.rows - 1) + 1).reduce((a, b) => a + b, 0);
    const ha = c.text_halign === 'align_left' ? 'left' : c.text_halign === 'align_right' ? 'right' : 'center';
    const va = c.text_valign === 'align_top' ? 'top' : c.text_valign === 'align_bottom' ? 'bottom' : 'middle';
    children.push({
      type: 'rect',
      shape: { x, y, width: w, height: hh },
      style: {
        fill: cssColor(c.bgcolor) ?? 'transparent',
        stroke: t.border_width > 0 ? cssColor(t.border_color) : null,
        lineWidth: t.border_width
      }
    });
    if (c.text) {
      children.push({
        type: 'text',
        x: ha === 'left' ? x + 6 : ha === 'right' ? x + w - 6 : x + w / 2,
        y: va === 'top' ? y + 3 : va === 'bottom' ? y + hh - 3 : y + hh / 2,
        style: {
          text: c.text,
          fontSize: fontOf(c),
          fontFamily: FAMILY[c.font] ?? 'sans-serif',
          fill: cssColor(c.text_color) ?? '#000',
          align: ha,
          verticalAlign: va
        }
      });
    }
  }
  return { type: 'group', children };
}

function custom(name, o, z, items, render) {
  return {
    name,
    type: 'custom',
    xAxisIndex: o.x,
    yAxisIndex: o.y,
    clip: true,
    animation: false,
    silent: true,
    z,
    // One datum per drawing; the drawing itself is read from the closure.
    data: items.map((_, i) => [i]),
    renderItem: (params, api) =>
      render(items[params.dataIndex], params.coordSys, api) ?? { type: 'group', children: [] }
  };
}

/** Every series for `drawings`. `o = { x, y, n, high, low, id, xOfMs }`. */
export function drawingSeries(drawings, o) {
  if (!drawings) return [];
  const id = `script-${o.id}-d`;
  const out = [];
  const d = drawings;
  if (d.boxes?.length) out.push(custom(`${id}b`, o, 1, d.boxes, (b, cs, api) => boxShape(b, o, cs, api)));
  if (d.linefills?.length)
    out.push(custom(`${id}f`, o, 1, d.linefills, (f, cs, api) => linefillShape(f, d.lines, o, cs, api)));
  if (d.lines?.length) out.push(custom(`${id}l`, o, 3, d.lines, (l, cs, api) => lineShape(l, o, cs, api)));
  if (d.polylines?.length) out.push(custom(`${id}p`, o, 3, d.polylines, (p, _cs, api) => polylineShape(p, o, api)));
  if (d.labels?.length) out.push(custom(`${id}t`, o, 4, d.labels, (l, _cs, api) => labelShape(l, o, api)));
  // Tables sit on the pane, not on bars: drawn once each, pinned to a corner.
  if (d.tables?.length) out.push(custom(`${id}g`, o, 5, d.tables, (t, cs) => tableShape(t, cs)));
  return out;
}

/** `drawings` with every bar-index x moved back by `from` (the warmup bars cut off). */
export function shiftDrawings(drawings, from) {
  if (!drawings || !from) return drawings;
  const bi = (xloc) => xloc !== 'bar_time';
  return {
    ...drawings,
    labels: drawings.labels.map((l) => (bi(l.xloc) ? { ...l, x: l.x - from } : l)),
    lines: drawings.lines.map((l) => (bi(l.xloc) ? { ...l, x1: l.x1 - from, x2: l.x2 - from } : l)),
    boxes: drawings.boxes.map((b) => (bi(b.xloc) ? { ...b, left: b.left - from, right: b.right - from } : b)),
    polylines: drawings.polylines.map((p) =>
      bi(p.xloc) ? { ...p, points: p.points.map(([x, y]) => [x - from, y]) } : p
    )
  };
}
