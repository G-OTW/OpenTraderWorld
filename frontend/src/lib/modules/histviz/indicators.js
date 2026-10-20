/** Indicator math + catalog for the visualization module.
 *
 * Two layers:
 *  1. Pure math fns over number arrays (sma, ema, …) returning arrays aligned to input
 *     length, `null` where lookback is insufficient (ECharts draws gaps for null).
 *  2. A CATALOG of indicator definitions the UI drives. Each def declares its `kind`
 *     ('overlay' on the price pane, or 'oscillator' in its own shared sub-pane), its
 *     editable `params` (with defaults + bounds), and a `compute(bars, params)` that
 *     returns `outputs`: an array of { name, kind:'line'|'bar', data, color?, dashed? }.
 *
 * Custom indicators (the shared node-graph library, `$lib/indicators/`) plug in at the bottom
 * of this file: same output shape, evaluated from their definition instead of a catalog entry.
 *
 * `renko` lives here too as a chart-transform (rebuilds a synthetic bar series). */
import { evalDef } from '$lib/indicators/evaluate.js';
import { evalFormula } from './formula.js';
// A few catalog entries borrow the engine port instead of a second implementation: these are
// the ones a strategy can be written on, so the line drawn here is the line the backtest read.
import {
  tema as temaCalc,
  vwap as vwapCalc,
  adx as adxCalc,
  supertrend as supertrendCalc
} from '$lib/indicators/math.js';

// ── Moving averages ──────────────────────────────────────────────────────────────

export function sma(close, period) {
  const out = new Array(close.length).fill(null);
  let sum = 0;
  for (let i = 0; i < close.length; i++) {
    sum += close[i];
    if (i >= period) sum -= close[i - period];
    if (i >= period - 1) out[i] = sum / period;
  }
  return out;
}

export function ema(close, period) {
  const out = new Array(close.length).fill(null);
  if (close.length < period) return out;
  const k = 2 / (period + 1);
  let prev = close.slice(0, period).reduce((a, b) => a + b, 0) / period;
  out[period - 1] = prev;
  for (let i = period; i < close.length; i++) {
    prev = close[i] * k + prev * (1 - k);
    out[i] = prev;
  }
  return out;
}

/** Double EMA: 2·EMA − EMA(EMA). Reduces lag vs a plain EMA. */
export function dema(close, period) {
  const e1 = ema(close, period);
  // EMA of e1 over its defined tail.
  const start = e1.findIndex((v) => v != null);
  const out = new Array(close.length).fill(null);
  if (start < 0) return out;
  const tail = e1.slice(start).map((v) => v ?? 0);
  const e2tail = ema(tail, period);
  for (let i = 0; i < e2tail.length; i++) {
    if (e2tail[i] != null && e1[start + i] != null) out[start + i] = 2 * e1[start + i] - e2tail[i];
  }
  return out;
}

/** Linearly weighted MA (recent bars weighted highest). */
export function wma(close, period) {
  const out = new Array(close.length).fill(null);
  const denom = (period * (period + 1)) / 2;
  for (let i = period - 1; i < close.length; i++) {
    let s = 0;
    for (let j = 0; j < period; j++) s += close[i - j] * (period - j);
    out[i] = s / denom;
  }
  return out;
}

/** Hull MA: WMA(2·WMA(n/2) − WMA(n), √n) — very low lag. */
export function hma(close, period) {
  const half = Math.max(1, Math.floor(period / 2));
  const sqrtP = Math.max(1, Math.round(Math.sqrt(period)));
  const wHalf = wma(close, half);
  const wFull = wma(close, period);
  const raw = close.map((_, i) =>
    wHalf[i] != null && wFull[i] != null ? 2 * wHalf[i] - wFull[i] : null
  );
  // WMA over the defined tail of `raw`.
  const start = raw.findIndex((v) => v != null);
  const out = new Array(close.length).fill(null);
  if (start < 0) return out;
  const tail = wma(raw.slice(start).map((v) => v ?? 0), sqrtP);
  for (let i = 0; i < tail.length; i++) out[start + i] = tail[i];
  return out;
}

/** Volume-weighted MA over `period` bars. */
export function vwma(close, volume, period) {
  const out = new Array(close.length).fill(null);
  for (let i = period - 1; i < close.length; i++) {
    let pv = 0;
    let vv = 0;
    for (let j = i - period + 1; j <= i; j++) {
      pv += close[j] * volume[j];
      vv += volume[j];
    }
    out[i] = vv ? pv / vv : null;
  }
  return out;
}

// ── Bands / oscillators ──────────────────────────────────────────────────────────

export function bollinger(close, period = 20, mult = 2) {
  const mid = sma(close, period);
  const upper = new Array(close.length).fill(null);
  const lower = new Array(close.length).fill(null);
  for (let i = period - 1; i < close.length; i++) {
    let s = 0;
    for (let j = i - period + 1; j <= i; j++) s += (close[j] - mid[i]) ** 2;
    const sd = Math.sqrt(s / period);
    upper[i] = mid[i] + mult * sd;
    lower[i] = mid[i] - mult * sd;
  }
  return { mid, upper, lower };
}

export function rsi(close, period = 14) {
  const out = new Array(close.length).fill(null);
  if (close.length <= period) return out;
  let gain = 0;
  let loss = 0;
  for (let i = 1; i <= period; i++) {
    const d = close[i] - close[i - 1];
    if (d >= 0) gain += d;
    else loss -= d;
  }
  gain /= period;
  loss /= period;
  out[period] = loss === 0 ? 100 : 100 - 100 / (1 + gain / loss);
  for (let i = period + 1; i < close.length; i++) {
    const d = close[i] - close[i - 1];
    gain = (gain * (period - 1) + (d > 0 ? d : 0)) / period;
    loss = (loss * (period - 1) + (d < 0 ? -d : 0)) / period;
    out[i] = loss === 0 ? 100 : 100 - 100 / (1 + gain / loss);
  }
  return out;
}

export function macd(close, fast = 12, slow = 26, signalP = 9) {
  const ef = ema(close, fast);
  const es = ema(close, slow);
  const line = close.map((_, i) => (ef[i] != null && es[i] != null ? ef[i] - es[i] : null));
  const defined = line.map((v) => (v == null ? 0 : v));
  const sig = new Array(close.length).fill(null);
  const firstIdx = line.findIndex((v) => v != null);
  if (firstIdx >= 0 && close.length - firstIdx >= signalP) {
    const k = 2 / (signalP + 1);
    let prev = defined.slice(firstIdx, firstIdx + signalP).reduce((a, b) => a + b, 0) / signalP;
    sig[firstIdx + signalP - 1] = prev;
    for (let i = firstIdx + signalP; i < close.length; i++) {
      prev = line[i] * k + prev * (1 - k);
      sig[i] = prev;
    }
  }
  const hist = line.map((v, i) => (v != null && sig[i] != null ? v - sig[i] : null));
  return { macd: line, signal: sig, hist };
}

/** Stochastic oscillator: %K (fast) smoothed to %D over high/low/close. */
export function stochastic(high, low, close, kPeriod = 14, dPeriod = 3) {
  const k = new Array(close.length).fill(null);
  for (let i = kPeriod - 1; i < close.length; i++) {
    let hh = -Infinity;
    let ll = Infinity;
    for (let j = i - kPeriod + 1; j <= i; j++) {
      if (high[j] > hh) hh = high[j];
      if (low[j] < ll) ll = low[j];
    }
    k[i] = hh === ll ? 50 : ((close[i] - ll) / (hh - ll)) * 100;
  }
  // %D = SMA of %K over dPeriod (ignoring leading nulls).
  const d = new Array(close.length).fill(null);
  for (let i = kPeriod - 1 + dPeriod - 1; i < close.length; i++) {
    let s = 0;
    let ok = true;
    for (let j = i - dPeriod + 1; j <= i; j++) {
      if (k[j] == null) ok = false;
      else s += k[j];
    }
    if (ok) d[i] = s / dPeriod;
  }
  return { k, d };
}

/** Average True Range (Wilder smoothing) over high/low/close. */
export function atr(high, low, close, period = 14) {
  const out = new Array(close.length).fill(null);
  if (close.length <= period) return out;
  const tr = new Array(close.length).fill(0);
  tr[0] = high[0] - low[0];
  for (let i = 1; i < close.length; i++) {
    tr[i] = Math.max(
      high[i] - low[i],
      Math.abs(high[i] - close[i - 1]),
      Math.abs(low[i] - close[i - 1])
    );
  }
  let prev = tr.slice(1, period + 1).reduce((a, b) => a + b, 0) / period;
  out[period] = prev;
  for (let i = period + 1; i < close.length; i++) {
    prev = (prev * (period - 1) + tr[i]) / period;
    out[i] = prev;
  }
  return out;
}

/** Rate of Change (%) vs `period` bars ago. */
export function roc(close, period = 12) {
  const out = new Array(close.length).fill(null);
  for (let i = period; i < close.length; i++) {
    const base = close[i - period];
    out[i] = base ? ((close[i] - base) / base) * 100 : null;
  }
  return out;
}

/** Momentum: close − close[period ago]. */
export function momentum(close, period = 10) {
  const out = new Array(close.length).fill(null);
  for (let i = period; i < close.length; i++) out[i] = close[i] - close[i - period];
  return out;
}

/** Williams %R over high/low/close (range −100..0). */
export function williamsR(high, low, close, period = 14) {
  const out = new Array(close.length).fill(null);
  for (let i = period - 1; i < close.length; i++) {
    let hh = -Infinity;
    let ll = Infinity;
    for (let j = i - period + 1; j <= i; j++) {
      if (high[j] > hh) hh = high[j];
      if (low[j] < ll) ll = low[j];
    }
    out[i] = hh === ll ? -50 : ((hh - close[i]) / (hh - ll)) * -100;
  }
  return out;
}

/** Commodity Channel Index over typical price. */
export function cci(high, low, close, period = 20) {
  const tp = close.map((_, i) => (high[i] + low[i] + close[i]) / 3);
  const out = new Array(close.length).fill(null);
  for (let i = period - 1; i < close.length; i++) {
    let mean = 0;
    for (let j = i - period + 1; j <= i; j++) mean += tp[j];
    mean /= period;
    let md = 0;
    for (let j = i - period + 1; j <= i; j++) md += Math.abs(tp[j] - mean);
    md /= period;
    out[i] = md ? (tp[i] - mean) / (0.015 * md) : 0;
  }
  return out;
}

/** On-Balance Volume (cumulative signed volume). */
export function obv(close, volume) {
  const out = new Array(close.length).fill(null);
  let run = 0;
  out[0] = 0;
  for (let i = 1; i < close.length; i++) {
    if (close[i] > close[i - 1]) run += volume[i];
    else if (close[i] < close[i - 1]) run -= volume[i];
    out[i] = run;
  }
  return out;
}

/** Money Flow Index (volume-weighted RSI) over high/low/close/volume. */
export function mfi(high, low, close, volume, period = 14) {
  const tp = close.map((_, i) => (high[i] + low[i] + close[i]) / 3);
  const out = new Array(close.length).fill(null);
  for (let i = period; i < close.length; i++) {
    let pos = 0;
    let neg = 0;
    for (let j = i - period + 1; j <= i; j++) {
      const flow = tp[j] * volume[j];
      if (tp[j] > tp[j - 1]) pos += flow;
      else if (tp[j] < tp[j - 1]) neg += flow;
    }
    out[i] = neg === 0 ? 100 : 100 - 100 / (1 + pos / neg);
  }
  return out;
}

/** TRIX: 1-period % change of a triple-smoothed EMA. */
export function trix(close, period = 15) {
  const smooth = (arr) => {
    const e = ema(arr.map((v) => v ?? 0), period);
    return e;
  };
  const e3 = smooth(smooth(smooth(close)));
  const out = new Array(close.length).fill(null);
  for (let i = 1; i < close.length; i++) {
    if (e3[i] != null && e3[i - 1]) out[i] = ((e3[i] - e3[i - 1]) / e3[i - 1]) * 100;
  }
  return out;
}

/** Rolling standard deviation of close. */
export function stddev(close, period = 20) {
  const out = new Array(close.length).fill(null);
  for (let i = period - 1; i < close.length; i++) {
    let mean = 0;
    for (let j = i - period + 1; j <= i; j++) mean += close[j];
    mean /= period;
    let s = 0;
    for (let j = i - period + 1; j <= i; j++) s += (close[j] - mean) ** 2;
    out[i] = Math.sqrt(s / period);
  }
  return out;
}

/** Keltner Channels: EMA(close) ± mult·ATR. */
export function keltner(high, low, close, period = 20, mult = 2) {
  const mid = ema(close, period);
  const a = atr(high, low, close, period);
  const upper = new Array(close.length).fill(null);
  const lower = new Array(close.length).fill(null);
  for (let i = 0; i < close.length; i++) {
    if (mid[i] != null && a[i] != null) {
      upper[i] = mid[i] + mult * a[i];
      lower[i] = mid[i] - mult * a[i];
    }
  }
  return { mid, upper, lower };
}

/** Donchian Channels: rolling highest-high / lowest-low. */
export function donchian(high, low, period = 20) {
  const upper = new Array(high.length).fill(null);
  const lower = new Array(high.length).fill(null);
  const mid = new Array(high.length).fill(null);
  for (let i = period - 1; i < high.length; i++) {
    let hh = -Infinity;
    let ll = Infinity;
    for (let j = i - period + 1; j <= i; j++) {
      if (high[j] > hh) hh = high[j];
      if (low[j] < ll) ll = low[j];
    }
    upper[i] = hh;
    lower[i] = ll;
    mid[i] = (hh + ll) / 2;
  }
  return { upper, lower, mid };
}

/** Parabolic SAR (Wilder) over high/low. `step`/`max` are the acceleration factor bounds. */
export function psar(high, low, step = 0.02, max = 0.2) {
  const n = high.length;
  const out = new Array(n).fill(null);
  if (n < 2) return out;
  let bull = true;
  let af = step;
  let ep = high[0];
  let sar = low[0];
  for (let i = 1; i < n; i++) {
    sar = sar + af * (ep - sar);
    if (bull) {
      if (low[i] < sar) {
        bull = false;
        sar = ep;
        ep = low[i];
        af = step;
      } else if (high[i] > ep) {
        ep = high[i];
        af = Math.min(max, af + step);
      }
    } else {
      if (high[i] > sar) {
        bull = true;
        sar = ep;
        ep = high[i];
        af = step;
      } else if (low[i] < ep) {
        ep = low[i];
        af = Math.min(max, af + step);
      }
    }
    out[i] = sar;
  }
  return out;
}

// ── Renko chart transform ────────────────────────────────────────────────────────

export function suggestBrick(close) {
  if (close.length < 2) return 1;
  const moves = [];
  for (let i = 1; i < close.length; i++) moves.push(Math.abs(close[i] - close[i - 1]));
  moves.sort((a, b) => a - b);
  const med = moves[Math.floor(moves.length / 2)] || 1;
  const scale = 10 ** Math.floor(Math.log10(med));
  return Math.max(scale, Math.round((med / scale) * 2) * scale) || 1;
}

export function renko(ts, close, brick) {
  const out = { ts: [], o: [], h: [], l: [], c: [] };
  if (!brick || brick <= 0 || !close.length) return out;
  let base = close[0];
  for (let i = 1; i < close.length; i++) {
    const price = close[i];
    while (price - base >= brick) {
      out.ts.push(ts[i]);
      out.o.push(base);
      out.c.push(base + brick);
      out.l.push(base);
      out.h.push(base + brick);
      base += brick;
    }
    while (base - price >= brick) {
      out.ts.push(ts[i]);
      out.o.push(base);
      out.c.push(base - brick);
      out.h.push(base);
      out.l.push(base - brick);
      base -= brick;
    }
  }
  return out;
}

// ── Chart types ────────────────────────────────────────────────────────────────

/** Plot styles, grouped as the picker lists them. Every one is drawn from OHLCV alone. */
export const CHART_TYPE_GROUPS = [
  ['bars', ['ohlc', 'candlestick', 'hollow', 'volcandles']],
  ['lines', ['line', 'linemarkers', 'step']],
  ['areas', ['area', 'hlcarea', 'baseline']],
  ['columns', ['columns', 'highlow']],
  ['transforms', ['heikinashi', 'renko', 'linebreak', 'kagi', 'pnf', 'range']]
];

/** Types that close on price rather than on the clock: no countdown, no comparison, no
 *  log scale, no volume. */
export const PRICE_DRIVEN = new Set(['renko', 'linebreak', 'kagi', 'pnf', 'range']);

/** Types that need a size (brick, reversal, box, range), stored on the pane as `brick`. */
export const SIZED_TYPES = new Set(['renko', 'kagi', 'pnf', 'range']);

/** Types whose price series is one close line (legend icon, line colour). */
export const LINE_TYPES = new Set(['line', 'linemarkers', 'step', 'area', 'baseline']);

/** The series a chart type draws from: the bars, or the transform it implies. */
export function chartView(type, bars, size) {
  if (!bars) return null;
  const withVol = (r) => ({ ...r, v: r.v ?? r.ts.map(() => 0) });
  switch (type) {
    case 'heikinashi':
      return heikinAshi(bars);
    case 'renko':
      return withVol(renko(bars.ts, bars.c, size));
    case 'linebreak':
      return lineBreak(bars.ts, bars.c, 3);
    case 'kagi':
      return kagi(bars.ts, bars.c, size);
    case 'pnf':
      return pointFigure(bars.ts, bars.c, size, 3);
    case 'range':
      return rangeBars(bars.ts, bars.c, size);
    default:
      return bars;
  }
}

// ── Other chart transforms ─────────────────────────────────────────────────────
// Each returns { ts, o, h, l, c, v } like a bar series, so indicators, tags and the legend
// read them the way they read real bars. All but Heikin Ashi close on price, not on the
// clock: their `ts` is the bar where the move completed, and they carry no volume.

/** Heikin Ashi: averaged bars, one per source bar. */
export function heikinAshi(bars) {
  const n = bars.c.length;
  const out = { ts: bars.ts, o: new Array(n), h: new Array(n), l: new Array(n), c: new Array(n), v: bars.v };
  for (let i = 0; i < n; i++) {
    const c = (bars.o[i] + bars.h[i] + bars.l[i] + bars.c[i]) / 4;
    const o = i === 0 ? (bars.o[i] + bars.c[i]) / 2 : (out.o[i - 1] + out.c[i - 1]) / 2;
    out.o[i] = o;
    out.c[i] = c;
    out.h[i] = Math.max(bars.h[i], o, c);
    out.l[i] = Math.min(bars.l[i], o, c);
  }
  return out;
}

const emptyBars = () => ({ ts: [], o: [], h: [], l: [], c: [], v: [] });
function pushBar(out, ts, o, c, h = Math.max(o, c), l = Math.min(o, c)) {
  out.ts.push(ts);
  out.o.push(o);
  out.c.push(c);
  out.h.push(h);
  out.l.push(l);
  out.v.push(0);
}

/** Line break (3 lines by default): a new line only when the close beats the last line; a
 *  reversal needs it to beat the extreme of the last `lines` lines. */
export function lineBreak(ts, close, lines = 3) {
  const out = emptyBars();
  if (close.length < 2) return out;
  let i = 1;
  while (i < close.length && close[i] === close[0]) i++;
  if (i >= close.length) return out;
  pushBar(out, ts[i], close[0], close[i]);
  for (i++; i < close.length; i++) {
    const p = close[i];
    const k = out.c.length - 1;
    const up = out.c[k] > out.o[k];
    const hi = out.h[k];
    const lo = out.l[k];
    const from = Math.max(0, k - lines + 1);
    if (up) {
      if (p > hi) pushBar(out, ts[i], hi, p);
      else if (p < Math.min(...out.l.slice(from))) pushBar(out, ts[i], lo, p);
    } else if (p < lo) pushBar(out, ts[i], lo, p);
    else if (p > Math.max(...out.h.slice(from))) pushBar(out, ts[i], hi, p);
  }
  return out;
}

/** Range bars on closes: every bar spans exactly `size` from low to high; the last one is
 *  still forming. */
export function rangeBars(ts, close, size) {
  const out = emptyBars();
  if (!size || size <= 0 || !close.length) return out;
  let o = close[0];
  let hi = o;
  let lo = o;
  for (let i = 1; i < close.length; i++) {
    const p = close[i];
    while (p > lo + size) {
      pushBar(out, ts[i], o, lo + size, lo + size, lo);
      o = lo + size;
      hi = lo = o;
    }
    while (p < hi - size) {
      pushBar(out, ts[i], o, hi - size, hi, hi - size);
      o = hi - size;
      hi = lo = o;
    }
    hi = Math.max(hi, p);
    lo = Math.min(lo, p);
  }
  pushBar(out, ts[close.length - 1], o, close[close.length - 1], hi, lo);
  return out;
}

/** Kagi on closes, reversal by `size`. One bar per vertical stroke (o → c), plus `kagi`:
 *  the stroke's thickness before and after it crosses the previous shoulder/waist
 *  (`yang` thick, `yin` thin) and the level where it switches (null: no switch). */
export function kagi(ts, close, size) {
  const out = { ...emptyBars(), kagi: { before: [], after: [], split: [] } };
  if (!size || size <= 0 || close.length < 2) return out;
  let start = close[0];
  let end = start;
  let dir = 0;
  let at = ts[0];
  const strokes = [];
  for (let i = 1; i < close.length; i++) {
    const p = close[i];
    if (!dir) {
      if (Math.abs(p - start) >= size) {
        dir = p > start ? 1 : -1;
        end = p;
        at = ts[i];
      }
      continue;
    }
    if ((dir > 0 && p > end) || (dir < 0 && p < end)) {
      end = p;
      at = ts[i];
    } else if (Math.abs(end - p) >= size) {
      strokes.push({ o: start, c: end, ts: at });
      start = end;
      end = p;
      dir = -dir;
      at = ts[i];
    }
  }
  if (dir) strokes.push({ o: start, c: end, ts: at });
  // Yang above the last shoulder, yin below the last waist; the first stroke sets the tone.
  let state = strokes[0] && strokes[0].c > strokes[0].o ? 'yang' : 'yin';
  let shoulder = null;
  let waist = null;
  for (const s of strokes) {
    const up = s.c > s.o;
    const before = state;
    let split = null;
    if (up && state === 'yin' && shoulder != null && s.c > shoulder) {
      state = 'yang';
      split = shoulder;
    } else if (!up && state === 'yang' && waist != null && s.c < waist) {
      state = 'yin';
      split = waist;
    }
    if (up) shoulder = s.c;
    else waist = s.c;
    pushBar(out, s.ts, s.o, s.c);
    out.kagi.before.push(before);
    out.kagi.after.push(state);
    out.kagi.split.push(split);
  }
  return out;
}

/** Point & figure on closes: box `size`, reversal of `reversal` boxes. One bar per column,
 *  low edge of its bottom box to high edge of its top box; `pnf.box` keeps the box size. */
export function pointFigure(ts, close, size, reversal = 3) {
  const out = { ...emptyBars(), pnf: { box: size } };
  if (!size || size <= 0 || close.length < 2) return out;
  const box = (p) => Math.floor(p / size);
  const s = box(close[0]);
  let col = null; // { up, top, bottom, ts }
  const cols = [];
  for (let i = 1; i < close.length; i++) {
    const k = box(close[i]);
    if (!col) {
      if (k > s) col = { up: true, bottom: s, top: k, ts: ts[i] };
      else if (k < s) col = { up: false, top: s, bottom: k, ts: ts[i] };
      continue;
    }
    if (col.up) {
      if (k > col.top) {
        col.top = k;
        col.ts = ts[i];
      } else if (k <= col.top - reversal) {
        cols.push(col);
        col = { up: false, top: col.top - 1, bottom: k, ts: ts[i] };
      }
    } else if (k < col.bottom) {
      col.bottom = k;
      col.ts = ts[i];
    } else if (k >= col.bottom + reversal) {
      cols.push(col);
      col = { up: true, bottom: col.bottom + 1, top: k, ts: ts[i] };
    }
  }
  if (col) cols.push(col);
  for (const c of cols) {
    const lo = c.bottom * size;
    const hi = (c.top + 1) * size;
    if (c.up) pushBar(out, c.ts, lo, hi);
    else pushBar(out, c.ts, hi, lo);
  }
  return out;
}

// ── Price source ───────────────────────────────────────────────────────────────

/** Selectable input series for single-source indicators (MAs, RSI, MACD, …). OHLC-based
 *  indicators (ATR, Stochastic, CCI, …) ignore this — they need the full bar. */
export const PRICE_SOURCES = [
  { key: 'close', label: 'Close' },
  { key: 'open', label: 'Open' },
  { key: 'high', label: 'High' },
  { key: 'low', label: 'Low' },
  { key: 'hl2', label: 'HL2 (median)' },
  { key: 'hlc3', label: 'HLC3 (typical)' },
  { key: 'ohlc4', label: 'OHLC4 (average)' },
  { key: 'volume', label: 'Volume' }
];

/** Derived sources: another instance's output (`ind:<instance id>:<output index>`) or a
 *  formula (`expr:<formula>`, see formula.js). */
export const IND = 'ind:';
export const EXPR = 'expr:';
export const isDerivedSource = (key) =>
  typeof key === 'string' && (key.startsWith(IND) || key.startsWith(EXPR));
/** The instance id an `ind:` source reads from, or null. */
export const sourceInstance = (key) =>
  typeof key === 'string' && key.startsWith(IND) ? key.slice(IND.length).split(':')[0] : null;

/** Resolve a price-source key to a number array aligned to the bars. Defaults to close. */
export function priceSource(b, key) {
  switch (key) {
    case 'open':
      return b.o;
    case 'high':
      return b.h;
    case 'low':
      return b.l;
    case 'hl2':
      return b.c.map((_, i) => (b.h[i] + b.l[i]) / 2);
    case 'hlc3':
      return b.c.map((_, i) => (b.h[i] + b.l[i] + b.c[i]) / 3);
    case 'ohlc4':
      return b.c.map((_, i) => (b.o[i] + b.h[i] + b.l[i] + b.c[i]) / 4);
    case 'volume':
      return b.v ?? b.c.map(() => 0);
    case 'close':
      return b.c;
    default:
      if (typeof key === 'string' && key.startsWith(IND)) {
        return b.ext?.get(key) ?? b.c.map(() => null);
      }
      if (typeof key === 'string' && key.startsWith(EXPR)) return evalFormula(key.slice(EXPR.length), b);
      return b.c;
  }
}

/** " (HLC3)", " (RSI 14)", " (ƒ my formula)": what a non-close source adds to a name. */
const srcLabel = (params) => {
  const key = params?.source;
  if (!key || key === 'close') return '';
  if (isDerivedSource(key)) return ` (${params.sourceName || (key.startsWith(EXPR) ? 'ƒ' : '?')})`;
  return ` (${PRICE_SOURCES.find((s) => s.key === key)?.label.split(' ')[0] ?? key})`;
};

// ── Catalog (UI-driven) ──────────────────────────────────────────────────────────

/** A numeric param spec: { key, label, default, min, max, step }. */
const P = (key, label, def, min = 1, max = 500, step = 1) => ({ key, label, default: def, min, max, step });

/** Catalog of indicators. `compute(bars, params)` → array of outputs:
 *  { name, kind:'line'|'bar', data:number|null[], color?, dashed? }. `color` keys index
 *  into the chart palette (theme colors); omitted falls through to a default rotation. */
export const CATALOG = [
  {
    type: 'sma',
    label: 'SMA — Simple MA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `SMA ${p.period}${srcLabel(p)}`, kind: 'line', data: sma(priceSource(b, p.source), p.period), color: 'amber' }
    ]
  },
  {
    type: 'ema',
    label: 'EMA — Exponential MA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 50)],
    compute: (b, p) => [
      { name: `EMA ${p.period}${srcLabel(p)}`, kind: 'line', data: ema(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'dema',
    label: 'DEMA — Double EMA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `DEMA ${p.period}${srcLabel(p)}`, kind: 'line', data: dema(priceSource(b, p.source), p.period), color: 'green' }
    ]
  },
  {
    type: 'wma',
    label: 'WMA — Weighted MA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `WMA ${p.period}${srcLabel(p)}`, kind: 'line', data: wma(priceSource(b, p.source), p.period), color: 'red' }
    ]
  },
  {
    type: 'hma',
    label: 'HMA — Hull MA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `HMA ${p.period}${srcLabel(p)}`, kind: 'line', data: hma(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'tema',
    label: 'TEMA (Triple EMA)',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `TEMA ${p.period}${srcLabel(p)}`, kind: 'line', data: temaCalc(priceSource(b, p.source), p.period), color: 'green' }
    ]
  },
  {
    type: 'vwap',
    label: 'VWAP (rolling)',
    kind: 'overlay',
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `VWAP ${p.period}`, kind: 'line', data: vwapCalc(b.h, b.l, b.c, b.v, p.period), color: 'amber' }
    ]
  },
  {
    type: 'supertrend',
    label: 'SuperTrend',
    kind: 'overlay',
    params: [P('period', 'Period', 10), P('mult', 'ATR mult', 3, 0.1, 10, 0.1)],
    compute: (b, p) => [
      { name: `SuperTrend ${p.period},${p.mult}`, kind: 'line', data: supertrendCalc(b.h, b.l, b.c, p.period, p.mult), color: 'accent' }
    ]
  },
  {
    type: 'vwma',
    label: 'VWMA — Volume-Weighted MA',
    kind: 'overlay',
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `VWMA ${p.period}`, kind: 'line', data: vwma(b.c, b.v, p.period), color: 'amber' }
    ]
  },
  {
    type: 'bollinger',
    label: 'Bollinger Bands',
    kind: 'overlay',
    sourceable: true,
    fillable: true,
    params: [P('period', 'Period', 20), P('mult', 'StdDev mult', 2, 1, 5, 0.5)],
    compute: (b, p) => {
      const bb = bollinger(priceSource(b, p.source), p.period, p.mult);
      return [
        { name: 'BB up', kind: 'line', data: bb.upper, color: 'muted', dashed: true, role: 'upper' },
        { name: 'BB mid', kind: 'line', data: bb.mid, color: 'muted', role: 'mid' },
        { name: 'BB low', kind: 'line', data: bb.lower, color: 'muted', dashed: true, role: 'lower' }
      ];
    }
  },
  {
    type: 'keltner',
    label: 'Keltner Channels',
    kind: 'overlay',
    fillable: true,
    params: [P('period', 'Period', 20), P('mult', 'ATR mult', 2, 1, 5, 0.5)],
    compute: (b, p) => {
      const k = keltner(b.h, b.l, b.c, p.period, p.mult);
      return [
        { name: 'KC up', kind: 'line', data: k.upper, color: 'muted', dashed: true, role: 'upper' },
        { name: 'KC mid', kind: 'line', data: k.mid, color: 'muted', role: 'mid' },
        { name: 'KC low', kind: 'line', data: k.lower, color: 'muted', dashed: true, role: 'lower' }
      ];
    }
  },
  {
    type: 'donchian',
    label: 'Donchian Channels',
    kind: 'overlay',
    fillable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => {
      const d = donchian(b.h, b.l, p.period);
      return [
        { name: 'DC up', kind: 'line', data: d.upper, color: 'muted', dashed: true, role: 'upper' },
        { name: 'DC mid', kind: 'line', data: d.mid, color: 'muted', role: 'mid' },
        { name: 'DC low', kind: 'line', data: d.lower, color: 'muted', dashed: true, role: 'lower' }
      ];
    }
  },
  {
    type: 'psar',
    label: 'Parabolic SAR',
    kind: 'overlay',
    params: [P('step', 'Step', 0.02, 0.001, 0.2, 0.001), P('max', 'Max step', 0.2, 0.01, 1, 0.01)],
    compute: (b, p) => [
      { name: 'PSAR', kind: 'scatter', data: psar(b.h, b.l, p.step, p.max), color: 'accent' }
    ]
  },
  {
    type: 'rsi',
    label: 'RSI',
    kind: 'oscillator',
    sourceable: true,
    params: [P('period', 'Period', 14)],
    compute: (b, p) => [
      { name: `RSI ${p.period}${srcLabel(p)}`, kind: 'line', data: rsi(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'macd',
    label: 'MACD',
    kind: 'oscillator',
    sourceable: true,
    params: [P('fast', 'Fast', 12), P('slow', 'Slow', 26), P('signal', 'Signal', 9)],
    compute: (b, p) => {
      const m = macd(priceSource(b, p.source), p.fast, p.slow, p.signal);
      return [
        { name: 'Hist', kind: 'bar', data: m.hist, color: 'updown' },
        { name: 'MACD', kind: 'line', data: m.macd, color: 'accent' },
        { name: 'Signal', kind: 'line', data: m.signal, color: 'amber' }
      ];
    }
  },
  {
    type: 'stochastic',
    label: 'Stochastic',
    kind: 'oscillator',
    params: [P('k', '%K period', 14), P('d', '%D period', 3)],
    compute: (b, p) => {
      const s = stochastic(b.h, b.l, b.c, p.k, p.d);
      return [
        { name: '%K', kind: 'line', data: s.k, color: 'accent' },
        { name: '%D', kind: 'line', data: s.d, color: 'amber' }
      ];
    }
  },
  {
    type: 'atr',
    label: 'ATR — Average True Range',
    kind: 'oscillator',
    params: [P('period', 'Period', 14)],
    compute: (b, p) => [{ name: `ATR ${p.period}`, kind: 'line', data: atr(b.h, b.l, b.c, p.period), color: 'accent' }]
  },
  {
    type: 'adx',
    label: 'ADX (Trend strength)',
    kind: 'oscillator',
    params: [P('period', 'Period', 14)],
    compute: (b, p) => [
      { name: `ADX ${p.period}`, kind: 'line', data: adxCalc(b.h, b.l, b.c, p.period), color: 'accent' }
    ]
  },
  {
    type: 'stddev',
    label: 'Std Deviation',
    kind: 'oscillator',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `StdDev ${p.period}${srcLabel(p)}`, kind: 'line', data: stddev(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'roc',
    label: 'ROC — Rate of Change',
    kind: 'oscillator',
    sourceable: true,
    params: [P('period', 'Period', 12)],
    compute: (b, p) => [
      { name: `ROC ${p.period}${srcLabel(p)}`, kind: 'line', data: roc(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'momentum',
    label: 'Momentum',
    kind: 'oscillator',
    sourceable: true,
    params: [P('period', 'Period', 10)],
    compute: (b, p) => [
      { name: `MOM ${p.period}${srcLabel(p)}`, kind: 'line', data: momentum(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'williamsr',
    label: 'Williams %R',
    kind: 'oscillator',
    params: [P('period', 'Period', 14)],
    compute: (b, p) => [
      { name: `%R ${p.period}`, kind: 'line', data: williamsR(b.h, b.l, b.c, p.period), color: 'accent' }
    ]
  },
  {
    type: 'cci',
    label: 'CCI — Commodity Channel Index',
    kind: 'oscillator',
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [{ name: `CCI ${p.period}`, kind: 'line', data: cci(b.h, b.l, b.c, p.period), color: 'accent' }]
  },
  {
    type: 'trix',
    label: 'TRIX',
    kind: 'oscillator',
    sourceable: true,
    params: [P('period', 'Period', 15)],
    compute: (b, p) => [
      { name: `TRIX ${p.period}${srcLabel(p)}`, kind: 'line', data: trix(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'obv',
    label: 'OBV — On-Balance Volume',
    kind: 'oscillator',
    params: [],
    compute: (b) => [{ name: 'OBV', kind: 'line', data: obv(b.c, b.v), color: 'accent' }]
  },
  {
    type: 'mfi',
    label: 'MFI — Money Flow Index',
    kind: 'oscillator',
    params: [P('period', 'Period', 14)],
    compute: (b, p) => [
      { name: `MFI ${p.period}`, kind: 'line', data: mfi(b.h, b.l, b.c, b.v, p.period), color: 'accent' }
    ]
  }
];

/** Default per-instance style overrides ('' / 0 mean "use the output's own color/width"). */
export const DEFAULT_STYLE = { color: '', fill: '', width: 0, lineType: '' };

export function catalogDef(type) {
  return CATALOG.find((d) => d.type === type);
}

/** Default params object for a catalog entry. Sourceable indicators seed `source: 'close'`. */
export function defaultParams(type) {
  const def = catalogDef(type);
  const out = {};
  for (const p of def?.params ?? []) out[p.key] = p.default;
  if (def?.sourceable) out.source = 'close';
  return out;
}

/** A short human label for an instance, e.g. "SMA 20" or "MACD (12,26,9)". A non-close
 *  source is appended, e.g. "RSI 14 (HLC3)". */
export function instanceLabel(type, params) {
  const def = catalogDef(type);
  if (!def) return type;
  const vals = def.params.map((p) => params[p.key]).join(',');
  const base =
    def.params.length === 1 ? `${def.type.toUpperCase()} ${vals}` : `${def.type.toUpperCase()} (${vals})`;
  return base + srcLabel(params);
}

// ── Custom indicators (the shared node-graph library) ────────────────────────────
//
// A custom instance carries the definition itself instead of a catalog type:
//   { type: 'custom', pane: 'overlay'|'pane', custom: { id, name, def } }
// `id` is the library row it came from (null once the row is gone — the embedded def keeps
// the chart drawing). Unlike catalog entries, which pane it lives in is the user's choice,
// so every pane decision goes through `paneOf` rather than the catalog's `kind`.

export const CUSTOM_TYPE = 'custom';
export const isCustom = (inst) => inst?.type === CUSTOM_TYPE;

// ── Script indicators (Pine Script v5/v6 compatible, run by the WASM engine) ─────────────
//
//   { type: 'script', pane: 'overlay'|'pane', script: { id, name, source, inputs, hidden } }
// `source` is embedded like a DAG's definition (the chart still draws when the library row is
// gone) and refreshed from the row on load. `inputs` are raw form values by input key,
// `hidden` the plot indices the user turned off. The run happens off the main thread
// (`$lib/script/runs.svelte.js`); its plots reach `computeAll` through `scriptOut`.

export const SCRIPT_TYPE = 'script';
export const isScript = (inst) => inst?.type === SCRIPT_TYPE;

/** Instances with their script refreshed from the library rows: name and source from the
 *  row, or `script.id = null` when the row is gone (the embedded source keeps drawing). */
export function refreshScriptInstances(instances, rows) {
  return instances.map((i) => {
    if (!isScript(i) || !i.script?.id) return i;
    const row = rows.find((r) => r.id === i.script.id);
    if (!row) return { ...i, script: { ...i.script, id: null } };
    return { ...i, script: { ...i.script, name: row.name, source: row.source ?? i.script.source } };
  });
}

/** Which pane an instance draws in: 'overlay' (price pane) or 'oscillator' (its own). */
export function paneOf(inst) {
  if (isCustom(inst) || isScript(inst)) return inst.pane === 'overlay' ? 'overlay' : 'oscillator';
  // A catalog study fed by a derived source names its pane: the price one, its own, or the
  // pane of the study it reads (`src:<pane key>`), so an SMA of an RSI sits on the RSI.
  if (inst?.pane) return inst.pane === 'overlay' ? 'overlay' : 'oscillator';
  return catalogDef(inst?.type)?.kind === 'overlay' ? 'overlay' : 'oscillator';
}

/** Sub-pane identity. Catalog oscillators of the same type share one pane (RSI over RSI);
 *  every custom indicator gets its own, since two definitions rarely share a scale. */
export function paneKey(inst) {
  if (isCustom(inst)) return `custom:${inst.id}`;
  if (isScript(inst)) return `script:${inst.id}`;
  if (typeof inst?.pane === 'string' && inst.pane.startsWith('src:')) return inst.pane.slice(4);
  if (inst?.pane === 'pane') return `own:${inst.id}`;
  return inst.type;
}

/** Series for one instance, in the chart's output shape. Catalog entries come from their
 *  `compute`; custom ones are evaluated from their DAG (same math as the backtest engine). */
export function computeInstance(inst, bars) {
  if (isCustom(inst)) {
    const def = inst.custom?.def;
    if (!def?.nodes?.length) return [];
    const name = inst.custom?.name || 'custom';
    return evalDef(def, bars).map((data, i, all) => ({
      name: all.length > 1 ? `${name} ${i + 1}` : name,
      kind: 'line',
      data,
      color: 'accent'
    }));
  }
  const def = catalogDef(inst.type);
  if (!def) return [];
  const src = inst.params?.source;
  if (!def.sourceable || !isDerivedSource(src)) return def.compute(bars, inst.params) ?? [];
  // A derived source starts late (an RSI has no value for its first bars) and the catalog
  // math expects a dense series: compute over its defined tail, gaps forward-filled, then
  // put the leading bars back as nulls.
  const series = priceSource(bars, src);
  const start = series.findIndex((x) => x != null && Number.isFinite(x));
  if (start < 0) return [];
  let last = series[start];
  const dense = series.slice(start).map((x) => (x != null && Number.isFinite(x) ? (last = x) : last));
  const cut = (a) => (Array.isArray(a) ? a.slice(start) : a);
  const tail = {
    ...bars,
    ts: cut(bars.ts),
    o: cut(bars.o),
    h: cut(bars.h),
    l: cut(bars.l),
    c: cut(bars.c),
    v: cut(bars.v),
    ext: new Map([[DENSE, dense]])
  };
  const sourceName = inst.params.sourceName ?? (src.startsWith(EXPR) ? 'ƒ' : '?');
  const outs = def.compute(tail, { ...inst.params, source: DENSE, sourceName }) ?? [];
  const pad = new Array(start).fill(null);
  return outs.map((o) => ({ ...o, data: [...pad, ...o.data] }));
}

/** The pre-resolved series a derived-source study is computed on (see computeInstance). */
const DENSE = `${IND}__dense`;

/** Every instance's outputs, keyed by instance id. A study reading another study's output
 *  is computed after it; a chain that never resolves (a cycle, a deleted source) reads nulls
 *  and draws nothing rather than looping. */
export function computeAll(instances, bars, scriptOut = new Map()) {
  const out = new Map();
  if (!bars) return out;
  const ext = new Map();
  const b = { ...bars, ext };
  const publish = (inst, outs) => {
    out.set(inst.id, outs);
    outs.forEach((o, k) => ext.set(`${IND}${inst.id}:${k}`, o.data));
  };
  // A script's outputs come from its last worker run, not from a synchronous compute.
  const outputsOf = (inst, b) => (isScript(inst) ? (scriptOut.get(inst.id) ?? []) : (computeInstance(inst, b) ?? []));
  let pending = [...instances];
  for (let pass = 0; pass < 6 && pending.length; pass++) {
    const next = [];
    for (const inst of pending) {
      const from = sourceInstance(inst.params?.source);
      if (from != null && !out.has(from) && !out.has(Number(from)) && pending.some((x) => String(x.id) === from)) {
        next.push(inst);
        continue;
      }
      publish(inst, outputsOf(inst, b));
    }
    if (next.length === pending.length) {
      for (const inst of next) publish(inst, outputsOf(inst, b));
      break;
    }
    pending = next;
  }
  for (const inst of pending) if (!out.has(inst.id)) publish(inst, outputsOf(inst, b));
  return out;
}
