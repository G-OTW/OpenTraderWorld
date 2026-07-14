//! Quant tools — pure risk/return math over price series.
//!
//! Everything here is stateless and dependency-free: callers pass aligned close-price
//! series (or simple period returns) and get back metrics ready to serialize. The API
//! layer is responsible for loading bars from histdata and aligning multi-asset series
//! by timestamp before calling in.
//!
//! Conventions:
//! - "returns" are simple period returns r_t = p_t / p_{t-1} - 1.
//! - Volatility/VaR are annualized using `periods_per_year` inferred from the dataset
//!   timeframe (see [`periods_per_year`]).
//! - VaR/CVaR are reported as positive loss fractions (0.05 = a 5% loss).

use serde::Serialize;

/// Trading periods per year for a histdata timeframe string (e.g. "1d", "1h", "15m").
/// Falls back to daily (252) for anything unrecognized.
pub fn periods_per_year(timeframe: &str) -> f64 {
    let tf = timeframe.trim().to_lowercase();
    // Split into number + unit.
    let split = tf.find(|c: char| c.is_alphabetic()).unwrap_or(tf.len());
    let (num, unit) = tf.split_at(split);
    let n: f64 = num.parse().unwrap_or(1.0);
    if n <= 0.0 {
        return 252.0;
    }
    // ~6.5 trading hours/day, 252 trading days/year.
    let per_day = match unit {
        "m" | "min" => 252.0 * 6.5 * 60.0 / n,
        "h" | "hour" => 252.0 * 6.5 / n,
        "d" | "day" => 252.0 / n,
        "w" | "wk" | "week" => 52.0 / n,
        "mo" | "month" => 12.0 / n,
        _ => 252.0 / n,
    };
    per_day.max(1.0)
}

/// Simple period returns from a close series. Length is `closes.len() - 1`.
pub fn returns(closes: &[f64]) -> Vec<f64> {
    closes
        .windows(2)
        .map(|w| if w[0] != 0.0 { w[1] / w[0] - 1.0 } else { 0.0 })
        .collect()
}

pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Sample standard deviation (n-1). Returns 0 for fewer than 2 points.
pub fn stddev(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() as f64 - 1.0);
    var.sqrt()
}

fn covariance(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 2 {
        return 0.0;
    }
    let (ma, mb) = (mean(&a[..n]), mean(&b[..n]));
    let s: f64 = (0..n).map(|i| (a[i] - ma) * (b[i] - mb)).sum();
    s / (n as f64 - 1.0)
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let (sa, sb) = (stddev(a), stddev(b));
    if sa == 0.0 || sb == 0.0 {
        return 0.0;
    }
    covariance(a, b) / (sa * sb)
}

// ── Single-asset metrics ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DrawdownPoint {
    pub ts: String,
    /// Drawdown from running peak, as a negative fraction (-0.1 = 10% below peak).
    pub dd: f64,
}

#[derive(Debug, Serialize)]
pub struct SingleResult {
    pub periods: usize,
    pub periods_per_year: f64,
    /// Annualized historical volatility (stddev of returns × √periods/yr).
    pub hv_annual: f64,
    /// Per-period volatility (raw stddev of returns).
    pub hv_period: f64,
    /// Worst peak-to-trough drop as a positive fraction (0.42 = -42%).
    pub max_drawdown: f64,
    /// Historical VaR at the chosen confidence, as a positive loss fraction.
    pub var_hist: f64,
    /// Parametric (normal) VaR at the chosen confidence.
    pub var_param: f64,
    /// Conditional VaR (expected shortfall) beyond the historical VaR.
    pub cvar: f64,
    pub confidence: f64,
    pub mean_return: f64,
    /// Drawdown curve for charting.
    pub drawdown_curve: Vec<DrawdownPoint>,
    /// Return distribution as histogram bins for charting.
    pub histogram: Histogram,
}

#[derive(Debug, Serialize)]
pub struct Histogram {
    /// Bin edges (len = bins + 1).
    pub edges: Vec<f64>,
    /// Count in each bin (len = bins).
    pub counts: Vec<u64>,
}

fn histogram(xs: &[f64], bins: usize) -> Histogram {
    let bins = bins.max(1);
    if xs.is_empty() {
        return Histogram { edges: vec![0.0, 0.0], counts: vec![0] };
    }
    let lo = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = if hi > lo { hi - lo } else { 1.0 };
    let edges: Vec<f64> = (0..=bins).map(|i| lo + span * i as f64 / bins as f64).collect();
    let mut counts = vec![0u64; bins];
    for &x in xs {
        let mut idx = ((x - lo) / span * bins as f64) as usize;
        if idx >= bins {
            idx = bins - 1;
        }
        counts[idx] += 1;
    }
    Histogram { edges, counts }
}

/// Max drawdown and the per-period drawdown curve from a close series.
fn drawdown(closes: &[f64], ts: &[String]) -> (f64, Vec<DrawdownPoint>) {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    let mut curve = Vec::with_capacity(closes.len());
    for (i, &p) in closes.iter().enumerate() {
        if p > peak {
            peak = p;
        }
        let dd = if peak > 0.0 { p / peak - 1.0 } else { 0.0 };
        if -dd > max_dd {
            max_dd = -dd;
        }
        curve.push(DrawdownPoint { ts: ts.get(i).cloned().unwrap_or_default(), dd });
    }
    (max_dd, curve)
}

/// Historical VaR: the `confidence` quantile of losses. Returns a positive loss fraction.
fn var_historical(rets: &[f64], confidence: f64) -> f64 {
    if rets.is_empty() {
        return 0.0;
    }
    let mut sorted = rets.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Lower tail at (1 - confidence).
    let idx = ((1.0 - confidence) * sorted.len() as f64).floor() as usize;
    let idx = idx.min(sorted.len() - 1);
    (-sorted[idx]).max(0.0)
}

/// CVaR / expected shortfall: mean loss in the tail at or below the VaR threshold.
fn cvar(rets: &[f64], confidence: f64) -> f64 {
    if rets.is_empty() {
        return 0.0;
    }
    let mut sorted = rets.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let cut = ((1.0 - confidence) * sorted.len() as f64).ceil() as usize;
    let cut = cut.max(1).min(sorted.len());
    let tail = &sorted[..cut];
    (-mean(tail)).max(0.0)
}

/// Standard-normal quantile (inverse CDF), Acklam's rational approximation.
fn norm_ppf(p: f64) -> f64 {
    let p = p.clamp(1e-9, 1.0 - 1e-9);
    let a = [
        -3.969683028665376e+01, 2.209460984245205e+02, -2.759285104469687e+02,
        1.383577518672690e+02, -3.066479806614716e+01, 2.506628277459239e+00,
    ];
    let b = [
        -5.447609879822406e+01, 1.615858368580409e+02, -1.556989798598866e+02,
        6.680131188771972e+01, -1.328068155288572e+01,
    ];
    let c = [
        -7.784894002430293e-03, -3.223964580411365e-01, -2.400758277161838e+00,
        -2.549732539343734e+00, 4.374664141464968e+00, 2.938163982698783e+00,
    ];
    let d = [
        7.784695709041462e-03, 3.224671290700398e-01, 2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    let plow = 0.02425;
    let phigh = 1.0 - plow;
    if p < plow {
        let q = (-2.0 * p.ln()).sqrt();
        (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    } else if p <= phigh {
        let q = p - 0.5;
        let r = q * q;
        (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q
            / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    }
}

/// Full single-asset analysis.
pub fn analyze_single(
    closes: &[f64],
    ts: &[String],
    timeframe: &str,
    confidence: f64,
) -> SingleResult {
    let ppy = periods_per_year(timeframe);
    let rets = returns(closes);
    let sd = stddev(&rets);
    let m = mean(&rets);
    let (max_dd, curve) = drawdown(closes, ts);
    // Parametric VaR: -(μ + z·σ) where z is the lower-tail quantile.
    let z = norm_ppf(1.0 - confidence);
    let var_param = (-(m + z * sd)).max(0.0);
    SingleResult {
        periods: rets.len(),
        periods_per_year: ppy,
        hv_annual: sd * ppy.sqrt(),
        hv_period: sd,
        max_drawdown: max_dd,
        var_hist: var_historical(&rets, confidence),
        var_param,
        cvar: cvar(&rets, confidence),
        confidence,
        mean_return: m,
        drawdown_curve: curve,
        histogram: histogram(&rets, 40),
    }
}

// ── Kelly (manual inputs) ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct KellyResult {
    /// Full-Kelly fraction of capital (can exceed 1 or go negative).
    pub kelly: f64,
    /// Clamped to [0, 1] for a sane sizing suggestion.
    pub kelly_clamped: f64,
    pub half_kelly: f64,
    pub quarter_kelly: f64,
    /// Reward/risk ratio b = avg_win / avg_loss.
    pub payoff: f64,
}

/// Kelly from win rate and average win/loss (both entered as positive magnitudes).
/// f* = p - (1 - p) / b, with b = avg_win / avg_loss.
pub fn kelly(win_rate: f64, avg_win: f64, avg_loss: f64) -> KellyResult {
    let p = win_rate.clamp(0.0, 1.0);
    let b = if avg_loss > 0.0 { avg_win / avg_loss } else { 0.0 };
    let f = if b > 0.0 { p - (1.0 - p) / b } else { 0.0 };
    let clamped = f.clamp(0.0, 1.0);
    KellyResult {
        kelly: f,
        kelly_clamped: clamped,
        half_kelly: clamped * 0.5,
        quarter_kelly: clamped * 0.25,
        payoff: b,
    }
}

// ── Position sizing ─────────────────────────────────────────────────────────────

/// Direction of the trade — determines which side of entry the stop sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Long,
    Short,
}

#[derive(Debug, Serialize)]
pub struct PositionSizeResult {
    /// Currency amount put at risk (risk_pct × stack, or the fixed amount).
    pub risk_amount: f64,
    /// Distance from entry to stop, in price units.
    pub stop_distance: f64,
    /// Per-unit loss if the stop is hit (== stop_distance for a 1× multiplier).
    pub risk_per_unit: f64,
    /// Suggested quantity (units/contracts). 0 when inputs are incoherent.
    pub quantity: f64,
    /// Position notional = quantity × entry × multiplier.
    pub notional: f64,
    /// Risk as a fraction of stack (risk_amount / stack).
    pub risk_fraction: f64,
    /// Notional as a fraction of stack (a leverage read-through).
    pub exposure_fraction: f64,
    /// Margin required at the chosen leverage (notional / leverage). None when no leverage given.
    pub margin_required: Option<f64>,
    /// Optional take-profit read-through.
    pub target_distance: Option<f64>,
    pub reward_amount: Option<f64>,
    /// Reward-to-risk ratio (target_distance / stop_distance).
    pub reward_risk: Option<f64>,
    /// True when notional exceeds what the stack supports at the given leverage (1× if none).
    pub over_leveraged: bool,
    /// Human-readable cautions (empty stop, risk > stack, over-leverage, …).
    pub warnings: Vec<String>,
}

/// Risk-based position sizing.
///
/// `risk_amount` is the currency at risk (caller resolves %-of-stack vs. fixed before calling).
/// `multiplier` scales price moves to P&L per unit (contract size / lot multiplier; 1.0 for spot).
/// `leverage` (>1) reports required margin and drives the over-leverage check; `None`/≤1 means cash.
/// `target` is an optional take-profit price for the reward read-through.
#[allow(clippy::too_many_arguments)]
pub fn position_size(
    stack: f64,
    risk_amount: f64,
    entry: f64,
    stop: f64,
    side: Side,
    multiplier: f64,
    leverage: Option<f64>,
    target: Option<f64>,
) -> PositionSizeResult {
    let mult = if multiplier > 0.0 { multiplier } else { 1.0 };
    let stop_distance = (entry - stop).abs();
    let risk_per_unit = stop_distance * mult;
    let mut warnings = Vec::new();

    // A stop on the wrong side of entry can't cap the loss it claims to.
    let stop_ok = match side {
        Side::Long => stop < entry,
        Side::Short => stop > entry,
    };
    if stop_distance > 0.0 && !stop_ok {
        warnings.push("Stop is on the wrong side of entry for this direction.".into());
    }

    let quantity = if risk_per_unit > 0.0 { risk_amount / risk_per_unit } else { 0.0 };
    let notional = quantity * entry * mult;

    let lev = leverage.filter(|l| *l > 0.0);
    let margin_required = lev.map(|l| notional / l);
    // Buying power = stack × leverage (or stack itself when unlevered).
    let buying_power = stack * lev.unwrap_or(1.0);
    let over_leveraged = stack > 0.0 && notional > buying_power + 1e-9;

    let risk_fraction = if stack > 0.0 { risk_amount / stack } else { 0.0 };
    let exposure_fraction = if stack > 0.0 { notional / stack } else { 0.0 };

    if stop_distance == 0.0 {
        warnings.push("Entry and stop are equal — no stop distance to size against.".into());
    }
    if stack > 0.0 && risk_amount > stack {
        warnings.push("Risk amount exceeds the whole stack.".into());
    }
    if over_leveraged {
        warnings.push("Position notional exceeds available buying power.".into());
    }

    let (target_distance, reward_amount, reward_risk) = match target {
        Some(tp) => {
            let td = (tp - entry).abs();
            let rew = td * mult * quantity;
            let rr = if stop_distance > 0.0 { Some(td / stop_distance) } else { None };
            (Some(td), Some(rew), rr)
        }
        None => (None, None, None),
    };

    PositionSizeResult {
        risk_amount,
        stop_distance,
        risk_per_unit,
        quantity,
        notional,
        risk_fraction,
        exposure_fraction,
        margin_required,
        target_distance,
        reward_amount,
        reward_risk,
        over_leveraged,
        warnings,
    }
}

// ── Asset-derived stop signals ──────────────────────────────────────────────────

/// Average True Range over the last `period` bars (Wilder true range from OHLC).
/// Falls back to close-to-close range when highs/lows are absent (all zero).
fn atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> f64 {
    let n = closes.len();
    if n < 2 {
        return 0.0;
    }
    let mut trs = Vec::with_capacity(n - 1);
    for i in 1..n {
        let h = highs.get(i).copied().unwrap_or(closes[i]);
        let l = lows.get(i).copied().unwrap_or(closes[i]);
        let pc = closes[i - 1];
        let tr = (h - l).max((h - pc).abs()).max((l - pc).abs());
        trs.push(tr);
    }
    let take = period.min(trs.len()).max(1);
    let tail = &trs[trs.len() - take..];
    mean(tail)
}

/// One suggested stop, expressed both as a price distance and a placed stop price.
#[derive(Debug, Serialize)]
pub struct StopSuggestion {
    /// Stable key for the client (e.g. "hv_2sigma", "atr_2", "swing").
    pub key: String,
    pub label: String,
    /// Stop distance in price units.
    pub distance: f64,
    /// Stop price placed on the correct side of `entry` for the trade direction.
    pub stop_price: f64,
    /// Distance as a fraction of entry (for display).
    pub distance_pct: f64,
}

#[derive(Debug, Serialize)]
pub struct AssetSignals {
    pub periods: usize,
    pub periods_per_year: f64,
    /// Per-period volatility (stddev of returns).
    pub hv_period: f64,
    pub hv_annual: f64,
    /// ATR over the window (last `atr_period` bars).
    pub atr: f64,
    /// Latest close in the window (a convenient default entry).
    pub last_close: f64,
    pub suggestions: Vec<StopSuggestion>,
}

/// Derive candidate stop distances for an asset from its OHLC series and a chosen `entry`.
///
/// Each signal is measured as a **fraction of the asset's own price** (σ of returns, ATR / last
/// close, swing depth / last close) and then applied to the user's `entry`. This keeps stops
/// coherent even when `entry` is a placeholder (e.g. 100) rather than the live asset price —
/// otherwise an absolute ATR of ~390 on BTC would be subtracted from a 100 entry into nonsense.
///
/// Produces vol-based (1σ/2σ of per-period returns), ATR-based (1×/2× ATR) and a recent swing
/// stop (lowest low for longs / highest high for shorts over the lookback).
pub fn asset_signals(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    ts: &[String],
    timeframe: &str,
    entry: f64,
    side: Side,
    atr_period: usize,
    swing_lookback: usize,
) -> AssetSignals {
    let _ = ts;
    let ppy = periods_per_year(timeframe);
    let rets = returns(closes);
    let sd = stddev(&rets);
    let a = atr(highs, lows, closes, atr_period.max(1));
    let last_close = closes.last().copied().unwrap_or(entry);
    let e = if entry > 0.0 { entry } else { last_close };
    // Reference price the ATR/swing were measured against (guarded for degenerate data).
    let ref_px = if last_close > 0.0 { last_close } else { e };

    let place = |dist: f64| match side {
        Side::Long => e - dist,
        Side::Short => e + dist,
    };
    // `frac` is the stop depth as a fraction of price; the distance in the user's entry units is
    // frac × entry, so the suggestion scales to whatever entry the user typed.
    let mk = |key: &str, label: &str, frac: f64| {
        let dist = frac.max(0.0) * e;
        StopSuggestion {
            key: key.into(),
            label: label.into(),
            distance: dist,
            stop_price: place(dist),
            distance_pct: frac.max(0.0),
        }
    };

    let atr_frac = if ref_px > 0.0 { a / ref_px } else { 0.0 };
    let mut suggestions = vec![
        mk("hv_1sigma", "1σ move (per-period vol)", sd),
        mk("hv_2sigma", "2σ move (per-period vol)", 2.0 * sd),
        mk("atr_1", "1× ATR", atr_frac),
        mk("atr_2", "2× ATR", 2.0 * atr_frac),
    ];

    // Recent swing stop: depth from the reference price to the window extreme, as a fraction.
    let look = swing_lookback.max(1).min(closes.len());
    if look >= 1 {
        let tail_lo = &lows[lows.len().saturating_sub(look)..];
        let tail_hi = &highs[highs.len().saturating_sub(look)..];
        let swing_frac = match side {
            Side::Long => {
                let lo = tail_lo.iter().cloned().fold(f64::INFINITY, f64::min);
                if ref_px > 0.0 { (ref_px - lo).max(0.0) / ref_px } else { 0.0 }
            }
            Side::Short => {
                let hi = tail_hi.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                if ref_px > 0.0 { (hi - ref_px).max(0.0) / ref_px } else { 0.0 }
            }
        };
        if swing_frac.is_finite() && swing_frac > 0.0 {
            suggestions.push(mk("swing", &format!("Recent swing ({look} bars)"), swing_frac));
        }
    }

    AssetSignals {
        periods: rets.len(),
        periods_per_year: ppy,
        hv_period: sd,
        hv_annual: sd * ppy.sqrt(),
        atr: a,
        last_close,
        suggestions,
    }
}

// ── Multi-asset: correlation, frontier, risk parity ─────────────────────────────

#[derive(Debug, Serialize)]
pub struct CorrelationMatrix {
    pub labels: Vec<String>,
    /// Row-major NxN correlation matrix.
    pub matrix: Vec<Vec<f64>>,
}

/// Pairwise correlation matrix of aligned return series.
pub fn correlation_matrix(labels: &[String], rets: &[Vec<f64>]) -> CorrelationMatrix {
    let n = rets.len();
    let mut matrix = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            matrix[i][j] = if i == j { 1.0 } else { correlation(&rets[i], &rets[j]) };
        }
    }
    CorrelationMatrix { labels: labels.to_vec(), matrix }
}

#[derive(Debug, Serialize)]
pub struct FrontierPoint {
    pub ret: f64,
    pub vol: f64,
    pub sharpe: f64,
    pub weights: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub struct FrontierResult {
    pub labels: Vec<String>,
    /// Monte-Carlo random portfolios (annualized).
    pub cloud: Vec<FrontierPoint>,
    pub min_vol: FrontierPoint,
    pub max_sharpe: FrontierPoint,
}

/// xorshift64 — deterministic, dependency-free RNG for reproducible clouds.
struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        // 53-bit mantissa → [0, 1).
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn annualized_mean(rets: &[Vec<f64>], ppy: f64) -> Vec<f64> {
    rets.iter().map(|r| mean(r) * ppy).collect()
}

fn cov_matrix(rets: &[Vec<f64>], ppy: f64) -> Vec<Vec<f64>> {
    let n = rets.len();
    let mut c = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            c[i][j] = covariance(&rets[i], &rets[j]) * ppy;
        }
    }
    c
}

fn port_return(mu: &[f64], w: &[f64]) -> f64 {
    mu.iter().zip(w).map(|(m, x)| m * x).sum()
}

fn port_vol(cov: &[Vec<f64>], w: &[f64]) -> f64 {
    let n = w.len();
    let mut v = 0.0;
    for i in 0..n {
        for j in 0..n {
            v += w[i] * w[j] * cov[i][j];
        }
    }
    v.max(0.0).sqrt()
}

fn point(mu: &[f64], cov: &[Vec<f64>], w: Vec<f64>, rf: f64) -> FrontierPoint {
    let ret = port_return(mu, &w);
    let vol = port_vol(cov, &w);
    let sharpe = if vol > 0.0 { (ret - rf) / vol } else { 0.0 };
    FrontierPoint { ret, vol, sharpe, weights: w }
}

/// Monte-Carlo efficient frontier over long-only fully-invested portfolios.
pub fn efficient_frontier(
    labels: &[String],
    rets: &[Vec<f64>],
    ppy: f64,
    samples: usize,
    rf: f64,
) -> FrontierResult {
    let n = rets.len();
    let mu = annualized_mean(rets, ppy);
    let cov = cov_matrix(rets, ppy);
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut cloud = Vec::with_capacity(samples);
    let mut best_sharpe = f64::NEG_INFINITY;
    let mut min_vol_val = f64::INFINITY;
    let mut max_sharpe_w = vec![1.0 / n as f64; n];
    let mut min_vol_w = vec![1.0 / n as f64; n];
    for _ in 0..samples {
        // Dirichlet-ish: uniform weights normalized.
        let raw: Vec<f64> = (0..n).map(|_| rng.next_f64() + 1e-9).collect();
        let s: f64 = raw.iter().sum();
        let w: Vec<f64> = raw.iter().map(|x| x / s).collect();
        let p = point(&mu, &cov, w.clone(), rf);
        if p.sharpe > best_sharpe {
            best_sharpe = p.sharpe;
            max_sharpe_w = w.clone();
        }
        if p.vol < min_vol_val {
            min_vol_val = p.vol;
            min_vol_w = w.clone();
        }
        cloud.push(p);
    }
    FrontierResult {
        labels: labels.to_vec(),
        cloud,
        min_vol: point(&mu, &cov, min_vol_w, rf),
        max_sharpe: point(&mu, &cov, max_sharpe_w, rf),
    }
}

#[derive(Debug, Serialize)]
pub struct RiskParityResult {
    pub labels: Vec<String>,
    pub weights: Vec<f64>,
    /// Each asset's share of total portfolio risk (≈ equal at convergence).
    pub risk_contribution: Vec<f64>,
    /// Annualized portfolio volatility at these weights.
    pub portfolio_vol: f64,
}

/// Risk parity via iterative inverse-vol-of-marginal-contribution updates.
/// Long-only, fully invested. Converges to equal risk contributions for typical inputs.
pub fn risk_parity(labels: &[String], rets: &[Vec<f64>], ppy: f64) -> RiskParityResult {
    let n = rets.len();
    let cov = cov_matrix(rets, ppy);
    // Seed with inverse-volatility weights.
    let vols: Vec<f64> = (0..n).map(|i| cov[i][i].max(1e-12).sqrt()).collect();
    let mut w: Vec<f64> = vols.iter().map(|v| 1.0 / v).collect();
    let s: f64 = w.iter().sum();
    for x in &mut w {
        *x /= s;
    }
    let target = 1.0 / n as f64;
    for _ in 0..500 {
        // Marginal risk contribution: (Σw)_i. Total risk = w' Σ w.
        let mut sigma_w = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                sigma_w[i] += cov[i][j] * w[j];
            }
        }
        let total: f64 = (0..n).map(|i| w[i] * sigma_w[i]).sum();
        if total <= 0.0 {
            break;
        }
        // Each asset's risk-contribution share; nudge weights toward equal shares.
        let mut new_w = w.clone();
        for i in 0..n {
            let rc = w[i] * sigma_w[i] / total;
            if rc > 0.0 {
                new_w[i] = w[i] * (target / rc).powf(0.5);
            }
        }
        let ns: f64 = new_w.iter().sum();
        for x in &mut new_w {
            *x /= ns;
        }
        let delta: f64 = (0..n).map(|i| (new_w[i] - w[i]).abs()).sum();
        w = new_w;
        if delta < 1e-9 {
            break;
        }
    }
    // Final risk contributions.
    let mut sigma_w = vec![0.0; n];
    for i in 0..n {
        for j in 0..n {
            sigma_w[i] += cov[i][j] * w[j];
        }
    }
    let total: f64 = (0..n).map(|i| w[i] * sigma_w[i]).sum();
    let rc: Vec<f64> = (0..n)
        .map(|i| if total > 0.0 { w[i] * sigma_w[i] / total } else { 0.0 })
        .collect();
    RiskParityResult {
        labels: labels.to_vec(),
        weights: w.clone(),
        risk_contribution: rc,
        portfolio_vol: total.sqrt(),
    }
}

// ── Monte-Carlo trade-sequence resampling ───────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Percentiles {
    pub p5: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p95: f64,
}

/// One step of the equity fan chart: the percentile band across all simulated paths at
/// trade index `step` (0 = starting capital).
#[derive(Debug, Serialize)]
pub struct FanPoint {
    pub step: usize,
    pub p5: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p95: f64,
}

#[derive(Debug, Serialize)]
pub struct MonteCarloResult {
    /// Number of source trades resampled from.
    pub source_trades: usize,
    /// Trades per simulated path (the horizon).
    pub horizon: usize,
    pub iterations: usize,
    pub start_capital: f64,
    /// "bootstrap" (IID with replacement) or "block" (fixed-length blocks, streak-preserving).
    pub method: String,
    /// Per-step equity percentile bands for the fan chart (len = horizon + 1).
    pub fan: Vec<FanPoint>,
    /// Distribution of final equity across paths.
    pub final_equity: Percentiles,
    /// Distribution of max drawdown (positive fraction) across paths.
    pub max_drawdown: Percentiles,
    /// Fraction of paths whose equity ever fell to/below the ruin threshold.
    pub risk_of_ruin: f64,
    /// The ruin threshold in currency (fraction × start_capital) the ruin test used.
    pub ruin_level: f64,
    /// Fraction of paths ending below the starting capital.
    pub prob_loss: f64,
    /// Median final equity's total return fraction vs. start ((p50 / start) - 1).
    pub median_return: f64,
    /// Histogram of final equity for charting.
    pub final_histogram: Histogram,
    /// Histogram of max drawdown for charting.
    pub drawdown_histogram: Histogram,
    /// The actual (un-resampled) realized equity curve, for overlay/reference.
    pub actual_curve: Vec<f64>,
}

fn percentiles(sorted: &[f64]) -> Percentiles {
    let q = |p: f64| quantile_sorted(sorted, p);
    Percentiles { p5: q(0.05), p25: q(0.25), p50: q(0.50), p75: q(0.75), p95: q(0.95) }
}

/// Linear-interpolated quantile of an already-sorted slice.
fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = p.clamp(0.0, 1.0) * (sorted.len() as f64 - 1.0);
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    let frac = idx - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// Monte-Carlo over a realized per-trade P&L sequence (account-currency deltas).
///
/// Each of `iterations` paths draws `horizon` trades from `pnls` — either IID with replacement
/// ("bootstrap") or in contiguous blocks of `block_len` ("block", preserving win/loss streaks and
/// autocorrelation) — and walks an equity curve from `start_capital`. We record every path's
/// final equity, its worst peak-to-trough drawdown, and whether it ever breached the ruin level
/// (`ruin_pct` × start_capital). Percentile bands over the paths give the fan chart and the
/// final-equity / max-drawdown distributions; the breach rate is the risk of ruin.
pub fn monte_carlo(
    pnls: &[f64],
    start_capital: f64,
    iterations: usize,
    horizon: usize,
    block_len: usize,
    ruin_pct: f64,
    seed: u64,
) -> MonteCarloResult {
    let iterations = iterations.clamp(100, 100_000);
    let horizon = horizon.clamp(1, 20_000);
    let block = block_len.clamp(1, pnls.len().max(1));
    let use_block = block > 1;
    let ruin_level = start_capital * ruin_pct.clamp(0.0, 1.0);
    let mut rng = Rng(seed | 1);

    // The fan chart only needs ~a few hundred x positions, so sample the steps: every
    // `stride`-th step plus the last. Bounds memory (iterations × sampled steps, not
    // iterations × horizon) and keeps the JSON payload flat regardless of horizon.
    let stride = horizon.div_ceil(256).max(1);
    let fan_steps: Vec<usize> =
        (0..=horizon).filter(|s| s % stride == 0 || *s == horizon).collect();

    // Column-major accumulation: for each sampled step, collect equity across all paths so we
    // can take per-step percentiles for the fan without holding every full path.
    let mut step_equity: Vec<Vec<f64>> = vec![Vec::with_capacity(iterations); fan_steps.len()];
    let mut finals = Vec::with_capacity(iterations);
    let mut dds = Vec::with_capacity(iterations);
    let mut ruined = 0usize;
    let mut losses = 0usize;

    let n = pnls.len();
    for _ in 0..iterations {
        let mut equity = start_capital;
        let mut peak = start_capital;
        let mut max_dd = 0.0_f64;
        let mut breached = false;
        step_equity[0].push(equity);
        let mut fan_i = 1usize; // next fan_steps slot to fill

        let mut src = 0usize; // running index within the current block draw
        for step in 0..horizon {
            let pnl = if use_block {
                if step % block == 0 {
                    // Start a new block at a random offset.
                    src = (rng.next_f64() * n as f64) as usize % n.max(1);
                } else {
                    src = (src + 1) % n.max(1);
                }
                pnls[src]
            } else {
                let i = (rng.next_f64() * n as f64) as usize % n.max(1);
                pnls[i]
            };
            equity += pnl;
            if equity > peak {
                peak = equity;
            }
            if peak > 0.0 {
                max_dd = max_dd.max((peak - equity) / peak);
            }
            if !breached && equity <= ruin_level {
                breached = true;
            }
            if fan_i < fan_steps.len() && fan_steps[fan_i] == step + 1 {
                step_equity[fan_i].push(equity);
                fan_i += 1;
            }
        }

        if breached {
            ruined += 1;
        }
        if equity < start_capital {
            losses += 1;
        }
        finals.push(equity);
        dds.push(max_dd);
    }

    // Per-step percentile bands.
    let fan: Vec<FanPoint> = step_equity
        .iter_mut()
        .enumerate()
        .map(|(i, col)| {
            col.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            FanPoint {
                step: fan_steps[i],
                p5: quantile_sorted(col, 0.05),
                p25: quantile_sorted(col, 0.25),
                p50: quantile_sorted(col, 0.50),
                p75: quantile_sorted(col, 0.75),
                p95: quantile_sorted(col, 0.95),
            }
        })
        .collect();

    finals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    dds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let final_pct = percentiles(&finals);
    let median_return =
        if start_capital > 0.0 { final_pct.p50 / start_capital - 1.0 } else { 0.0 };

    // The actual realized equity curve (source order, no resampling), for overlay.
    let mut actual_curve = Vec::with_capacity(n + 1);
    let mut eq = start_capital;
    actual_curve.push(eq);
    for &p in pnls {
        eq += p;
        actual_curve.push(eq);
    }

    MonteCarloResult {
        source_trades: n,
        horizon,
        iterations,
        start_capital,
        method: if use_block { "block".into() } else { "bootstrap".into() },
        fan,
        final_equity: final_pct,
        max_drawdown: percentiles(&dds),
        risk_of_ruin: ruined as f64 / iterations as f64,
        ruin_level,
        prob_loss: losses as f64 / iterations as f64,
        median_return,
        final_histogram: histogram(&finals, 40),
        drawdown_histogram: histogram(&dds, 30),
        actual_curve,
    }
}

// ── Seasonality (month / weekday / hour buckets) ─────────────────────────────────

/// One bucket of a seasonal grouping (a month, weekday, or hour).
#[derive(Debug, Serialize)]
pub struct SeasonBucket {
    /// 0-based bucket key (0=Jan / 0=Mon / 0=00h depending on the axis).
    pub key: usize,
    /// Number of return observations in the bucket.
    pub count: usize,
    /// Mean of the chosen metric over the bucket (mean return, or per-period vol).
    pub value: f64,
    /// Fraction of positive returns in the bucket (win rate). Meaningful for the return metric.
    pub win_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct SeasonalityResult {
    pub periods: usize,
    /// "return" (mean period return) or "volatility" (stddev of period returns).
    pub metric: String,
    /// 12 month-of-year buckets (Jan..Dec).
    pub month: Vec<SeasonBucket>,
    /// 7 weekday buckets (Mon..Sun).
    pub weekday: Vec<SeasonBucket>,
    /// 24 hour-of-day buckets — omitted (empty) for daily+ timeframes where hour is degenerate.
    pub hour: Vec<SeasonBucket>,
    /// month × weekday matrix of the metric (rows = 12 months, cols = 7 weekdays); NaN→null.
    pub month_weekday: Vec<Vec<Option<f64>>>,
    /// True when the timeframe is intraday, so the hour axis is meaningful.
    pub has_hour: bool,
}

/// Aggregate a slice of (bucket-index, return) samples into `n` buckets.
fn agg_buckets(samples: &[(usize, f64)], n: usize, vol: bool) -> Vec<SeasonBucket> {
    let mut groups: Vec<Vec<f64>> = vec![Vec::new(); n];
    for &(b, r) in samples {
        if b < n {
            groups[b].push(r);
        }
    }
    groups
        .into_iter()
        .enumerate()
        .map(|(key, xs)| {
            let count = xs.len();
            let value = if vol { stddev(&xs) } else { mean(&xs) };
            let wins = xs.iter().filter(|x| **x > 0.0).count();
            let win_rate = if count > 0 { wins as f64 / count as f64 } else { 0.0 };
            SeasonBucket { key, count, value, win_rate }
        })
        .collect()
}

/// Seasonality of per-period returns bucketed by month, weekday, and (intraday only) hour.
///
/// `months`/`weekdays`/`hours` are the calendar components of each **bar** (0-based: month 0=Jan,
/// weekday 0=Mon, hour 0..23); they are aligned to `closes` (one component per bar). Returns are
/// bar-to-bar, so each return `r_t` is attributed to bar `t`'s calendar bucket. `metric` selects
/// mean return vs. per-bucket volatility. `has_hour` gates the hour axis for intraday data.
#[allow(clippy::too_many_arguments)]
pub fn seasonality(
    closes: &[f64],
    months: &[u8],
    weekdays: &[u8],
    hours: &[u8],
    metric_vol: bool,
    has_hour: bool,
) -> SeasonalityResult {
    let rets = returns(closes);
    // Attribute return r[i] (from close i→i+1) to bar i+1's calendar bucket.
    let mut m_s = Vec::with_capacity(rets.len());
    let mut w_s = Vec::with_capacity(rets.len());
    let mut h_s = Vec::with_capacity(rets.len());
    // month×weekday accumulation.
    let mut mw: Vec<Vec<Vec<f64>>> = vec![vec![Vec::new(); 7]; 12];
    for (i, &r) in rets.iter().enumerate() {
        let bar = i + 1;
        let mo = *months.get(bar).unwrap_or(&0) as usize;
        let wd = *weekdays.get(bar).unwrap_or(&0) as usize;
        let hr = *hours.get(bar).unwrap_or(&0) as usize;
        if mo < 12 {
            m_s.push((mo, r));
        }
        if wd < 7 {
            w_s.push((wd, r));
        }
        if has_hour && hr < 24 {
            h_s.push((hr, r));
        }
        if mo < 12 && wd < 7 {
            mw[mo][wd].push(r);
        }
    }

    let month_weekday: Vec<Vec<Option<f64>>> = mw
        .iter()
        .map(|row| {
            row.iter()
                .map(|xs| {
                    if xs.is_empty() {
                        None
                    } else {
                        Some(if metric_vol { stddev(xs) } else { mean(xs) })
                    }
                })
                .collect()
        })
        .collect();

    SeasonalityResult {
        periods: rets.len(),
        metric: if metric_vol { "volatility".into() } else { "return".into() },
        month: agg_buckets(&m_s, 12, metric_vol),
        weekday: agg_buckets(&w_s, 7, metric_vol),
        hour: if has_hour { agg_buckets(&h_s, 24, metric_vol) } else { Vec::new() },
        month_weekday,
        has_hour,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_size_risk_math() {
        // $10k stack, risk $100, long 100 → 95 stop (5 wide) → 20 units, $2000 notional.
        let r = position_size(10_000.0, 100.0, 100.0, 95.0, Side::Long, 1.0, None, None);
        assert!((r.quantity - 20.0).abs() < 1e-9);
        assert!((r.notional - 2000.0).abs() < 1e-9);
        assert!((r.risk_fraction - 0.01).abs() < 1e-9);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn position_size_over_leverage_flag() {
        // Notional $20k on a $10k stack at 1× is over buying power; at 2.5× it is not.
        let a = position_size(10_000.0, 200.0, 100.0, 99.0, Side::Long, 1.0, Some(1.0), None);
        assert!(a.over_leveraged);
        let b = position_size(10_000.0, 200.0, 100.0, 99.0, Side::Long, 1.0, Some(2.5), None);
        assert!(!b.over_leveraged);
        assert!((b.margin_required.unwrap() - b.notional / 2.5).abs() < 1e-6);
    }

    #[test]
    fn asset_signals_scale_to_entry_not_asset_price() {
        // Asset trades near 50_000 (so ATR is in the hundreds); entry is a placeholder 100.
        // Every suggested stop must land just under entry (fraction-scaled), never negative.
        let closes: Vec<f64> = (0..100).map(|i| 50_000.0 + (i as f64) * 10.0).collect();
        let highs: Vec<f64> = closes.iter().map(|c| c + 200.0).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 200.0).collect();
        let ts: Vec<String> = vec![String::new(); closes.len()];
        let sig = asset_signals(&highs, &lows, &closes, &ts, "1h", 100.0, Side::Long, 14, 20);
        for s in &sig.suggestions {
            assert!(s.stop_price > 0.0, "{}: stop went non-positive", s.key);
            assert!(s.stop_price < 100.0, "{}: stop not below long entry", s.key);
            // distance_pct is a fraction of price and should be small/sane, not asset-scaled.
            assert!(s.distance_pct >= 0.0 && s.distance_pct < 1.0, "{}: bad pct", s.key);
        }
    }

    #[test]
    fn monte_carlo_all_wins_never_ruins() {
        // Every trade is +100 → equity only rises → no path can breach a 50% ruin level,
        // final equity == start + horizon*100, and max drawdown is 0.
        let pnls = vec![100.0; 20];
        let mc = monte_carlo(&pnls, 10_000.0, 2_000, 30, 1, 0.5, 12345);
        assert_eq!(mc.risk_of_ruin, 0.0);
        assert_eq!(mc.prob_loss, 0.0);
        assert!((mc.final_equity.p50 - (10_000.0 + 30.0 * 100.0)).abs() < 1e-6);
        assert!(mc.max_drawdown.p95 < 1e-9);
        assert_eq!(mc.fan.len(), 31);
        assert!((mc.fan[0].p50 - 10_000.0).abs() < 1e-6);
    }

    #[test]
    fn monte_carlo_reproducible_and_ruin_detected() {
        // Big losers on a thin stack → some paths must breach the ruin level; and a fixed seed
        // reproduces the exact risk-of-ruin.
        let pnls = vec![50.0, 50.0, -400.0, 50.0];
        let a = monte_carlo(&pnls, 1_000.0, 5_000, 20, 1, 0.5, 999);
        let b = monte_carlo(&pnls, 1_000.0, 5_000, 20, 1, 0.5, 999);
        assert_eq!(a.risk_of_ruin, b.risk_of_ruin);
        assert!(a.risk_of_ruin > 0.0, "expected some ruined paths");
        assert!(a.max_drawdown.p50 > 0.0);
    }

    #[test]
    fn seasonality_buckets_align_and_count() {
        // 4 bars → 3 returns attributed to bars 1..3. Give distinct months so buckets separate.
        let closes = vec![100.0, 110.0, 99.0, 108.9]; // +10%, -10%, +10%
        let months = vec![0u8, 0, 1, 1]; // bar1→Jan, bar2→Feb, bar3→Feb
        let weekdays = vec![0u8, 0, 1, 2];
        let hours = vec![0u8; 4];
        let s = seasonality(&closes, &months, &weekdays, &hours, false, false);
        assert_eq!(s.periods, 3);
        assert_eq!(s.month.len(), 12);
        assert_eq!(s.month[0].count, 1); // one return in January (bar 1)
        assert_eq!(s.month[1].count, 2); // two returns in February (bars 2,3)
        assert!(s.hour.is_empty()); // has_hour=false
        assert_eq!(s.month_weekday.len(), 12);
        assert_eq!(s.month_weekday[0].len(), 7);
    }
}
