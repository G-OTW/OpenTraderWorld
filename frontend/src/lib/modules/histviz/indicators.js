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
 * `renko` lives here too as a chart-transform (rebuilds a synthetic bar series). */

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
  { key: 'ohlc4', label: 'OHLC4 (average)' }
];

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
    case 'close':
    default:
      return b.c;
  }
}

const srcLabel = (key) =>
  key && key !== 'close'
    ? ` (${PRICE_SOURCES.find((s) => s.key === key)?.label.split(' ')[0] ?? key})`
    : '';

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
      { name: `SMA ${p.period}${srcLabel(p.source)}`, kind: 'line', data: sma(priceSource(b, p.source), p.period), color: 'amber' }
    ]
  },
  {
    type: 'ema',
    label: 'EMA — Exponential MA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 50)],
    compute: (b, p) => [
      { name: `EMA ${p.period}${srcLabel(p.source)}`, kind: 'line', data: ema(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'dema',
    label: 'DEMA — Double EMA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `DEMA ${p.period}${srcLabel(p.source)}`, kind: 'line', data: dema(priceSource(b, p.source), p.period), color: 'green' }
    ]
  },
  {
    type: 'wma',
    label: 'WMA — Weighted MA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `WMA ${p.period}${srcLabel(p.source)}`, kind: 'line', data: wma(priceSource(b, p.source), p.period), color: 'red' }
    ]
  },
  {
    type: 'hma',
    label: 'HMA — Hull MA',
    kind: 'overlay',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `HMA ${p.period}${srcLabel(p.source)}`, kind: 'line', data: hma(priceSource(b, p.source), p.period), color: 'accent' }
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
      { name: `RSI ${p.period}${srcLabel(p.source)}`, kind: 'line', data: rsi(priceSource(b, p.source), p.period), color: 'accent' }
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
    type: 'stddev',
    label: 'Std Deviation',
    kind: 'oscillator',
    sourceable: true,
    params: [P('period', 'Period', 20)],
    compute: (b, p) => [
      { name: `StdDev ${p.period}${srcLabel(p.source)}`, kind: 'line', data: stddev(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'roc',
    label: 'ROC — Rate of Change',
    kind: 'oscillator',
    sourceable: true,
    params: [P('period', 'Period', 12)],
    compute: (b, p) => [
      { name: `ROC ${p.period}${srcLabel(p.source)}`, kind: 'line', data: roc(priceSource(b, p.source), p.period), color: 'accent' }
    ]
  },
  {
    type: 'momentum',
    label: 'Momentum',
    kind: 'oscillator',
    sourceable: true,
    params: [P('period', 'Period', 10)],
    compute: (b, p) => [
      { name: `MOM ${p.period}${srcLabel(p.source)}`, kind: 'line', data: momentum(priceSource(b, p.source), p.period), color: 'accent' }
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
      { name: `TRIX ${p.period}${srcLabel(p.source)}`, kind: 'line', data: trix(priceSource(b, p.source), p.period), color: 'accent' }
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
export const DEFAULT_STYLE = { color: '', fill: '', width: 0 };

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
  return base + srcLabel(params.source);
}
