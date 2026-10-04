//! One test per lookahead fix of the 2026-10-03 audit (`ProjectSpecs/audit/BACKTEST_LOOKAHEAD_261003.md`).
//! Each one fails on the engine as it was before the fix.

use super::tests::{cond, settings_always_long, tests_bars};
use super::*;

fn ts(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("t{i}")).collect()
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

fn close_above(v: f64) -> SignalGroup {
    SignalGroup {
        logic: "all".into(),
        conditions: vec![Signal { left: Operand::Price { field: "close".into() }, op: Op::Above, right: Some(Operand::Const { value: v }) }],
    }
}

/// #2: `exit_on_reverse` is decided at the close where the entry group stops holding, and
/// leaves at the next open, never at that same close.
#[test]
fn exit_on_reverse_leaves_at_the_next_open() {
    let t = ts(5);
    let o = [100.0, 101.0, 102.0, 99.0, 98.0];
    let h = [101.0, 102.0, 102.5, 99.5, 98.5];
    let l = [99.5, 100.5, 98.0, 98.5, 97.5];
    let c = [100.6, 101.5, 98.5, 99.0, 98.0];
    let mut s = settings_always_long();
    let sd = s.long.as_mut().unwrap();
    sd.entry = Some(close_above(100.5));
    sd.exit_on_reverse = true;
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let tr = &r.trades[0];
    assert_eq!(tr.entry_ts, "t1");
    assert_eq!(tr.exit_reason, "signal");
    // Entry stops holding at t2's close (98.5): out at t3's open, not at 98.5.
    assert_eq!(tr.exit_ts, "t3");
    approx(tr.exit_price, 99.0);
}

/// #2, forward: the market exit `exit_on_reverse` decided at the last close makes room for an
/// entry at the next open, and an exit limit it decides is reported resting.
#[test]
fn forward_run_reports_an_exit_on_reverse_limit() {
    let t = ts(4);
    let o = [100.0, 101.0, 102.0, 101.0];
    let h = [101.0, 102.0, 102.5, 101.5];
    let l = [99.5, 100.5, 101.0, 99.0];
    let c = [100.6, 101.5, 102.0, 99.5];
    let mut s = settings_always_long();
    let sd = s.long.as_mut().unwrap();
    sd.entry = Some(close_above(100.5));
    sd.exit_on_reverse = true;
    // At market: still held after the last close, it leaves at the next open.
    let r = run_portfolio_forced(&s, &[&tests_bars(&t, &o, &h, &l, &c)], &[]);
    assert!(r.trades.iter().all(|x| x.exit_reason != "signal"), "{:?}", r.trades);
    assert_eq!(r.open_positions.len(), 1, "{:?}", r.trades);
    s.execution.exit_order = OrderSpec { kind: "limit".into(), offset: 0.01, ..OrderSpec::default() };
    let r = run_portfolio_forced(&s, &[&tests_bars(&t, &o, &h, &l, &c)], &[]);
    let p = &r.open_positions[0];
    approx(p.exit_limit.unwrap(), 99.5 * 1.01);
    assert_eq!(p.exit_limit_reason.as_deref(), Some("signal"));
}

/// #12: an exit limit left unfilled goes to market at the next open.
#[test]
fn expired_exit_limit_goes_to_market_at_the_next_open() {
    let t = ts(5);
    let o = [100.0; 5];
    let h = [100.5; 5];
    let l = [99.5; 5];
    let c = [100.0, 100.0, 100.2, 100.0, 100.0];
    let mut s = settings_always_long();
    s.long.as_mut().unwrap().exit = Some(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
    s.execution.exit_order = OrderSpec { kind: "limit".into(), offset: 0.05, ..OrderSpec::default() };
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    assert_eq!(r.trades[0].exit_ts, "t3");
    approx(r.trades[0].exit_price, 100.0);
}

/// #5: a position stopped inside a bar still held its slot at that bar's open, so a second
/// asset cannot take it at that open under `max_open_positions: 1`.
#[test]
fn a_slot_freed_inside_the_bar_is_not_free_at_its_open() {
    let t = ts(4);
    // A: enters at t1's open, stopped inside t2.
    let (ao, ah, al, ac) = ([100.0, 100.0, 100.0, 100.0], [100.5, 100.5, 100.5, 100.5], [99.5, 99.5, 90.0, 99.5], [100.0; 4]);
    // B: its entry signal fires at t1's close only, so it wants t2's open.
    let (bo, bh, bl, bc) = ([50.0; 4], [50.5; 4], [49.5; 4], [50.0, 50.0, 50.0, 50.0]);
    let mut s = settings_always_long();
    s.long.as_mut().unwrap().stop_loss_pct = 0.05;
    s.risk.max_open_positions = Some(1);
    let a = Bars { ticker: "A", ..tests_bars(&t, &ao, &ah, &al, &ac) };
    let b = Bars { ticker: "B", ..tests_bars(&t, &bo, &bh, &bl, &bc) };
    let r = run_portfolio(&s, &[&a, &b]);
    let b_entries: Vec<&str> = r.trades.iter().filter(|t| t.ticker == "B").map(|t| t.entry_ts.as_str()).collect();
    assert!(!b_entries.contains(&"t2"), "B entered at the open A still held: {:?}", r.trades);
}

/// #1: an add due at a bar's open fills there even when that bar then stops the position: the
/// stop closes the position the add made, not the one before it.
#[test]
fn an_add_at_the_open_is_not_dropped_by_a_later_stop() {
    let t = ts(4);
    let o = [100.0, 100.0, 100.0, 100.0];
    let h = [100.5, 100.5, 100.5, 100.5];
    let l = [99.5, 99.5, 90.0, 99.5];
    let c = [100.0; 4];
    let mut s = settings_always_long();
    s.pyramiding = 2;
    s.long.as_mut().unwrap().stop_loss_pct = 0.05;
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let tr = &r.trades[0];
    assert_eq!(tr.exit_ts, "t2");
    assert_eq!(tr.exit_reason, "stop_loss");
    assert_eq!(tr.entries, 2, "the add at t2's open was dropped");
    approx(tr.qty, 2.0);
    approx(tr.exit_price, 95.0);
}

/// #1, limit add: a resting add that fills inside the bar fills before a stop below it.
#[test]
fn a_limit_add_fills_before_a_stop_below_it() {
    let t = ts(4);
    let o = [100.0, 100.0, 100.0, 100.0];
    let h = [100.5, 100.5, 100.5, 100.5];
    let l = [99.5, 98.9, 90.0, 99.5];
    let c = [100.0; 4];
    let mut s = settings_always_long();
    s.pyramiding = 2;
    s.long.as_mut().unwrap().stop_loss_pct = 0.05;
    s.execution.entry_order = OrderSpec { kind: "limit".into(), offset: 0.01, ..OrderSpec::default() };
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let tr = &r.trades[0];
    assert_eq!(tr.entries, 2, "{:?}", r.trades);
    assert_eq!(tr.exit_ts, "t2");
    approx(tr.exit_price, 99.0 * 0.95);
}

/// #6: the stop an add moves (breakeven on the new average) is tested on the add's own bar.
#[test]
fn the_add_bar_is_tested_against_the_stop_the_add_moved() {
    let t = ts(5);
    let o = [100.0, 100.0, 110.0, 110.0, 110.0];
    let h = [100.5, 110.0, 110.5, 110.5, 110.5];
    let l = [99.5, 99.5, 104.0, 109.5, 109.5];
    let c = [100.0, 110.0, 110.0, 110.0, 110.0];
    let mut s = settings_always_long();
    s.pyramiding = 2;
    s.long.as_mut().unwrap().stop_loss_pct = 0.10;
    s.pyramid_steps.after_add_sl = "breakeven".into();
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let tr = &r.trades[0];
    assert_eq!(tr.entries, 2);
    assert_eq!(tr.exit_ts, "t2", "breakeven at 105 was not tested on t2 (low 104)");
    approx(tr.exit_price, 105.0);
}

/// #7: two sessions on one daily row (crypto opens 00:00 UTC, a stock 14:30 UTC). An entry at
/// the crypto open is sized on the stock's previous close, not on the stock's open 14 h later.
#[test]
fn an_earlier_session_does_not_see_a_later_open() {
    let day = |d: usize, hm: &str| format!("2024-01-0{}T{hm}:00Z", d + 1);
    let ta: Vec<String> = (0..5).map(|d| day(d, "00:00")).collect();
    let tb: Vec<String> = (0..5).map(|d| day(d, "14:30")).collect();
    // A crosses above 50 at d2's close: enters at d3's 00:00 open (60).
    let (ao, ah, al, ac) = ([10.0, 10.0, 10.0, 60.0, 60.0], [10.0, 10.0, 60.0, 60.0, 60.0], [10.0, 10.0, 10.0, 60.0, 60.0], [10.0, 10.0, 60.0, 60.0, 60.0]);
    // B crosses at d1's close: 10 units at d2's open (100); d3 opens at 200 at 14:30.
    let (bo, bh, bl, bc) = ([40.0, 40.0, 100.0, 200.0, 200.0], [40.0, 60.0, 100.0, 200.0, 200.0], [40.0, 40.0, 100.0, 200.0, 200.0], [40.0, 60.0, 100.0, 200.0, 200.0]);
    let mut s = settings_always_long();
    s.long.as_mut().unwrap().entry = Some(SignalGroup {
        logic: "all".into(),
        conditions: vec![Signal { left: Operand::Price { field: "close".into() }, op: Op::CrossesAbove, right: Some(Operand::Const { value: 50.0 }) }],
    });
    s.sizing = Sizing::PercentEquity { percent: 10.0 };
    let a = Bars { ticker: "A", ..tests_bars(&ta, &ao, &ah, &al, &ac) };
    let b = Bars { ticker: "B", ..tests_bars(&tb, &bo, &bh, &bl, &bc) };
    let r = run_portfolio(&s, &[&a, &b]);
    let ta3 = r.trades.iter().find(|t| t.ticker == "A").expect("A traded");
    // Equity at A's open: 10 000 (B still at its d2 close of 100), not 11 000.
    approx(ta3.qty, 1_000.0 / 60.0);
}

/// #11: after a limit filled inside its bar, the trail ratchets from what the position lived
/// through (fill, close), not from a high that printed before the fill.
#[test]
fn the_trail_ignores_the_high_before_a_mid_bar_fill() {
    let t = ts(4);
    let o = [100.0, 100.0, 100.0, 100.0];
    let h = [100.2, 110.0, 100.5, 100.5];
    let l = [99.9, 98.9, 99.5, 99.5];
    let c = [100.0, 100.0, 100.0, 100.0];
    let mut s = settings_always_long();
    s.long.as_mut().unwrap().trailing_stop = Some(Trail { kind: "pct".into(), value: 0.02, ..Trail::default() });
    s.execution.entry_order = OrderSpec { kind: "limit".into(), offset: 0.01, ..OrderSpec::default() };
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let tr = &r.trades[0];
    assert_eq!(tr.entry_ts, "t1");
    // A trail from 110 (107.8) would have stopped it at t2's open.
    assert_ne!(tr.exit_reason, "trailing_stop", "{:?}", r.trades);
}

/// #15: a trade stopped inside a bar did not live through the bar's high after the stop.
#[test]
fn excursions_stop_at_the_exit() {
    let t = ts(4);
    let o = [100.0, 100.0, 100.0, 100.0];
    let h = [100.0, 100.0, 110.0, 100.0];
    let l = [100.0, 100.0, 90.0, 100.0];
    let c = [100.0; 4];
    let mut s = settings_always_long();
    s.long.as_mut().unwrap().stop_loss_pct = 0.05;
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let tr = &r.trades[0];
    assert_eq!(tr.exit_reason, "stop_loss");
    approx(tr.mfe, 0.0);
    approx(tr.mae, 5.0);
}

/// #15: a market exit at a bar's open takes nothing from the rest of that bar.
#[test]
fn excursions_ignore_the_bar_after_an_exit_at_its_open() {
    let t = ts(4);
    let o = [100.0, 100.0, 100.0, 100.0];
    let h = [100.0, 100.0, 120.0, 100.0];
    let l = [100.0, 100.0, 80.0, 100.0];
    let c = [100.0; 4];
    let mut s = settings_always_long();
    s.long.as_mut().unwrap().exit = Some(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let tr = &r.trades[0];
    assert_eq!(tr.exit_ts, "t2");
    approx(tr.mfe, 0.0);
    approx(tr.mae, 0.0);
}

/// #13: with `take_profit_fill: through`, a target touched exactly at the bar's high is not
/// filled; it fills once the price trades beyond it.
#[test]
fn take_profit_through_needs_a_trade_beyond_the_level() {
    let t = ts(5);
    let o = [100.0; 5];
    let h = [100.0, 100.5, 102.0, 102.5, 100.5];
    let l = [100.0, 99.5, 99.5, 99.5, 99.5];
    let c = [100.0; 5];
    let mut s = settings_always_long();
    s.long.as_mut().unwrap().take_profit_pct = 0.02;
    let touch = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    assert_eq!(touch.trades[0].exit_ts, "t2");
    s.execution.take_profit_fill = "through".into();
    let through = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    assert_eq!(through.trades[0].exit_ts, "t3");
    assert_eq!(through.trades[0].exit_reason, "take_profit");
    let mut bad = settings_always_long();
    bad.execution.take_profit_fill = "maybe".into();
    assert!(bad.validate().is_some());
}

/// #17: the size a forward run announces is taken on the equity marked at the last close, not
/// on cash that already paid the spread and fee of closing the open position on paper; the
/// order also carries its amount of money.
#[test]
fn forward_orders_are_sized_on_marked_equity() {
    let t = ts(3);
    let px = [100.0; 3];
    let mut s = settings_always_long();
    s.sizing = Sizing::PercentEquity { percent: 50.0 };
    s.spread_pct = 0.02;
    s.long.as_mut().unwrap().exit = Some(SignalGroup { logic: "all".into(), conditions: vec![cond(2.0, 1.0)] });
    let r = run_portfolio_forced(&s, &[&tests_bars(&t, &px, &px, &px, &px)], &[]);
    let first = &r.trades[0];
    let open = &r.open_positions[0];
    let marked = 10_000.0 + first.pnl + open.qty * (100.0 - open.avg_price) - open.fees;
    let o = r.pending_orders.iter().find(|o| o.kind == "entry").unwrap_or_else(|| panic!("{:?} {:?} {:?}", r.trades, r.open_positions, r.pending_orders));
    approx(o.qty.unwrap(), marked * 0.5 / 101.0);
    approx(o.amount.unwrap(), o.qty.unwrap() * 101.0);
}

fn grid(lower: f64, upper: f64, levels: usize) -> Settings {
    let mut s = settings_always_long();
    s.kind = "grid".into();
    s.grid = Some(GridConfig { lower, upper, levels, qty_per_level: 1.0, ..GridConfig::default() });
    s
}

fn daily(n: usize) -> Vec<String> {
    // Monday 1 January 2024 onwards.
    (0..n).map(|d| format!("2024-01-{:02}T00:00:00Z", d + 1)).collect()
}

/// #4: a grid run without a band ladders the range known before each bar: a prefix of the data
/// reproduces every trade the full run closed in it, whatever the later bars do.
#[test]
fn a_grid_without_a_band_never_reads_the_future_range() {
    let n = 40;
    let t = ts(n);
    let mut c: Vec<f64> = (0..n).map(|i| 100.0 + ((i as f64) * 0.9).sin() * 5.0).collect();
    c[n - 1] = 300.0; // a spike the first bars cannot know about
    let o: Vec<f64> = std::iter::once(100.0).chain(c[..n - 1].iter().copied()).collect();
    let h: Vec<f64> = (0..n).map(|i| o[i].max(c[i]) + 0.5).collect();
    let l: Vec<f64> = (0..n).map(|i| o[i].min(c[i]) - 0.5).collect();
    let mut s = grid(0.0, 0.0, 6);
    s.grid = None;
    let full = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let k = n - 2;
    let part = run(&s, &tests_bars(&t[..k], &o[..k], &h[..k], &l[..k], &c[..k]));
    let closed = |r: &RunResult| -> Vec<(String, String, String)> {
        r.trades.iter().filter(|x| x.exit_ts < t[k - 1]).map(|x| (x.entry_ts.clone(), x.exit_ts.clone(), format!("{:.6}", x.exit_price))).collect()
    };
    assert!(!closed(&full).is_empty());
    assert_eq!(closed(&full), closed(&part));
}

/// #10: a long grid only buys on lines below the price it rests under: a bar starting mid-ladder
/// does not buy every line above the market at the line's price.
#[test]
fn a_grid_does_not_buy_lines_above_the_market() {
    let t = ts(3);
    let (o, h, l, c) = ([100.0; 3], [101.0; 3], [99.0; 3], [100.0; 3]);
    let r = run(&grid(50.0, 150.0, 11), &tests_bars(&t, &o, &h, &l, &c));
    assert_eq!(r.grid.as_ref().unwrap().fills, 0, "{:?}", r.trades);
}

/// #8: a circuit stop fills at the stop (not the close), after the buys the price met on its way
/// down to it.
#[test]
fn a_grid_circuit_stop_fills_at_the_stop_after_the_path() {
    let t = ts(3);
    let o = [100.5, 100.5, 99.0];
    let h = [100.6, 100.6, 99.0];
    let l = [100.4, 80.0, 99.0];
    let c = [100.5, 99.0, 99.0];
    let mut s = grid(91.0, 101.0, 11);
    s.grid.as_mut().unwrap().stop_below = 95.5;
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let stops: Vec<&Trade> = r.trades.iter().filter(|x| x.exit_reason == "stop_loss").collect();
    // Lines 100, 99, 98, 97, 96 bought on the way from 100.5 to 95.5.
    assert_eq!(stops.len(), 5, "{:?}", r.trades);
    for x in stops {
        approx(x.exit_price, 95.5);
    }
}

/// #9: outside the window with `flat`, the inventory leaves at the open; a target the bar
/// reaches later is not paid.
#[test]
fn a_grid_session_end_liquidates_before_the_bar() {
    let t = daily(3);
    let o = [105.0, 105.0, 105.0];
    let h = [105.5, 112.0, 105.5];
    let l = [99.0, 104.0, 104.0];
    let c = [101.0, 105.0, 105.0];
    let mut s = grid(90.0, 110.0, 3);
    s.filters.weekdays = vec![1];
    s.filters.on_window_end = "flat".into();
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let x = &r.trades[0];
    assert_eq!(x.exit_reason, "session_end", "{:?}", r.trades);
    approx(x.exit_price, 105.0);
}

/// #3: the out-of-sample return is not a ranking key (the in-sample ranking itself is tested in
/// `optimize::tests::a_split_grid_ranks_on_its_in_sample_block`).
#[test]
fn the_out_of_sample_return_cannot_rank_a_grid() {
    assert!(!optimize::is_metric("oos_return_pct"));
}

/// #21: the deflated Sharpe de-annualizes with the clock's measured rate (a 24/7 hourly series is
/// ~8766 rows a year), the same one the engine annualized with, not a US-session year (1638).
#[test]
fn the_deflated_sharpe_reads_the_measured_clock() {
    let t: Vec<String> = (0..49).map(|h| format!("2024-01-{:02}T{:02}:00:00Z", 1 + h / 24, h % 24)).collect();
    let px = vec![100.0; 49];
    let (rows, rate) = clock_rate(&[&tests_bars(&t, &px, &px, &px, &px)]);
    assert_eq!(rows, 49);
    assert!((rate.unwrap() - 365.25 * 24.0).abs() < 1e-6, "{rate:?}");
    assert!((crate::quant::periods_per_year("1h") - 365.25 * 24.0).abs() > 1000.0);
}

/// #8: both circuit stops inside one bar: a bar that opened past one was stopped there first.
#[test]
fn a_grid_bar_that_opens_past_a_stop_takes_that_stop() {
    let t = ts(3);
    let o = [100.5, 100.5, 120.0];
    let h = [100.6, 100.6, 121.0];
    let l = [100.4, 100.0, 80.0];
    let c = [100.5, 100.2, 100.0];
    let mut s = grid(91.0, 101.0, 11);
    let g = s.grid.as_mut().unwrap();
    g.stop_below = 95.5;
    g.stop_above = 110.0;
    let r = run(&s, &tests_bars(&t, &o, &h, &l, &c));
    let stops: Vec<&Trade> = r.trades.iter().filter(|x| x.exit_reason == "stop_loss").collect();
    assert!(!stops.is_empty(), "{:?}", r.trades);
    for x in stops {
        approx(x.exit_price, 120.0);
    }
}
