/** The range a study's output can take, so a chart pane frames a bounded indicator whole (an
 *  RSI from 0 to 100) instead of zooming on the window's values. The Pine side computes the
 *  same thing at compile time (`otw-script` `check/range.rs`, `PlotMeta.range`).
 *
 *  A range is `[lo, hi]`, or `null` when the output is unbounded or unknown: never a guess.
 *  `paneScale` joins it with the data, so a value past a declared bound is still shown. */

const INF = Infinity;
const EMPTY = [INF, -INF];
const TOP = [-INF, INF];
const PCT = [0, 100];

/** Ranges of the DAG's built-in indicators, whatever they are fed. */
const FIXED = { rsi: PCT, stoch_k: PCT, stoch_d: PCT, mfi: PCT, adx: PCT, willr: [-100, 0] };
/** Chained built-ins that stay within their source (averages). */
const KEEPS_SOURCE = new Set(['sma', 'ema', 'wma']);

const empty = (a) => a[0] > a[1];
const join = (a, b) => [Math.min(a[0], b[0]), Math.max(a[1], b[1])];
const add = (a, b) => (empty(a) || empty(b) ? EMPTY : [a[0] + b[0], a[1] + b[1]]);
const neg = (a) => (empty(a) ? a : [-a[1], -a[0]]);
// 0 × ∞ is 0: the values are finite, an infinite end only says "unbounded".
const m0 = (x, y) => (x === 0 || y === 0 ? 0 : x * y);
function mul(a, b) {
  if (empty(a) || empty(b)) return EMPTY;
  const p = [m0(a[0], b[0]), m0(a[0], b[1]), m0(a[1], b[0]), m0(a[1], b[1])];
  return [Math.min(...p), Math.max(...p)];
}
/** A zero divisor yields `null` in the DAG, so a divisor touching 0 at one end loses that end. */
function div(a, b) {
  if (empty(a) || empty(b) || (b[0] === 0 && b[1] === 0)) return EMPTY;
  if (b[0] < 0 && b[1] > 0) return TOP;
  const negative = b[1] <= 0;
  const r = (d) => (d === 0 ? (negative ? -INF : INF) : 1 / d);
  return mul(a, [r(b[1]), r(b[0])]);
}
function abs(a) {
  if (empty(a) || a[0] >= 0) return a;
  if (a[1] <= 0) return neg(a);
  return [0, Math.max(-a[0], a[1])];
}

/** `[lo, hi]` when both ends are finite and apart, else `null`. */
export const bounded = (a) => (a && Number.isFinite(a[0]) && Number.isFinite(a[1]) && a[0] < a[1] ? [a[0], a[1]] : null);

/** The range of each output of a custom-indicator DAG, in `evalDef`'s order. */
export function defBounds(def) {
  const nodes = def?.nodes ?? [];
  const iv = [];
  const get = (i) => iv[i] ?? TOP;
  for (const n of nodes) {
    let r = TOP;
    switch (n?.op) {
      case 'const': r = Number.isFinite(n.value ?? 0) ? [n.value ?? 0, n.value ?? 0] : TOP; break;
      case 'price': r = n.field === 'volume' ? [0, INF] : TOP; break;
      case 'indicator':
        if (FIXED[n.indicator]) r = FIXED[n.indicator];
        else if (n.src != null && KEEPS_SOURCE.has(n.indicator)) r = get(n.src);
        break;
      case 'add': r = add(get(n.a), get(n.b)); break;
      case 'sub': r = add(get(n.a), neg(get(n.b))); break;
      case 'mul': r = mul(get(n.a), get(n.b)); break;
      case 'div': r = div(get(n.a), get(n.b)); break;
      case 'min': { const [a, b] = [get(n.a), get(n.b)]; r = [Math.min(a[0], b[0]), Math.min(a[1], b[1])]; break; }
      case 'max': { const [a, b] = [get(n.a), get(n.b)]; r = [Math.max(a[0], b[0]), Math.max(a[1], b[1])]; break; }
      case 'abs': r = abs(get(n.a)); break;
      case 'neg': r = neg(get(n.a)); break;
      case 'shift':
      case 'sma_of':
      case 'ema_of':
      case 'highest':
      case 'lowest': r = get(n.a); break;
      case 'change': { const a = get(n.a); r = add(a, neg(a)); break; }
      case 'clamp': { const a = get(n.a); r = [Math.max(a[0], n.lo), Math.min(a[1], n.hi)]; break; }
    }
    iv.push(r.some(Number.isNaN) ? TOP : r);
  }
  const outIdx = def?.outputs?.length ? def.outputs : [def?.output ?? 0];
  return outIdx.map((i) => bounded(iv[i]));
}

/** Share of the framed span added above and below a bounded pane. */
export const BOUND_MARGIN = 0.05;

/** How a pane's y axis frames its studies: `null` when none of them is bounded or draws a
 *  level (the axis keeps fitting the data alone), else `{ min, max }` functions of the data
 *  extent ECharts hands an axis: the join of the data, the declared ranges and the levels,
 *  plus `BOUND_MARGIN` on each side. `ranges` are `[lo, hi]`, `levels` numbers. */
export function paneScale(ranges, levels) {
  let lo = INF;
  let hi = -INF;
  for (const r of ranges) if (r) [lo, hi] = [Math.min(lo, r[0]), Math.max(hi, r[1])];
  for (const v of levels) if (Number.isFinite(v)) [lo, hi] = [Math.min(lo, v), Math.max(hi, v)];
  if (!(lo <= hi)) return null;
  const span = (e) => {
    const a = Number.isFinite(e?.min) ? Math.min(e.min, lo) : lo;
    const b = Number.isFinite(e?.max) ? Math.max(e.max, hi) : hi;
    const pad = (b - a) * BOUND_MARGIN || Math.abs(a) * BOUND_MARGIN || 1;
    return [a - pad, b + pad];
  };
  return { min: (e) => span(e)[0], max: (e) => span(e)[1] };
}
