//! `ta.*` beyond the 20 most used (phase 4). Same rules as `ta.rs`: per-call-site state, the
//! arithmetic of the reference implementation in the same order, a non-finite intermediate is
//! `na` (as the VM makes it), pinned by `tests/fixtures/ta_ref`.

use super::ta::{cross, ema_step, f, len, rma_step, sma_step, st, true_range, win_extreme, Ema, Prev, Rma, Sma, Win};
use super::{Native, State};
use crate::vm::value::Value;
use crate::vm::Ctx;

macro_rules! native {
    ($ty:ty, $body:expr) => {
        Some(Native { f: $body, init: Some(|| Box::new(<$ty>::default()) as State) })
    };
}

fn fin(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}
fn add(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    fin(a? + b?)
}
fn sub(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    fin(a? - b?)
}
fn mul(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    fin(a? * b?)
}
fn div(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    fin(a? / b?)
}
/// Pine comparisons: `na` on either side is false.
fn gt(a: Option<f64>, b: Option<f64>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a > b)
}
fn lt(a: Option<f64>, b: Option<f64>) -> bool {
    gt(b, a)
}
/// `math.max` / `math.min`: `na` when either is.
fn max(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    Some(a?.max(b?))
}
fn min(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    Some(a?.min(b?))
}

/// Bar `k` back of a price column, `None` before the first bar or when not finite.
fn bar(ctx: &Ctx, col: fn(&crate::input::Bars) -> &Vec<f64>, k: usize) -> Option<f64> {
    let i = ctx.bar.checked_sub(k)?;
    col(&ctx.input.bars).get(i).copied().and_then(fin)
}
fn high(b: &crate::input::Bars) -> &Vec<f64> {
    &b.high
}
fn low(b: &crate::input::Bars) -> &Vec<f64> {
    &b.low
}
fn close(b: &crate::input::Bars) -> &Vec<f64> {
    &b.close
}

fn volume(ctx: &Ctx) -> Result<Option<f64>, (crate::diag::RtCode, Vec<String>)> {
    Ok(fin(super::volume_at(ctx, ctx.bar)?))
}

/// `math.sum` over the last `n` values, oldest first; `na` until full or with an `na` inside.
fn window_sum(w: &mut Win, v: Option<f64>, n: usize) -> Option<f64> {
    w.push(v, n);
    if w.0.len() < n {
        return None;
    }
    let mut s = 0.0;
    for x in &w.0 {
        s += (*x)?;
    }
    fin(s)
}

/// Window values that are not `na`, sorted; `None` until the window is full.
fn sorted(w: &mut Win, v: Option<f64>, n: usize) -> Option<Vec<f64>> {
    w.push(v, n);
    let mut out: Vec<f64> = w.newest_first(n)?.filter_map(|x| *x).collect();
    out.sort_by(f64::total_cmp);
    (!out.is_empty()).then_some(out)
}

/// Offset (0 or negative) of the window's extreme, the newest on a tie.
fn extreme_bars(w: &mut Win, v: Option<f64>, n: usize, high: bool) -> Value {
    w.push(v, n);
    let Some(it) = w.newest_first(n) else { return Value::Na };
    let mut best: Option<(f64, usize)> = None;
    for (i, x) in it.enumerate() {
        let Some(x) = *x else { continue };
        if best.is_none_or(|(b, _)| if high { x > b } else { x < b }) {
            best = Some((x, i));
        }
    }
    best.map(|(_, i)| Value::Int(-(i as i64))).unwrap_or(Value::Na)
}

/// Mean absolute deviation around `mean` over the window (`ta.dev`).
fn mean_dev(w: &Sma, n: usize, mean: f64) -> Option<f64> {
    let mut sum = 0.0;
    for x in w.0.newest_first(n)? {
        sum += ((*x)? - mean).abs();
    }
    fin(sum / n as f64)
}

/// Sum of squared deviations around `avg` over `n` (`n - 1` unbiased).
fn sq_dev(w: &Sma, n: usize, avg: f64, biased: bool) -> Option<f64> {
    let mut sum = 0.0;
    for x in w.0.newest_first(n)? {
        let d = x.unwrap_or(f64::NAN) - avg;
        sum += d * d;
    }
    fin(sum / if biased { n as f64 } else { n as f64 - 1.0 })
}

#[derive(Default, Clone)]
struct Cum(f64);

#[derive(Default, Clone)]
struct Since(Option<usize>);

#[derive(Default, Clone)]
struct Wins(Win, Win);

#[derive(Default, Clone)]
struct Supertrend {
    atr: Rma,
    prev_atr: Option<f64>,
    prev_lower: Option<f64>,
    prev_upper: Option<f64>,
    prev_st: Option<f64>,
}

#[derive(Default, Clone)]
struct Sar {
    result: Option<f64>,
    max_min: Option<f64>,
    acc: Option<f64>,
    is_below: bool,
}

#[derive(Default, Clone)]
struct SmaPair(Sma, Sma);

#[derive(Default, Clone)]
struct Mfi {
    prev: Option<f64>,
    up: Win,
    down: Win,
}

#[derive(Default, Clone)]
struct Dmi {
    tr: Rma,
    plus: Rma,
    minus: Rma,
    adx: Rma,
    fix_plus: Option<f64>,
    fix_minus: Option<f64>,
}

#[derive(Default, Clone)]
struct Tsi {
    prev: Option<f64>,
    e1: Ema,
    e2: Ema,
    a1: Ema,
    a2: Ema,
}

/// `ta.obv` (a variable): `ta.cum(math.sign(ta.change(close)) * volume)`.
pub fn obv() -> Native {
    Native {
        f: |ctx, _, s| {
            let ch = sub(bar(ctx, close, 0), bar(ctx, close, 1));
            let term = mul(ch.map(|x| if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }), volume(ctx)?);
            let c: &mut Cum = st(s);
            c.0 += term.unwrap_or(0.0);
            Ok(Value::float(c.0))
        },
        init: Some(|| Box::new(Cum::default()) as State),
    }
}

pub fn native(name: &str) -> Option<Native> {
    match name {
        "ta.cum" => native!(Cum, |_, a, s| {
            let c: &mut Cum = st(s);
            c.0 += a[0].num().unwrap_or(0.0);
            Ok(Value::float(c.0))
        }),
        "ta.barssince" => native!(Since, |ctx, a, s| {
            let since: &mut Since = st(s);
            if a[0].truthy() {
                since.0 = Some(ctx.bar);
            }
            Ok(since.0.map(|b| Value::Int((ctx.bar - b) as i64)).unwrap_or(Value::Na))
        }),
        "ta.linreg" => native!(Win, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let w: &mut Win = st(s);
            w.push(a[0].num(), n);
            let line = || -> Option<f64> {
                let off = a[2].num()?;
                let (mut sx, mut sy, mut sxy, mut sxx) = (0.0, 0.0, 0.0, 0.0);
                for (i, y) in w.newest_first(n)?.enumerate() {
                    let x = (n - 1 - i) as f64;
                    let y = (*y)?;
                    sx += x;
                    sy += y;
                    sxy += x * y;
                    sxx += x * x;
                }
                let nf = n as f64;
                let slope = fin((nf * sxy - sx * sy) / (nf * sxx - sx * sx))?;
                let intercept = (sy - slope * sx) / nf;
                fin(intercept + slope * (nf - 1.0 - off))
            };
            Ok(f(line()))
        }),
        "ta.stoch" => native!(Wins, |ctx, a, s| {
            let Some(n) = len(ctx, &a[3])? else { return Ok(Value::Na) };
            let w: &mut Wins = st(s);
            w.0.push(a[1].num(), n);
            w.1.push(a[2].num(), n);
            let (hh, ll) = (win_extreme(&w.0, n, true), win_extreme(&w.1, n, false));
            Ok(f(div(mul(Some(100.0), sub(a[0].num(), ll)), sub(hh, ll))))
        }),
        "ta.cross" => native!(Prev, |_, a, s| {
            let p: &mut Prev = st(s);
            let mut q = p.clone();
            let over = cross(p, a[0].num(), a[1].num(), true);
            let under = cross(&mut q, a[0].num(), a[1].num(), false);
            Ok(Value::Bool(over || under))
        }),
        "ta.median" => native!(Win, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let Some(v) = sorted(st(s), a[0].num(), n) else { return Ok(Value::Na) };
            let m = v.len();
            Ok(Value::float(if m % 2 == 1 { v[m / 2] } else { (v[m / 2 - 1] + v[m / 2]) / 2.0 }))
        }),
        "ta.percentile_nearest_rank" => native!(Win, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let Some(v) = sorted(st(s), a[0].num(), n) else { return Ok(Value::Na) };
            let Some(p) = a[2].num() else { return Ok(Value::Na) };
            let rank = libm::ceil(p.clamp(0.0, 100.0) / 100.0 * v.len() as f64) as usize;
            Ok(Value::float(v[rank.clamp(1, v.len()) - 1]))
        }),
        "ta.percentile_linear_interpolation" => native!(Win, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let Some(v) = sorted(st(s), a[0].num(), n) else { return Ok(Value::Na) };
            let Some(p) = a[2].num() else { return Ok(Value::Na) };
            let pos = p.clamp(0.0, 100.0) / 100.0 * (v.len() - 1) as f64;
            let (lo, hi) = (libm::floor(pos) as usize, (libm::ceil(pos) as usize).min(v.len() - 1));
            Ok(Value::float(v[lo] + (pos - lo as f64) * (v[hi] - v[lo])))
        }),
        "ta.supertrend" => native!(Supertrend, |ctx, a, s| {
            let t: &mut Supertrend = st(s);
            let atr = match len(ctx, &a[1])? {
                Some(n) => rma_step(&mut t.atr, true_range(ctx, true), n),
                None => None,
            };
            let src = div(add(bar(ctx, high, 0), bar(ctx, low, 0)), Some(2.0));
            let mut upper = add(src, mul(a[0].num(), atr));
            let mut lower = sub(src, mul(a[0].num(), atr));
            let prev_lower = t.prev_lower.unwrap_or(0.0);
            let prev_upper = t.prev_upper.unwrap_or(0.0);
            let close1 = bar(ctx, close, 1);
            if !(gt(lower, Some(prev_lower)) || lt(close1, Some(prev_lower))) {
                lower = Some(prev_lower);
            }
            if !(lt(upper, Some(prev_upper)) || gt(close1, Some(prev_upper))) {
                upper = Some(prev_upper);
            }
            let c = bar(ctx, close, 0);
            let dir: i64 = if t.prev_atr.is_none() {
                1
            } else if t.prev_st == Some(prev_upper) {
                if gt(c, upper) { -1 } else { 1 }
            } else if lt(c, lower) {
                1
            } else {
                -1
            };
            let trend = if dir == -1 { lower } else { upper };
            (t.prev_atr, t.prev_lower, t.prev_upper, t.prev_st) = (atr, lower, upper, trend);
            Ok(Value::Tuple(vec![f(trend), Value::Int(dir)].into()))
        }),
        "ta.highestbars" | "ta.lowestbars" => {
            let body: super::NativeFn = if name == "ta.highestbars" {
                |ctx, a, s| Ok(len(ctx, &a[1])?.map(|n| extreme_bars(st(s), a[0].num(), n, true)).unwrap_or(Value::Na))
            } else {
                |ctx, a, s| Ok(len(ctx, &a[1])?.map(|n| extreme_bars(st(s), a[0].num(), n, false)).unwrap_or(Value::Na))
            };
            Some(Native { f: body, init: Some(|| Box::new(Win::default()) as State) })
        }
        "ta.percentrank" => native!(Win, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let w: &mut Win = st(s);
            w.push(a[0].num(), n + 1);
            let rank = || -> Option<f64> {
                let mut it = w.newest_first(n + 1)?;
                let cur = (*it.next()?)?;
                let cnt = it.filter(|x| x.is_some_and(|x| x <= cur)).count();
                fin(100.0 * cnt as f64 / n as f64)
            };
            Ok(f(rank()))
        }),
        "ta.roc" | "ta.mom" => {
            let body: super::NativeFn = if name == "ta.roc" {
                |ctx, a, s| {
                    let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
                    let old = back(st(s), a[0].num(), n);
                    Ok(f(div(mul(Some(100.0), sub(a[0].num(), old)), old)))
                }
            } else {
                |ctx, a, s| {
                    let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
                    let old = back(st(s), a[0].num(), n);
                    Ok(f(sub(a[0].num(), old)))
                }
            };
            Some(Native { f: body, init: Some(|| Box::new(Win::default()) as State) })
        }
        "ta.sar" => native!(Sar, |ctx, a, s| {
            let (start, inc, top) = (a[0].num(), a[1].num(), a[2].num());
            let t: &mut Sar = st(s);
            let i = ctx.bar;
            let (h, l) = (bar(ctx, high, 0), bar(ctx, low, 0));
            let mut first = false;
            if i == 1 {
                if gt(bar(ctx, close, 0), bar(ctx, close, 1)) {
                    t.is_below = true;
                    t.max_min = h;
                    t.result = bar(ctx, low, 1);
                } else {
                    t.is_below = false;
                    t.max_min = l;
                    t.result = bar(ctx, high, 1);
                }
                first = true;
                t.acc = start;
            }
            t.result = add(t.result, mul(t.acc, sub(t.max_min, t.result)));
            if t.is_below {
                if gt(t.result, l) {
                    first = true;
                    t.is_below = false;
                    t.result = max(h, t.max_min);
                    t.max_min = l;
                    t.acc = start;
                }
            } else if lt(t.result, h) {
                first = true;
                t.is_below = true;
                t.result = min(l, t.max_min);
                t.max_min = h;
                t.acc = start;
            }
            if !first {
                if t.is_below {
                    if gt(h, t.max_min) {
                        t.max_min = h;
                        t.acc = min(add(t.acc, inc), top);
                    }
                } else if lt(l, t.max_min) {
                    t.max_min = l;
                    t.acc = min(add(t.acc, inc), top);
                }
            }
            if t.is_below {
                t.result = min(t.result, bar(ctx, low, 1));
                if i > 1 {
                    t.result = min(t.result, bar(ctx, low, 2));
                }
            } else {
                t.result = max(t.result, bar(ctx, high, 1));
                if i > 1 {
                    t.result = max(t.result, bar(ctx, high, 2));
                }
            }
            Ok(f(t.result))
        }),
        "ta.dev" => native!(Sma, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let w: &mut Sma = st(s);
            let Some(mean) = sma_step(w, a[0].num(), n) else { return Ok(Value::Na) };
            Ok(f(mean_dev(w, n, mean)))
        }),
        "ta.cci" => native!(Sma, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let w: &mut Sma = st(s);
            let Some(mean) = sma_step(w, a[0].num(), n) else { return Ok(Value::Na) };
            Ok(f(div(sub(a[0].num(), Some(mean)), mul(Some(0.015), mean_dev(w, n, mean)))))
        }),
        "ta.variance" => native!(Sma, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let biased = !matches!(a[2], Value::Bool(false));
            Ok(f(super::ta::variance_step(st(s), a[0].num(), n, biased)))
        }),
        "ta.bb" => native!(Sma, |ctx, a, s| {
            let na = || Value::Tuple(vec![Value::Na, Value::Na, Value::Na].into());
            let Some(n) = len(ctx, &a[1])? else { return Ok(na()) };
            let w: &mut Sma = st(s);
            let basis = sma_step(w, a[0].num(), n);
            let dev = mul(a[2].num(), basis.and_then(|b| sq_dev(w, n, b, true)).map(libm::sqrt));
            Ok(Value::Tuple(vec![f(basis), f(add(basis, dev)), f(sub(basis, dev))].into()))
        }),
        "ta.correlation" => native!(SmaPair, |ctx, a, s| {
            let Some(n) = len(ctx, &a[2])? else { return Ok(Value::Na) };
            let p: &mut SmaPair = st(s);
            let (ma, mb) = (sma_step(&mut p.0, a[0].num(), n), sma_step(&mut p.1, a[1].num(), n));
            let corr = || -> Option<f64> {
                let (ma, mb) = (ma?, mb?);
                let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
                for (x, y) in p.0 .0.newest_first(n)?.zip(p.1 .0.newest_first(n)?) {
                    let (da, db) = ((*x)? - ma, (*y)? - mb);
                    sab += da * db;
                    saa += da * da;
                    sbb += db * db;
                }
                fin(sab / libm::sqrt(saa * sbb))
            };
            Ok(f(corr()))
        }),
        "ta.rising" | "ta.falling" => {
            let body: super::NativeFn = if name == "ta.rising" {
                |ctx, a, s| Ok(Value::Bool(len(ctx, &a[1])?.is_some_and(|n| monotone(st(s), a[0].num(), n, true))))
            } else {
                |ctx, a, s| Ok(Value::Bool(len(ctx, &a[1])?.is_some_and(|n| monotone(st(s), a[0].num(), n, false))))
            };
            Some(Native { f: body, init: Some(|| Box::new(Win::default()) as State) })
        }
        "ta.mfi" => native!(Mfi, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let vol = volume(ctx)?;
            let m: &mut Mfi = st(s);
            let x = a[0].num();
            let ch = sub(x, m.prev);
            m.prev = x;
            let up = mul(vol, if ch.is_some_and(|c| c <= 0.0) { Some(0.0) } else { x });
            let down = mul(vol, if ch.is_some_and(|c| c >= 0.0) { Some(0.0) } else { x });
            let (upper, lower) = (window_sum(&mut m.up, up, n), window_sum(&mut m.down, down, n));
            Ok(f(sub(Some(100.0), div(Some(100.0), add(Some(1.0), div(upper, lower))))))
        }),
        "ta.dmi" => native!(Dmi, |ctx, a, s| {
            let na = || Value::Tuple(vec![Value::Na, Value::Na, Value::Na].into());
            let (Some(n), Some(sig)) = (len(ctx, &a[0])?, len(ctx, &a[1])?) else { return Ok(na()) };
            let d: &mut Dmi = st(s);
            let up = sub(bar(ctx, high, 0), bar(ctx, high, 1));
            let down = sub(bar(ctx, low, 0), bar(ctx, low, 1)).map(|x| -x);
            let plus_dm = up.map(|u| if gt(Some(u), down) && u > 0.0 { u } else { 0.0 });
            let minus_dm = down.map(|x| if gt(Some(x), up) && x > 0.0 { x } else { 0.0 });
            let trur = rma_step(&mut d.tr, true_range(ctx, false), n);
            if let Some(p) = div(mul(Some(100.0), rma_step(&mut d.plus, plus_dm, n)), trur) {
                d.fix_plus = Some(p);
            }
            if let Some(m) = div(mul(Some(100.0), rma_step(&mut d.minus, minus_dm, n)), trur) {
                d.fix_minus = Some(m);
            }
            let (plus, minus) = (d.fix_plus, d.fix_minus);
            let sum = add(plus, minus);
            let denom = if sum == Some(0.0) { Some(1.0) } else { sum };
            let adx = mul(Some(100.0), rma_step(&mut d.adx, div(sub(plus, minus).map(f64::abs), denom), sig));
            Ok(Value::Tuple(vec![f(plus), f(minus), f(adx)].into()))
        }),
        "ta.range" => native!(Win, |ctx, a, s| {
            let Some(n) = len(ctx, &a[1])? else { return Ok(Value::Na) };
            let w: &mut Win = st(s);
            w.push(a[0].num(), n);
            Ok(f(sub(win_extreme(w, n, true), win_extreme(w, n, false))))
        }),
        "ta.wpr" => native!(Wins, |ctx, a, s| {
            let Some(n) = len(ctx, &a[0])? else { return Ok(Value::Na) };
            let w: &mut Wins = st(s);
            w.0.push(bar(ctx, high, 0), n);
            w.1.push(bar(ctx, low, 0), n);
            let (hh, ll) = (win_extreme(&w.0, n, true), win_extreme(&w.1, n, false));
            Ok(f(div(mul(Some(100.0), sub(bar(ctx, close, 0), hh)), sub(hh, ll))))
        }),
        "ta.tsi" => native!(Tsi, |ctx, a, s| {
            let (Some(short), Some(long)) = (len(ctx, &a[1])?, len(ctx, &a[2])?) else { return Ok(Value::Na) };
            let t: &mut Tsi = st(s);
            let x = a[0].num();
            let pc = sub(x, t.prev);
            t.prev = x;
            let num = ema_step(&mut t.e2, ema_step(&mut t.e1, pc, long), short);
            let den = ema_step(&mut t.a2, ema_step(&mut t.a1, pc.map(f64::abs), long), short);
            Ok(f(div(num, den)))
        }),
        _ => None,
    }
}

/// The value `n` calls back, `None` until there is one.
fn back(w: &mut Win, v: Option<f64>, n: usize) -> Option<f64> {
    w.push(v, n + 1);
    if w.0.len() < n + 1 {
        return None;
    }
    w.0[0]
}

/// The newest value is above (below) each of the `n` before it.
fn monotone(w: &mut Win, v: Option<f64>, n: usize, up: bool) -> bool {
    w.push(v, n + 1);
    let Some(mut it) = w.newest_first(n + 1) else { return false };
    let cur = *it.next().expect("n + 1 values");
    it.all(|x| if up { gt(cur, *x) } else { lt(cur, *x) })
}
