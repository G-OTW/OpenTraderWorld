//! Cross-check harness: runs the engine over real OHLCV exported to CSV and writes every
//! figure to JSON, so an independent reference implementation (scipy, statsmodels, arch,
//! nolds, hmmlearn, scikit-learn, PyPortfolioOpt) can be compared on the same bars.
//!
//! Ignored by default; run with
//! `OTW_QUANT_REF=<dir> cargo test -p otw-core quant::reference -- --ignored`
//! where `<dir>/fx/<NAME>.csv` holds `ts,open,high,low,close,volume` rows. Output lands in
//! `<dir>/rust/<NAME>.json`.

use serde_json::{json, Value};

use super::*;

pub(super) struct Ohlcv {
    pub ts: Vec<String>,
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub volume: Vec<f64>,
}

pub(super) fn load(path: &std::path::Path) -> Ohlcv {
    let text = std::fs::read_to_string(path).expect("fixture");
    let mut o = Ohlcv { ts: vec![], open: vec![], high: vec![], low: vec![], close: vec![], volume: vec![] };
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split(',').collect();
        o.ts.push(f[0].to_string());
        o.open.push(f[1].parse().unwrap());
        o.high.push(f[2].parse().unwrap());
        o.low.push(f[3].parse().unwrap());
        o.close.push(f[4].parse().unwrap());
        o.volume.push(f[5].parse().unwrap_or(0.0));
    }
    o
}

fn single(name: &str, d: &Ohlcv, ppy: f64) -> Value {
    let lr = stats::log_returns(&d.close);
    let sr = returns(&d.close);
    let lp: Vec<f64> = d.close.iter().map(|c| c.ln()).collect();
    let bars: Vec<vol::Bar> = (0..d.close.len())
        .map(|i| vol::Bar { open: d.open[i], high: d.high[i], low: d.low[i], close: d.close[i] })
        .collect();
    let vr: Vec<Value> = [2, 4, 8, 16].iter().map(|&q| json!(stats::variance_ratio(&lp, q))).collect();
    let pct: Vec<f64> = sr.iter().map(|r| r * 100.0).collect();
    let series = events::Series { open: &d.open, close: &d.close, volume: &d.volume };
    let conds = [
        ("gap_down_1pct", events::Condition::GapDown { pct: 0.01 }),
        ("big_down_2pct", events::Condition::BigDown { pct: 0.02 }),
        ("rsi_below_30", events::Condition::RsiBelow { period: 14, level: 30.0 }),
        ("cross_above_sma50", events::Condition::CrossAboveSma { period: 50 }),
        ("new_high_20", events::Condition::NewHigh { period: 20 }),
        ("streak_down_3", events::Condition::StreakDown { count: 3 }),
        ("volume_spike_2x", events::Condition::VolumeSpike { period: 20, mult: 2.0 }),
    ];
    let ev: serde_json::Map<String, Value> = conds
        .iter()
        .map(|(k, c)| {
            let raw = events::fire(c, &series);
            let kept = events::thin(&raw, 1);
            (k.to_string(), json!({ "events": raw, "study": events::study(&d.close, &d.ts, &kept, raw.len(), &[1, 5, 20], 5, 20) }))
        })
        .collect();
    json!({
        "name": name,
        "ppy": ppy,
        "moments": stats::moments(&lr),
        "acf": stats::acf(&lr, 20),
        "pacf": stats::pacf(&lr, 20),
        "acf_abs": stats::acf(&lr.iter().map(|v| v.abs()).collect::<Vec<_>>(), 20),
        "ljung_box": stats::ljung_box(&lr, &[5, 10, 20]),
        "hurst": stats::hurst(&lr),
        "variance_ratio": vr,
        "adf_logprice": stats::adf(&lp, true, 1),
        "adf_logret": stats::adf(&lr, true, 1),
        "kpss_logprice": stats::kpss(&lp),
        "kpss_logret": stats::kpss(&lr),
        "half_life_logprice": stats::half_life(&lp),
        "tail": stats::tail_risk(&sr, 0.95, 0.10),
        "sharpe": stats::sharpe_inference(&sr, ppy, 0.0, 0.0, 0.95),
        "vol": vol::vol_report(&bars, &d.ts, ppy, 21, 10),
        "hmm2": regime::fit(&pct, &d.ts[1..], ppy, 2, 1000),
        "events": ev,
    })
}

#[test]
#[ignore]
fn dump_reference_outputs() {
    let Ok(dir) = std::env::var("OTW_QUANT_REF") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(dir.join("rust")).unwrap();
    for (name, ppy) in [("SPY", 252.0), ("QQQ", 252.0), ("GLD", 252.0), ("TLT", 252.0), ("BTC1d", 365.0), ("BTC1h", 8760.0)] {
        let d = load(&dir.join("fx").join(format!("{name}.csv")));
        let v = single(name, &d, ppy);
        std::fs::write(dir.join("rust").join(format!("{name}.json")), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }
}

/// Intersect several fixtures on their exact timestamps, returning the shared clock and each
/// series' closes on it, and write the aligned table next to the output so the reference side
/// reads the very same numbers.
fn aligned(dir: &std::path::Path, names: &[&str], out: &str) -> (Vec<String>, Vec<Vec<f64>>) {
    let data: Vec<Ohlcv> = names.iter().map(|n| load(&dir.join("fx").join(format!("{n}.csv")))).collect();
    let mut common: Vec<String> = data[0].ts.clone();
    for d in &data[1..] {
        let set: std::collections::HashSet<&String> = d.ts.iter().collect();
        common.retain(|t| set.contains(t));
    }
    let closes: Vec<Vec<f64>> = data
        .iter()
        .map(|d| {
            let idx: std::collections::HashMap<&String, usize> = d.ts.iter().enumerate().map(|(i, t)| (t, i)).collect();
            common.iter().map(|t| d.close[idx[t]]).collect()
        })
        .collect();
    let mut csv = format!("ts,{}\n", names.join(","));
    for (i, t) in common.iter().enumerate() {
        csv.push_str(t);
        for c in &closes {
            csv.push_str(&format!(",{}", c[i]));
        }
        csv.push('\n');
    }
    std::fs::write(dir.join("rust").join(out), csv).unwrap();
    (common, closes)
}

fn pair(clock: &[String], y: &[f64], x: &[f64]) -> Value {
    let (ly, lx): (Vec<f64>, Vec<f64>) = (y.iter().map(|v| v.ln()).collect(), x.iter().map(|v| v.ln()).collect());
    let (ry, rx) = (returns(y), returns(x));
    let levels: Vec<Vec<f64>> = (0..ly.len()).map(|t| vec![ly[t], lx[t]]).collect();
    json!({
        "eg_yx": pairs::engle_granger(&ly, &lx),
        "eg_xy": pairs::engle_granger(&lx, &ly),
        "johansen": pairs::johansen(&levels, 1),
        "spread": pairs::spread(&ly, &lx, clock, 60),
        "rolling": pairs::rolling(&ry, &rx, &clock[1..], 60),
        "lead_lag": pairs::lagged_correlation(&ry, &rx, 5),
        "granger_x_to_y": pairs::granger(&ry, &rx, 5),
        "granger_y_to_x": pairs::granger(&rx, &ry, 5),
    })
}

#[test]
#[ignore]
fn dump_multi_asset_outputs() {
    let Ok(dir) = std::env::var("OTW_QUANT_REF") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(dir.join("rust")).unwrap();
    let etf = ["SPY", "QQQ", "GLD", "TLT", "IWM", "EFA", "VNQ"];
    let (clock, closes) = aligned(&dir, &etf, "etf_aligned.csv");
    let labels: Vec<String> = etf.iter().map(|s| s.to_string()).collect();
    let rets: Vec<Vec<f64>> = closes.iter().map(|c| returns(c)).collect();
    let (cc, cb) = aligned(&dir, &["ETH1h", "BTC1h"], "crypto_aligned.csv");
    let lv3: Vec<Vec<f64>> = (0..clock.len()).map(|t| vec![closes[0][t].ln(), closes[1][t].ln(), closes[4][t].ln()]).collect();
    let eq = vec![1.0 / 7.0; 7];
    let methods = [basket::Linkage::Single, basket::Linkage::Complete, basket::Linkage::Average, basket::Linkage::Ward];
    let factors1 = vec![("SPY".to_string(), rets[0].clone())];
    let factors3 = vec![("SPY".to_string(), rets[0].clone()), ("TLT".to_string(), rets[3].clone()), ("GLD".to_string(), rets[2].clone())];
    let v = json!({
        "pair_qqq_spy": pair(&clock, &closes[1], &closes[0]),
        "pair_gld_tlt": pair(&clock, &closes[2], &closes[3]),
        "pair_eth_btc": pair(&cc, &cb[0], &cb[1]),
        "johansen3": pairs::johansen(&lv3, 2),
        "correlation": correlation_matrix(&labels, &rets),
        "pca": basket::pca(&labels, &rets),
        "clustering": methods.iter().map(|m| basket::clustering(&labels, &rets, *m)).collect::<Vec<_>>(),
        "hrp": basket::hrp(&labels, &rets, 252.0, basket::Linkage::Single),
        "strength": basket::relative_strength(&labels, &closes, 252.0),
        "stress": basket::stress(&labels, &clock, &closes, &eq),
        "frontier": efficient_frontier(&labels, &rets, 252.0, 5000, 0.0),
        "risk_parity": risk_parity(&labels, &rets, 252.0),
        "reg1": basket::regression("QQQ", &rets[1], &factors1, 252.0, 0.0),
        "reg3": basket::regression("QQQ", &rets[1], &factors3, 252.0, 0.02),
    });
    std::fs::write(dir.join("rust").join("multi.json"), serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

#[test]
#[ignore]
fn time_large_series() {
    let Ok(dir) = std::env::var("OTW_QUANT_REF") else { return };
    let d = load(&std::path::PathBuf::from(dir).join("fx").join("BTC1m.csv"));
    let lr = stats::log_returns(&d.close);
    let lp: Vec<f64> = d.close.iter().map(|c| c.ln()).collect();
    let t = std::time::Instant::now();
    let lap = |label: &str| {
        eprintln!("{label}: {:?}", t.elapsed());
    };
    let _ = stats::moments(&lr);
    lap("moments");
    let _ = stats::correlogram(&lr, 20);
    lap("correlogram");
    let _ = stats::hurst(&lr);
    lap("hurst");
    let _ = stats::variance_ratio(&lp, 16);
    lap("vr");
    let _ = stats::adf(&lp, true, 1);
    lap("adf price");
    let _ = stats::adf(&lr, true, 1);
    lap("adf returns");
    let _ = stats::kpss(&lp);
    lap("kpss");
    let _ = stats::qq_points(&lr, 400);
    lap("qq");
    let bars: Vec<vol::Bar> = (0..d.close.len()).map(|i| vol::Bar { open: d.open[i], high: d.high[i], low: d.low[i], close: d.close[i] }).collect();
    let _ = vol::vol_report(&bars, &d.ts, 525600.0, 21, 21);
    lap("vol");
    let pct: Vec<f64> = returns(&d.close).iter().map(|r| r * 100.0).collect();
    let _ = regime::fit(&pct, &d.ts[1..], 525600.0, 3, 1000);
    lap("hmm3");
}

// ── Strategy section and calculators ─────────────────────────────────────────────────────

fn sma_settings(fast: usize, slow: usize, stop: f64, tp: f64, both: bool) -> crate::backtest::Settings {
    let cross = |op: &str| {
        json!({"logic": "all", "conditions": [{"op": op,
            "left": {"kind": "indicator", "period": fast, "indicator": "sma"},
            "right": {"kind": "indicator", "period": slow, "indicator": "sma"}}]})
    };
    let side = |entry: &str, exit: &str| {
        json!({"entry": cross(entry), "exit": cross(exit), "stop_loss_pct": stop, "take_profit_pct": tp, "exit_on_reverse": false})
    };
    serde_json::from_value(json!({
        "kind": "signals",
        "mode": if both { "both" } else { "long" },
        "long": side("crosses_above", "crosses_below"),
        "short": side("crosses_below", "crosses_above"),
        "sizing": {"mode": "percent_equity", "percent": 100},
        "starting_capital": 10000,
        "leverage": 1,
        "spread_pct": 0.0005,
        "fees": {"amount_kind": "pct", "per": "trade", "amount": 0.1},
        "pyramiding": 1,
        "instrument": {"min_qty": 0, "lot_step": 0, "multiplier": 1}
    }))
    .unwrap()
}

fn run_on(d: &Ohlcv, s: &crate::backtest::Settings) -> crate::backtest::RunResult {
    let b = crate::backtest::Bars {
        ticker: "",
        ts: &d.ts,
        open: &d.open,
        high: &d.high,
        low: &d.low,
        close: &d.close,
        volume: &d.volume,
    };
    crate::backtest::run_portfolio(s, &[&b])
}

fn trade_case(d: &Ohlcv, fast: usize, slow: usize, stop: f64, tp: f64, both: bool) -> Value {
    let s = sma_settings(fast, slow, stop, tp, both);
    let r = run_on(d, &s);
    let rows: Vec<strategy::TradeRow> = r
        .trades
        .iter()
        .map(|t| strategy::TradeRow {
            exit_ts: t.exit_ts.clone(),
            pnl: t.pnl,
            mae: t.mae,
            mfe: t.mfe,
            risk: (stop > 0.0).then(|| t.entry_price * t.qty.abs() * stop),
        })
        .collect();
    let pnls: Vec<f64> = r.trades.iter().map(|t| t.pnl).collect();
    json!({
        "fast": fast, "slow": slow, "stop": stop, "tp": tp, "both": both,
        "trades": r.trades,
        "analytics": strategy::trade_analytics(&rows),
        "mc_iid": monte_carlo(&pnls, 10_000.0, 2000, pnls.len(), 1, 0.5, 42),
        "mc_block": monte_carlo(&pnls, 10_000.0, 2000, pnls.len() * 2, 5, 0.3, 7),
    })
}

#[test]
#[ignore]
fn dump_strategy_outputs() {
    let Ok(dir) = std::env::var("OTW_QUANT_REF") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(dir.join("rust")).unwrap();
    let spy = load(&dir.join("fx").join("SPY.csv"));
    let btc = load(&dir.join("fx").join("BTC1h.csv"));
    let btcd = load(&dir.join("fx").join("BTC1d.csv"));

    let trades = json!({
        "spy_long_stop": trade_case(&spy, 20, 50, 0.02, 0.06, false),
        "spy_both": trade_case(&spy, 10, 30, 0.0, 0.0, true),
        "btc_long": trade_case(&btc, 9, 21, 0.0, 0.0, false),
        "btc_both_stop": trade_case(&btc, 20, 50, 0.01, 0.0, true),
    });

    // A 16-trial SMA grid on SPY: the sweep PBO and the deflated Sharpe are computed on.
    let mut labels = Vec::new();
    let mut curves = Vec::new();
    let mut rets = Vec::new();
    for fast in [5usize, 10, 20, 30] {
        for slow in [50usize, 100, 150, 200] {
            let r = run_on(&spy, &sma_settings(fast, slow, 0.0, 0.0, false));
            labels.push(format!("{fast}/{slow}"));
            rets.push(r.equity.windows(2).map(|w| w[1].equity / w[0].equity - 1.0).collect::<Vec<f64>>());
            curves.push(r.equity.iter().map(|p| (p.ts.clone(), p.equity)).collect::<Vec<_>>());
        }
    }
    let mut csv = String::from("ts");
    for l in &labels {
        csv.push(',');
        csv.push_str(l);
    }
    csv.push('\n');
    for i in 0..curves[0].len() {
        csv.push_str(&curves[0][i].0);
        for c in &curves {
            csv.push_str(&format!(",{:.17e}", c[i].1));
        }
        csv.push('\n');
    }
    std::fs::write(dir.join("rust").join("grid_equity.csv"), csv).unwrap();
    let sharpes: Vec<f64> = rets.iter().map(|r| mean(r) / stddev(r) * 252f64.sqrt()).collect();
    let best = (0..sharpes.len()).max_by(|a, b| sharpes[*a].partial_cmp(&sharpes[*b]).unwrap()).unwrap();
    let m = stats::moments(&rets[best]).unwrap();
    let pick: Vec<usize> = vec![0, 5, 10, 15];
    let sub_labels: Vec<String> = pick.iter().map(|&i| labels[i].clone()).collect();
    let sub_curves: Vec<Vec<(String, f64)>> = pick.iter().map(|&i| curves[i].clone()).collect();
    let grid = json!({
        "labels": labels,
        "pbo16": strategy::pbo(&rets, 16, 252.0),
        "pbo8": strategy::pbo(&rets, 8, 252.0),
        "deflated": deflated_sharpe(&sharpes, rets[0].len(), 252.0, Some((m.skew_biased, m.excess_kurtosis_biased + 3.0))),
        "compare": strategy::compare(&sub_labels, &sub_curves, 252.0, 10),
        "compare_labels": sub_labels,
    });

    let ruin_cases = [
        (0.55, 1.0, 0.1, 1.0, strategy::Sizing::Fixed, 5000usize),
        (0.45, 1.5, 0.02, 0.5, strategy::Sizing::Fixed, 2000),
        (0.40, 2.0, 0.05, 0.3, strategy::Sizing::Fixed, 2000),
        (0.50, 1.2, 0.03, 0.5, strategy::Sizing::Fractional, 3000),
        (0.35, 2.5, 0.02, 0.4, strategy::Sizing::Fractional, 3000),
        (0.60, 0.8, 0.05, 0.5, strategy::Sizing::Fractional, 3000),
        (0.40, 1.2, 0.02, 0.5, strategy::Sizing::Fixed, 1000),
    ];
    let ruin: Vec<Value> = ruin_cases
        .iter()
        .map(|&(p, w, f, u, sz, h)| {
            json!({"p": p, "w": w, "f": f, "u": u, "fractional": sz == strategy::Sizing::Fractional, "horizon": h,
                   "out": strategy::risk_of_ruin(p, w, f, u, sz, h, 40_000, 99)})
        })
        .collect();

    let mut options = Vec::new();
    for call in [true, false] {
        for k in [80.0, 100.0, 120.0] {
            for days in [30.0, 365.0] {
                for (r, q) in [(0.0, 0.0), (0.05, 0.02)] {
                    for v in [0.15, 0.6] {
                        let t = days / 365.0;
                        let g = calc::bsm(call, 100.0, k, t, r, q, v).unwrap();
                        options.push(json!({
                            "call": call, "k": k, "days": days, "r": r, "q": q, "v": v, "greeks": g,
                            "iv": calc::implied_vol(call, g.price, 100.0, k, t, r, q),
                            "crr_eu": calc::binomial(call, false, 100.0, k, t, r, q, v, 500),
                            "crr_am": calc::binomial(call, true, 100.0, k, t, r, q, v, 500),
                        }));
                    }
                }
            }
        }
    }
    let basis_cases = [
        (100.0, 101.5, 90.0, Some(0.05), 0.0, Some((102.9, 180.0))),
        (4500.0, 4480.0, 45.0, Some(0.04), 0.018, None),
        (60000.0, 61200.0, 120.0, None, 0.0, Some((62100.0, 210.0))),
        (80.0, 78.5, 30.0, Some(0.03), 0.0, Some((77.0, 60.0))),
    ];
    let basis: Vec<Value> = basis_cases
        .iter()
        .map(|&(s, f, d, r, q, n)| json!({"s": s, "f": f, "d": d, "r": r, "q": q, "next": n, "out": calc::basis(s, f, d, r, q, n)}))
        .collect();
    let comp_cases = [(10_000.0, 0.07, 30usize, 0.0, Some(100_000.0)), (5000.0, 0.005, 120, 200.0, Some(50_000.0)), (0.0, 0.01, 60, 100.0, Some(10_000.0)), (1000.0, 0.0, 10, 50.0, Some(1500.0))];
    let compound: Vec<Value> = comp_cases
        .iter()
        .map(|&(pv, r, n, c, t)| json!({"pv": pv, "r": r, "n": n, "c": c, "t": t, "out": calc::compounding(pv, r, n, c, t)}))
        .collect();
    let vt = json!({
        "spy_rolling": calc::vol_target(&spy.ts, &spy.close, 252.0, 0.10, 20, calc::VolEstimator::Rolling, 2.0, Some(100_000.0)),
        "spy_ewma": calc::vol_target(&spy.ts, &spy.close, 252.0, 0.10, 30, calc::VolEstimator::Ewma, 1.5, Some(100_000.0)),
        "btc_rolling": calc::vol_target(&btcd.ts, &btcd.close, 365.0, 0.40, 30, calc::VolEstimator::Rolling, 1.0, Some(50_000.0)),
        "btc_ewma": calc::vol_target(&btcd.ts, &btcd.close, 365.0, 0.40, 20, calc::VolEstimator::Ewma, 3.0, None),
    });
    let sharpe_cases = [(1.2, 756usize, 252.0, -0.5, 6.0, 0.0), (0.8, 120, 12.0, 0.0, 3.0, 0.5), (2.0, 2000, 365.0, -1.2, 12.0, 1.0), (0.3, 5000, 252.0, 0.2, 4.0, 0.0)];
    let sharpe: Vec<Value> = sharpe_cases
        .iter()
        .map(|&(s, n, ppy, sk, ku, b)| json!({"s": s, "n": n, "ppy": ppy, "skew": sk, "kurt": ku, "bench": b,
            "out": stats::sharpe_from_summary(s, n, ppy, sk, ku, b, 0.95)}))
        .collect();
    let kelly_cases = [(0.55, 1.2, 1.0), (0.4, 2.5, 1.0), (0.3, 1.0, 1.0), (0.62, 150.0, 210.0)];
    let kelly: Vec<Value> = kelly_cases.iter().map(|&(p, w, l)| json!({"p": p, "w": w, "l": l, "out": kelly(p, w, l)})).collect();
    let size_cases = [
        (10_000.0, 100.0, 100.0, 95.0, Side::Long, 1.0, None, Some(115.0)),
        (25_000.0, 250.0, 4500.0, 4520.0, Side::Short, 50.0, Some(20.0), Some(4440.0)),
        (5000.0, 50.0, 1.105, 1.1, Side::Long, 100_000.0, Some(30.0), None),
        (2000.0, 100.0, 60_000.0, 58_500.0, Side::Long, 1.0, Some(10.0), None),
    ];
    let size: Vec<Value> = size_cases
        .iter()
        .map(|&(st, ra, e, sp, side, m, lev, tg)| json!({"stack": st, "risk": ra, "entry": e, "stop": sp,
            "long": side == Side::Long, "mult": m, "lev": lev, "target": tg,
            "out": position_size(st, ra, e, sp, side, m, lev, tg)}))
        .collect();

    let v = json!({
        "trades": trades, "grid": grid, "ruin": ruin, "options": options, "basis": basis,
        "compound": compound, "voltarget": vt, "sharpe": sharpe, "kelly": kelly, "size": size,
    });
    std::fs::write(dir.join("rust").join("strategy.json"), serde_json::to_string_pretty(&v).unwrap()).unwrap();
}
