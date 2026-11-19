//! Closed-trade analytics behind the report's statistics tables.
//!
//! Port of the frontend's `analysis/trades.js`, so the report prints the same numbers the
//! results screen shows. It reads the lean trade projection the report `extra` carries
//! (`ticker`, `direction`, `pnl`, `return_pct`, `mae`, `mfe`, `bars_held`), never the engine
//! types, which keeps the report free of the `Trade` shape.
//!
//! One sign convention to keep in mind: the engine stores `mae` (heat) as a positive
//! magnitude. `avg_heat` is returned negated, the way the UI shows it.

use serde_json::Value;

/// One closed trade, as the report reads it.
#[derive(Clone, Copy)]
pub struct T<'a> {
    pub ticker: &'a str,
    pub long: bool,
    pub pnl: f64,
    pub return_pct: f64,
    /// Worst open loss reached in-trade, positive magnitude.
    pub mae: f64,
    /// Best open profit reached in-trade, positive magnitude.
    pub mfe: f64,
    pub bars_held: f64,
}

/// Read the trade projection out of a report `extra` block.
pub fn read(trades: &Value) -> Vec<T<'_>> {
    let Some(rows) = trades.as_array() else { return Vec::new() };
    rows.iter()
        .map(|t| {
            let f = |k: &str| t.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            T {
                ticker: t.get("ticker").and_then(Value::as_str).unwrap_or(""),
                long: t.get("direction").and_then(Value::as_str) != Some("short"),
                pnl: f("pnl"),
                return_pct: f("return_pct"),
                mae: f("mae"),
                mfe: f("mfe"),
                bars_held: f("bars_held"),
            }
        })
        .collect()
}

/// What a set of closed trades came to. Every mean is `None` when nothing feeds it, so an
/// empty scope prints "–" instead of a fake zero.
#[derive(Default)]
pub struct Stats {
    pub trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub pnl: f64,
    pub gross_win: f64,
    pub gross_loss: f64,
    pub profit_factor: Option<f64>,
    pub expectancy: Option<f64>,
    pub avg_win: Option<f64>,
    pub avg_loss: Option<f64>,
    pub best: Option<f64>,
    pub worst: Option<f64>,
    pub avg_pnl_pct: Option<f64>,
    pub avg_bars: Option<f64>,
    /// Mean result over the heat it took, averaged only over trades that had heat.
    pub avg_r: Option<f64>,
    pub avg_runup: Option<f64>,
    /// Mean heat, negative (a loss the trade sat through before closing).
    pub avg_heat: Option<f64>,
    /// Worst peak-to-trough drop of the closed-trade equity curve, positive.
    pub max_drawdown: f64,
    pub max_consec_wins: usize,
    pub max_consec_losses: usize,
}

fn mean(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
}

/// Fold a set of trades into its statistics. Order matters for the streaks and the drawdown,
/// so the caller passes them in the order the engine closed them.
pub fn stats(ts: &[T]) -> Stats {
    let mut s = Stats { trades: ts.len(), ..Default::default() };
    let (mut equity, mut peak) = (0.0f64, 0.0f64);
    let (mut run_w, mut run_l) = (0usize, 0usize);
    let (mut wins, mut losses, mut pcts, mut bars, mut rs, mut runups, mut heats) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for t in ts {
        s.pnl += t.pnl;
        equity += t.pnl;
        peak = peak.max(equity);
        s.max_drawdown = s.max_drawdown.max(peak - equity);
        pcts.push(t.return_pct);
        bars.push(t.bars_held);
        runups.push(t.mfe);
        heats.push(-t.mae);
        if t.mae > 0.0 {
            rs.push(t.pnl / t.mae);
        }
        if t.pnl > 0.0 {
            s.wins += 1;
            s.gross_win += t.pnl;
            wins.push(t.pnl);
            run_w += 1;
            run_l = 0;
        } else if t.pnl < 0.0 {
            s.losses += 1;
            s.gross_loss += -t.pnl;
            losses.push(t.pnl);
            run_l += 1;
            run_w = 0;
        } else {
            run_w = 0;
            run_l = 0;
        }
        s.max_consec_wins = s.max_consec_wins.max(run_w);
        s.max_consec_losses = s.max_consec_losses.max(run_l);
    }
    if s.trades > 0 {
        s.win_rate = s.wins as f64 * 100.0 / s.trades as f64;
        s.expectancy = Some(s.pnl / s.trades as f64);
        s.best = Some(ts.iter().map(|t| t.pnl).fold(f64::NEG_INFINITY, f64::max));
        s.worst = Some(ts.iter().map(|t| t.pnl).fold(f64::INFINITY, f64::min));
    }
    if s.gross_loss > 1e-9 {
        s.profit_factor = Some(s.gross_win / s.gross_loss);
    }
    s.avg_win = mean(&wins);
    s.avg_loss = mean(&losses);
    s.avg_pnl_pct = mean(&pcts);
    s.avg_bars = mean(&bars);
    s.avg_r = mean(&rs);
    s.avg_runup = mean(&runups);
    s.avg_heat = mean(&heats);
    s
}

/// Trades bucketed by their percentage result, winners and losers counted apart.
pub struct Bucket {
    pub from: f64,
    pub to: f64,
    pub winners: usize,
    pub losers: usize,
}

/// Histogram of per-trade returns over ~16 buckets snapped to a readable width
/// (…, 0.5, 1, 2, 5, 10, …), the same rule the results screen's distribution uses.
pub fn return_buckets(ts: &[T]) -> Vec<Bucket> {
    if ts.is_empty() {
        return Vec::new();
    }
    let lo = ts.iter().map(|t| t.return_pct).fold(f64::INFINITY, f64::min);
    let hi = ts.iter().map(|t| t.return_pct).fold(f64::NEG_INFINITY, f64::max);
    let span = (hi - lo).max(1e-6);
    let raw = span / 16.0;
    let mag = 10f64.powf(raw.log10().floor());
    let w = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|k| k * mag)
        .find(|k| *k >= raw)
        .unwrap_or(mag * 10.0);
    let idx = |v: f64| (v / w).floor() as i64;
    let (from, to) = (idx(lo), idx(hi));
    // A pathological spread can't be allowed to allocate a million empty rows.
    if to - from > 60 {
        return Vec::new();
    }
    let mut buckets: Vec<Bucket> = (from..=to)
        .map(|i| Bucket { from: i as f64 * w, to: (i + 1) as f64 * w, winners: 0, losers: 0 })
        .collect();
    for t in ts {
        let Some(b) = buckets.get_mut((idx(t.return_pct) - from) as usize) else { continue };
        if t.pnl >= 0.0 {
            b.winners += 1;
        } else {
            b.losers += 1;
        }
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(pnl: f64, pct: f64, mae: f64, mfe: f64, long: bool) -> T<'static> {
        T { ticker: "X", long, pnl, return_pct: pct, mae, mfe, bars_held: 10.0 }
    }

    #[test]
    fn folds_trades_into_the_screen_s_numbers() {
        let ts = vec![
            t(100.0, 2.0, 50.0, 120.0, true),
            t(-40.0, -1.0, 60.0, 10.0, true),
            t(-30.0, -0.5, 30.0, 5.0, false),
            t(70.0, 1.5, 20.0, 90.0, false),
        ];
        let s = stats(&ts);
        assert_eq!((s.trades, s.wins, s.losses), (4, 2, 2));
        assert!((s.pnl - 100.0).abs() < 1e-9);
        assert!((s.gross_win - 170.0).abs() < 1e-9);
        assert!((s.gross_loss - 70.0).abs() < 1e-9);
        assert!((s.profit_factor.unwrap() - 170.0 / 70.0).abs() < 1e-9);
        assert!((s.expectancy.unwrap() - 25.0).abs() < 1e-9);
        assert_eq!(s.max_consec_losses, 2);
        // Peak 100 after the first trade, trough 30 after the third.
        assert!((s.max_drawdown - 70.0).abs() < 1e-9);
        // Heat is reported negative.
        assert!(s.avg_heat.unwrap() < 0.0);
        assert!(s.avg_r.unwrap().is_finite());
    }

    #[test]
    fn buckets_span_the_observed_returns() {
        let ts = vec![
            t(1.0, -3.0, 0.0, 0.0, true),
            t(1.0, 0.5, 0.0, 0.0, true),
            t(-1.0, 4.0, 0.0, 0.0, true),
        ];
        let b = return_buckets(&ts);
        assert!(!b.is_empty());
        assert!(b.first().unwrap().from <= -3.0);
        assert!(b.last().unwrap().to >= 4.0);
        assert_eq!(b.iter().map(|x| x.winners + x.losers).sum::<usize>(), 3);
    }

    #[test]
    fn reads_the_report_projection() {
        let v = json!([{ "ticker": "BTCUSDT", "direction": "short", "pnl": 5.0,
                         "return_pct": 1.0, "mae": 2.0, "mfe": 7.0, "bars_held": 3 }]);
        let ts = read(&v);
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].ticker, "BTCUSDT");
        assert!(!ts[0].long);
        assert!((ts[0].bars_held - 3.0).abs() < 1e-9);
    }
}
