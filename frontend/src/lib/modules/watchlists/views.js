/** Pure helpers behind the watchlist table extras and the heatmap, compare and
 * correlation views. No imports: the unit tests in tests/watchlists-*.test.js load this
 * file directly. */

/** Column catalog (mirrors `watchlists_api::COLUMNS`). `field` reads `item.quote`. */
export const COLUMN_DEFS = [
  { id: 'c24', key: 'watchlists.table.h24', kind: 'change', field: 'change_24h' },
  { id: 'c3', key: 'watchlists.table.d3', kind: 'change', field: 'change_3d' },
  { id: 'c7', key: 'watchlists.table.d7', kind: 'change', field: 'change_7d' },
  { id: 'c30', key: 'watchlists.table.d30', kind: 'change', field: 'change_30d' },
  { id: 'spark', key: 'watchlists.table.trend', kind: 'spark' },
  { id: 'range52', key: 'watchlists.col.range52', kind: 'range' },
  { id: 'dist52h', key: 'watchlists.col.dist52h', kind: 'dist52h' },
  { id: 'mcap', key: 'watchlists.col.mcap', kind: 'big', field: 'mcap' },
  { id: 'vol', key: 'watchlists.col.vol', kind: 'big', field: 'volume' },
  { id: 'pe', key: 'watchlists.col.pe', kind: 'ratio', field: 'pe' },
  { id: 'divy', key: 'watchlists.col.divy', kind: 'pct', field: 'div_yield' },
  { id: 'rsi', key: 'watchlists.col.rsi', kind: 'ratio', field: 'rsi_14' },
  { id: 'targets', key: 'watchlists.col.targets', kind: 'targets' },
  { id: 'notes', key: 'watchlists.table.notes', kind: 'notes' }
];

export const DEFAULT_COLUMNS = ['c24', 'c3', 'c7', 'c30', 'spark', 'range52', 'notes'];

/** Flag palette (mirrors `otw_store::watchlists::FLAGS`), each with its theme color. Tokens
 *  only: the theme accent is sage, so blue and purple read the chart identity blue and
 *  purple, and orange sits between the red and amber tokens. */
export const FLAGS = [
  { id: 'red', color: 'var(--red)' },
  { id: 'orange', color: 'color-mix(in oklab, var(--red) 45%, var(--amber))' },
  { id: 'yellow', color: 'var(--amber)' },
  { id: 'green', color: 'var(--green)' },
  { id: 'blue', color: 'var(--chart-1)' },
  { id: 'purple', color: 'var(--chart-6)' }
];
export const flagColor = (id) => FLAGS.find((f) => f.id === id)?.color ?? null;

const num = (v) => (typeof v === 'number' && isFinite(v) ? v : null);

/** Where `price` sits between the 52-week `low` and `high`, 0 to 1 (clamped). Null when the
 *  range is missing or empty: no bar is drawn rather than a made-up one. */
export function rangePosition(price, low, high) {
  const p = num(price);
  const lo = num(low);
  const hi = num(high);
  if (p == null || lo == null || hi == null || hi <= lo) return null;
  return Math.min(1, Math.max(0, (p - lo) / (hi - lo)));
}

/** Distance from the current price to a target, in percent: positive when the target is
 *  above the price, negative below. Null without both numbers. */
export function distancePct(price, target) {
  const p = num(price);
  const t = num(target);
  if (p == null || t == null || p === 0) return null;
  return (t / p - 1) * 100;
}

/** Virtual position P/L: qty bought at `entry`, valued at `price`. Null when incomplete. */
export function virtualPnl(qty, entry, price) {
  const q = num(qty);
  const e = num(entry);
  const p = num(price);
  if (q == null || e == null || p == null || e === 0) return null;
  return { pnl: q * (p - e), pct: (p / e - 1) * 100 };
}

/** A series of [t, close] rebased to 100 at its first point. */
export function normalizeSeries(points) {
  const base = points?.[0]?.[1];
  if (!num(base)) return [];
  return points.map(([t, v]) => [t, (v / base) * 100]);
}

/** Large numbers as 1.23T / 456.7B / 12.3M / 4.5K. */
export function fmtBig(n) {
  const v = num(n);
  if (v == null) return null;
  const a = Math.abs(v);
  if (a >= 1e12) return `${(v / 1e12).toFixed(2)}T`;
  if (a >= 1e9) return `${(v / 1e9).toFixed(2)}B`;
  if (a >= 1e6) return `${(v / 1e6).toFixed(2)}M`;
  if (a >= 1e3) return `${(v / 1e3).toFixed(1)}K`;
  return v.toFixed(0);
}

/** Market cap in USD, compact: $1.52T / $76.0B / $912.4M. `fmtCompactMoney` stops at
 *  billions ($1523.7B for Bitcoin), so caps keep their own scale here. Null stays null. */
export function fmtCap(n) {
  const v = num(n);
  if (v == null) return null;
  const a = Math.abs(v);
  const sign = v < 0 ? '−' : '';
  if (a >= 1e12) return `${sign}$${(a / 1e12).toFixed(2)}T`;
  if (a >= 1e9) return `${sign}$${(a / 1e9).toFixed(1)}B`;
  if (a >= 1e6) return `${sign}$${(a / 1e6).toFixed(1)}M`;
  if (a >= 1e3) return `${sign}$${(a / 1e3).toFixed(1)}K`;
  return `${sign}$${a.toFixed(0)}`;
}

/**
 * Squarified treemap. `tiles` = [{ id, weight }] (weight > 0); returns
 * [{ id, x, y, w, h }] filling `width` x `height`. A tile with an unknown size (weight
 * null, 0 or negative) gets the median known weight, or 1 when none is known, so every
 * symbol stays visible and "unknown" never reads as "tiny".
 */
export function treemap(tiles, width, height) {
  if (!tiles?.length || width <= 0 || height <= 0) return [];
  const known = tiles.map((t) => num(t.weight)).filter((w) => w != null && w > 0).sort((a, b) => a - b);
  const fill = known.length ? known[Math.floor(known.length / 2)] : 1;
  const items = tiles
    .map((t) => ({ id: t.id, weight: num(t.weight) > 0 ? t.weight : fill }))
    .sort((a, b) => b.weight - a.weight);
  const total = items.reduce((s, t) => s + t.weight, 0);
  const scale = (width * height) / total;
  const areas = items.map((t) => ({ id: t.id, area: t.weight * scale }));

  const out = [];
  let x = 0;
  let y = 0;
  let w = width;
  let h = height;
  const worst = (row, side) => {
    const s = row.reduce((a, r) => a + r.area, 0);
    let max = 0;
    let min = Infinity;
    for (const r of row) {
      max = Math.max(max, r.area);
      min = Math.min(min, r.area);
    }
    return Math.max((side * side * max) / (s * s), (s * s) / (side * side * min));
  };
  const layout = (row) => {
    const s = row.reduce((a, r) => a + r.area, 0);
    if (w >= h) {
      // Column on the left, stacked vertically.
      const cw = s / h;
      let cy = y;
      for (const r of row) {
        const rh = r.area / cw;
        out.push({ id: r.id, x, y: cy, w: cw, h: rh });
        cy += rh;
      }
      x += cw;
      w -= cw;
    } else {
      const rh = s / w;
      let cx = x;
      for (const r of row) {
        const rw = r.area / rh;
        out.push({ id: r.id, x: cx, y, w: rw, h: rh });
        cx += rw;
      }
      y += rh;
      h -= rh;
    }
  };
  let row = [];
  for (const a of areas) {
    const side = Math.min(w, h);
    if (!row.length || worst([...row, a], side) <= worst(row, side)) {
      row.push(a);
    } else {
      layout(row);
      row = [a];
    }
  }
  if (row.length) layout(row);
  return out;
}

/** Heatmap tile wash: the alpha of the tone colour for a change of `v`, growing with the
 *  magnitude up to `cap` and never past `max`, so a tile is a tint and never a saturated
 *  block. Null for an unknown change (the tile stays on the neutral surface). */
export function heatAlpha(v, cap = 8, min = 0.1, max = 0.5) {
  const n = num(v);
  if (n == null) return null;
  if (n === 0) return 0;
  return min + (max - min) * Math.min(1, Math.abs(n) / cap);
}

/** Price shortened for tight labels (52-week ends): $108.4K from 10,000 up, else the
 *  given formatter. Null stays null. */
export function shortPrice(n, fmt = (v) => v.toFixed(2)) {
  const v = num(n);
  if (v == null) return null;
  const a = Math.abs(v);
  if (a >= 1e9) return `$${(v / 1e9).toFixed(1)}B`;
  if (a >= 1e6) return `$${(v / 1e6).toFixed(1)}M`;
  if (a >= 1e4) return `$${(v / 1e3).toFixed(1)}K`;
  return fmt(v);
}

const hexRgb = (h) => {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(String(h ?? '').trim());
  return m ? m.slice(1, 4).map((x) => parseInt(x, 16)) : null;
};

/**
 * Diverging fill for a correlation `r` in [-1, 1]: the neutral midpoint at 0, the negative
 * pole at -1, the positive pole at +1, interpolated in sRGB. `stops` = [neg, mid, pos] as
 * #rrggbb (what `chartColors().diverge` returns). `reach` (0..1) caps how far toward a pole
 * the strongest value goes. Returns `rgb(r, g, b)`, or null for an unknown r.
 */
export function divergeColor(r, stops, reach = 0.85) {
  const v = num(r);
  if (v == null || !stops) return null;
  const [neg, mid, pos] = stops.map(hexRgb);
  if (!neg || !mid || !pos) return null;
  const k = Math.min(1, Math.abs(v)) * reach;
  const pole = v < 0 ? neg : pos;
  const c = mid.map((m, i) => Math.round(m + (pole[i] - m) * k));
  return `rgb(${c.join(', ')})`;
}

/** WCAG relative luminance of `rgb(...)` or #rrggbb, 0..1; null when unreadable. */
export function luminance(color) {
  const s = String(color ?? '');
  const m = /rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(s);
  const rgb = m ? m.slice(1, 4).map(Number) : hexRgb(s);
  if (!rgb) return null;
  const [r, g, b] = rgb.map((x) => {
    const c = x / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Of two inks, the one with the higher contrast on `fill` (a label inside a coloured cell). */
export function inkOn(fill, a, b) {
  const lf = luminance(fill);
  const la = luminance(a);
  const lb = luminance(b);
  if (lf == null || la == null || lb == null) return a;
  const ratio = (x, y) => (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
  return ratio(lf, la) >= ratio(lf, lb) ? a : b;
}

/** Rows grouped by section, in display order: ungrouped first, then sections in the order
 *  their first row appears. Returns [{ section: string|null, items }]. */
export function groupBySection(items) {
  const groups = [];
  const byName = new Map();
  for (const it of items) {
    const s = it.section ?? null;
    if (!byName.has(s)) {
      const g = { section: s, items: [] };
      byName.set(s, g);
      groups.push(g);
    }
    byName.get(s).items.push(it);
  }
  groups.sort((a, b) => (a.section === null ? -1 : b.section === null ? 1 : 0));
  return groups;
}

/**
 * Move one row before `beforeId` (or to the end of `section` when null), changing its
 * section. Returns the full new order as [{ id, section }], what the reorder API takes.
 */
export function moveRow(items, id, section, beforeId = null) {
  const moving = items.find((i) => i.id === id);
  if (!moving) return items.map((i) => ({ id: i.id, section: i.section ?? null }));
  const rest = items.filter((i) => i.id !== id);
  const row = { ...moving, section: section ?? null };
  let at = beforeId ? rest.findIndex((i) => i.id === beforeId) : -1;
  if (at < 0) {
    // End of the target section: after its last row, else at the very end.
    const last = rest.map((i) => i.section ?? null).lastIndexOf(section ?? null);
    at = last < 0 ? rest.length : last + 1;
  }
  rest.splice(at, 0, row);
  return rest.map((i) => ({ id: i.id, section: i.section ?? null }));
}
