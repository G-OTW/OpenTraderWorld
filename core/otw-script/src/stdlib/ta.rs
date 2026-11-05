//! `ta.*`. Each call site keeps its own state and advances only when the call runs (Pine's
//! semantics). The arithmetic follows the reference implementations printed in the Pine
//! Reference Manual, in the same order, so a script spelling them out gives identical numbers
//! (see `tests/fixtures/ta_ref`).

use std::collections::VecDeque;

use super::{time, Native, State};
use crate::diag::RtCode;
use crate::input::{TfUnit, Timeframe};
use crate::vm::value::Value;
use crate::vm::Ctx;

pub(super) fn st<T: 'static>(s: Option<&mut State>) -> &mut T {
    super::state_mut(s)
}

/// A window length; past the run's history limit it is refused (memory), not clamped.
pub(super) fn len(ctx: &Ctx, v: &Value) -> Result<Option<usize>, (RtCode, Vec<String>)> {
    match v.int().filter(|n| *n > 0) {
        Some(n) if n as usize > ctx.max_history => Err((RtCode::HistoryTooDeep, vec![n.to_string()])),
        n => Ok(n.map(|n| n as usize)),
    }
}

/// The last `n` values, newest at the back.
#[derive(Default, Clone)]
pub(super) struct Win(pub(super) VecDeque<Option<f64>>);

impl Win {
    pub(super) fn push(&mut self, v: Option<f64>, n: usize) {
        self.0.push_back(v);
        while self.0.len() > n {
            self.0.pop_front();
        }
    }
    /// `x[i]` for i = 0 (newest) .. n-1, or `None` when history is short.
    pub(super) fn newest_first(&self, n: usize) -> Option<impl Iterator<Item = &Option<f64>>> {
        (self.0.len() >= n).then(|| self.0.iter().rev().take(n))
    }
}

/// `pine_sma`: sum of x[i] / n, newest first; `na` if any value is `na`.
pub(super) fn sma_of(w: &Win, n: usize) -> Option<f64> {
    let mut sum = 0.0;
    for x in w.newest_first(n)? {
        sum += (*x)? / n as f64;
    }
    Some(sum)
}

#[derive(Default, Clone)]
pub(super) struct Sma(pub(super) Win);

pub(super) fn sma_step(s: &mut Sma, v: Option<f64>, n: usize) -> Option<f64> {
    s.0.push(v, n);
    sma_of(&s.0, n)
}

/// `pine_ema`: seeded with the first value.
#[derive(Default, Clone)]
pub(super) struct Ema(Option<f64>);

pub(super) fn ema_step(s: &mut Ema, v: Option<f64>, n: usize) -> Option<f64> {
    let alpha = 2.0 / (n as f64 + 1.0);
    let next = match s.0 {
        None => v,
        Some(p) => v.map(|x| alpha * x + (1.0 - alpha) * p),
    };
    s.0 = next.filter(|x| x.is_finite());
    s.0
}

/// `pine_rma`: seeded with an SMA (its window fills only while seeding).
#[derive(Default, Clone)]
pub(super) struct Rma {
    prev: Option<f64>,
    seed: Sma,
}

pub(super) fn rma_step(s: &mut Rma, v: Option<f64>, n: usize) -> Option<f64> {
    let alpha = 1.0 / n as f64;
    let next = match s.prev {
        None => sma_step(&mut s.seed, v, n),
        Some(p) => v.map(|x| alpha * x + (1.0 - alpha) * p),
    };
    s.prev = next.filter(|x| x.is_finite());
    s.prev
}

/// `pine_wma`: weights (n - i) * n, newest first.
#[derive(Default, Clone)]
struct Wma(Win);

fn wma_step(s: &mut Wma, v: Option<f64>, n: usize) -> Option<f64> {
    s.0.push(v, n);
    let (mut norm, mut sum) = (0.0, 0.0);
    for (i, x) in s.0.newest_first(n)?.enumerate() {
        let weight = ((n - i) * n) as f64;
        norm += weight;
        sum += (*x)? * weight;
    }
    Some(sum / norm).filter(|x| x.is_finite())
}

/// Sum of squared deviations around `ta.sma` over `n` (`n - 1` unbiased), `na` while the
/// average is. `ta.stdev` is its square root.
pub(super) fn variance_step(w: &mut Sma, v: Option<f64>, n: usize, biased: bool) -> Option<f64> {
    let avg = sma_step(w, v, n)?;
    let mut sum = 0.0;
    for x in w.0.newest_first(n).into_iter().flatten() {
        let d = x.unwrap_or(f64::NAN) - avg;
        sum += d * d;
    }
    let div = if biased { n as f64 } else { n as f64 - 1.0 };
    Some(sum / div).filter(|x| x.is_finite())
}

#[derive(Default, Clone)]
struct Hma {
    half: Wma,
    full: Wma,
    out: Wma,
}

#[derive(Default, Clone)]
struct Pair(Sma, Sma);

#[derive(Default, Clone)]
struct Rsi {
    prev: Option<f64>,
    up: Rma,
    down: Rma,
}

#[derive(Default, Clone)]
struct Macd {
    fast: Ema,
    slow: Ema,
    signal: Ema,
}

#[derive(Default, Clone)]
pub(super) struct Prev(Option<f64>, Option<f64>);

#[derive(Default, Clone)]
struct Occurrences(VecDeque<Value>);

#[derive(Default, Clone)]
struct Vwap {
    period: Option<i64>,
    pv: f64,
    v: f64,
    pv2: f64,
}

pub(super) fn f(v: Option<f64>) -> Value {
    v.map(Value::float).unwrap_or(Value::Na)
}

pub(super) fn true_range(ctx: &Ctx, handle_na: bool) -> Option<f64> {
    let b = &ctx.input.bars;
    let i = ctx.bar;
    let (h, l) = (*b.high.get(i)?, *b.low.get(i)?);
    if i == 0 {
        return if handle_na { Some(h - l).filter(|x| x.is_finite()) } else { None };
    }
    // Like `math.max` in Pine, an `na` previous close makes the range `na`.
    let c = b.close[i - 1];
    Some((h - l).max((h - c).abs()).max((l - c).abs())).filter(|x| x.is_finite() && c.is_finite())
}

/// `ta.tr` (the variable) = `ta.tr(false)`.
pub fn tr_value(ctx: &Ctx) -> Value {
    f(true_range(ctx, false))
}

pub(super) fn win_extreme(w: &Win, n: usize, high: bool) -> Option<f64> {
    let it = w.newest_first(n)?.filter_map(|x| *x);
    if high {
        it.fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))))
    } else {
        it.fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))
    }
}

fn pivot(w: &mut Win, v: Option<f64>, left: usize, right: usize, high: bool) -> Option<f64> {
    let n = left + right + 1;
    w.push(v, n);
    if w.0.len() < n {
        return None;
    }
    let cand = w.0[left]?;
    let ok = w.0.iter().enumerate().filter(|(i, _)| *i != left).all(|(_, x)| match x {
        Some(x) => {
            if high {
                cand > *x
            } else {
                cand < *x
            }
        }
        None => false,
    });
    ok.then_some(cand)
}

macro_rules! native {
    ($ty:ty, $body:expr) => {
        Some(Native { f: $body, init: Some(|| Box::new(<$ty>::default()) as State) })
    };
}

pub fn native(name: &str) -> Option<Native> {
    match name {
        "ta.sma" => native!(Sma, |ctx, a, s| Ok(match len(ctx, &a[1])? {
            Some(n) => f(sma_step(st(s), a[0].num(), n)),
            None => Value::Na,
        })),
        "ta.ema" => native!(Ema, |ctx, a, s| Ok(match len(ctx, &a[1])? {
            Some(n) => f(ema_step(st(s), a[0].num(), n)),
            None => Value::Na,
        })),
        "ta.rma" => native!(Rma, |ctx, a, s| Ok(match len(ctx, &a[1])? {
            Some(n) => f(rma_step(st(s), a[0].num(), n)),
            None => Value::Na,
        })),
        "ta.wma" => native!(Wma, |ctx, a, s| Ok(match len(ctx, &a[1])? {
            Some(n) => f(wma_step(st(s), a[0].num(), n)),
            None => Value::Na,
        })),
        "ta.hma" => native!(Hma, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let h: &mut Hma = st(s);
            let v = a[0].num();
            let half = wma_step(&mut h.half, v, (n / 2).max(1));
            let full = wma_step(&mut h.full, v, n);
            let diff = match (half, full) {
                (Some(x), Some(y)) => Some(2.0 * x - y),
                _ => None,
            };
            Ok(f(wma_step(&mut h.out, diff, (libm::floor(libm::sqrt(n as f64)) as usize).max(1))))
        }),
        "ta.vwma" => native!(Pair, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let p: &mut Pair = st(s);
            let vol = Some(super::volume_at(ctx, ctx.bar)?).filter(|x| x.is_finite());
            let pv = match (a[0].num(), vol) {
                (Some(x), Some(v)) => Some(x * v),
                _ => None,
            };
            let (num, den) = (sma_step(&mut p.0, pv, n), sma_step(&mut p.1, vol, n));
            Ok(match (num, den) {
                (Some(x), Some(d)) if d != 0.0 => Value::float(x / d),
                _ => Value::Na,
            })
        }),
        "ta.stdev" => native!(Sma, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let biased = !matches!(a[2], Value::Bool(false));
            Ok(f(variance_step(st(s), a[0].num(), n, biased).map(libm::sqrt)))
        }),
        "ta.change" => native!(Occurrences, |ctx, a, s| {
            let n = len(ctx, &a[1])?.unwrap_or(1);
            let h: &mut Occurrences = st(s);
            h.0.push_front(a[0].clone());
            h.0.truncate(n + 1);
            let Some(old) = h.0.get(n) else { return Ok(Value::Na) };
            Ok(match (&a[0], old) {
                (Value::Bool(x), Value::Bool(y)) => Value::Bool(x != y),
                (cur, old) => crate::vm::binary(crate::ast::BinOp::Sub, cur, old, false),
            })
        }),
        "ta.highest" => native!(Win, |ctx, a, s| Ok(match len(ctx, &a[1])? {
            Some(n) => {
                let w: &mut Win = st(s);
                w.push(a[0].num(), n);
                f(win_extreme(w, n, true))
            }
            None => Value::Na,
        })),
        "ta.lowest" => native!(Win, |ctx, a, s| Ok(match len(ctx, &a[1])? {
            Some(n) => {
                let w: &mut Win = st(s);
                w.push(a[0].num(), n);
                f(win_extreme(w, n, false))
            }
            None => Value::Na,
        })),
        "ta.tr" => Some(Native { f: |ctx, a, _| Ok(f(true_range(ctx, a[0].truthy()))), init: None }),
        "ta.atr" => native!(Rma, |ctx, a, s| Ok(match len(ctx, &a[0])? {
            Some(n) => f(rma_step(st(s), true_range(ctx, true), n)),
            None => Value::Na,
        })),
        "ta.rsi" => native!(Rsi, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let r: &mut Rsi = st(s);
            let x = a[0].num();
            let (u, d) = match (x, r.prev) {
                (Some(x), Some(p)) => (Some((x - p).max(0.0)), Some((p - x).max(0.0))),
                _ => (None, None),
            };
            r.prev = x;
            let (up, down) = (rma_step(&mut r.up, u, n), rma_step(&mut r.down, d, n));
            Ok(match (up, down) {
                (Some(up), Some(down)) if down == 0.0 && up > 0.0 => Value::Float(100.0),
                (Some(up), Some(down)) => {
                    let rs = up / down;
                    Value::float(100.0 - 100.0 / (1.0 + rs))
                }
                _ => Value::Na,
            })
        }),
        "ta.macd" => native!(Macd, |ctx, a, s| {
            let (Some(fl), Some(sl), Some(gl)) = (len(ctx, &a[1])?, len(ctx, &a[2])?, len(ctx, &a[3])?) else { return Ok(Value::Na) };
            let m: &mut Macd = st(s);
            let x = a[0].num();
            let line = match (ema_step(&mut m.fast, x, fl), ema_step(&mut m.slow, x, sl)) {
                (Some(p), Some(q)) => Some(p - q),
                _ => None,
            };
            let signal = ema_step(&mut m.signal, line, gl);
            let hist = match (line, signal) {
                (Some(p), Some(q)) => Some(p - q),
                _ => None,
            };
            Ok(Value::Tuple(vec![f(line), f(signal), f(hist)].into()))
        }),
        "ta.crossover" | "ta.crossunder" => {
            let over = name == "ta.crossover";
            let body: super::NativeFn = if over {
                |_, a, s| Ok(Value::Bool(cross(st(s), a[0].num(), a[1].num(), true)))
            } else {
                |_, a, s| Ok(Value::Bool(cross(st(s), a[0].num(), a[1].num(), false)))
            };
            Some(Native { f: body, init: Some(|| Box::new(Prev::default()) as State) })
        }
        "ta.pivothigh" => native!(Win, |ctx, a, s| {
            let (Some(l), Some(r)) = (a[1].int(), a[2].int()) else { return Ok(Value::Na) };
            len(ctx, &Value::Int(l.max(0) + r.max(0) + 1))?;
            Ok(f(pivot(st(s), a[0].num(), l.max(0) as usize, r.max(0) as usize, true)))
        }),
        "ta.pivotlow" => native!(Win, |ctx, a, s| {
            let (Some(l), Some(r)) = (a[1].int(), a[2].int()) else { return Ok(Value::Na) };
            len(ctx, &Value::Int(l.max(0) + r.max(0) + 1))?;
            Ok(f(pivot(st(s), a[0].num(), l.max(0) as usize, r.max(0) as usize, false)))
        }),
        "ta.valuewhen" => native!(Occurrences, |_, a, s| {
            let occ = a[2].int().unwrap_or(0).max(0) as usize;
            let o: &mut Occurrences = st(s);
            if a[0].truthy() {
                o.0.push_front(a[1].clone());
                o.0.truncate(occ + 1);
            }
            Ok(o.0.get(occ).cloned().unwrap_or(Value::Na))
        }),
        "ta.vwap" => native!(Vwap, |ctx, a, s| {
            let w: &mut Vwap = st(s);
            // Reset on the anchor when given, else on each new day in the exchange timezone.
            let reset = if a[1].is_na() {
                let z = time::exchange_zone(ctx)?;
                let day = Timeframe { multiplier: 1, unit: TfUnit::Day };
                let t = ctx.input.bars.time.get(ctx.bar).copied();
                let start = t.and_then(|t| time::period_start(t, day, z));
                let r = w.period != start;
                w.period = start;
                r
            } else {
                a[1].truthy()
            };
            if reset {
                (w.pv, w.v, w.pv2) = (0.0, 0.0, 0.0);
            }
            let vol = Some(super::volume_at(ctx, ctx.bar)?).filter(|x| x.is_finite());
            if let (Some(x), Some(v)) = (a[0].num(), vol) {
                w.pv += x * v;
                w.v += v;
                w.pv2 += x * x * v;
            }
            let vwap = (w.v != 0.0).then(|| w.pv / w.v);
            match a[2].num() {
                None => Ok(f(vwap)),
                Some(m) => {
                    let sd = vwap.map(|p| libm::sqrt((w.pv2 / w.v - p * p).max(0.0)));
                    let band = |sign: f64| match (vwap, sd) {
                        (Some(p), Some(d)) => Value::float(p + sign * m * d),
                        _ => Value::Na,
                    };
                    Ok(Value::Tuple(vec![f(vwap), band(1.0), band(-1.0)].into()))
                }
            }
        }),
        _ => super::ta_more::native(name),
    }
}

pub(super) fn cross(p: &mut Prev, a: Option<f64>, b: Option<f64>, over: bool) -> bool {
    let r = match (a, b, p.0, p.1) {
        (Some(a), Some(b), Some(pa), Some(pb)) => {
            if over {
                a > b && pa < pb
            } else {
                a < b && pa > pb
            }
        }
        _ => false,
    };
    (p.0, p.1) = (a, b);
    r
}

