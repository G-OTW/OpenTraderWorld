/** Built-in indicator math for custom-indicator definitions — a 1:1 port of the Rust engine
 *  (`core/otw-core/src/backtest/indicators.rs`), `Option<f64>` becoming `null`.
 *
 *  Why a port and not the chart module's own math (`modules/histviz/indicators.js`): a custom
 *  indicator must draw exactly what the backtester computes, down to the warm-up bar and the
 *  smoothing seed. The chart catalog's functions are written for display and differ in those
 *  details, so the two libraries stay separate on purpose. **Any change here must be mirrored
 *  in the Rust file** — `scripts/indicator-parity/` cross-checks them.
 *
 *  Every function takes plain arrays and returns an array of the same length, `null` where the
 *  lookback isn't satisfied. `*_opt` variants take a series that already has holes (the output
 *  of an earlier step) and are the ones a chained indicator uses.
 */

const nulls = (n) => new Array(n).fill(null);

// ── Helpers over already-holed series ────────────────────────────────────────────

/** SMA over an Option series: a window containing a hole stays null. */
export function smaOpt(src, period) {
  const out = nulls(src.length);
  if (period === 0) return out;
  for (let i = 0; i < src.length; i++) {
    if (i + 1 < period) continue;
    let sum = 0;
    let ok = true;
    for (let j = i + 1 - period; j <= i; j++) {
      if (src[j] == null) {
        ok = false;
        break;
      }
      sum += src[j];
    }
    if (ok) out[i] = sum / period;
  }
  return out;
}

/** EMA over an Option series: seeds from the first `period` defined values (holes read as 0,
 *  matching the Rust `ema_opt`). */
export function emaOpt(src, period) {
  const out = nulls(src.length);
  if (period === 0) return out;
  const start = src.findIndex((v) => v != null);
  if (start < 0) return out;
  const tail = src.slice(start).map((v) => v ?? 0);
  const e = ema(tail, period);
  for (let i = 0; i < e.length; i++) out[start + i] = e[i];
  return out;
}

export function wmaOpt(src, period) {
  const out = nulls(src.length);
  if (period === 0) return out;
  const start = src.findIndex((v) => v != null);
  if (start < 0) return out;
  const tail = src.slice(start).map((v) => v ?? 0);
  const w = wma(tail, period);
  for (let i = 0; i < w.length; i++) out[start + i] = w[i];
  return out;
}

const zip = (a, b, f) => a.map((x, i) => (x != null && b[i] != null ? f(x, b[i]) : null));

export function demaOpt(src, period) {
  const e1 = emaOpt(src, period);
  const e2 = emaOpt(e1, period);
  return zip(e1, e2, (a, b) => 2 * a - b);
}

export function temaOpt(src, period) {
  const e1 = emaOpt(src, period);
  const e2 = emaOpt(e1, period);
  const e3 = emaOpt(e2, period);
  return src.map((_, i) =>
    e1[i] != null && e2[i] != null && e3[i] != null ? 3 * e1[i] - 3 * e2[i] + e3[i] : null
  );
}

export function hmaOpt(src, period) {
  if (period < 2) return nulls(src.length);
  const half = wmaOpt(src, Math.max(1, Math.floor(period / 2)));
  const full = wmaOpt(src, period);
  const diff = zip(half, full, (h, f) => 2 * h - f);
  return wmaOpt(diff, Math.max(1, Math.round(Math.sqrt(period))));
}

/** RSI (Wilder) over an Option series, starting at the first defined value. */
export function rsiOpt(src, period) {
  const n = src.length;
  const out = nulls(n);
  if (period === 0) return out;
  const start = src.findIndex((v) => v != null);
  if (start < 0 || start + period >= n) return out;
  const val = (i) => (src[i] == null ? NaN : src[i]);
  let gain = 0;
  let loss = 0;
  for (let i = start + 1; i <= start + period; i++) {
    const d = val(i) - val(i - 1);
    if (d >= 0) gain += d;
    else loss -= d;
  }
  gain /= period;
  loss /= period;
  out[start + period] = loss === 0 ? 100 : 100 - 100 / (1 + gain / loss);
  for (let i = start + period + 1; i < n; i++) {
    if (src[i] == null || src[i - 1] == null) continue;
    const d = val(i) - val(i - 1);
    gain = (gain * (period - 1) + Math.max(d, 0)) / period;
    loss = (loss * (period - 1) + Math.max(-d, 0)) / period;
    out[i] = loss === 0 ? 100 : 100 - 100 / (1 + gain / loss);
  }
  return out;
}

export function rocOpt(src, period) {
  const out = nulls(src.length);
  if (period === 0) return out;
  for (let i = period; i < src.length; i++) {
    const now = src[i];
    const then = src[i - period];
    if (now != null && then != null && then !== 0) out[i] = (now / then - 1) * 100;
  }
  return out;
}

export function momentumOpt(src, period) {
  const out = nulls(src.length);
  if (period === 0) return out;
  for (let i = period; i < src.length; i++) {
    if (src[i] != null && src[i - period] != null) out[i] = src[i] - src[i - period];
  }
  return out;
}

export function stddevOpt(src, period) {
  const out = nulls(src.length);
  if (period === 0) return out;
  for (let i = period - 1; i < src.length; i++) {
    const w = src.slice(i + 1 - period, i + 1);
    if (w.some((v) => v == null)) continue;
    const mean = w.reduce((a, b) => a + b, 0) / period;
    const varr = w.reduce((a, v) => a + (v - mean) * (v - mean), 0) / period;
    out[i] = Math.sqrt(varr);
  }
  return out;
}

export function macdLineOpt(src, fast, slow) {
  return zip(emaOpt(src, fast), emaOpt(src, slow), (f, s) => f - s);
}

// ── Moving averages ──────────────────────────────────────────────────────────────

export function sma(close, period) {
  const out = nulls(close.length);
  if (period === 0) return out;
  let sum = 0;
  for (let i = 0; i < close.length; i++) {
    sum += close[i];
    if (i >= period) sum -= close[i - period];
    if (i + 1 >= period) out[i] = sum / period;
  }
  return out;
}

export function ema(close, period) {
  const out = nulls(close.length);
  if (period === 0 || close.length < period) return out;
  const k = 2 / (period + 1);
  let prev = close.slice(0, period).reduce((a, b) => a + b, 0) / period;
  out[period - 1] = prev;
  for (let i = period; i < close.length; i++) {
    prev = close[i] * k + prev * (1 - k);
    out[i] = prev;
  }
  return out;
}

export function dema(close, period) {
  const e1 = ema(close, period);
  const e2 = emaOpt(e1, period);
  return zip(e1, e2, (a, b) => 2 * a - b);
}

export function tema(close, period) {
  const e1 = ema(close, period);
  const e2 = emaOpt(e1, period);
  const e3 = emaOpt(e2, period);
  return close.map((_, i) =>
    e1[i] != null && e2[i] != null && e3[i] != null ? 3 * e1[i] - 3 * e2[i] + e3[i] : null
  );
}

export function wma(close, period) {
  const out = nulls(close.length);
  if (period === 0) return out;
  const denom = (period * (period + 1)) / 2;
  for (let i = period - 1; i < close.length; i++) {
    let s = 0;
    for (let j = 0; j < period; j++) s += close[i - j] * (period - j);
    out[i] = s / denom;
  }
  return out;
}

export function hma(close, period) {
  if (period < 2) return nulls(close.length);
  const half = wma(close, Math.max(1, Math.floor(period / 2)));
  const full = wma(close, period);
  const diff = zip(half, full, (h, f) => 2 * h - f);
  return wmaOpt(diff, Math.max(1, Math.round(Math.sqrt(period))));
}

/** Rolling VWAP over `period` bars (typical price weighted by volume). */
export function vwap(high, low, close, volume, period) {
  const n = close.length;
  const out = nulls(n);
  if (period === 0) return out;
  let pv = 0;
  let v = 0;
  for (let i = 0; i < n; i++) {
    const tp = (high[i] + low[i] + close[i]) / 3;
    pv += tp * volume[i];
    v += volume[i];
    if (i >= period) {
      const tpo = (high[i - period] + low[i - period] + close[i - period]) / 3;
      pv -= tpo * volume[i - period];
      v -= volume[i - period];
    }
    if (i + 1 >= period && v > 0) out[i] = pv / v;
  }
  return out;
}

// ── Momentum ─────────────────────────────────────────────────────────────────────

export function rsi(close, period) {
  const out = nulls(close.length);
  if (period === 0 || close.length <= period) return out;
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
    gain = (gain * (period - 1) + Math.max(d, 0)) / period;
    loss = (loss * (period - 1) + Math.max(-d, 0)) / period;
    out[i] = loss === 0 ? 100 : 100 - 100 / (1 + gain / loss);
  }
  return out;
}

const hiLo = (high, low, from, to) => {
  let hh = -Infinity;
  let ll = Infinity;
  for (let j = from; j <= to; j++) {
    if (high[j] > hh) hh = high[j];
    if (low[j] < ll) ll = low[j];
  }
  return [hh, ll];
};

/** Stochastic %K: raw K over `period`, smoothed by an SMA of `smooth` bars. */
export function stochK(high, low, close, period, smooth) {
  const n = close.length;
  const raw = nulls(n);
  if (period === 0) return raw;
  for (let i = period - 1; i < n; i++) {
    const [hh, ll] = hiLo(high, low, i + 1 - period, i);
    raw[i] = hh > ll ? ((close[i] - ll) / (hh - ll)) * 100 : 50;
  }
  return smooth > 1 ? smaOpt(raw, smooth) : raw;
}

/** Stochastic %D: SMA of %K over `smooth` bars. */
export function stochD(high, low, close, period, smooth) {
  const s = Math.max(1, smooth);
  return smaOpt(stochK(high, low, close, period, s), s);
}

export function cci(high, low, close, period) {
  const n = close.length;
  const out = nulls(n);
  if (period === 0) return out;
  const tp = close.map((c, i) => (high[i] + low[i] + c) / 3);
  for (let i = period - 1; i < n; i++) {
    const w = tp.slice(i + 1 - period, i + 1);
    const mean = w.reduce((a, b) => a + b, 0) / period;
    const md = w.reduce((a, v) => a + Math.abs(v - mean), 0) / period;
    out[i] = md > 0 ? (tp[i] - mean) / (0.015 * md) : 0;
  }
  return out;
}

/** Williams %R (−100..0). */
export function willr(high, low, close, period) {
  const n = close.length;
  const out = nulls(n);
  if (period === 0) return out;
  for (let i = period - 1; i < n; i++) {
    const [hh, ll] = hiLo(high, low, i + 1 - period, i);
    out[i] = hh > ll ? ((hh - close[i]) / (hh - ll)) * -100 : -50;
  }
  return out;
}

export function roc(close, period) {
  const out = nulls(close.length);
  if (period === 0) return out;
  for (let i = period; i < close.length; i++) {
    if (close[i - period] !== 0) out[i] = (close[i] / close[i - period] - 1) * 100;
  }
  return out;
}

export function momentum(close, period) {
  const out = nulls(close.length);
  if (period === 0) return out;
  for (let i = period; i < close.length; i++) out[i] = close[i] - close[i - period];
  return out;
}

export function macdLine(close, fast, slow) {
  return zip(ema(close, fast), ema(close, slow), (f, s) => f - s);
}

export function macdSignal(close, fast, slow, signal) {
  return emaOpt(macdLine(close, fast, slow), signal);
}

export function macdHist(close, fast, slow, signal) {
  const line = macdLine(close, fast, slow);
  return zip(line, emaOpt(line, signal), (l, s) => l - s);
}

/** Average Directional Index (Wilder). */
export function adx(high, low, close, period) {
  const n = close.length;
  const out = nulls(n);
  if (period === 0 || n <= 2 * period) return out;
  let smTr = 0;
  let smPdm = 0;
  let smMdm = 0;
  const dx = nulls(n);
  for (let i = 1; i < n; i++) {
    const tr = Math.max(
      high[i] - low[i],
      Math.abs(high[i] - close[i - 1]),
      Math.abs(low[i] - close[i - 1])
    );
    const up = high[i] - high[i - 1];
    const down = low[i - 1] - low[i];
    const pdm = up > down && up > 0 ? up : 0;
    const mdm = down > up && down > 0 ? down : 0;
    if (i <= period) {
      smTr += tr;
      smPdm += pdm;
      smMdm += mdm;
    } else {
      smTr = smTr - smTr / period + tr;
      smPdm = smPdm - smPdm / period + pdm;
      smMdm = smMdm - smMdm / period + mdm;
    }
    if (i >= period && smTr > 0) {
      const pdi = (100 * smPdm) / smTr;
      const mdi = (100 * smMdm) / smTr;
      const sum = pdi + mdi;
      dx[i] = sum > 0 ? (100 * Math.abs(pdi - mdi)) / sum : 0;
    }
  }
  const first = 2 * period - 1;
  let prev = 0;
  for (let i = period; i <= first; i++) prev += dx[i] ?? 0;
  prev /= period;
  out[first] = prev;
  for (let i = first + 1; i < n; i++) {
    prev = (prev * (period - 1) + (dx[i] ?? 0)) / period;
    out[i] = prev;
  }
  return out;
}

/** Money Flow Index (volume-weighted RSI). */
export function mfi(high, low, close, volume, period) {
  const n = close.length;
  const out = nulls(n);
  if (period === 0 || n <= period) return out;
  const tp = close.map((c, i) => (high[i] + low[i] + c) / 3);
  for (let i = period; i < n; i++) {
    let pos = 0;
    let neg = 0;
    for (let j = i + 1 - period; j <= i; j++) {
      const mf = tp[j] * volume[j];
      if (tp[j] > tp[j - 1]) pos += mf;
      else if (tp[j] < tp[j - 1]) neg += mf;
    }
    out[i] = neg === 0 ? 100 : 100 - 100 / (1 + pos / neg);
  }
  return out;
}

/** On-Balance Volume (cumulative from the first bar). */
export function obv(close, volume) {
  const n = close.length;
  const out = nulls(n);
  if (n === 0) return out;
  let acc = 0;
  out[0] = 0;
  for (let i = 1; i < n; i++) {
    if (close[i] > close[i - 1]) acc += volume[i];
    else if (close[i] < close[i - 1]) acc -= volume[i];
    out[i] = acc;
  }
  return out;
}

// ── Volatility & bands ───────────────────────────────────────────────────────────

export function atr(high, low, close, period) {
  const n = close.length;
  const out = nulls(n);
  if (period === 0 || n <= period) return out;
  const tr = new Array(n).fill(0);
  tr[0] = high[0] - low[0];
  for (let i = 1; i < n; i++) {
    tr[i] = Math.max(
      high[i] - low[i],
      Math.abs(high[i] - close[i - 1]),
      Math.abs(low[i] - close[i - 1])
    );
  }
  let prev = 0;
  for (let i = 1; i <= period; i++) prev += tr[i];
  prev /= period;
  out[period] = prev;
  for (let i = period + 1; i < n; i++) {
    prev = (prev * (period - 1) + tr[i]) / period;
    out[i] = prev;
  }
  return out;
}

/** Rolling population standard deviation of close. */
export function stddev(close, period) {
  const n = close.length;
  const out = nulls(n);
  if (period === 0) return out;
  for (let i = period - 1; i < n; i++) {
    const w = close.slice(i + 1 - period, i + 1);
    const mean = w.reduce((a, b) => a + b, 0) / period;
    const varr = w.reduce((a, v) => a + (v - mean) * (v - mean), 0) / period;
    out[i] = Math.sqrt(varr);
  }
  return out;
}

/** Bollinger band: SMA ± mult·stddev. `band` is -1 (lower), 0 (mid), 1 (upper). */
export function bollinger(close, period, mult, band) {
  const mid = sma(close, period);
  if (band === 0) return mid;
  return zip(mid, stddev(close, period), (m, s) => m + band * mult * s);
}

/** Keltner channel: EMA(close) ± mult·ATR. `band` is -1 (lower) or 1 (upper). */
export function keltner(high, low, close, period, mult, band) {
  return zip(ema(close, period), atr(high, low, close, period), (m, s) => m + band * mult * s);
}

/** Donchian channel over `period` bars (current bar included). `band`: -1 lower, 0 mid, 1 upper. */
export function donchian(high, low, period, band) {
  const n = high.length;
  const out = nulls(n);
  if (period === 0) return out;
  for (let i = period - 1; i < n; i++) {
    const [hh, ll] = hiLo(high, low, i + 1 - period, i);
    out[i] = band === 1 ? hh : band === -1 ? ll : (hh + ll) / 2;
  }
  return out;
}

/** SuperTrend line (period + ATR multiplier). */
export function supertrend(high, low, close, period, mult) {
  const n = close.length;
  const out = nulls(n);
  const a = atr(high, low, close, period);
  let fu = NaN;
  let fl = NaN;
  let up = true;
  for (let i = 0; i < n; i++) {
    if (a[i] == null) continue;
    const hl2 = (high[i] + low[i]) / 2;
    const bu = hl2 + mult * a[i];
    const bl = hl2 - mult * a[i];
    if (Number.isNaN(fu)) {
      fu = bu;
      fl = bl;
      up = close[i] > hl2;
    } else {
      fu = bu < fu || close[i - 1] > fu ? bu : fu;
      fl = bl > fl || close[i - 1] < fl ? bl : fl;
      if (up && close[i] < fl) up = false;
      else if (!up && close[i] > fu) up = true;
    }
    out[i] = up ? fl : fu;
  }
  return out;
}

/** Parabolic SAR. `step` is the acceleration increment; max acceleration = 10·step. */
export function psar(high, low, step) {
  const n = high.length;
  const out = nulls(n);
  if (n < 2 || step <= 0) return out;
  const maxAf = step * 10;
  let long = high[1] + low[1] >= high[0] + low[0];
  let sar = long ? low[0] : high[0];
  let ep = long ? high[1] : low[1];
  let af = step;
  out[1] = sar;
  for (let i = 2; i < n; i++) {
    sar += af * (ep - sar);
    if (long) {
      sar = Math.min(sar, low[i - 1], low[i - 2]);
      if (low[i] < sar) {
        long = false;
        sar = ep;
        ep = low[i];
        af = step;
      } else if (high[i] > ep) {
        ep = high[i];
        af = Math.min(af + step, maxAf);
      }
    } else {
      sar = Math.max(sar, high[i - 1], high[i - 2]);
      if (high[i] > sar) {
        long = true;
        sar = ep;
        ep = high[i];
        af = step;
      } else if (low[i] < ep) {
        ep = low[i];
        af = Math.min(af + step, maxAf);
      }
    }
    out[i] = sar;
  }
  return out;
}
