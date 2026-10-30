//! Strategy and trade-list analytics: probability of backtest overfitting (CSCV), R-multiples
//! and SQN with the MAE/MFE picture, side-by-side comparison of several runs, and risk of ruin.

use serde::Serialize;

use super::stats::moments;
use super::{correlation_matrix, histogram, mean, stddev, CorrelationMatrix, DeflatedSharpe, Histogram};

// ── Probability of backtest overfitting (Bailey, Borwein, López de Prado, Zhu 2015) ──────
//
// Combinatorially symmetric cross-validation: cut the common timeline into S blocks, and for
// every way of taking half of them as in-sample, pick the trial with the best in-sample
// Sharpe and see where it ranks out of sample. PBO is the share of splits where the in-sample
// winner lands in the bottom half out of sample.

#[derive(Debug, Serialize)]
pub struct Pbo {
    pub trials: usize,
    /// Blocks the timeline was cut into (S).
    pub partitions: usize,
    /// Observations used (a multiple of S; the oldest remainder is dropped).
    pub observations: usize,
    /// Number of in-sample / out-of-sample splits, C(S, S/2).
    pub splits: usize,
    /// Share of splits whose in-sample winner ranked at or below the median out of sample.
    pub pbo: f64,
    /// Share of splits whose in-sample winner lost money out of sample (Sharpe < 0).
    pub prob_oos_loss: f64,
    /// Logit of the winner's relative out-of-sample rank, one per split.
    pub logits: Histogram,
    /// Least-squares fit of OOS Sharpe on IS Sharpe of the winner (annualized). A slope well
    /// below 1 is the performance degradation the in-sample number hides.
    pub slope: f64,
    pub intercept: f64,
    /// (IS, OOS) Sharpe of the winner, annualized, at most 2,000 splits evenly sampled.
    pub pairs: Vec<(f64, f64)>,
}

/// Average ranks (1-based, ties share the mean rank), as `scipy.stats.rankdata`.
fn rank_average(xs: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..xs.len()).collect();
    idx.sort_by(|&a, &b| xs[a].partial_cmp(&xs[b]).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0; xs.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && xs[idx[j + 1]] == xs[idx[i]] {
            j += 1;
        }
        let r = (i + j) as f64 / 2.0 + 1.0;
        for k in i..=j {
            ranks[idx[k]] = r;
        }
        i = j + 1;
    }
    ranks
}

/// PBO over `returns[trial][t]` (every trial on the same clock). `partitions` is S, even,
/// clamped to 4..=16. Returns None when there are fewer than 2 trials or too little data
/// (each block needs at least 2 observations).
pub fn pbo(returns: &[Vec<f64>], partitions: usize, ppy: f64) -> Option<Pbo> {
    let n = returns.len();
    if n < 2 {
        return None;
    }
    let len = returns.iter().map(Vec::len).min()?;
    let s = (partitions.clamp(4, 16) / 2) * 2;
    let m = len / s;
    if m < 2 {
        return None;
    }
    let used = m * s;
    let skip = len - used;
    // Per trial, per block: count, sum, sum of squares. A split then costs O(N·S).
    let mut sums = vec![vec![(0.0f64, 0.0f64); s]; n];
    for (k, r) in returns.iter().enumerate() {
        let r = &r[r.len() - len..];
        for b in 0..s {
            let blk = &r[skip + b * m..skip + (b + 1) * m];
            sums[k][b] = (blk.iter().sum(), blk.iter().map(|x| x * x).sum());
        }
    }
    let half_n = (m * s / 2) as f64;
    let sharpe = |sum: f64, sq: f64| -> f64 {
        let mu = sum / half_n;
        let var = (sq - half_n * mu * mu) / (half_n - 1.0);
        if var > 1e-300 { mu / var.sqrt() } else { 0.0 }
    };
    let ann = ppy.max(1.0).sqrt();

    let mut logits = Vec::new();
    let mut pairs_all = Vec::new();
    let mut below = 0usize;
    let mut losses = 0usize;
    // Enumerate the S/2-subsets as bitmasks; symmetric pairs are both counted, as in the paper.
    for mask in 0u32..(1u32 << s) {
        if mask.count_ones() as usize != s / 2 {
            continue;
        }
        let mut is_sr = Vec::with_capacity(n);
        let mut oos_sr = Vec::with_capacity(n);
        for k in 0..n {
            let (mut a, mut b, mut c, mut d) = (0.0, 0.0, 0.0, 0.0);
            for (blk, &(sum, sq)) in sums[k].iter().enumerate() {
                if mask & (1 << blk) != 0 {
                    a += sum;
                    b += sq;
                } else {
                    c += sum;
                    d += sq;
                }
            }
            is_sr.push(sharpe(a, b));
            oos_sr.push(sharpe(c, d));
        }
        let mut best = 0;
        for k in 1..n {
            if is_sr[k] > is_sr[best] {
                best = k;
            }
        }
        let w = rank_average(&oos_sr)[best] / (n as f64 + 1.0);
        let lambda = (w / (1.0 - w)).ln();
        if lambda <= 0.0 {
            below += 1;
        }
        if oos_sr[best] < 0.0 {
            losses += 1;
        }
        logits.push(lambda);
        pairs_all.push((is_sr[best] * ann, oos_sr[best] * ann));
    }
    let splits = logits.len();
    let (mx, my) = (
        pairs_all.iter().map(|p| p.0).sum::<f64>() / splits as f64,
        pairs_all.iter().map(|p| p.1).sum::<f64>() / splits as f64,
    );
    let sxx: f64 = pairs_all.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = pairs_all.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    let step = splits.div_ceil(2000).max(1);
    Some(Pbo {
        trials: n,
        partitions: s,
        observations: used,
        splits,
        pbo: below as f64 / splits as f64,
        prob_oos_loss: losses as f64 / splits as f64,
        logits: histogram(&logits, 30),
        slope,
        intercept: my - slope * mx,
        pairs: pairs_all.into_iter().step_by(step).collect(),
    })
}

// ── Trade list: expectancy, R-multiples, SQN, MAE / MFE ──────────────────────────────────

/// One closed trade as the analytics read it.
#[derive(Debug, Clone)]
pub struct TradeRow {
    pub exit_ts: String,
    pub pnl: f64,
    /// Worst open loss during the trade, positive, account currency.
    pub mae: f64,
    /// Best open profit during the trade, positive, account currency.
    pub mfe: f64,
    /// Currency lost if the initial stop had been hit (1R), when the run had a fixed stop.
    pub risk: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct MaeMfePoint {
    pub mae_r: f64,
    pub mfe_r: f64,
    pub r: f64,
    pub win: bool,
}

#[derive(Debug, Serialize)]
pub struct TradeAnalytics {
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub avg_win: Option<f64>,
    pub avg_loss: Option<f64>,
    /// Average win over average loss (magnitudes).
    pub payoff: Option<f64>,
    /// Mean P&L per trade, account currency (= win_rate·avg_win − loss_rate·|avg_loss|).
    pub expectancy: f64,
    /// "stop": 1R is the initial stop risk of each trade. "avg_loss": 1R is the average loss
    /// (Van Tharp's stand-in when there is no fixed stop).
    pub r_basis: String,
    /// 1R in account currency under the avg_loss basis.
    pub r_unit: Option<f64>,
    pub expectancy_r: f64,
    pub sd_r: f64,
    /// System quality number: √N · mean(R) / sd(R).
    pub sqn: f64,
    /// SQN with N capped at 100, Van Tharp's comparable score.
    pub sqn_100: f64,
    pub best_r: f64,
    pub worst_r: f64,
    pub r_histogram: Histogram,
    /// Share of trades at or beyond each R level, for the tail read.
    pub r_ge_2: f64,
    pub r_le_minus_1: f64,
    /// Mean MFE over mean MAE (the edge ratio, above 1 when trades run further for you).
    pub edge_ratio: Option<f64>,
    /// False when no trade recorded an excursion (grid and DCA fills do not track them).
    pub has_excursions: bool,
    /// 95th percentile of the heat (MAE, in R) that winning trades sat through.
    pub winners_mae_p95_r: Option<f64>,
    /// Share of losing trades that were at +1R or better at some point.
    pub losers_reached_1r: Option<f64>,
    pub points: Vec<MaeMfePoint>,
    /// Cumulative R after each trade, in close order.
    pub r_curve: Vec<(String, f64)>,
}

/// Linear-interpolated quantile, as `numpy.quantile` (default method).
fn quantile(xs: &[f64], p: f64) -> f64 {
    let mut s = xs.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pos = p.clamp(0.0, 1.0) * (s.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    s[lo] + (s[hi] - s[lo]) * (pos - lo as f64)
}

pub fn trade_analytics(rows: &[TradeRow]) -> Option<TradeAnalytics> {
    let n = rows.len();
    if n < 2 {
        return None;
    }
    let wins: Vec<f64> = rows.iter().filter(|t| t.pnl > 0.0).map(|t| t.pnl).collect();
    let losses: Vec<f64> = rows.iter().filter(|t| t.pnl < 0.0).map(|t| t.pnl).collect();
    let avg_win = (!wins.is_empty()).then(|| mean(&wins));
    let avg_loss = (!losses.is_empty()).then(|| mean(&losses));
    let use_stop = rows.iter().all(|t| t.risk.is_some_and(|r| r > 0.0));
    let r_unit = if use_stop { None } else { Some(avg_loss?.abs()) };
    let unit = |t: &TradeRow| if use_stop { t.risk.unwrap_or(1.0) } else { r_unit.unwrap_or(1.0) };
    let r: Vec<f64> = rows.iter().map(|t| t.pnl / unit(t)).collect();
    let mu = mean(&r);
    let sd = stddev(&r);
    let sqn_of = |k: f64| if sd > 0.0 { k.sqrt() * mu / sd } else { 0.0 };
    let points: Vec<MaeMfePoint> = rows
        .iter()
        .zip(&r)
        .map(|(t, &rv)| MaeMfePoint { mae_r: t.mae / unit(t), mfe_r: t.mfe / unit(t), r: rv, win: t.pnl > 0.0 })
        .collect();
    let win_heat: Vec<f64> = points.iter().filter(|p| p.win).map(|p| p.mae_r).collect();
    let losers: Vec<&MaeMfePoint> = points.iter().filter(|p| p.r < 0.0).collect();
    let has_excursions = rows.iter().any(|t| t.mae > 0.0 || t.mfe > 0.0);
    let mae_mean = mean(&rows.iter().map(|t| t.mae).collect::<Vec<_>>());
    let mfe_mean = mean(&rows.iter().map(|t| t.mfe).collect::<Vec<_>>());
    let mut cum = 0.0;
    let r_curve = rows
        .iter()
        .zip(&r)
        .map(|(t, &rv)| {
            cum += rv;
            (t.exit_ts.clone(), cum)
        })
        .collect();
    Some(TradeAnalytics {
        trades: n,
        wins: wins.len(),
        losses: losses.len(),
        win_rate: wins.len() as f64 / n as f64,
        avg_win,
        avg_loss,
        payoff: match (avg_win, avg_loss) {
            (Some(w), Some(l)) if l != 0.0 => Some(w / l.abs()),
            _ => None,
        },
        expectancy: mean(&rows.iter().map(|t| t.pnl).collect::<Vec<_>>()),
        r_basis: if use_stop { "stop" } else { "avg_loss" }.into(),
        r_unit,
        expectancy_r: mu,
        sd_r: sd,
        sqn: sqn_of(n as f64),
        sqn_100: sqn_of(n.min(100) as f64),
        best_r: r.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        worst_r: r.iter().cloned().fold(f64::INFINITY, f64::min),
        r_histogram: histogram(&r, 30),
        r_ge_2: r.iter().filter(|v| **v >= 2.0).count() as f64 / n as f64,
        r_le_minus_1: r.iter().filter(|v| **v <= -1.0).count() as f64 / n as f64,
        edge_ratio: (mae_mean > 0.0).then(|| mfe_mean / mae_mean),
        has_excursions,
        winners_mae_p95_r: (has_excursions && !win_heat.is_empty()).then(|| quantile(&win_heat, 0.95)),
        losers_reached_1r: (has_excursions && !losers.is_empty())
            .then(|| losers.iter().filter(|p| p.mfe_r >= 1.0).count() as f64 / losers.len() as f64),
        points,
        r_curve,
    })
}

// ── Several runs side by side ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RunSummary {
    pub label: String,
    pub total_return: f64,
    pub ann_return: f64,
    pub ann_vol: f64,
    pub sharpe: Option<f64>,
    pub max_drawdown: f64,
    pub skew: Option<f64>,
    pub kurtosis: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub labels: Vec<String>,
    pub observations: usize,
    pub first_ts: String,
    pub last_ts: String,
    /// Each run's equity rebased to 1 at the first common timestamp, thinned for charting.
    pub ts: Vec<String>,
    pub curves: Vec<Vec<f64>>,
    pub summaries: Vec<RunSummary>,
    pub correlation: CorrelationMatrix,
    /// Selection haircut across the compared runs, treated as the trials of one search.
    pub deflated: Option<DeflatedSharpe>,
    pub pbo: Option<Pbo>,
}

/// Compare equity curves (`(ts, equity)`, ascending) on their common timestamps.
pub fn compare(labels: &[String], curves: &[Vec<(String, f64)>], ppy: f64, partitions: usize) -> Option<Comparison> {
    use std::collections::HashMap;
    if curves.len() < 2 {
        return None;
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    for c in curves {
        for (ts, _) in c {
            *count.entry(ts.as_str()).or_default() += 1;
        }
    }
    let k = curves.len();
    let mut common: Vec<&str> = count.into_iter().filter(|(_, v)| *v == k).map(|(t, _)| t).collect();
    common.sort_unstable();
    if common.len() < 30 {
        return None;
    }
    let aligned: Vec<Vec<f64>> = curves
        .iter()
        .map(|c| {
            let m: HashMap<&str, f64> = c.iter().map(|(t, e)| (t.as_str(), *e)).collect();
            common.iter().map(|t| m[t]).collect()
        })
        .collect();
    if aligned.iter().any(|c| c[0] <= 0.0) {
        return None;
    }
    let rets: Vec<Vec<f64>> = aligned
        .iter()
        .map(|c| c.windows(2).map(|w| if w[0] > 0.0 { w[1] / w[0] - 1.0 } else { 0.0 }).collect())
        .collect();
    let t = rets[0].len();
    let summaries: Vec<RunSummary> = labels
        .iter()
        .zip(aligned.iter().zip(&rets))
        .map(|(l, (eq, r))| {
            let mu = mean(r) * ppy;
            let vol = stddev(r) * ppy.sqrt();
            let mut peak = eq[0];
            let mut dd = 0.0f64;
            for &e in eq {
                peak = peak.max(e);
                if peak > 0.0 {
                    dd = dd.max(1.0 - e / peak);
                }
            }
            let mo = moments(r);
            RunSummary {
                label: l.clone(),
                total_return: eq[eq.len() - 1] / eq[0] - 1.0,
                ann_return: mu,
                ann_vol: vol,
                sharpe: (vol > 0.0).then(|| mu / vol),
                max_drawdown: dd,
                skew: mo.as_ref().map(|m| m.skew_biased),
                kurtosis: mo.as_ref().map(|m| m.excess_kurtosis_biased + 3.0),
            }
        })
        .collect();
    let sharpes: Vec<f64> = summaries.iter().filter_map(|s| s.sharpe).collect();
    let best = summaries
        .iter()
        .enumerate()
        .filter(|(_, s)| s.sharpe.is_some())
        .max_by(|a, b| a.1.sharpe.partial_cmp(&b.1.sharpe).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);
    let deflated = best.and_then(|b| {
        let s = &summaries[b];
        super::deflated_sharpe(&sharpes, t, ppy, s.skew.zip(s.kurtosis))
    });
    let step = common.len().div_ceil(1500).max(1);
    let keep: Vec<usize> = (0..common.len()).filter(|i| i % step == 0 || *i == common.len() - 1).collect();
    Some(Comparison {
        labels: labels.to_vec(),
        observations: t,
        first_ts: common[0].to_string(),
        last_ts: common[common.len() - 1].to_string(),
        ts: keep.iter().map(|&i| common[i].to_string()).collect(),
        curves: aligned.iter().map(|c| keep.iter().map(|&i| c[i] / c[0]).collect()).collect(),
        correlation: correlation_matrix(labels, &rets),
        deflated,
        pbo: pbo(&rets, partitions, ppy),
        summaries,
    })
}

// ── Risk of ruin ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sizing {
    /// The same currency amount (risk % of the STARTING capital) on every trade.
    Fixed,
    /// Risk % of the CURRENT capital on every trade.
    Fractional,
}

#[derive(Debug, Serialize)]
pub struct Ruin {
    pub win_rate: f64,
    pub payoff: f64,
    pub risk: f64,
    pub ruin: f64,
    /// Expected R per trade, p·W − q.
    pub expectancy_r: f64,
    /// Vince's closed form for fixed-amount risk (None under fractional sizing).
    pub vince: Option<f64>,
    /// Cramér-Lundberg bound exp(−r·D) on the infinite-horizon ruin probability, with the
    /// adjustment coefficient r solving E[exp(−r·X)] = 1 over one trade's outcome X.
    pub lundberg: Option<f64>,
    pub adjustment: Option<f64>,
    /// Simulated probability of reaching the ruin level within `horizon` trades.
    pub simulated: f64,
    pub simulated_se: f64,
    pub horizon: usize,
    pub paths: usize,
    /// Cumulative share of paths ruined by each trade count (for the chart), thinned.
    pub by_trade: Vec<(usize, f64)>,
}

/// Root r > 0 of p·exp(−r·a) + q·exp(r·b) = 1 (a win of +a, a loss of −b), which exists when
/// the drift p·a − q·b is positive.
pub fn adjustment_coefficient(p: f64, a: f64, b: f64) -> Option<f64> {
    let q = 1.0 - p;
    if !(p > 0.0 && q > 0.0 && a > 0.0 && b > 0.0) || p * a - q * b <= 0.0 {
        return None;
    }
    let g = |r: f64| p * (-r * a).exp() + q * (r * b).exp() - 1.0;
    let mut hi = 1.0 / b;
    while g(hi) < 0.0 {
        hi *= 2.0;
        if hi > 1e12 {
            return None;
        }
    }
    let mut lo = 0.0;
    // g < 0 just right of 0 (negative slope there), > 0 at hi.
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if g(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

/// Risk of ruin for a win rate `p`, a payoff `w` (average win in R, 1R = the risked amount),
/// a risk per trade `f` (fraction of capital) and a ruin level `u` (fraction of the starting
/// capital lost).
#[allow(clippy::too_many_arguments)]
pub fn risk_of_ruin(p: f64, w: f64, f: f64, u: f64, sizing: Sizing, horizon: usize, paths: usize, seed: u64) -> Ruin {
    let p = p.clamp(0.0, 1.0);
    let q = 1.0 - p;
    let f = f.clamp(1e-6, 0.99);
    let u = u.clamp(1e-6, 1.0);
    let horizon = horizon.clamp(1, 100_000);
    let paths = paths.clamp(100, 200_000);
    let e = p * w - q;
    let (vince, lundberg, adjustment) = match sizing {
        Sizing::Fixed => {
            let units = u / f;
            let a = (p * w * w + q).sqrt();
            let pp = 0.5 * (1.0 + e / a);
            let v = if e <= 0.0 { 1.0 } else { ((1.0 - pp) / pp).powf(units / a).min(1.0) };
            let r = adjustment_coefficient(p, w, 1.0);
            (Some(v), Some(r.map_or(1.0, |r| (-r * units).exp())), r)
        }
        Sizing::Fractional => {
            let up = (1.0 + f * w).ln();
            let down = -(1.0 - f).ln();
            let d = if u >= 1.0 { f64::INFINITY } else { -(1.0 - u).ln() };
            let r = adjustment_coefficient(p, up, down);
            (None, Some(r.map_or(1.0, |r| (-r * d).exp())), r)
        }
    };
    // Simulation: the same walk, trade by trade, stopped at the ruin level.
    let mut rng = super::Rng(seed | 1);
    let floor = 1.0 - u;
    let mut first = vec![0usize; horizon + 1];
    let mut ruined = 0usize;
    for _ in 0..paths {
        let mut eq = 1.0f64;
        for k in 1..=horizon {
            let win = rng.next_f64() < p;
            eq = match sizing {
                Sizing::Fixed => eq + if win { f * w } else { -f },
                Sizing::Fractional => eq * if win { 1.0 + f * w } else { 1.0 - f },
            };
            if eq <= floor + 1e-12 {
                first[k] += 1;
                ruined += 1;
                break;
            }
        }
    }
    let sim = ruined as f64 / paths as f64;
    let step = horizon.div_ceil(300).max(1);
    let mut acc = 0usize;
    let mut by_trade = Vec::new();
    for (k, c) in first.iter().enumerate().skip(1) {
        acc += c;
        if k % step == 0 || k == horizon {
            by_trade.push((k, acc as f64 / paths as f64));
        }
    }
    Ruin {
        win_rate: p,
        payoff: w,
        risk: f,
        ruin: u,
        expectancy_r: e,
        vince,
        lundberg,
        adjustment,
        simulated: sim,
        simulated_se: (sim * (1.0 - sim) / paths as f64).sqrt(),
        horizon,
        paths,
        by_trade,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_match_scipy_average() {
        assert_eq!(rank_average(&[3.0, 1.0, 2.0, 2.0]), vec![4.0, 1.0, 2.5, 2.5]);
    }

    #[test]
    fn identical_trials_carry_no_overfitting_signal_and_noise_sits_near_half() {
        // Independent noise trials: PBO around 0.5.
        let mut s = 12345u64;
        let mut u = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let rets: Vec<Vec<f64>> = (0..20).map(|_| (0..1600).map(|_| u() * 0.01).collect()).collect();
        let p = pbo(&rets, 10, 252.0).unwrap();
        assert_eq!(p.splits, 252);
        assert!(p.pbo > 0.25 && p.pbo < 0.75, "{}", p.pbo);
        // A trial with a real drift wins in and out of sample: PBO near 0.
        let mut good = rets.clone();
        good[3] = good[3].iter().map(|x| x + 0.004).collect();
        assert!(pbo(&good, 10, 252.0).unwrap().pbo < 0.05);
    }

    #[test]
    fn gamblers_ruin_is_the_even_payoff_case() {
        // Win +1, lose -1, p = 0.55, 10 units: (q/p)^10.
        let r = risk_of_ruin(0.55, 1.0, 0.1, 1.0, Sizing::Fixed, 10, 100, 1);
        let exact = (0.45f64 / 0.55).powi(10);
        assert!((r.vince.unwrap() - exact).abs() < 1e-12);
        assert!((r.lundberg.unwrap() - exact).abs() < 1e-9);
    }

    #[test]
    fn r_multiples_use_the_stop_when_every_trade_has_one() {
        let row = |pnl: f64, risk: Option<f64>| TradeRow { exit_ts: String::new(), pnl, mae: 0.0, mfe: 0.0, risk };
        let a = trade_analytics(&[row(200.0, Some(100.0)), row(-100.0, Some(100.0)), row(50.0, Some(50.0))]).unwrap();
        assert_eq!(a.r_basis, "stop");
        assert!((a.expectancy_r - (2.0 - 1.0 + 1.0) / 3.0).abs() < 1e-12);
        let b = trade_analytics(&[row(200.0, None), row(-100.0, None), row(-50.0, None)]).unwrap();
        assert_eq!(b.r_basis, "avg_loss");
        assert!((b.r_unit.unwrap() - 75.0).abs() < 1e-12);
    }
}
