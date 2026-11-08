/** Browser-side evaluation of a custom-indicator DAG — a port of `backtest::custom::eval`.
 *
 *  The backtester evaluates these graphs in Rust; the chart module has the bars in hand
 *  already and needs the series per frame, so it evaluates the same graph here. Same node
 *  ops, same warm-up, same `null` propagation as the Rust file, over `math.js` (itself a port
 *  of the engine's indicator math) — a custom indicator plots exactly what it backtests.
 *
 *  Bars come in as the chart's parallel arrays `{ o, h, l, c, v }`, plus `t` (each bar's
 *  stamp, epoch ms) when a graph reads a `series` node.
 */
import * as m from './math.js';
import { isChainableIndicator } from './dag.js';

/** Hard cap mirroring `custom::MAX_NODES` (the API rejects anything bigger at save time). */
export const MAX_NODES = 64;

const nulls = (n) => new Array(n).fill(null);

/** Node indices a node reads from — used by the structural check. */
function refs(node) {
  switch (node.op) {
    case 'price':
    case 'const':
    case 'series':
      return [];
    case 'indicator':
      return node.src != null ? [node.src] : [];
    default:
      return [node.a, node.b].filter((x) => x != null);
  }
}

/** Structural validation, same rules as the Rust `validate`: bounded, output in range, and
 *  every reference points at an *earlier* node. Returns an error string or null. */
export function validateDef(def) {
  const nodes = def?.nodes;
  if (!Array.isArray(nodes) || !nodes.length) return 'indicator has no steps';
  if (nodes.length > MAX_NODES) return `indicator has too many steps (max ${MAX_NODES})`;
  const out = def.output ?? 0;
  if (out >= nodes.length) return 'output step is out of range';
  for (let i = 0; i < nodes.length; i++) {
    for (const r of refs(nodes[i])) {
      if (r >= i) return `step ${i + 1} references step ${r + 1} which is not before it`;
    }
  }
  return null;
}

function priceSeries(field, b) {
  switch (field) {
    case 'open':
      return [...b.o];
    case 'high':
      return [...b.h];
    case 'low':
      return [...b.l];
    case 'volume':
      return [...b.v];
    case 'close':
    default:
      return [...b.c];
  }
}

/** A built-in read from the bars (OHLCV). Mirrors `resolve_builtin`: a zero/absent param
 *  falls back to that indicator's default. */
function resolveBuiltin(node, b) {
  const p = (d) => (node.period ? node.period : d);
  const f = (d) => (node.fast ? node.fast : d);
  const sl = (d) => (node.slow ? node.slow : d);
  const mu = (d) => (node.mult > 0 ? node.mult : d);
  const sp = (d) => (node.signal_period ? node.signal_period : d);
  const { h, l, c, v } = b;
  switch (node.indicator) {
    case 'sma': return m.sma(c, p(20));
    case 'ema': return m.ema(c, p(20));
    case 'dema': return m.dema(c, p(20));
    case 'tema': return m.tema(c, p(20));
    case 'wma': return m.wma(c, p(20));
    case 'hma': return m.hma(c, p(20));
    case 'vwap': return m.vwap(h, l, c, v, p(20));
    case 'rsi': return m.rsi(c, p(14));
    case 'stoch_k': return m.stochK(h, l, c, p(14), sp(3));
    case 'stoch_d': return m.stochD(h, l, c, p(14), sp(3));
    case 'cci': return m.cci(h, l, c, p(20));
    case 'willr': return m.willr(h, l, c, p(14));
    case 'roc': return m.roc(c, p(12));
    case 'momentum': return m.momentum(c, p(10));
    case 'macd': return m.macdLine(c, f(12), sl(26));
    case 'macd_signal': return m.macdSignal(c, f(12), sl(26), sp(9));
    case 'macd_hist': return m.macdHist(c, f(12), sl(26), sp(9));
    case 'adx': return m.adx(h, l, c, p(14));
    case 'mfi': return m.mfi(h, l, c, v, p(14));
    case 'obv': return m.obv(c, v);
    case 'atr': return m.atr(h, l, c, p(14));
    case 'stddev': return m.stddev(c, p(20));
    case 'bb_upper': return m.bollinger(c, p(20), mu(2), 1);
    case 'bb_mid': return m.bollinger(c, p(20), mu(2), 0);
    case 'bb_lower': return m.bollinger(c, p(20), mu(2), -1);
    case 'keltner_upper': return m.keltner(h, l, c, p(20), mu(2), 1);
    case 'keltner_lower': return m.keltner(h, l, c, p(20), mu(2), -1);
    case 'donchian_upper': return m.donchian(h, l, p(20), 1);
    case 'donchian_mid': return m.donchian(h, l, p(20), 0);
    case 'donchian_lower': return m.donchian(h, l, p(20), -1);
    case 'supertrend': return m.supertrend(h, l, c, p(10), mu(3));
    case 'psar': return m.psar(h, l, mu(0.02));
    default: return nulls(c.length);
  }
}

/** A single-series built-in applied to an earlier step's output (`resolve_chainable`). */
function resolveChainable(node, src) {
  const p = (d) => (node.period ? node.period : d);
  const f = (d) => (node.fast ? node.fast : d);
  const sl = (d) => (node.slow ? node.slow : d);
  const sp = (d) => (node.signal_period ? node.signal_period : d);
  switch (node.indicator) {
    case 'sma': return m.smaOpt(src, p(20));
    case 'ema': return m.emaOpt(src, p(20));
    case 'dema': return m.demaOpt(src, p(20));
    case 'tema': return m.temaOpt(src, p(20));
    case 'wma': return m.wmaOpt(src, p(20));
    case 'hma': return m.hmaOpt(src, p(20));
    case 'rsi': return m.rsiOpt(src, p(14));
    case 'roc': return m.rocOpt(src, p(12));
    case 'momentum': return m.momentumOpt(src, p(10));
    case 'stddev': return m.stddevOpt(src, p(20));
    case 'macd': return m.macdLineOpt(src, f(12), sl(26));
    case 'macd_signal': return m.emaOpt(m.macdLineOpt(src, f(12), sl(26)), sp(9));
    case 'macd_hist': {
      const line = m.macdLineOpt(src, f(12), sl(26));
      const sig = m.emaOpt(line, sp(9));
      return line.map((x, i) => (x != null && sig[i] != null ? x - sig[i] : null));
    }
    default: return nulls(src.length);
  }
}

// ── Transform nodes (same semantics as custom.rs) ──

const zip2 = (a, b, f) => a.map((x, i) => (x != null && b[i] != null ? f(x, b[i]) : null));
const map1 = (a, f) => a.map((x) => (x != null ? f(x) : null));
const shift = (a, k) => a.map((_, i) => (i >= k ? a[i - k] : null));
const change = (a, k) =>
  a.map((_, i) => (i >= k && a[i] != null && a[i - k] != null ? a[i] - a[i - k] : null));

function rollingMean(a, period) {
  return a.map((_, i) => {
    if (i + 1 < period) return null;
    let sum = 0;
    for (let j = i + 1 - period; j <= i; j++) {
      if (a[j] == null) return null;
      sum += a[j];
    }
    return sum / period;
  });
}

/** EMA seeded on the first defined value (custom.rs `ema_of`, not the catalog's `ema`). */
function emaOf(a, period) {
  const alpha = 2 / (period + 1);
  const out = nulls(a.length);
  let prev = null;
  for (let i = 0; i < a.length; i++) {
    if (a[i] == null) continue;
    prev = prev == null ? a[i] : alpha * a[i] + (1 - alpha) * prev;
    out[i] = prev;
  }
  return out;
}

function rollingExt(a, period, hi) {
  return a.map((_, i) => {
    if (i + 1 < period) return null;
    let best = hi ? -Infinity : Infinity;
    for (let j = i + 1 - period; j <= i; j++) {
      if (a[j] == null) return null;
      best = hi ? Math.max(best, a[j]) : Math.min(best, a[j]);
    }
    return best;
  });
}

/** Where an observation `[period_start_ms, known_ms|null, value]` sits on the time axis: the
 *  instant it can be read unless the node asks for the period (`at: 'period'`). The server
 *  fills `known_ms` (`fundamentals::custom::placed_at`: its release, else the end of its
 *  period), so the period arithmetic has one home. */
export function placeObservation(o, at) {
  return at !== 'period' && o[1] != null ? o[1] : o[0];
}

/** A step over the bars: each bar reads the last point placed at or before its stamp, never
 *  one interpolated or read ahead (`align::fill_points` with `Fill::Step`). Two points on one
 *  stamp: the later in input wins. */
export function stepFill(times, points) {
  const pts = points
    .map((p, i) => [p[0], p[1], i])
    .filter((p) => Number.isFinite(p[1]))
    .sort((a, b) => a[0] - b[0] || a[2] - b[2]);
  const out = new Array(times.length).fill(null);
  let j = 0;
  let held = null;
  for (let i = 0; i < times.length; i++) {
    while (j < pts.length && pts[j][0] <= times[i]) held = pts[j++][1];
    out[i] = held;
  }
  return out;
}

/** A bar reads a point only on the bar it lands on, the first stamped at or after it with no
 *  bar between them, null elsewhere: nothing is held (`align::fill_points` with `Fill::Exact`).
 *  A point older than the first bar lands nowhere. */
export function exactFill(times, points) {
  const pts = points
    .map((p, i) => [p[0], p[1], i])
    .filter((p) => Number.isFinite(p[1]))
    .sort((a, b) => a[0] - b[0] || a[2] - b[2]);
  const out = new Array(times.length).fill(null);
  let j = 0;
  let held = null;
  for (let i = 0; i < times.length; i++) {
    while (j < pts.length && pts[j][0] <= times[i]) held = pts[j++];
    if (held && (held[0] === times[i] || (i > 0 && held[0] > times[i - 1]))) out[i] = held[1];
  }
  return out;
}

/** A `series` node: the observations the caller loaded (`ctx.series[id]`), placed and laid over
 *  the bars, held (`fill: 'step'`, the default) or only where a point lands (`fill: 'exact'`).
 *  Nothing loaded, or bars without stamps: all null, never a guess. */
function seriesNode(node, b, n, ctx) {
  const obs = ctx?.series?.[node.series];
  if (!obs || !Array.isArray(b.t) || b.t.length !== n) return nulls(n);
  const lay = node.fill === 'exact' ? exactFill : stepFill;
  return lay(b.t, obs.map((o) => [placeObservation(o, node.at), o[2]]));
}

function evalNode(node, prev, b, n, ctx) {
  const get = (i) => prev[i] ?? nulls(n);
  const period = Math.max(1, node.period ?? 1);
  switch (node.op) {
    case 'price': return priceSeries(node.field, b);
    case 'const': return new Array(n).fill(node.value ?? 0);
    case 'series': return seriesNode(node, b, n, ctx);
    case 'indicator':
      if (node.src == null) return resolveBuiltin(node, b);
      return isChainableIndicator(node.indicator) ? resolveChainable(node, get(node.src)) : nulls(n);
    case 'add': return zip2(get(node.a), get(node.b), (x, y) => x + y);
    case 'sub': return zip2(get(node.a), get(node.b), (x, y) => x - y);
    case 'mul': return zip2(get(node.a), get(node.b), (x, y) => x * y);
    case 'div': return zip2(get(node.a), get(node.b), (x, y) => (y !== 0 ? x / y : null));
    case 'min': return zip2(get(node.a), get(node.b), Math.min);
    case 'max': return zip2(get(node.a), get(node.b), Math.max);
    case 'abs': return map1(get(node.a), Math.abs);
    case 'neg': return map1(get(node.a), (x) => -x);
    case 'shift': return shift(get(node.a), node.n ?? 0);
    case 'sma_of': return rollingMean(get(node.a), period);
    case 'ema_of': return emaOf(get(node.a), period);
    case 'highest': return rollingExt(get(node.a), period, true);
    case 'lowest': return rollingExt(get(node.a), period, false);
    case 'change': return change(get(node.a), period);
    case 'clamp': return map1(get(node.a), (x) => Math.min(node.hi, Math.max(node.lo, x)));
    default: return nulls(n);
  }
}

/** Evaluate every node once and return the series of the requested output nodes.
 *  `def.outputs` (the builder's multi-output list) wins over the single `def.output`.
 *  An invalid graph yields all-`null` series rather than throwing. `ctx.series` maps a series
 *  id to its observations `[period_start_ms, released_ms|null, value]`, for `series` nodes. */
export function evalDef(def, bars, ctx = null) {
  const n = bars?.c?.length ?? 0;
  const outIdx = def?.outputs?.length ? def.outputs : [def?.output ?? 0];
  if (!n || validateDef(def)) return outIdx.map(() => nulls(n));
  const series = [];
  for (const node of def.nodes) series.push(evalNode(node, series, bars, n, ctx));
  return outIdx.map((i) => series[i] ?? nulls(n));
}
