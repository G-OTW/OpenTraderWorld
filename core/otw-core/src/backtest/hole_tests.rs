//! A hole in the data is a break: no position crosses it, and no entry fills until every
//! input has warmed up again on the far side (see [`super::holes`]).

use super::tests::{settings_always_long, tests_bars};
use super::*;

/// Ten hourly crypto candles from a Saturday: five, then three days of nothing, then five.
fn stamps() -> Vec<String> {
    let mut out: Vec<String> = (0..5).map(|h| format!("2026-10-03T{h:02}:00:00Z")).collect();
    out.extend((0..5).map(|h| format!("2026-10-06T{h:02}:00:00Z")));
    out
}

fn rising() -> [Vec<f64>; 4] {
    let c: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
    let o: Vec<f64> = c.iter().map(|x| x - 0.5).collect();
    let h: Vec<f64> = c.iter().map(|x| x + 1.0).collect();
    let l: Vec<f64> = c.iter().map(|x| x - 1.0).collect();
    [o, h, l, c]
}

#[test]
fn a_position_open_at_a_hole_is_excluded_and_never_booked() {
    let t = stamps();
    let [o, h, l, c] = rising();
    let s = settings_always_long();
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));

    assert_eq!(r.data_gaps, vec![DataGap { ticker: "X".into(), from: t[4].clone(), to: t[5].clone() }]);
    // In at bar 1, cut at bar 4's close: listed, not booked.
    assert_eq!(r.excluded_trades.len(), 1);
    let x = &r.excluded_trades[0];
    assert_eq!((x.entry_ts.as_str(), x.exit_ts.as_str(), x.exit_reason.as_str()), (t[1].as_str(), t[4].as_str(), "data_gap"));
    // The first bar after the hole fills on a signal read before it: cold. Back in at bar 6.
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].entry_ts, t[6]);
    // The statistics and the cash only know the booked trade.
    assert_eq!(r.stats.trades, 1);
    let booked: f64 = r.trades.iter().map(|t| t.pnl).sum();
    assert!((r.stats.net_pnl - booked).abs() < 1e-9, "{} vs {booked}", r.stats.net_pnl);
    // Flat between the cut and the re-entry: equity is back to the starting capital.
    assert!((r.equity[5].equity - s.starting_capital).abs() < 1e-9);
}

#[test]
fn an_indicator_waits_its_own_length_after_a_hole() {
    let t = stamps();
    let [o, h, l, c] = rising();
    let mut s = settings_always_long();
    s.long.as_mut().unwrap().entry = Some(SignalGroup {
        logic: "all".into(),
        conditions: vec![Signal {
            left: Operand::Indicator { indicator: "sma".into(), period: 3, fast: 0, slow: 0, mult: 0.0, signal_period: 0 },
            op: Op::Above,
            right: Some(Operand::Const { value: 0.0 }),
        }],
    });
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    // Bars 5, 6 and 7 rebuild the SMA(3) from candles that followed each other: in at bar 8.
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].entry_ts, t[8]);
}

#[test]
fn unparseable_or_regular_stamps_change_nothing() {
    let t: Vec<String> = (0..10).map(|h| format!("2026-10-03T{h:02}:00:00Z")).collect();
    let [o, h, l, c] = rising();
    let r = run(&settings_always_long(), &tests_bars(&t, &o, &h, &l, &c));
    assert!(r.data_gaps.is_empty() && r.excluded_trades.is_empty());
    assert_eq!(r.trades.len(), 1);
}
