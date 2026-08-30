//! Indicator math for the backtest engine. Same family as the visualization module, in
//! Rust. Each returns a `Vec<Option<f64>>` aligned to the input, `None` where lookback is
//! insufficient (so signal evaluation can skip warm-up bars cleanly).

/// SMA over an already-Option series (used to smooth other indicators). Values before the
/// first `Some` — and windows containing a `None` — stay `None`.
pub fn sma_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if period == 0 {
        return out;
    }
    for i in 0..src.len() {
        if i + 1 < period {
            continue;
        }
        let w = &src[i + 1 - period..=i];
        if w.iter().all(|v| v.is_some()) {
            out[i] = Some(w.iter().map(|v| v.unwrap()).sum::<f64>() / period as f64);
        }
    }
    out
}

/// EMA over an Option series: seeds from the first `period` defined values.
pub fn ema_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if period == 0 {
        return out;
    }
    let start = match src.iter().position(|v| v.is_some()) {
        Some(s) => s,
        None => return out,
    };
    let tail: Vec<f64> = src[start..].iter().map(|v| v.unwrap_or(0.0)).collect();
    for (i, v) in ema(&tail, period).into_iter().enumerate() {
        out[start + i] = v;
    }
    out
}

pub fn wma_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if period == 0 {
        return out;
    }
    let start = match src.iter().position(|v| v.is_some()) {
        Some(s) => s,
        None => return out,
    };
    let tail: Vec<f64> = src[start..].iter().map(|v| v.unwrap_or(0.0)).collect();
    for (i, v) in wma(&tail, period).into_iter().enumerate() {
        out[start + i] = v;
    }
    out
}

/// The subset of built-ins that operate on a *single* input series (not OHLC), so they can be
/// chained onto the output of an earlier step (e.g. `HullMA(@rsi)`, `MACD(@rsi)`). OHLC-derived
/// indicators (ATR, Stoch, CCI, ADX, VWAP…) are excluded — they need high/low/volume and have no
/// meaning applied to a derived series.
pub fn is_chainable(indicator: &str) -> bool {
    matches!(
        indicator,
        "sma" | "ema" | "dema" | "tema" | "wma" | "hma"
            | "rsi" | "roc" | "momentum" | "stddev"
            | "macd" | "macd_signal" | "macd_hist"
    )
}

/// Resolve a chainable built-in over an arbitrary input series `src` (the output of an earlier
/// step). Mirrors `resolve_builtin` for the single-series subset; `None` inputs propagate. Unknown
/// / non-chainable names yield all-`None`.
pub fn resolve_chainable(
    indicator: &str,
    src: &[Option<f64>],
    period: usize,
    fast: usize,
    slow: usize,
    signal_period: usize,
) -> Vec<Option<f64>> {
    let p = |d: usize| if period == 0 { d } else { period };
    let f = |d: usize| if fast == 0 { d } else { fast };
    let sl = |d: usize| if slow == 0 { d } else { slow };
    let sp = |d: usize| if signal_period == 0 { d } else { signal_period };
    match indicator {
        "sma" => sma_opt(src, p(20)),
        "ema" => ema_opt(src, p(20)),
        "dema" => dema_opt(src, p(20)),
        "tema" => tema_opt(src, p(20)),
        "wma" => wma_opt(src, p(20)),
        "hma" => hma_opt(src, p(20)),
        "rsi" => rsi_opt(src, p(14)),
        "roc" => roc_opt(src, p(12)),
        "momentum" => momentum_opt(src, p(10)),
        "stddev" => stddev_opt(src, p(20)),
        "macd" => macd_line_opt(src, f(12), sl(26)),
        "macd_signal" => ema_opt(&macd_line_opt(src, f(12), sl(26)), sp(9)),
        "macd_hist" => {
            let line = macd_line_opt(src, f(12), sl(26));
            let sig = ema_opt(&line, sp(9));
            zip_opt(&line, &sig, |l, s| Some(l - s))
        }
        _ => vec![None; src.len()],
    }
}

/// Combine two Option series element-wise; `None` where either input is `None`.
fn zip_opt(a: &[Option<f64>], b: &[Option<f64>], f: impl Fn(f64, f64) -> Option<f64>) -> Vec<Option<f64>> {
    a.iter()
        .zip(b)
        .map(|(x, y)| match (x, y) {
            (Some(x), Some(y)) => f(*x, *y),
            _ => None,
        })
        .collect()
}

fn dema_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let e1 = ema_opt(src, period);
    let e2 = ema_opt(&e1, period);
    zip_opt(&e1, &e2, |a, b| Some(2.0 * a - b))
}

fn tema_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let e1 = ema_opt(src, period);
    let e2 = ema_opt(&e1, period);
    let e3 = ema_opt(&e2, period);
    (0..src.len())
        .map(|i| match (e1[i], e2[i], e3[i]) {
            (Some(a), Some(b), Some(c)) => Some(3.0 * a - 3.0 * b + c),
            _ => None,
        })
        .collect()
}

fn hma_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    if period < 2 {
        return vec![None; src.len()];
    }
    let half = wma_opt(src, (period / 2).max(1));
    let full = wma_opt(src, period);
    let diff = zip_opt(&half, &full, |h, f| Some(2.0 * h - f));
    wma_opt(&diff, (period as f64).sqrt().round().max(1.0) as usize)
}

/// RSI over an Option series (Wilder smoothing), starting from the first defined value.
fn rsi_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let n = src.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    let start = match src.iter().position(|v| v.is_some()) {
        Some(s) => s,
        None => return out,
    };
    // Need `period` deltas from `start`; bail if not enough defined bars follow.
    if start + period >= n {
        return out;
    }
    let val = |i: usize| src[i].unwrap_or(f64::NAN);
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in (start + 1)..=(start + period) {
        let d = val(i) - val(i - 1);
        if d >= 0.0 {
            gain += d;
        } else {
            loss -= d;
        }
    }
    gain /= period as f64;
    loss /= period as f64;
    out[start + period] = Some(if loss == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + gain / loss) });
    for i in (start + period + 1)..n {
        if src[i].is_none() || src[i - 1].is_none() {
            continue;
        }
        let d = val(i) - val(i - 1);
        gain = (gain * (period as f64 - 1.0) + d.max(0.0)) / period as f64;
        loss = (loss * (period as f64 - 1.0) + (-d).max(0.0)) / period as f64;
        out[i] = Some(if loss == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + gain / loss) });
    }
    out
}

fn roc_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let n = src.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for i in period..n {
        if let (Some(now), Some(then)) = (src[i], src[i - period]) {
            if then != 0.0 {
                out[i] = Some((now / then - 1.0) * 100.0);
            }
        }
    }
    out
}

fn momentum_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let n = src.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for i in period..n {
        if let (Some(now), Some(then)) = (src[i], src[i - period]) {
            out[i] = Some(now - then);
        }
    }
    out
}

fn stddev_opt(src: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let n = src.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for i in (period.saturating_sub(1))..n {
        if i + 1 < period {
            continue;
        }
        let w = &src[i + 1 - period..=i];
        if w.iter().any(|v| v.is_none()) {
            continue;
        }
        let vals: Vec<f64> = w.iter().map(|v| v.unwrap()).collect();
        let mean = vals.iter().sum::<f64>() / period as f64;
        let var = vals.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / period as f64;
        out[i] = Some(var.sqrt());
    }
    out
}

fn macd_line_opt(src: &[Option<f64>], fast: usize, slow: usize) -> Vec<Option<f64>> {
    let ef = ema_opt(src, fast);
    let es = ema_opt(src, slow);
    zip_opt(&ef, &es, |f, s| Some(f - s))
}

// ── Moving averages ────────────────────────────────────────────────────────────

pub fn sma(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; close.len()];
    if period == 0 {
        return out;
    }
    let mut sum = 0.0;
    for i in 0..close.len() {
        sum += close[i];
        if i >= period {
            sum -= close[i - period];
        }
        if i + 1 >= period {
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

pub fn ema(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; close.len()];
    if period == 0 || close.len() < period {
        return out;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut prev = close[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(prev);
    for i in period..close.len() {
        prev = close[i] * k + prev * (1.0 - k);
        out[i] = Some(prev);
    }
    out
}

pub fn dema(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let e1 = ema(close, period);
    let e2 = ema_opt(&e1, period);
    e1.iter()
        .zip(&e2)
        .map(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => Some(2.0 * a - b),
            _ => None,
        })
        .collect()
}

pub fn tema(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let e1 = ema(close, period);
    let e2 = ema_opt(&e1, period);
    let e3 = ema_opt(&e2, period);
    (0..close.len())
        .map(|i| match (e1[i], e2[i], e3[i]) {
            (Some(a), Some(b), Some(c)) => Some(3.0 * a - 3.0 * b + c),
            _ => None,
        })
        .collect()
}

pub fn wma(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; close.len()];
    if period == 0 {
        return out;
    }
    let denom = (period * (period + 1)) as f64 / 2.0;
    for i in (period - 1)..close.len() {
        let mut s = 0.0;
        for j in 0..period {
            s += close[i - j] * (period - j) as f64;
        }
        out[i] = Some(s / denom);
    }
    out
}

/// Hull MA: WMA(2·WMA(n/2) − WMA(n), √n).
pub fn hma(close: &[f64], period: usize) -> Vec<Option<f64>> {
    if period < 2 {
        return vec![None; close.len()];
    }
    let half = wma(close, (period / 2).max(1));
    let full = wma(close, period);
    let diff: Vec<Option<f64>> = half
        .iter()
        .zip(&full)
        .map(|(h, f)| match (h, f) {
            (Some(h), Some(f)) => Some(2.0 * h - f),
            _ => None,
        })
        .collect();
    wma_opt(&diff, (period as f64).sqrt().round().max(1.0) as usize)
}

/// Rolling VWAP over `period` bars (typical price weighted by volume).
pub fn vwap(high: &[f64], low: &[f64], close: &[f64], volume: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    let (mut pv, mut v) = (0.0, 0.0);
    for i in 0..n {
        let tp = (high[i] + low[i] + close[i]) / 3.0;
        pv += tp * volume[i];
        v += volume[i];
        if i >= period {
            let tpo = (high[i - period] + low[i - period] + close[i - period]) / 3.0;
            pv -= tpo * volume[i - period];
            v -= volume[i - period];
        }
        if i + 1 >= period && v > 0.0 {
            out[i] = Some(pv / v);
        }
    }
    out
}

// ── Momentum ───────────────────────────────────────────────────────────────────

pub fn rsi(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; close.len()];
    if period == 0 || close.len() <= period {
        return out;
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in 1..=period {
        let d = close[i] - close[i - 1];
        if d >= 0.0 {
            gain += d;
        } else {
            loss -= d;
        }
    }
    gain /= period as f64;
    loss /= period as f64;
    out[period] = Some(if loss == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + gain / loss) });
    for i in (period + 1)..close.len() {
        let d = close[i] - close[i - 1];
        gain = (gain * (period as f64 - 1.0) + d.max(0.0)) / period as f64;
        loss = (loss * (period as f64 - 1.0) + (-d).max(0.0)) / period as f64;
        out[i] = Some(if loss == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + gain / loss) });
    }
    out
}

/// Stochastic %K: raw K over `period`, smoothed by an SMA of `smooth` bars.
pub fn stoch_k(high: &[f64], low: &[f64], close: &[f64], period: usize, smooth: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut raw = vec![None; n];
    if period == 0 {
        return raw;
    }
    for i in (period - 1)..n {
        let w = i + 1 - period..=i;
        let hh = high[w.clone()].iter().cloned().fold(f64::MIN, f64::max);
        let ll = low[w].iter().cloned().fold(f64::MAX, f64::min);
        raw[i] = Some(if hh > ll { (close[i] - ll) / (hh - ll) * 100.0 } else { 50.0 });
    }
    if smooth > 1 { sma_opt(&raw, smooth) } else { raw }
}

/// Stochastic %D: SMA of %K over `smooth` bars.
pub fn stoch_d(high: &[f64], low: &[f64], close: &[f64], period: usize, smooth: usize) -> Vec<Option<f64>> {
    let k = stoch_k(high, low, close, period, smooth.max(1));
    sma_opt(&k, smooth.max(1))
}

/// Commodity Channel Index.
pub fn cci(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    let tp: Vec<f64> = (0..n).map(|i| (high[i] + low[i] + close[i]) / 3.0).collect();
    for i in (period - 1)..n {
        let w = &tp[i + 1 - period..=i];
        let mean = w.iter().sum::<f64>() / period as f64;
        let md = w.iter().map(|v| (v - mean).abs()).sum::<f64>() / period as f64;
        out[i] = Some(if md > 0.0 { (tp[i] - mean) / (0.015 * md) } else { 0.0 });
    }
    out
}

/// Williams %R (−100..0).
pub fn willr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for i in (period - 1)..n {
        let w = i + 1 - period..=i;
        let hh = high[w.clone()].iter().cloned().fold(f64::MIN, f64::max);
        let ll = low[w].iter().cloned().fold(f64::MAX, f64::min);
        out[i] = Some(if hh > ll { (hh - close[i]) / (hh - ll) * -100.0 } else { -50.0 });
    }
    out
}

/// Rate of change in percent over `period` bars.
pub fn roc(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; close.len()];
    if period == 0 {
        return out;
    }
    for i in period..close.len() {
        if close[i - period] != 0.0 {
            out[i] = Some((close[i] / close[i - period] - 1.0) * 100.0);
        }
    }
    out
}

/// Momentum: close − close `period` bars ago.
pub fn momentum(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; close.len()];
    if period == 0 {
        return out;
    }
    for i in period..close.len() {
        out[i] = Some(close[i] - close[i - period]);
    }
    out
}

/// MACD line (EMA fast − EMA slow). Signal/hist aren't needed for signal evaluation.
pub fn macd_line(close: &[f64], fast: usize, slow: usize) -> Vec<Option<f64>> {
    let ef = ema(close, fast);
    let es = ema(close, slow);
    close
        .iter()
        .enumerate()
        .map(|(i, _)| match (ef[i], es[i]) {
            (Some(f), Some(s)) => Some(f - s),
            _ => None,
        })
        .collect()
}

/// MACD signal line: EMA of the MACD line over `signal` bars.
pub fn macd_signal(close: &[f64], fast: usize, slow: usize, signal: usize) -> Vec<Option<f64>> {
    ema_opt(&macd_line(close, fast, slow), signal)
}

/// MACD histogram: line − signal.
pub fn macd_hist(close: &[f64], fast: usize, slow: usize, signal: usize) -> Vec<Option<f64>> {
    let line = macd_line(close, fast, slow);
    let sig = ema_opt(&line, signal);
    line.iter()
        .zip(&sig)
        .map(|(l, s)| match (l, s) {
            (Some(l), Some(s)) => Some(l - s),
            _ => None,
        })
        .collect()
}

/// Average Directional Index (Wilder).
pub fn adx(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period == 0 || n <= 2 * period {
        return out;
    }
    let (mut sm_tr, mut sm_pdm, mut sm_mdm) = (0.0, 0.0, 0.0);
    let mut dx = vec![None; n];
    for i in 1..n {
        let tr = (high[i] - low[i])
            .max((high[i] - close[i - 1]).abs())
            .max((low[i] - close[i - 1]).abs());
        let up = high[i] - high[i - 1];
        let down = low[i - 1] - low[i];
        let pdm = if up > down && up > 0.0 { up } else { 0.0 };
        let mdm = if down > up && down > 0.0 { down } else { 0.0 };
        if i <= period {
            sm_tr += tr;
            sm_pdm += pdm;
            sm_mdm += mdm;
        } else {
            sm_tr = sm_tr - sm_tr / period as f64 + tr;
            sm_pdm = sm_pdm - sm_pdm / period as f64 + pdm;
            sm_mdm = sm_mdm - sm_mdm / period as f64 + mdm;
        }
        if i >= period && sm_tr > 0.0 {
            let pdi = 100.0 * sm_pdm / sm_tr;
            let mdi = 100.0 * sm_mdm / sm_tr;
            let sum = pdi + mdi;
            dx[i] = Some(if sum > 0.0 { 100.0 * (pdi - mdi).abs() / sum } else { 0.0 });
        }
    }
    // First ADX = mean of the first `period` DX values, then Wilder-smoothed.
    let first = 2 * period - 1;
    let mut prev = dx[period..=first].iter().map(|v| v.unwrap_or(0.0)).sum::<f64>() / period as f64;
    out[first] = Some(prev);
    for i in (first + 1)..n {
        prev = (prev * (period as f64 - 1.0) + dx[i].unwrap_or(0.0)) / period as f64;
        out[i] = Some(prev);
    }
    out
}

/// Money Flow Index (volume-weighted RSI).
pub fn mfi(high: &[f64], low: &[f64], close: &[f64], volume: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period == 0 || n <= period {
        return out;
    }
    let tp: Vec<f64> = (0..n).map(|i| (high[i] + low[i] + close[i]) / 3.0).collect();
    for i in period..n {
        let (mut pos, mut neg) = (0.0, 0.0);
        for j in (i + 1 - period)..=i {
            let mf = tp[j] * volume[j];
            if tp[j] > tp[j - 1] {
                pos += mf;
            } else if tp[j] < tp[j - 1] {
                neg += mf;
            }
        }
        out[i] = Some(if neg == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + pos / neg) });
    }
    out
}

/// On-Balance Volume (cumulative from the first bar).
pub fn obv(close: &[f64], volume: &[f64]) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if n == 0 {
        return out;
    }
    let mut acc = 0.0;
    out[0] = Some(0.0);
    for i in 1..n {
        if close[i] > close[i - 1] {
            acc += volume[i];
        } else if close[i] < close[i - 1] {
            acc -= volume[i];
        }
        out[i] = Some(acc);
    }
    out
}

// ── Volatility & bands ─────────────────────────────────────────────────────────

pub fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period == 0 || n <= period {
        return out;
    }
    let mut tr = vec![0.0; n];
    tr[0] = high[0] - low[0];
    for i in 1..n {
        tr[i] = (high[i] - low[i])
            .max((high[i] - close[i - 1]).abs())
            .max((low[i] - close[i - 1]).abs());
    }
    let mut prev = tr[1..=period].iter().sum::<f64>() / period as f64;
    out[period] = Some(prev);
    for i in (period + 1)..n {
        prev = (prev * (period as f64 - 1.0) + tr[i]) / period as f64;
        out[i] = Some(prev);
    }
    out
}

/// Rolling population standard deviation of close.
pub fn stddev(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for i in (period - 1)..n {
        let w = &close[i + 1 - period..=i];
        let mean = w.iter().sum::<f64>() / period as f64;
        let var = w.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / period as f64;
        out[i] = Some(var.sqrt());
    }
    out
}

/// Bollinger band: SMA ± mult·stddev. `band` is -1 (lower), 0 (mid), 1 (upper).
pub fn bollinger(close: &[f64], period: usize, mult: f64, band: i8) -> Vec<Option<f64>> {
    let mid = sma(close, period);
    if band == 0 {
        return mid;
    }
    let sd = stddev(close, period);
    mid.iter()
        .zip(&sd)
        .map(|(m, s)| match (m, s) {
            (Some(m), Some(s)) => Some(m + band as f64 * mult * s),
            _ => None,
        })
        .collect()
}

/// Keltner channel: EMA(close) ± mult·ATR. `band` is -1 (lower) or 1 (upper).
pub fn keltner(high: &[f64], low: &[f64], close: &[f64], period: usize, mult: f64, band: i8) -> Vec<Option<f64>> {
    let mid = ema(close, period);
    let a = atr(high, low, close, period);
    mid.iter()
        .zip(&a)
        .map(|(m, s)| match (m, s) {
            (Some(m), Some(s)) => Some(m + band as f64 * mult * s),
            _ => None,
        })
        .collect()
}

/// Donchian channel over `period` bars (current bar included). `band`: -1 lower, 0 mid, 1 upper.
pub fn donchian(high: &[f64], low: &[f64], period: usize, band: i8) -> Vec<Option<f64>> {
    let n = high.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for i in (period - 1)..n {
        let w = i + 1 - period..=i;
        let hh = high[w.clone()].iter().cloned().fold(f64::MIN, f64::max);
        let ll = low[w].iter().cloned().fold(f64::MAX, f64::min);
        out[i] = Some(match band {
            1 => hh,
            -1 => ll,
            _ => (hh + ll) / 2.0,
        });
    }
    out
}

/// SuperTrend line (period + ATR multiplier).
pub fn supertrend(high: &[f64], low: &[f64], close: &[f64], period: usize, mult: f64) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    let a = atr(high, low, close, period);
    let (mut fu, mut fl) = (f64::NAN, f64::NAN);
    let mut up = true;
    for i in 0..n {
        let Some(atr_i) = a[i] else { continue };
        let hl2 = (high[i] + low[i]) / 2.0;
        let bu = hl2 + mult * atr_i;
        let bl = hl2 - mult * atr_i;
        if fu.is_nan() {
            fu = bu;
            fl = bl;
            up = close[i] > hl2;
        } else {
            fu = if bu < fu || close[i - 1] > fu { bu } else { fu };
            fl = if bl > fl || close[i - 1] < fl { bl } else { fl };
            if up && close[i] < fl {
                up = false;
            } else if !up && close[i] > fu {
                up = true;
            }
        }
        out[i] = Some(if up { fl } else { fu });
    }
    out
}

/// Parabolic SAR. `step` is the acceleration increment; max acceleration = 10·step
/// (classic 0.02 / 0.2).
pub fn psar(high: &[f64], low: &[f64], step: f64) -> Vec<Option<f64>> {
    let n = high.len();
    let mut out = vec![None; n];
    if n < 2 || step <= 0.0 {
        return out;
    }
    let max_af = step * 10.0;
    let mut long = high[1] + low[1] >= high[0] + low[0];
    let mut sar = if long { low[0] } else { high[0] };
    let mut ep = if long { high[1] } else { low[1] };
    let mut af = step;
    out[1] = Some(sar);
    for i in 2..n {
        sar += af * (ep - sar);
        if long {
            sar = sar.min(low[i - 1]).min(low[i - 2]);
            if low[i] < sar {
                long = false;
                sar = ep;
                ep = low[i];
                af = step;
            } else if high[i] > ep {
                ep = high[i];
                af = (af + step).min(max_af);
            }
        } else {
            sar = sar.max(high[i - 1]).max(high[i - 2]);
            if high[i] > sar {
                long = true;
                sar = ep;
                ep = high[i];
                af = step;
            } else if low[i] < ep {
                ep = low[i];
                af = (af + step).min(max_af);
            }
        }
        out[i] = Some(sar);
    }
    out
}
