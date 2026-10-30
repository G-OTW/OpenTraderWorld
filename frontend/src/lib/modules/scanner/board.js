/** Pure helpers for the scanner board: hexagon packing, color scales, row metrics, assigned
 *  colors and the persisted layout. No DOM, no fetch: the views render what these return, and
 *  `tests/scanner-board.test.js` pins them. */

// ── Honeycomb packing ──────────────────────────────────────────────────────────

const SQRT3 = Math.sqrt(3);

/** Pointy-top hexagons of circumradius `r`, packed in rows: odd rows shift half a cell right.
 *  Returns the cell centers and the box they need. `gap` is the space between neighbours. */
export function hexLayout(n, width, r, gap = 2) {
  const w = SQRT3 * r + gap; // center to center, same row
  const h = 1.5 * r + gap * (SQRT3 / 2); // center to center, next row
  const usable = Math.max(width - w / 2, w);
  const cols = Math.max(1, Math.floor(usable / w));
  const cells = [];
  for (let i = 0; i < n; i++) {
    const row = Math.floor(i / cols);
    const col = i % cols;
    cells.push({ x: w / 2 + col * w + (row % 2 ? w / 2 : 0), y: r + row * h });
  }
  const rows = n ? Math.ceil(n / cols) : 0;
  const wide = n > cols ? cols * w + w / 2 : Math.min(n, cols) * w;
  return { cols, rows, width: wide, height: rows ? 2 * r + (rows - 1) * h : 0, cells };
}

/** The largest radius in [min, max] that fits `n` cells in `width` x `height`. */
export function fitRadius(n, width, height, min = 14, max = 56, gap = 2) {
  for (let r = max; r > min; r -= 1) {
    if (hexLayout(n, width, r, gap).height <= height) return r;
  }
  return min;
}

/** SVG points of a pointy-top hexagon. */
export function hexPoints(cx, cy, r) {
  const pts = [];
  for (let k = 0; k < 6; k++) {
    const a = (Math.PI / 180) * (60 * k - 90);
    pts.push(`${(cx + r * Math.cos(a)).toFixed(2)},${(cy + r * Math.sin(a)).toFixed(2)}`);
  }
  return pts.join(' ');
}

// ── Colors ─────────────────────────────────────────────────────────────────────

function parseHex(hex) {
  const h = String(hex).trim().replace('#', '');
  const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h.slice(0, 6);
  const n = parseInt(full, 16);
  if (!Number.isFinite(n) || full.length !== 6) return null;
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

const toHex = (rgb) => '#' + rgb.map((v) => Math.round(v).toString(16).padStart(2, '0')).join('');

/** `a` to `b` by `t` in [0, 1], in sRGB. */
export function mix(a, b, t) {
  const pa = parseHex(a);
  const pb = parseHex(b);
  if (!pa || !pb) return t < 0.5 ? a : b;
  const k = Math.min(1, Math.max(0, t));
  return toHex(pa.map((v, i) => v + (pb[i] - v) * k));
}

/** Relative luminance (WCAG). */
export function luminance(hex) {
  const p = parseHex(hex);
  if (!p) return 0.5;
  const [r, g, b] = p.map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** The text ink that reads on a fill: dark on light fills, light on dark ones. */
export function inkOn(fill, dark = '#17191c', light = '#ffffff') {
  const L = luminance(fill);
  const onDark = (1.05) / (L + 0.05);
  const onLight = (L + 0.05) / (luminance(dark) + 0.05);
  return onLight >= onDark ? dark : light;
}

/** Which scale a metric wants: signed change diverges around 0, the rest are magnitudes,
 *  a signal is lit or not. */
export function scaleKind(metric) {
  if (metric === 'change_pct' || metric === 'change_from_open') return 'diverging';
  if (String(metric).startsWith('signal:')) return 'binary';
  return 'sequential';
}

/** A color function for `values` under `metric`. `colors` = { neg, mid, pos, low, high, lit,
 *  off, none }. Diverging is symmetric around 0 (the larger side sets the span, so equal
 *  moves get equal color); sequential goes low to high over [0, max]; hit recency is newest
 *  dark. A missing value gets `none`. */
export function colorScale(metric, values, colors) {
  const kind = scaleKind(metric);
  const finite = values.filter((v) => typeof v === 'number' && Number.isFinite(v));
  if (kind === 'binary') {
    return (v) => (v === true ? colors.lit : v === false ? colors.off : colors.none);
  }
  if (kind === 'diverging') {
    const span = Math.max(...finite.map((v) => Math.abs(v)), 1e-9);
    const fn = (v) => {
      if (typeof v !== 'number' || !Number.isFinite(v)) return colors.none;
      const t = Math.max(-1, Math.min(1, v / span));
      return t >= 0 ? mix(colors.mid, colors.pos, t) : mix(colors.mid, colors.neg, -t);
    };
    fn.domain = [-span, span];
    return fn;
  }
  // Recency: fresh is strong, a day old is faint.
  if (metric === 'hit_recency') {
    const fn = (v) => {
      if (typeof v !== 'number' || !Number.isFinite(v)) return colors.none;
      return mix(colors.high, colors.low, Math.min(1, Math.max(0, v) / 24));
    };
    fn.domain = [0, 24];
    return fn;
  }
  const max = Math.max(...finite, 1e-9);
  const fn = (v) => {
    if (typeof v !== 'number' || !Number.isFinite(v)) return colors.none;
    return mix(colors.low, colors.high, Math.max(0, v) / max);
  };
  fn.domain = [0, max];
  return fn;
}

/** A stored color (`chart-3`, a hex, or empty) as a CSS value. */
export function cssColor(token) {
  if (!token) return '';
  return /^chart-[1-8]$/.test(token) ? `var(--${token})` : token;
}

/** The same, as a concrete color for a canvas (ECharts), given the theme's series list. */
export function concreteColor(token, series) {
  if (!token) return '';
  const m = /^chart-([1-8])$/.exec(token);
  return m ? series[Number(m[1]) - 1] : token;
}

/** The next unused palette slot, in fixed order; the ninth and later reuse nothing and
 *  return '' (the entity then inherits). */
export function nextSlot(used) {
  for (let i = 1; i <= 8; i++) if (!used.includes(`chart-${i}`)) return `chart-${i}`;
  return '';
}

/** An item's own color, else the first hive holding it that has one. */
export function itemColor(item, hives) {
  if (item.color) return item.color;
  for (const h of hives) if (h.color && (h.item_ids ?? []).includes(item.id)) return h.color;
  return '';
}

// ── Row metrics ────────────────────────────────────────────────────────────────

const pct = (a, b) => (Number.isFinite(a) && Number.isFinite(b) && b !== 0 ? (a / b - 1) * 100 : null);

/** One board row's measures. `live` = { close } from the live feed when the board is live. */
export function rowMetrics(item, live = null, now = Date.now()) {
  const close = Number.isFinite(live?.close) ? live.close : item.last_close;
  const hitAge = item.last_hit_at ? (now - Date.parse(item.last_hit_at)) / 3.6e6 : null;
  return {
    close,
    change_pct: pct(close, item.prev_close),
    change_from_open: pct(close, item.day_open),
    volume_ratio:
      Number.isFinite(item.last_volume) && Number.isFinite(item.avg_volume) && item.avg_volume > 0
        ? item.last_volume / item.avg_volume
        : null,
    hit_recency: Number.isFinite(hitAge) ? hitAge : null
  };
}

/** The value a view colors by. */
export function metricValue(item, metric, live = null, now = Date.now()) {
  if (String(metric).startsWith('signal:')) {
    const v = item.signal_state?.[metric.slice(7)];
    return typeof v === 'boolean' ? v : null;
  }
  return rowMetrics(item, live, now)[metric] ?? null;
}

/** Closes of the sparkline (`[[t, close], ...]` or plain numbers). */
export function sparkCloses(spark) {
  return (spark ?? []).map((p) => (Array.isArray(p) ? p[1] : p)).filter((v) => Number.isFinite(v));
}

/** The spark normalized to 100 at its first close, with stamps in ms. */
export function normalized(spark) {
  const pts = (spark ?? []).filter((p) => Array.isArray(p) && Number.isFinite(p[1]));
  if (!pts.length || !pts[0][1]) return [];
  const base = pts[0][1];
  return pts.map(([t, c]) => [t * 1000, (c / base) * 100]);
}

/** Sort rows by a metric (desc, missing last) or by ticker. */
export function sortRows(rows, key, live = {}, now = Date.now()) {
  const out = [...rows];
  if (!key || key === 'ticker') return out.sort((a, b) => a.ticker.localeCompare(b.ticker));
  const v = (r) => {
    const x = metricValue(r, key, live[r.id], now);
    return typeof x === 'boolean' ? (x ? 1 : 0) : x;
  };
  return out.sort((a, b) => {
    const va = v(a);
    const vb = v(b);
    if (va == null && vb == null) return a.ticker.localeCompare(b.ticker);
    if (va == null) return 1;
    if (vb == null) return -1;
    return key === 'hit_recency' ? va - vb : vb - va;
  });
}

// ── Grid columns ───────────────────────────────────────────────────────────────

/** The grid's measure columns, in display order. The item column always shows; a column
 *  list may also hold `signal:<id>` and `trend` (the sparkline). */
export const GRID_COLUMNS = [
  ['close', 'scanner.col.close'],
  ['change_pct', 'scanner.col.change'],
  ['change_from_open', 'scanner.col.fromOpen'],
  ['volume_ratio', 'scanner.col.volume'],
  ['hit_recency', 'scanner.col.lastHit']
];

/** What a table shows before anyone picks: every measure and the trend, no signal. */
export const DEFAULT_GRID_COLUMNS = [...GRID_COLUMNS.map(([k]) => k), 'trend'];

// ── Board layout (persisted in scanner_views.layout and .sort) ────────────────

export const SIZES = ['s', 'm', 'l'];
export const LIVE_CAP = 100;

/** A view's layout with every field defaulted, whatever was stored. */
export function normalizeLayout(raw) {
  const l = raw && typeof raw === 'object' ? raw : {};
  return {
    size: SIZES.includes(l.size) ? l.size : 'm',
    live: l.live === true,
    sort: typeof l.sort === 'string' ? l.sort : 'ticker',
    hitsOnly: l.hitsOnly === true,
    lines: l.lines === 'indicator' ? 'indicator' : 'close'
  };
}

/** Move a view from one place to another; returns the views with their new `sort`. */
export function reorder(views, from, to) {
  const list = [...views];
  if (from < 0 || from >= list.length || to < 0 || to >= list.length) return list.map((v, i) => ({ ...v, sort: i }));
  const [moved] = list.splice(from, 1);
  list.splice(to, 0, moved);
  return list.map((v, i) => ({ ...v, sort: i }));
}

/** Which items a live board subscribes: the visible items of its live views, once each.
 *  Over the cap the whole board stays on bar close (`over` true) rather than going live on
 *  an arbitrary part of it. */
export function liveSet(views, boards, cap = LIVE_CAP) {
  const seen = new Map();
  for (const v of views) {
    if (!normalizeLayout(v.layout).live) continue;
    for (const it of boards[v.hive_id]?.items ?? []) if (!seen.has(it.id)) seen.set(it.id, it);
  }
  const all = [...seen.values()];
  const over = all.length > cap;
  return { items: over ? [] : all, over, total: all.length };
}
