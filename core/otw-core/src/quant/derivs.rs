//! Derivatives on provider data: a futures curve built from its dated contracts, an
//! implied-volatility surface from an option chain, and implied against realized volatility.
//!
//! Dates are Julian day numbers here (`time::Date::to_julian_day`), so a calendar-day count is
//! a subtraction. The API layer turns them back into ISO dates.

use serde::Serialize;

use super::calc::{bsm, implied_vol};
use super::{mean, ols, stddev};

fn iso(jd: i32) -> String {
    time::Date::from_julian_day(jd).map(|d| d.to_string()).unwrap_or_default()
}

/// Price of a series on a date, by binary search over its sorted `(day, close)` pairs.
fn price_on(closes: &[(i32, f64)], day: i32) -> Option<f64> {
    closes.binary_search_by_key(&day, |p| p.0).ok().map(|i| closes[i].1)
}

// ── Futures curve ────────────────────────────────────────────────────────────────────────

/// One dated contract and its daily closes, sorted by day.
#[derive(Debug, Clone)]
pub struct FutSeries {
    pub ticker: String,
    pub last_trade: i32,
    pub closes: Vec<(i32, f64)>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CurvePoint {
    pub ticker: String,
    pub last_trade: String,
    pub dte: i32,
    pub price: f64,
    /// ln(F / F_front) annualized over the days between the two expiries.
    pub carry_from_front: Option<f64>,
    /// ln(F / S) annualized over the contract's days to expiry.
    pub carry_from_spot: Option<f64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Roll {
    pub date: String,
    pub from: String,
    pub to: String,
    pub from_price: Option<f64>,
    pub to_price: f64,
    /// to / from − 1: the price jump a spliced chart shows and a back-adjusted one removes.
    pub gap: Option<f64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CurveRow {
    pub date: String,
    pub front: String,
    pub dte: i32,
    pub front_price: f64,
    pub next_price: Option<f64>,
    /// ln(F_next / F_front) × 365 / (days between the expiries). Positive in contango.
    pub roll_yield: Option<f64>,
    /// Ratio back-adjusted continuous price: the return of holding the front and rolling,
    /// scaled so the last value is the current front price.
    pub continuous: f64,
    pub spot: Option<f64>,
    pub basis: Option<f64>,
    pub basis_pct: Option<f64>,
    /// ln(F / S) × 365 / dte: the carry the market prices, which is the implied repo net of
    /// the yield.
    pub carry: Option<f64>,
    pub implied_repo: Option<f64>,
    pub fair: Option<f64>,
    pub mispricing: Option<f64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ContractUsed {
    pub ticker: String,
    pub last_trade: String,
    pub bars: usize,
    pub first: Option<String>,
    pub last: Option<String>,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct CurveSummary {
    pub days: usize,
    pub first: String,
    pub last: String,
    pub contango_share: Option<f64>,
    pub mean_roll_yield: Option<f64>,
    pub current_roll_yield: Option<f64>,
    /// Holding the front and rolling, over the whole sample.
    pub futures_return: f64,
    pub futures_ann: Option<f64>,
    /// Spot over the dates both series share, and the futures return over those same dates.
    pub spot_return: Option<f64>,
    pub futures_return_on_spot_dates: Option<f64>,
    /// ln(futures growth) − ln(spot growth), per year: what rolling added or cost.
    pub roll_return_ann: Option<f64>,
    pub mean_carry: Option<f64>,
    pub current_carry: Option<f64>,
    pub mean_mispricing_pct: Option<f64>,
    pub rolls: usize,
    /// Days whose return had to come from the new contract because the held one had no
    /// price that day.
    pub gaps: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct FuturesCurve {
    pub as_of: String,
    pub shape: &'static str,
    pub curve: Vec<CurvePoint>,
    pub rolls: Vec<Roll>,
    pub rows: Vec<CurveRow>,
    pub summary: CurveSummary,
    pub contracts: Vec<ContractUsed>,
}

/// Term structure, roll and basis from the dated contracts of one product.
///
/// The front on a day is the nearest contract with more than `roll_days` calendar days left:
/// a calendar rule, so the roll dates do not depend on which bars a provider happens to
/// have. A day the front has no price is skipped. The position held over (t−1, t] is the
/// front of t−1, which is rolled into the front of t at t's close.
pub fn futures_curve(
    series: &[FutSeries],
    spot: Option<&[(i32, f64)]>,
    rate: Option<f64>,
    yield_q: f64,
    roll_days: i32,
) -> Option<FuturesCurve> {
    let mut s: Vec<&FutSeries> = series.iter().filter(|c| !c.closes.is_empty()).collect();
    s.sort_by_key(|c| (c.last_trade, c.ticker.clone()));
    if s.is_empty() {
        return None;
    }
    let mut days: Vec<i32> = s.iter().flat_map(|c| c.closes.iter().map(|p| p.0)).collect();
    days.sort_unstable();
    days.dedup();

    let front_of = |d: i32| s.iter().position(|c| c.last_trade - d > roll_days);
    let spot_on = |d: i32| spot.and_then(|sp| price_on(sp, d));

    let mut rows = Vec::new();
    let mut rolls = Vec::new();
    let mut index: Vec<f64> = Vec::new();
    let mut fronts: Vec<usize> = Vec::new();
    let mut row_days: Vec<i32> = Vec::new();
    let mut gaps = 0usize;
    for &d in &days {
        let Some(fi) = front_of(d) else { continue };
        let Some(fp) = price_on(&s[fi].closes, d) else { continue };
        // Return over (previous row, d] on the contract held through it.
        let level = match (fronts.last(), row_days.last(), index.last()) {
            (Some(&held), Some(&pd), Some(&lvl)) => {
                let r = match (price_on(&s[held].closes, d), price_on(&s[held].closes, pd)) {
                    (Some(a), Some(b)) if b > 0.0 => a / b - 1.0,
                    _ => {
                        gaps += 1;
                        match price_on(&s[fi].closes, pd) {
                            Some(b) if b > 0.0 => fp / b - 1.0,
                            _ => 0.0,
                        }
                    }
                };
                if held != fi {
                    let from_price = price_on(&s[held].closes, d);
                    rolls.push(Roll {
                        date: iso(d),
                        from: s[held].ticker.clone(),
                        to: s[fi].ticker.clone(),
                        from_price,
                        to_price: fp,
                        gap: from_price.filter(|p| *p > 0.0).map(|p| fp / p - 1.0),
                    });
                }
                lvl * (1.0 + r)
            }
            _ => 1.0,
        };
        index.push(level);
        fronts.push(fi);
        row_days.push(d);

        let dte = s[fi].last_trade - d;
        let next_price = s.get(fi + 1).and_then(|n| price_on(&n.closes, d));
        let roll_yield = next_price.and_then(|np| {
            let span = s[fi + 1].last_trade - s[fi].last_trade;
            (span > 0 && np > 0.0 && fp > 0.0).then(|| (np / fp).ln() * 365.0 / span as f64)
        });
        let sp = spot_on(d).filter(|v| *v > 0.0);
        let t = dte as f64 / 365.0;
        let carry = sp.filter(|_| dte > 0 && fp > 0.0).map(|v| (fp / v).ln() / t);
        let fair = match (sp, rate) {
            (Some(v), Some(r)) => Some(v * ((r - yield_q) * t).exp()),
            _ => None,
        };
        rows.push(CurveRow {
            date: iso(d),
            front: s[fi].ticker.clone(),
            dte,
            front_price: fp,
            next_price,
            roll_yield,
            continuous: 0.0,
            spot: sp,
            basis: sp.map(|v| fp - v),
            basis_pct: sp.map(|v| fp / v - 1.0),
            carry,
            implied_repo: carry.map(|c| c + yield_q),
            fair,
            mispricing: fair.map(|f| fp - f),
        });
    }
    if rows.is_empty() {
        return None;
    }
    // Back-adjust by ratio so the latest point is the price actually quoted today.
    let last_lvl = *index.last()?;
    let scale = rows.last()?.front_price / last_lvl;
    for (r, l) in rows.iter_mut().zip(&index) {
        r.continuous = l * scale;
    }

    // The curve on the latest day the front printed.
    let as_of = *row_days.last()?;
    let s_now = spot_on(as_of).filter(|v| *v > 0.0);
    let mut curve: Vec<CurvePoint> = Vec::new();
    for c in s.iter().filter(|c| c.last_trade >= as_of) {
        let Some(p) = price_on(&c.closes, as_of) else { continue };
        let dte = c.last_trade - as_of;
        let carry_from_front = curve.first().and_then(|f: &CurvePoint| {
            let span = dte - f.dte;
            (span > 0 && p > 0.0 && f.price > 0.0).then(|| (p / f.price).ln() * 365.0 / span as f64)
        });
        curve.push(CurvePoint {
            ticker: c.ticker.clone(),
            last_trade: iso(c.last_trade),
            dte,
            price: p,
            carry_from_front,
            carry_from_spot: s_now.filter(|_| dte > 0 && p > 0.0).map(|v| (p / v).ln() * 365.0 / dte as f64),
        });
    }
    let shape = if curve.len() < 2 {
        "flat"
    } else if curve.windows(2).all(|w| w[1].price > w[0].price) {
        "contango"
    } else if curve.windows(2).all(|w| w[1].price < w[0].price) {
        "backwardation"
    } else {
        "mixed"
    };

    let ry: Vec<f64> = rows.iter().filter_map(|r| r.roll_yield).collect();
    let pairs: Vec<(f64, f64)> =
        rows.iter().filter_map(|r| r.next_price.map(|n| (r.front_price, n))).collect();
    let span_days = (row_days.last()? - row_days.first()?) as f64;
    let fut_total = index.last()? / index.first()? - 1.0;
    // Spot against the continuous series on the first and last day both have.
    let both: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].spot.is_some()).collect();
    let (mut spot_ret, mut fut_on_spot, mut roll_ann) = (None, None, None);
    if let (Some(&a), Some(&b)) = (both.first(), both.last()) {
        if b > a {
            let sr = rows[b].spot? / rows[a].spot?;
            let fr = index[b] / index[a];
            spot_ret = Some(sr - 1.0);
            fut_on_spot = Some(fr - 1.0);
            let yrs = (row_days[b] - row_days[a]) as f64 / 365.25;
            if yrs > 0.0 {
                roll_ann = Some((fr.ln() - sr.ln()) / yrs);
            }
        }
    }
    let carries: Vec<f64> = rows.iter().filter_map(|r| r.carry).collect();
    let mis: Vec<f64> = rows
        .iter()
        .filter_map(|r| match (r.mispricing, r.spot) {
            (Some(m), Some(sp)) => Some(m / sp),
            _ => None,
        })
        .collect();
    let summary = CurveSummary {
        days: rows.len(),
        first: rows.first()?.date.clone(),
        last: rows.last()?.date.clone(),
        contango_share: (!pairs.is_empty())
            .then(|| pairs.iter().filter(|(f, n)| n > f).count() as f64 / pairs.len() as f64),
        mean_roll_yield: (!ry.is_empty()).then(|| mean(&ry)),
        current_roll_yield: rows.last()?.roll_yield,
        futures_return: fut_total,
        futures_ann: (span_days > 0.0).then(|| (1.0 + fut_total).powf(365.25 / span_days) - 1.0),
        spot_return: spot_ret,
        futures_return_on_spot_dates: fut_on_spot,
        roll_return_ann: roll_ann,
        mean_carry: (!carries.is_empty()).then(|| mean(&carries)),
        current_carry: rows.last()?.carry,
        mean_mispricing_pct: (!mis.is_empty()).then(|| mean(&mis)),
        rolls: rolls.len(),
        gaps,
    };
    let contracts = series
        .iter()
        .map(|c| ContractUsed {
            ticker: c.ticker.clone(),
            last_trade: iso(c.last_trade),
            bars: c.closes.len(),
            first: c.closes.first().map(|p| iso(p.0)),
            last: c.closes.last().map(|p| iso(p.0)),
        })
        .collect();
    Some(FuturesCurve { as_of: iso(as_of), shape, curve, rolls, rows, summary, contracts })
}

// ── Implied-volatility surface ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OptQuote {
    pub ticker: String,
    pub expiry: i32,
    pub strike: f64,
    pub call: bool,
    pub price: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct SmilePoint {
    pub ticker: String,
    pub strike: f64,
    pub call: bool,
    pub price: f64,
    /// ln(K / F).
    pub moneyness: f64,
    pub iv: f64,
    pub delta: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct Expiry {
    pub expiry: String,
    pub dte: i32,
    pub forward: f64,
    pub atm_iv: Option<f64>,
    /// IV of the 25-delta call minus the 25-delta put.
    pub rr25: Option<f64>,
    /// Mean of the two 25-delta IVs minus the ATM IV.
    pub bf25: Option<f64>,
    pub put25: Option<f64>,
    pub call25: Option<f64>,
    pub points: Vec<SmilePoint>,
    /// Quotes with no volatility that reproduces them (below intrinsic, or above the bound).
    pub dropped: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct Surface {
    pub spot: f64,
    pub as_of: String,
    pub rate: f64,
    pub dividend: f64,
    pub expiries: Vec<Expiry>,
    /// ATM total variance σ²T falling from one expiry to the next is a calendar arbitrage.
    pub calendar_violations: Vec<String>,
    pub front_atm: Option<f64>,
    pub back_atm: Option<f64>,
    pub quotes: usize,
    pub solved: usize,
}

/// Linear interpolation of `y` at `x0` over points sorted by `x`; `None` outside the range.
fn interp(pts: &[(f64, f64)], x0: f64) -> Option<f64> {
    for w in pts.windows(2) {
        let ((x1, y1), (x2, y2)) = (w[0], w[1]);
        if (x1 <= x0 && x0 <= x2) || (x2 <= x0 && x0 <= x1) {
            if (x2 - x1).abs() < 1e-15 {
                return Some((y1 + y2) / 2.0);
            }
            return Some(y1 + (y2 - y1) * (x0 - x1) / (x2 - x1));
        }
    }
    None
}

/// Black-Scholes-Merton implied volatility of every quote, organized by expiry.
///
/// Each strike is read on its out-of-the-money side (puts below the forward, calls at or
/// above), which is where the price holds time value rather than intrinsic value, and where
/// early exercise of an American option is worth next to nothing. ATM IV interpolates in
/// ln(K/F) at zero; the 25-delta points interpolate IV against the BSM delta of each side.
pub fn iv_surface(spot: f64, as_of: i32, quotes: &[OptQuote], rate: f64, q: f64) -> Surface {
    let mut exps: Vec<i32> = quotes.iter().map(|o| o.expiry).filter(|e| *e > as_of).collect();
    exps.sort_unstable();
    exps.dedup();
    let mut out = Vec::new();
    let mut solved = 0;
    for &e in &exps {
        let dte = e - as_of;
        let t = dte as f64 / 365.0;
        let fwd = spot * ((rate - q) * t).exp();
        let mut pts = Vec::new();
        let mut dropped = 0;
        for o in quotes.iter().filter(|o| o.expiry == e) {
            match implied_vol(o.call, o.price, spot, o.strike, t, rate, q) {
                Some(iv) => {
                    let delta = bsm(o.call, spot, o.strike, t, rate, q, iv).map(|g| g.delta).unwrap_or(f64::NAN);
                    pts.push(SmilePoint {
                        ticker: o.ticker.clone(),
                        strike: o.strike,
                        call: o.call,
                        price: o.price,
                        moneyness: (o.strike / fwd).ln(),
                        iv,
                        delta,
                    });
                }
                None => dropped += 1,
            }
        }
        solved += pts.len();
        pts.sort_by(|a, b| a.strike.total_cmp(&b.strike).then(a.call.cmp(&b.call)));
        // One reading per strike: the out-of-the-money side when both were quoted.
        let mut otm: Vec<&SmilePoint> = Vec::new();
        for p in &pts {
            let is_otm = if p.call { p.strike >= fwd } else { p.strike < fwd };
            match otm.last() {
                Some(last) if (last.strike - p.strike).abs() < 1e-9 => {
                    if is_otm {
                        otm.pop();
                        otm.push(p);
                    }
                }
                _ => otm.push(p),
            }
        }
        let by_k: Vec<(f64, f64)> = otm.iter().map(|p| (p.moneyness, p.iv)).collect();
        let atm_iv = interp(&by_k, 0.0);
        let mut calls: Vec<(f64, f64)> =
            pts.iter().filter(|p| p.call && p.strike >= fwd).map(|p| (p.delta, p.iv)).collect();
        calls.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut puts: Vec<(f64, f64)> =
            pts.iter().filter(|p| !p.call && p.strike < fwd).map(|p| (p.delta, p.iv)).collect();
        puts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let call25 = interp(&calls, 0.25);
        let put25 = interp(&puts, -0.25);
        let rr25 = match (call25, put25) {
            (Some(c), Some(p)) => Some(c - p),
            _ => None,
        };
        let bf25 = match (call25, put25, atm_iv) {
            (Some(c), Some(p), Some(a)) => Some((c + p) / 2.0 - a),
            _ => None,
        };
        out.push(Expiry {
            expiry: iso(e),
            dte,
            forward: fwd,
            atm_iv,
            rr25,
            bf25,
            put25,
            call25,
            points: pts,
            dropped,
        });
    }
    let mut calendar_violations = Vec::new();
    let atm: Vec<(&Expiry, f64)> =
        out.iter().filter_map(|e| e.atm_iv.map(|v| (e, v * v * e.dte as f64 / 365.0))).collect();
    for w in atm.windows(2) {
        if w[1].1 < w[0].1 {
            calendar_violations.push(w[1].0.expiry.clone());
        }
    }
    let front_atm = out.iter().find_map(|e| e.atm_iv);
    let back_atm = out.iter().rev().find_map(|e| e.atm_iv);
    Surface {
        spot,
        as_of: iso(as_of),
        rate,
        dividend: q,
        expiries: out,
        calendar_violations,
        front_atm,
        back_atm,
        quotes: quotes.len(),
        solved,
    }
}

// ── Implied against realized volatility ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct IvRow {
    pub date: String,
    pub iv: f64,
    pub close: f64,
    /// Close-to-close volatility over the trailing window, annualized with 252.
    pub rv: Option<f64>,
    /// The same over the window that follows: what the IV of that day was forecasting.
    pub rv_forward: Option<f64>,
    pub hv: Option<f64>,
    pub rank: Option<f64>,
    pub percentile: Option<f64>,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct IvSummary {
    pub observations: usize,
    pub window: usize,
    pub lookback: usize,
    pub current_iv: f64,
    pub current_rv: Option<f64>,
    pub current_hv: Option<f64>,
    pub rank: Option<f64>,
    pub percentile: Option<f64>,
    pub mean_iv: f64,
    pub mean_rv_forward: Option<f64>,
    /// Mean of IV minus the realized volatility that followed.
    pub mean_vrp: Option<f64>,
    /// Share of days IV came in above the realized volatility that followed.
    pub iv_above_share: Option<f64>,
    /// Forward realized regressed on IV: slope 1 and intercept 0 is an unbiased forecast.
    pub fwd_alpha: Option<f64>,
    pub fwd_beta: Option<f64>,
    pub fwd_r2: Option<f64>,
    /// Correlation of daily IV changes with daily log returns (the leverage effect).
    pub iv_return_corr: Option<f64>,
    pub forecast_pairs: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct IvHistory {
    pub rows: Vec<IvRow>,
    pub summary: IvSummary,
}

fn corr(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 3 {
        return None;
    }
    let (ma, mb) = (mean(&a[..n]), mean(&b[..n]));
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for i in 0..n {
        sab += (a[i] - ma) * (b[i] - mb);
        saa += (a[i] - ma).powi(2);
        sbb += (b[i] - mb).powi(2);
    }
    (saa > 0.0 && sbb > 0.0).then(|| sab / (saa * sbb).sqrt())
}

/// IV against realized volatility on common days. `days`, `iv`, `closes` (and `hv` when
/// given) are aligned. Rank and percentile look back over `lookback` days: the rank places
/// today between the window's low and high, the percentile is the share of the prior days in
/// the window with a lower IV.
pub fn iv_history(
    days: &[i32],
    iv: &[f64],
    closes: &[f64],
    hv: Option<&[f64]>,
    window: usize,
    lookback: usize,
) -> Option<IvHistory> {
    let n = days.len().min(iv.len()).min(closes.len());
    if n < window + 2 || window < 2 {
        return None;
    }
    let r: Vec<f64> = (1..n).map(|i| (closes[i] / closes[i - 1]).ln()).collect();
    // r[i-1] is the return into day i.
    let ann = 252f64.sqrt();
    let rv = |end: usize| -> Option<f64> {
        // Returns into days end-window+1 ..= end.
        (end >= window && end < n).then(|| stddev(&r[end - window..end]) * ann)
    };
    let mut rows = Vec::with_capacity(n);
    for i in 0..n {
        let (rank, percentile) = if lookback >= 2 && i + 1 >= lookback {
            let w = &iv[i + 1 - lookback..=i];
            let (lo, hi) = w.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
            let prior = &iv[i + 1 - lookback..i];
            (
                (hi > lo).then(|| (iv[i] - lo) / (hi - lo)),
                Some(prior.iter().filter(|&&v| v < iv[i]).count() as f64 / prior.len() as f64),
            )
        } else {
            (None, None)
        };
        rows.push(IvRow {
            date: iso(days[i]),
            iv: iv[i],
            close: closes[i],
            rv: rv(i),
            rv_forward: rv(i + window),
            hv: hv.and_then(|h| h.get(i).copied()).filter(|v| v.is_finite()),
            rank,
            percentile,
        });
    }
    let pairs: Vec<(f64, f64)> = rows.iter().filter_map(|r| r.rv_forward.map(|f| (r.iv, f))).collect();
    let (xs, ys): (Vec<f64>, Vec<f64>) = pairs.iter().copied().unzip();
    let fit = ols(&ys, &xs);
    let div: Vec<f64> = (1..n).map(|i| iv[i] - iv[i - 1]).collect();
    let last = rows.last()?;
    let summary = IvSummary {
        observations: n,
        window,
        lookback,
        current_iv: last.iv,
        current_rv: last.rv,
        current_hv: last.hv,
        rank: last.rank,
        percentile: last.percentile,
        mean_iv: mean(&iv[..n]),
        mean_rv_forward: (!ys.is_empty()).then(|| mean(&ys)),
        mean_vrp: (!pairs.is_empty()).then(|| mean(&pairs.iter().map(|(a, b)| a - b).collect::<Vec<_>>())),
        iv_above_share: (!pairs.is_empty())
            .then(|| pairs.iter().filter(|(a, b)| a > b).count() as f64 / pairs.len() as f64),
        fwd_alpha: fit.map(|f| f.0),
        fwd_beta: fit.map(|f| f.1),
        fwd_r2: fit.map(|f| f.2),
        iv_return_corr: corr(&div, &r),
        forecast_pairs: pairs.len(),
    };
    Some(IvHistory { rows, summary })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jd(y: i32, m: u8, d: u8) -> i32 {
        time::Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap().to_julian_day()
    }

    #[test]
    fn rolls_on_the_calendar_and_back_adjusts_to_the_current_front() {
        let a = FutSeries { ticker: "A".into(), last_trade: 110, closes: vec![(100, 100.0), (101, 101.0), (102, 102.0), (103, 103.0)] };
        let b = FutSeries { ticker: "B".into(), last_trade: 200, closes: vec![(100, 105.0), (101, 106.0), (102, 108.0), (103, 110.0), (104, 111.0)] };
        let c = futures_curve(&[b.clone(), a.clone()], None, None, 0.0, 8).unwrap();
        // Day 102 has 8 days left on A, which is the roll: B becomes the front.
        let fronts: Vec<&str> = c.rows.iter().map(|r| r.front.as_str()).collect();
        assert_eq!(fronts, ["A", "A", "B", "B", "B"]);
        assert_eq!(c.rolls.len(), 1);
        assert_eq!(c.rolls[0].date, iso(102));
        assert!((c.rolls[0].gap.unwrap() - (108.0 / 102.0 - 1.0)).abs() < 1e-12);
        // Held A over (101, 102], then B: growth 101/100 · 102/101 · 110/108 · 111/110.
        let g = 101.0 / 100.0 * 102.0 / 101.0 * 110.0 / 108.0 * 111.0 / 110.0;
        assert!((c.summary.futures_return - (g - 1.0)).abs() < 1e-12);
        assert!((c.rows.last().unwrap().continuous - 111.0).abs() < 1e-9);
        assert!((c.rows[0].continuous - 111.0 / g).abs() < 1e-9);
        // Roll yield on day 100: ln(105/100) over 90 days between expiries.
        assert!((c.rows[0].roll_yield.unwrap() - (1.05f64).ln() * 365.0 / 90.0).abs() < 1e-12);
    }

    #[test]
    fn basis_carry_and_fair_value_use_calendar_days() {
        let f = FutSeries { ticker: "ESZ".into(), last_trade: jd(2025, 12, 19), closes: vec![(jd(2025, 9, 1), 5541.2)] };
        let s = [(jd(2025, 9, 1), 5500.0)];
        let c = futures_curve(&[f], Some(&s), Some(0.045), 0.013, 0).unwrap();
        let r = &c.rows[0];
        let dte = (jd(2025, 12, 19) - jd(2025, 9, 1)) as f64;
        assert!((r.carry.unwrap() - (5541.2f64 / 5500.0).ln() * 365.0 / dte).abs() < 1e-12);
        assert!((r.fair.unwrap() - 5500.0 * ((0.045 - 0.013) * dte / 365.0).exp()).abs() < 1e-9);
        assert!((r.implied_repo.unwrap() - r.carry.unwrap() - 0.013).abs() < 1e-15);
        assert_eq!(c.shape, "flat");
    }

    #[test]
    fn a_bsm_chain_comes_back_at_its_own_volatility() {
        let (s, r, q, asof) = (100.0, 0.03, 0.01, 1000);
        let mut quotes = Vec::new();
        for (e, v) in [(1030, 0.2), (1090, 0.22)] {
            for k in [80.0, 90.0, 95.0, 97.5, 100.0, 101.0, 102.5, 105.0, 110.0, 120.0] {
                let t = (e - asof) as f64 / 365.0;
                // A skew: puts richer than calls.
                let vol = v + 0.3 * (100.0f64 / k).ln();
                for call in [true, false] {
                    let p = bsm(call, s, k, t, r, q, vol).unwrap().price;
                    quotes.push(OptQuote { ticker: format!("{e}{k}{call}"), expiry: e, strike: k, call, price: p });
                }
            }
        }
        let sf = iv_surface(s, asof, &quotes, r, q);
        assert_eq!(sf.solved, quotes.len());
        for e in &sf.expiries {
            for p in &e.points {
                let vol = if e.dte == 30 { 0.2 } else { 0.22 } + 0.3 * (100.0f64 / p.strike).ln();
                assert!((p.iv - vol).abs() < 1e-6, "{} {}", p.strike, p.iv);
            }
            assert!(e.rr25.unwrap() < 0.0);
        }
        assert!(sf.calendar_violations.is_empty());
    }

    #[test]
    fn realized_windows_line_up_with_their_days() {
        let n = 40;
        let days: Vec<i32> = (0..n).collect();
        let closes: Vec<f64> = (0..n).map(|i| 100.0 * (1.0 + 0.01 * ((i * 7 % 5) as f64 - 2.0)).powi(1) + i as f64).collect();
        let iv: Vec<f64> = (0..n).map(|i| 0.2 + 0.001 * i as f64).collect();
        let h = iv_history(&days, &iv, &closes, None, 5, 10).unwrap();
        let r: Vec<f64> = (1..n as usize).map(|i| (closes[i] / closes[i - 1]).ln()).collect();
        let want = stddev(&r[5..10]) * 252f64.sqrt();
        assert!((h.rows[10].rv.unwrap() - want).abs() < 1e-12);
        assert!((h.rows[5].rv_forward.unwrap() - want).abs() < 1e-12);
        assert!(h.rows[4].rv.is_none());
        // IV rises every day: top of its range, above every prior day.
        assert_eq!(h.rows[20].rank, Some(1.0));
        assert_eq!(h.rows[20].percentile, Some(1.0));
    }
}
