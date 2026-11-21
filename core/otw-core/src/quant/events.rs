//! Conditional forward returns and event studies over OHLCV bars.
//!
//! An event is a bar on which a condition fires, judged on that bar's close. What happens next
//! is measured from that close: the forward return over h bars, close_{t+h} / close_t − 1,
//! and the average path from `pre` bars before to `post` bars after. Every horizon is compared
//! with the unconditional forward return over the same bars, because "SPY rose 0.4% in the
//! week after a gap down" means nothing until you know it rose 0.3% in an average week.

use serde::{Deserialize, Serialize};

use crate::backtest::indicators::{rsi, sma};

use super::special::{t_ppf, t_two_sided};
use super::{mean, quantile_sorted, stddev, Rng};

/// A condition on the bar series. Percent thresholds are fractions (0.02 = 2%).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    /// Open at least `pct` above the previous close.
    GapUp { pct: f64 },
    /// Open at least `pct` below the previous close.
    GapDown { pct: f64 },
    /// Close-to-close return of at least `pct`.
    BigUp { pct: f64 },
    /// Close-to-close return of at most −`pct`.
    BigDown { pct: f64 },
    /// Close crosses above its `period`-bar simple moving average.
    CrossAboveSma { period: usize },
    /// Close crosses below its `period`-bar simple moving average.
    CrossBelowSma { period: usize },
    /// Wilder RSI(`period`) crosses below `level`.
    RsiBelow { period: usize, level: f64 },
    /// Wilder RSI(`period`) crosses above `level`.
    RsiAbove { period: usize, level: f64 },
    /// Close above the highest close of the previous `period` bars.
    NewHigh { period: usize },
    /// Close below the lowest close of the previous `period` bars.
    NewLow { period: usize },
    /// The `count`-th consecutive higher close.
    StreakUp { count: usize },
    /// The `count`-th consecutive lower close.
    StreakDown { count: usize },
    /// Volume at least `mult` × the average of the previous `period` bars.
    VolumeSpike { period: usize, mult: f64 },
}

pub struct Series<'a> {
    pub open: &'a [f64],
    pub close: &'a [f64],
    pub volume: &'a [f64],
}

/// Indices of bars where the condition fires.
pub fn fire(cond: &Condition, s: &Series) -> Vec<usize> {
    let n = s.close.len();
    let c = s.close;
    let ret = |i: usize| if c[i - 1] > 0.0 { c[i] / c[i - 1] - 1.0 } else { 0.0 };
    let mut out = Vec::new();
    match *cond {
        Condition::GapUp { pct } => {
            out.extend((1..n).filter(|&i| c[i - 1] > 0.0 && s.open[i] / c[i - 1] - 1.0 >= pct))
        }
        Condition::GapDown { pct } => {
            out.extend((1..n).filter(|&i| c[i - 1] > 0.0 && s.open[i] / c[i - 1] - 1.0 <= -pct))
        }
        Condition::BigUp { pct } => out.extend((1..n).filter(|&i| ret(i) >= pct)),
        Condition::BigDown { pct } => out.extend((1..n).filter(|&i| ret(i) <= -pct)),
        Condition::CrossAboveSma { period } | Condition::CrossBelowSma { period } => {
            let up = matches!(cond, Condition::CrossAboveSma { .. });
            let m = sma(c, period.max(1));
            for i in 1..n {
                if let (Some(a), Some(b)) = (m[i - 1], m[i]) {
                    let crossed = if up { c[i - 1] <= a && c[i] > b } else { c[i - 1] >= a && c[i] < b };
                    if crossed {
                        out.push(i);
                    }
                }
            }
        }
        Condition::RsiBelow { period, level } | Condition::RsiAbove { period, level } => {
            let below = matches!(cond, Condition::RsiBelow { .. });
            let r = rsi(c, period.max(1));
            for i in 1..n {
                if let (Some(a), Some(b)) = (r[i - 1], r[i]) {
                    if (below && a >= level && b < level) || (!below && a <= level && b > level) {
                        out.push(i);
                    }
                }
            }
        }
        Condition::NewHigh { period } | Condition::NewLow { period } => {
            let high = matches!(cond, Condition::NewHigh { .. });
            let p = period.max(1);
            for i in p..n {
                let w = &c[i - p..i];
                let hit = if high {
                    c[i] > w.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                } else {
                    c[i] < w.iter().cloned().fold(f64::INFINITY, f64::min)
                };
                if hit {
                    out.push(i);
                }
            }
        }
        Condition::StreakUp { count } | Condition::StreakDown { count } => {
            let up = matches!(cond, Condition::StreakUp { .. });
            let mut run = 0usize;
            for i in 1..n {
                let moved = if up { c[i] > c[i - 1] } else { c[i] < c[i - 1] };
                run = if moved { run + 1 } else { 0 };
                if run == count.max(1) {
                    out.push(i);
                }
            }
        }
        Condition::VolumeSpike { period, mult } => {
            let p = period.max(1);
            let v = s.volume;
            for i in p..n {
                let avg = mean(&v[i - p..i]);
                if avg > 0.0 && v[i] >= mult * avg {
                    out.push(i);
                }
            }
        }
    }
    out
}

/// Drop events that start within `gap` bars of the previous kept one, so overlapping
/// forward windows are not counted as independent evidence.
pub fn thin(events: &[usize], gap: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for &e in events {
        if out.last().is_none_or(|&l| e >= l + gap.max(1)) {
            out.push(e);
        }
    }
    out
}

#[derive(Debug, Serialize)]
pub struct HorizonStats {
    pub horizon: usize,
    /// Events with a full forward window.
    pub n: usize,
    pub mean: f64,
    pub median: f64,
    pub stdev: f64,
    pub hit_rate: f64,
    /// One-sample t-test of the mean forward return against zero.
    pub t_stat: f64,
    pub p_value: f64,
    /// 95% t confidence interval of the mean.
    pub ci_low: f64,
    pub ci_high: f64,
    /// 95% percentile bootstrap interval of the mean (2 000 resamples, fixed seed).
    pub boot_low: f64,
    pub boot_high: f64,
    /// Unconditional mean and hit rate over every bar with a full window.
    pub base_mean: f64,
    pub base_hit_rate: f64,
    /// t statistic of the event mean against the unconditional mean.
    pub t_vs_base: f64,
    pub p_vs_base: f64,
}

#[derive(Debug, Serialize)]
pub struct PathPoint {
    pub offset: i64,
    pub mean: f64,
    pub low: f64,
    pub high: f64,
}

#[derive(Debug, Serialize)]
pub struct EventStudy {
    pub events: usize,
    pub events_raw: usize,
    pub horizons: Vec<HorizonStats>,
    /// Average cumulative return path around the event (relative to the event close), with a
    /// 95% band on the mean.
    pub path: Vec<PathPoint>,
    /// Event stamps (most recent last), capped for display.
    pub dates: Vec<String>,
}

fn boot_ci(xs: &[f64], seed: u64) -> (f64, f64) {
    if xs.len() < 2 {
        let m = mean(xs);
        return (m, m);
    }
    let mut rng = Rng(seed | 1);
    let n = xs.len();
    let mut means: Vec<f64> = (0..2000)
        .map(|_| (0..n).map(|_| xs[(rng.next_f64() * n as f64) as usize % n]).sum::<f64>() / n as f64)
        .collect();
    means.sort_by(f64::total_cmp);
    (quantile_sorted(&means, 0.025), quantile_sorted(&means, 0.975))
}

fn horizon_stats(h: usize, fwd: &[f64], base: &[f64]) -> HorizonStats {
    let n = fwd.len();
    let m = mean(fwd);
    let sd = stddev(fwd);
    let mut s = fwd.to_vec();
    s.sort_by(f64::total_cmp);
    let se = if n > 1 { sd / (n as f64).sqrt() } else { f64::NAN };
    let df = (n as f64 - 1.0).max(1.0);
    let t = if se > 0.0 { m / se } else { f64::NAN };
    let tq = t_ppf(0.975, df);
    let base_mean = mean(base);
    let tb = if se > 0.0 { (m - base_mean) / se } else { f64::NAN };
    let (bl, bh) = boot_ci(fwd, 0x5EED ^ h as u64);
    HorizonStats {
        horizon: h,
        n,
        mean: m,
        median: if n > 0 { quantile_sorted(&s, 0.5) } else { f64::NAN },
        stdev: sd,
        hit_rate: if n > 0 { fwd.iter().filter(|v| **v > 0.0).count() as f64 / n as f64 } else { f64::NAN },
        t_stat: t,
        p_value: if t.is_finite() { t_two_sided(t, df) } else { f64::NAN },
        ci_low: m - tq * se,
        ci_high: m + tq * se,
        boot_low: bl,
        boot_high: bh,
        base_mean,
        base_hit_rate: if base.is_empty() { f64::NAN } else { base.iter().filter(|v| **v > 0.0).count() as f64 / base.len() as f64 },
        t_vs_base: tb,
        p_vs_base: if tb.is_finite() { t_two_sided(tb, df) } else { f64::NAN },
    }
}

pub fn study(close: &[f64], ts: &[String], events: &[usize], raw: usize, horizons: &[usize], pre: usize, post: usize) -> EventStudy {
    let n = close.len();
    let fwd_ret = |i: usize, h: usize| (i + h < n && close[i] > 0.0).then(|| close[i + h] / close[i] - 1.0);
    let horizons_out = horizons
        .iter()
        .map(|&h| {
            let fwd: Vec<f64> = events.iter().filter_map(|&e| fwd_ret(e, h)).collect();
            let base: Vec<f64> = (0..n).filter_map(|i| fwd_ret(i, h)).collect();
            horizon_stats(h, &fwd, &base)
        })
        .collect();
    let path = (-(pre as i64)..=post as i64)
        .map(|o| {
            let xs: Vec<f64> = events
                .iter()
                .filter_map(|&e| {
                    let j = e as i64 + o;
                    (j >= 0 && (j as usize) < n && close[e] > 0.0).then(|| close[j as usize] / close[e] - 1.0)
                })
                .collect();
            let m = mean(&xs);
            let se = if xs.len() > 1 { stddev(&xs) / (xs.len() as f64).sqrt() } else { 0.0 };
            PathPoint { offset: o, mean: m, low: m - 1.96 * se, high: m + 1.96 * se }
        })
        .collect();
    let dates = events.iter().rev().take(500).rev().map(|&e| ts.get(e).cloned().unwrap_or_default()).collect();
    EventStudy { events: events.len(), events_raw: raw, horizons: horizons_out, path, dates }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaks_and_thinning() {
        let c = [1.0, 2.0, 3.0, 4.0, 3.0, 4.0, 5.0, 6.0];
        let v = vec![0.0; c.len()];
        let s = Series { open: &c, close: &c, volume: &v };
        assert_eq!(fire(&Condition::StreakUp { count: 2 }, &s), vec![2, 6]);
        assert_eq!(thin(&[1, 2, 3, 10, 11], 5), vec![1, 10]);
    }

    #[test]
    fn forward_returns_are_measured_from_the_event_close() {
        let c: Vec<f64> = (1..=20).map(|v| v as f64).collect();
        let ts: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        let st = study(&c, &ts, &[4], 1, &[5], 2, 2);
        assert!((st.horizons[0].mean - (10.0 / 5.0 - 1.0)).abs() < 1e-12);
        assert_eq!(st.path.len(), 5);
        assert!((st.path[2].mean).abs() < 1e-12);
    }
}
