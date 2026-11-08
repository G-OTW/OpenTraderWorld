/**
 * Pure geometry and data helpers for the shared chart layer. No DOM, no runes: unit
 * tested in `frontend/tests/ui-charts.test.js`.
 */

const finite = (v) => typeof v === 'number' && Number.isFinite(v);

function niceStep(raw) {
  if (!(raw > 0)) return 1;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const f = raw / mag;
  const nice = f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10;
  return nice * mag;
}

const decimalsOf = (step) => {
  const s = String(step);
  if (s.includes('e-')) return Number(s.split('e-')[1]);
  return s.includes('.') ? s.split('.')[1].length : 0;
};

/**
 * Round tick values covering [min, max], about `count` intervals (3 to 5 ticks for
 * the default 4). The first tick is <= min and the last >= max, so the ticks double
 * as the axis domain. A flat range is widened around its value; non-finite input
 * gives [].
 *
 *   niceTicks(0, 87)   -> [0, 25, 50, 75, 100]
 *   niceTicks(-12, 30) -> [-20, 0, 20, 40]
 */
export function niceTicks(min, max, count = 4) {
  if (!finite(min) || !finite(max)) return [];
  if (min > max) [min, max] = [max, min];
  if (min === max) {
    const pad = Math.abs(min) * 0.1 || 1;
    min -= pad;
    max += pad;
  }
  const n = Math.max(2, Math.round(count));
  let step = niceStep((max - min) / n);
  let lo = Math.floor(min / step) * step;
  let hi = Math.ceil(max / step) * step;
  // Rounding outward can add an interval; keep within 3..(n+1) ticks.
  while ((hi - lo) / step > n + 0.5) {
    step = niceStep(step * 1.01);
    lo = Math.floor(min / step) * step;
    hi = Math.ceil(max / step) * step;
  }
  const d = decimalsOf(step);
  const out = [];
  for (let v = lo; v <= hi + step / 2; v += step) out.push(Number(v.toFixed(d)) + 0);
  return out;
}

/**
 * Any timestamp to unix seconds: a Date, epoch seconds, epoch milliseconds (> 1e11),
 * a "YYYY-MM-DD" date (local midnight, so a month end never slips a day) or another
 * ISO string. Unparseable input gives null.
 */
export function toSeconds(ts) {
  if (ts == null || ts === '') return null;
  if (ts instanceof Date) {
    const t = ts.getTime();
    return Number.isFinite(t) ? t / 1000 : null;
  }
  if (typeof ts === 'number') {
    if (!Number.isFinite(ts)) return null;
    return Math.abs(ts) > 1e11 ? ts / 1000 : ts;
  }
  const s = String(ts);
  const d = new Date(/^\d{4}-\d{2}-\d{2}$/.test(s) ? `${s}T00:00:00` : s);
  const t = d.getTime();
  return Number.isFinite(t) ? t / 1000 : null;
}

/**
 * Rebase a series to 100 at its first finite, positive value (the comparison form for
 * symbols or portfolios of different sizes). Accepts plain values or [ts, value, ...]
 * points and returns the same shape. Nulls stay null. A series with no positive
 * starting value cannot be indexed: every value comes back null.
 *
 *   indexTo100([50, 55, 45])  -> [100, 110, 90]
 */
export function indexTo100(input = []) {
  const isPoints = input.some((p) => Array.isArray(p));
  const val = (p) => (isPoints ? p?.[1] : p);
  const base = input.map(val).find((v) => finite(v) && v !== 0);
  const ok = finite(base) && base > 0;
  const map = (v) => (ok && finite(v) ? (v * 100) / base : null);
  return isPoints ? input.map((p) => [p[0], map(p[1]), ...p.slice(2)]) : input.map(map);
}

/**
 * Join several point lists on one sorted, de-duplicated time axis (unix seconds).
 * Returns { xs, ys, notes }. `ys[s][i]` is series s at xs[i]: its value, `null` for an
 * explicit null (a real gap, drawn as a break) or `undefined` when the series simply
 * has no point at that time (an alignment artifact: uPlot draws straight across it).
 * `notes[s][i]` is the point's third element (an estimate flag or note), else null.
 */
export function alignSeries(lists = []) {
  const set = new Set();
  const norm = lists.map((pts) =>
    (pts ?? [])
      .map((p) => [toSeconds(p?.[0]), finite(p?.[1]) ? p[1] : null, p?.[2] ?? null])
      .filter((p) => p[0] != null)
  );
  for (const pts of norm) for (const p of pts) set.add(p[0]);
  const xs = [...set].sort((a, b) => a - b);
  const pos = new Map(xs.map((x, i) => [x, i]));
  const ys = norm.map(() => new Array(xs.length).fill(undefined));
  const notes = norm.map(() => new Array(xs.length).fill(null));
  norm.forEach((pts, s) => {
    for (const [x, y, n] of pts) {
      ys[s][pos.get(x)] = y;
      if (n) notes[s][pos.get(x)] = n;
    }
  });
  return { xs, ys, notes };
}

/**
 * Split one aligned series into a solid run (measured) and a dashed run (estimated).
 * The dashed run also takes the measured point just before each estimate, so the two
 * strokes join instead of leaving a hole. `flags[i]` truthy marks an estimate.
 * `undefined` entries (alignment artifacts) are kept as they are in both runs.
 */
export function splitEstimated(ys = [], flags = []) {
  const est = (i) => !!flags[i];
  const nextDefined = new Array(ys.length).fill(-1);
  for (let i = ys.length - 2, next = ys[ys.length - 1] === undefined ? -1 : ys.length - 1; i >= 0; i--) {
    nextDefined[i] = next;
    if (ys[i] !== undefined) next = i;
  }
  const solid = ys.map((y, i) => (y === undefined ? undefined : est(i) ? null : y));
  const dashed = ys.map((y, i) => {
    if (y === undefined) return undefined;
    const j = nextDefined[i];
    return est(i) || (j >= 0 && est(j)) ? y : null;
  });
  return { solid, dashed, any: ys.some((y, i) => y !== undefined && est(i)) };
}

/** [min, max] over every finite number in the arrays (plus `extra` values). */
export function extent(arrays = [], extra = []) {
  let lo = Infinity;
  let hi = -Infinity;
  const take = (v) => {
    if (finite(v)) {
      if (v < lo) lo = v;
      if (v > hi) hi = v;
    }
  };
  for (const a of arrays) for (const v of a ?? []) take(v);
  for (const v of extra) take(v);
  return lo === Infinity ? null : [lo, hi];
}

/** Linear map from [d0, d1] to [r0, r1]. */
export function linear(d0, d1, r0, r1) {
  const span = d1 - d0 || 1;
  return (v) => r0 + ((v - d0) / span) * (r1 - r0);
}

/**
 * SVG path of a bar with its data end rounded and its baseline end square.
 * `end` is the side away from the baseline: 'top' | 'bottom' | 'left' | 'right' |
 * 'none'. The radius shrinks to fit a short bar.
 */
export function barPath(x, y, w, h, r = 4, end = 'top') {
  if (!(w > 0) || !(h > 0)) return '';
  const vertical = end === 'top' || end === 'bottom';
  const rr = end === 'none' ? 0 : Math.max(0, Math.min(r, (vertical ? w : h) / 2, vertical ? h : w));
  const f = (n) => Math.round(n * 100) / 100;
  const [X, Y, W, H, R] = [x, y, w, h, rr].map(f);
  if (R === 0) return `M${X},${Y}h${W}v${H}h${-W}Z`;
  switch (end) {
    case 'top':
      return `M${X},${f(Y + H)}V${f(Y + R)}Q${X},${Y} ${f(X + R)},${Y}H${f(X + W - R)}Q${f(X + W)},${Y} ${f(X + W)},${f(Y + R)}V${f(Y + H)}Z`;
    case 'bottom':
      return `M${X},${Y}H${f(X + W)}V${f(Y + H - R)}Q${f(X + W)},${f(Y + H)} ${f(X + W - R)},${f(Y + H)}H${f(X + R)}Q${X},${f(Y + H)} ${X},${f(Y + H - R)}Z`;
    case 'right':
      return `M${X},${Y}H${f(X + W - R)}Q${f(X + W)},${Y} ${f(X + W)},${f(Y + R)}V${f(Y + H - R)}Q${f(X + W)},${f(Y + H)} ${f(X + W - R)},${f(Y + H)}H${X}Z`;
    case 'left':
      return `M${f(X + W)},${Y}V${f(Y + H)}H${f(X + R)}Q${X},${f(Y + H)} ${X},${f(Y + H - R)}V${f(Y + R)}Q${X},${Y} ${f(X + R)},${Y}Z`;
    default:
      return `M${X},${Y}h${W}v${H}h${-W}Z`;
  }
}

/** Signed change of `v` against `start`, in percent of |start|; null when it cannot
 *  be measured (a missing or zero start). */
export function changePct(v, start) {
  if (!finite(v) || !finite(start) || start === 0) return null;
  return ((v - start) / Math.abs(start)) * 100;
}

/** Estimated monospace advance at 10px, for label fitting without a DOM. */
export const MONO_CHAR_PX = 6.1;
export const textWidth = (s, px = 10) => String(s ?? '').length * MONO_CHAR_PX * (px / 10);
