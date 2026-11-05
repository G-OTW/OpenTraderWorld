//! Researchers on fixture sets, the three source mappings, and the parity with the lab.

use super::researchers::*;
use super::sources::{self, EngineMeta, EngineTrade};
use super::*;
use crate::journal_whatif::{self as whatif, Book, FamilyMove, FamilySide, LabTrade, Scenario};
use otw_store::journal::FeeSchedule;
use serde_json::json;
use time::macros::datetime;
use time::{Duration, OffsetDateTime};

const MONDAY: OffsetDateTime = datetime!(2026-01-05 00:00 UTC);

/// One closed trade entered `day` days after a Monday at `hour`, held `hold` minutes.
fn trade(i: usize, net: f64, day: i64, hour: i64, hold: i64) -> SetTrade {
    let entry = MONDAY + Duration::days(day) + Duration::hours(hour);
    let exit = entry + Duration::minutes(hold);
    SetTrade {
        lab: LabTrade {
            id: Uuid::from_u128(i as u128 + 1),
            ticker: "AAPL".into(),
            asset_class: "stock".into(),
            side: "long".into(),
            exit_at: exit,
            entry_local: Some(entry),
            exit_local: exit,
            qty: 1.0,
            entry_price: Some(100.0),
            net,
            fees: 1.0,
            risk: None,
            fx: 1.0,
            regime: None,
            mae_pct: None,
            mfe_pct: None,
        },
        notional: Some(100.0),
        stop_pct: None,
    }
}

fn set_of(kind: SourceKind, mut trades: Vec<SetTrade>) -> TradeSet {
    trades.sort_by_key(|t| t.lab.exit_at);
    TradeSet { kind, currency: Some("USD".into()), trades, unmeasured: Vec::new() }
}

/// `per` trades at each (hour, net) pair, one per day so no two share a session.
fn by_hours(groups: &[(i64, f64, usize)]) -> Vec<SetTrade> {
    let mut out = Vec::new();
    for (hour, net, per) in groups {
        for _ in 0..*per {
            let i = out.len();
            out.push(trade(i, *net, i as i64, *hour, 30));
        }
    }
    out
}

fn keys(v: &[Insight]) -> Vec<&str> {
    v.iter().map(|i| i.key.as_str()).collect()
}

fn ctx() -> Context {
    Context::default()
}

// ── Hours and weekdays ───────────────────────────────────────────────────────

#[test]
fn the_worst_hour_is_stated_above_the_floor_and_the_threshold() {
    let set = set_of(SourceKind::Journal, by_hours(&[(9, 20.0, 10), (10, 10.0, 10), (15, -15.0, 10)]));
    let found = hours(&set, &ctx());
    let worst = found.iter().find(|i| i.key == "hour_worst").expect("worst hour stated");
    assert_eq!(worst.values["hour"], 15.0);
    assert_eq!(worst.values["trades"], 10.0);
    assert_eq!(worst.evidence.n, 30);
    assert!((worst.evidence.delta_pnl.unwrap() - 150.0).abs() < 1e-9);
    match &worst.apply {
        Some(Scenario::Filter(f)) => {
            let w = f.window.as_ref().unwrap();
            assert_eq!((w.hours.clone(), w.exclude), (vec![15], true));
        }
        other => panic!("expected a filter scenario, got {other:?}"),
    }
    let best = found.iter().find(|i| i.key == "hour_best").expect("best hour stated");
    assert_eq!(best.values["hour"], 9.0);
    assert!((best.evidence.delta_pnl.unwrap() - 100.0).abs() < 1e-9, "200 at 1.5x adds 100");
}

#[test]
fn hours_stay_silent_below_the_floor() {
    let set = set_of(SourceKind::Journal, by_hours(&[(9, 20.0, 5), (10, 10.0, 5), (15, -15.0, 5)]));
    assert!(hours(&set, &ctx()).is_empty());
}

#[test]
fn hours_stay_silent_below_the_effect_threshold() {
    // Dropping hour 15 would add 5 on a turnover of 305: under 5%.
    let set = set_of(SourceKind::Journal, by_hours(&[(9, 20.0, 10), (10, 10.0, 10), (15, -0.5, 10)]));
    let found = hours(&set, &ctx());
    assert!(!keys(&found).contains(&"hour_worst"), "{:?}", keys(&found));
}

#[test]
fn the_worst_weekday_is_dropped_by_weekday() {
    // Monday to Friday, five weeks: Fridays lose.
    let mut trades = Vec::new();
    for week in 0..5 {
        for d in 0..5 {
            let i = trades.len();
            let net = if d == 4 { -30.0 } else { 15.0 };
            trades.push(trade(i, net, week * 7 + d, 10, 60));
        }
    }
    let set = set_of(SourceKind::Journal, trades);
    let found = weekday(&set, &ctx());
    let worst = found.iter().find(|i| i.key == "weekday_worst").expect("friday stated");
    assert_eq!(worst.values["weekday"], 5.0);
    assert!((worst.evidence.delta_pnl.unwrap() - 150.0).abs() < 1e-9);
}

/// The parity the page promises: an insight's money is the lab's `delta_net` for the
/// same scenario, run through the lab's own `run` (a window filter reads no candle, so
/// the lazy pool is never connected).
#[tokio::test]
async fn an_insight_delta_is_the_lab_delta_for_the_same_scenario() {
    let mut trades = by_hours(&[(9, 20.0, 10), (10, 10.0, 10), (15, -15.0, 10)]);
    trades[3].lab.entry_local = None; // an undated trade: unmeasured on both sides
    let set = set_of(SourceKind::Journal, trades);
    let found = research(&set, &ctx(), None);
    let worst = found.iter().find(|i| i.key == "hour_worst").expect("worst hour stated");
    let pool = sqlx::PgPool::connect_lazy("postgres://otw@localhost/never").expect("lazy pool");
    let book = Book { trades: set.lab(), raw: Default::default(), unmeasured: Vec::new() };
    let lab = whatif::run(&pool, &book, worst.apply.as_ref().unwrap(), "USD").await.expect("lab run");
    assert_eq!(worst.evidence.delta_pnl, Some(lab.delta_net));
    assert_eq!(worst.evidence.delta_dd, Some(lab.delta_drawdown));
    assert_eq!(worst.evidence.n, lab.before.n);
    assert_eq!(lab.unmeasured.len(), 1);
}

// ── Holding time ─────────────────────────────────────────────────────────────

#[test]
fn losers_held_longer_than_winners_is_stated() {
    let mut trades = Vec::new();
    for i in 0..12 {
        trades.push(trade(i, 10.0, i as i64, 10, 30));
        trades.push(trade(100 + i, -8.0, i as i64, 14, 120));
    }
    let set = set_of(SourceKind::Journal, trades);
    let found = hold_time(&set, &ctx());
    assert_eq!(keys(&found), vec!["hold_losers_longer"]);
    assert_eq!(found[0].values["ratio"], 4.0);
    assert!(found[0].apply.is_none(), "a time stop is counted in candles: not guessed");
}

#[test]
fn hold_time_is_silent_when_holds_match_or_the_sample_is_thin() {
    let even: Vec<SetTrade> = (0..24).map(|i| trade(i, if i % 2 == 0 { 10.0 } else { -8.0 }, i as i64, 10, 60)).collect();
    assert!(hold_time(&set_of(SourceKind::Journal, even), &ctx()).is_empty());
    let thin: Vec<SetTrade> =
        (0..10).map(|i| trade(i, if i % 2 == 0 { 10.0 } else { -8.0 }, i as i64, 10, 30 + 200 * (i as i64 % 2))).collect();
    assert!(hold_time(&set_of(SourceKind::Journal, thin), &ctx()).is_empty());
}

// ── Stop geometry ────────────────────────────────────────────────────────────

fn stopped(n: usize, stop: f64, winner_mae: f64) -> Vec<SetTrade> {
    (0..n)
        .map(|i| {
            let mut t = trade(i, if i % 3 == 0 { -10.0 } else { 12.0 }, i as i64, 10, 60);
            t.stop_pct = Some(stop);
            t.lab.mae_pct = Some(if i % 3 == 0 { stop } else { winner_mae });
            t
        })
        .collect()
}

#[test]
fn tight_and_wide_stops_are_read_against_the_winners_adverse_move() {
    let tight = stop_geometry(&set_of(SourceKind::Journal, stopped(24, 1.0, 0.9)), &ctx());
    assert_eq!(keys(&tight), vec!["stop_tight"]);
    assert_eq!(tight[0].values["pct"], 1.1);
    assert!(tight[0].evidence.delta_risk.unwrap() > 0.0);
    assert!(matches!(tight[0].apply, Some(Scenario::Exits(_))));

    let wide = stop_geometry(&set_of(SourceKind::Journal, stopped(24, 2.0, 0.2)), &ctx());
    assert_eq!(keys(&wide), vec!["stop_wide"]);
    assert!(wide[0].evidence.delta_risk.unwrap() < 0.0);

    assert!(stop_geometry(&set_of(SourceKind::Journal, stopped(24, 2.0, 1.2)), &ctx()).is_empty());
    assert!(stop_geometry(&set_of(SourceKind::Journal, stopped(12, 1.0, 0.9)), &ctx()).is_empty());
}

// ── Concentration ────────────────────────────────────────────────────────────

fn tickers(groups: &[(&str, f64, usize)]) -> Vec<SetTrade> {
    let mut out = Vec::new();
    for (ticker, net, per) in groups {
        for _ in 0..*per {
            let i = out.len();
            let mut t = trade(i, *net, i as i64, 10, 60);
            t.lab.ticker = (*ticker).into();
            out.push(t);
        }
    }
    out
}

#[test]
fn a_losing_ticker_is_named_with_what_the_book_makes_without_it() {
    let set = set_of(SourceKind::Journal, tickers(&[("AAPL", 10.0, 15), ("TSLA", -8.0, 10)]));
    let found = concentration(&set, &ctx());
    let drag = found.iter().find(|i| i.key == "ticker_drag").expect("drag stated");
    assert_eq!(drag.labels, vec!["TSLA".to_string()]);
    assert!((drag.evidence.delta_pnl.unwrap() - 80.0).abs() < 1e-9);
    assert!(drag.apply.is_none());
    // One instrument only: nothing to compare it with.
    let one = set_of(SourceKind::Journal, tickers(&[("AAPL", 10.0, 25)]));
    assert!(concentration(&one, &ctx()).is_empty());
}

#[test]
fn a_sector_is_stated_only_from_a_stored_sic_code() {
    let set = set_of(SourceKind::Journal, tickers(&[("AAPL", 5.0, 20), ("MSFT", 5.0, 5)]));
    let none = concentration(&set, &ctx());
    assert!(!keys(&none).contains(&"sector_share"), "no SIC code, no sector");
    let mut c = ctx();
    c.sectors.insert("AAPL".into(), ("3571".into(), "Electronic Computers".into()));
    c.sectors.insert("MSFT".into(), ("7372".into(), "Prepackaged Software".into()));
    let found = concentration(&set, &c);
    let s = found.iter().find(|i| i.key == "sector_share").expect("sector stated");
    assert_eq!(s.labels, vec!["Electronic Computers".to_string(), "3571".to_string()]);
    assert_eq!(s.values["share"], 80.0);
}

// ── Fees ─────────────────────────────────────────────────────────────────────

fn micro_book() -> Vec<SetTrade> {
    (0..24)
        .map(|i| {
            let mut t = trade(i, if i % 2 == 0 { 50.0 } else { -40.0 }, i as i64, 10, 60);
            t.lab.ticker = "MNQZ6".into();
            t.lab.asset_class = "future".into();
            t.lab.qty = 10.0;
            t.lab.entry_price = Some(20_000.0);
            t.lab.fees = 10.0;
            t
        })
        .collect()
}

fn per_contract(amount: f64) -> FeeSchedule {
    FeeSchedule {
        id: Uuid::from_u128(77),
        name: "Full size".into(),
        amount,
        amount_kind: "fixed".into(),
        per: "contract".into(),
        currency: "USD".into(),
        position: 0.0,
    }
}

#[test]
fn without_a_fee_schedule_the_insight_says_so_and_guesses_nothing() {
    let set = set_of(SourceKind::Journal, micro_book());
    let mut c = ctx();
    assert!(instrument_fees(&set, &c).is_empty(), "no journal, nowhere to add one");
    c.journal_installed = true;
    let found = instrument_fees(&set, &c);
    assert_eq!(keys(&found), vec!["fees_no_schedule"]);
    assert_eq!(found[0].values["fees"], 240.0);
    assert!(found[0].evidence.delta_pnl.is_none());
}

#[test]
fn the_cheapest_schedule_is_the_lab_fees_scenario_to_the_cent() {
    let set = set_of(SourceKind::Journal, micro_book());
    let schedule = per_contract(0.5);
    let mut c = ctx();
    c.journal_installed = true;
    c.schedule_rates.insert(schedule.id, vec![Some(1.0); set.trades.len()]);
    c.schedules = vec![schedule.clone()];
    let found = instrument_fees(&set, &c);
    let i = &found[0];
    assert_eq!(i.key, "fees_family_saving");
    assert_eq!(i.labels, vec!["Full size".to_string(), "MNQ".to_string(), "NQ".to_string()]);
    // The lab's own fees scenario on the same trades.
    let family = whatif::family("mnq_nq").unwrap();
    let mv = FamilyMove { family, to: FamilySide::Large };
    let lab = set.lab();
    let (steps, detail) = whatif::fees(&lab, &schedule, Some(mv), &vec![1.0; lab.len()], 0);
    let o = whatif::assemble(&lab, steps, detail, i.apply.clone().unwrap(), Vec::new(), "USD");
    assert_eq!(i.evidence.delta_pnl, Some(o.delta_net));
    assert!((o.delta_net - 24.0 * 9.5).abs() < 1e-9, "10 micros = 1 full size at 0.5");
    match &i.apply {
        Some(Scenario::Fees(p)) => assert_eq!((p.family.as_deref(), p.to), (Some("mnq_nq"), FamilySide::Large)),
        other => panic!("expected a fees scenario, got {other:?}"),
    }
}

#[test]
fn a_dearer_schedule_or_an_unpriced_one_states_nothing() {
    let set = set_of(SourceKind::Journal, micro_book());
    let mut c = ctx();
    c.journal_installed = true;
    // 20 a contract: dearer on the micros (200 against 10) and on the full size (20 against 10).
    let dear = per_contract(20.0);
    c.schedule_rates.insert(dear.id, vec![Some(1.0); set.trades.len()]);
    c.schedules = vec![dear.clone()];
    assert!(instrument_fees(&set, &c).is_empty());
    let cheap = per_contract(0.5);
    c.schedule_rates.insert(cheap.id, vec![None; set.trades.len()]);
    c.schedules = vec![cheap];
    assert!(instrument_fees(&set, &c).is_empty(), "a schedule that cannot be priced is not guessed");
}

// ── Sizing after losses ──────────────────────────────────────────────────────

fn martingale() -> Vec<SetTrade> {
    // Win, loss, loss, then a trade at twice the size: eight times.
    let mut out = Vec::new();
    for b in 0..8 {
        for (k, (net, size)) in [(10.0, 100.0), (-5.0, 100.0), (-5.0, 100.0), (8.0, 200.0)].iter().enumerate() {
            let i = out.len();
            let mut t = trade(i, *net, (b * 4 + k) as i64, 10, 60);
            t.notional = Some(*size);
            out.push(t);
        }
    }
    out
}

#[test]
fn sizing_up_after_losses_is_read_on_a_run_with_the_behaviour_measure() {
    let set = set_of(SourceKind::Backtest, martingale());
    let found = sizing_streaks(&set, &ctx());
    assert_eq!(keys(&found), vec!["size_after_losses"]);
    assert_eq!(found[0].values["escalation"], 100.0);
    assert_eq!(found[0].evidence.n, 8);
    assert!((found[0].evidence.delta_pnl.unwrap() + 32.0).abs() < 1e-9, "the doubled winners at base size");
    // The journal's behaviour tab states it already: not twice.
    assert!(sizing_streaks(&set_of(SourceKind::Journal, martingale()), &ctx()).is_empty());
}

// ── Symmetry ─────────────────────────────────────────────────────────────────

#[test]
fn a_losing_side_is_named_against_the_earning_one() {
    let mut trades = Vec::new();
    for i in 0..12 {
        trades.push(trade(i, 10.0, i as i64, 10, 60));
        let mut s = trade(100 + i, -6.0, i as i64, 12, 60);
        s.lab.side = "short".into();
        trades.push(s);
    }
    let found = symmetry(&set_of(SourceKind::Journal, trades.clone()), &ctx());
    assert_eq!(keys(&found), vec!["side_drag"]);
    assert_eq!(found[0].labels, vec!["short".to_string(), "long".to_string()]);
    assert!((found[0].evidence.delta_pnl.unwrap() - 72.0).abs() < 1e-9);
    for t in trades.iter_mut().filter(|t| t.lab.side == "short") {
        t.lab.net = 4.0;
    }
    assert!(symmetry(&set_of(SourceKind::Journal, trades), &ctx()).is_empty(), "both sides earn");
}

// ── research() ───────────────────────────────────────────────────────────────

#[test]
fn a_run_only_carries_filters_and_the_strongest_finding_comes_first() {
    let mut trades = by_hours(&[(9, 20.0, 10), (10, 10.0, 10), (15, -15.0, 10)]);
    for (i, t) in trades.iter_mut().enumerate() {
        t.stop_pct = Some(2.0);
        t.lab.mae_pct = Some(if t.lab.net > 0.0 { 0.2 } else { 2.0 });
        let _ = i;
    }
    let journal = research(&set_of(SourceKind::Journal, trades.clone()), &ctx(), None);
    let run = research(&set_of(SourceKind::Backtest, trades), &ctx(), None);
    let stop_j = journal.iter().find(|i| i.key == "stop_wide").expect("journal stop");
    assert!(stop_j.apply.is_some());
    let stop_r = run.iter().find(|i| i.key == "stop_wide").expect("run stop");
    assert!(stop_r.apply.is_none(), "a run's stop is not a filter");
    assert!(run.iter().find(|i| i.key == "hour_worst").unwrap().apply.is_some());
    // Money first, largest share first.
    let money: Vec<f64> = run.iter().filter_map(|i| i.evidence.delta_pnl.map(f64::abs)).collect();
    assert!(money.windows(2).all(|w| w[0] >= w[1]), "{money:?}");
    assert!(run.iter().position(|i| i.evidence.delta_pnl.is_none()) > run.iter().rposition(|i| i.evidence.delta_pnl.is_some()));
    let only = research(&set_of(SourceKind::Journal, by_hours(&[(9, 20.0, 10), (15, -15.0, 10)])), &ctx(), Some(&["weekday".to_string()]));
    assert!(only.iter().all(|i| i.researcher == "weekday"));
}

// ── Sources ──────────────────────────────────────────────────────────────────

fn settings() -> crate::backtest::Settings {
    serde_json::from_value(json!({
        "mode": "both",
        "sizing": { "mode": "fixed_qty", "qty": 1 },
        "long": { "stop_loss_pct": 0.02 },
        "short": {},
        "instrument": { "multiplier": 2 },
        "filters": { "tz_offset_min": 120 }
    }))
    .expect("settings")
}

fn engine_trade(entry: &str, exit: &str, dir: &str, pnl: f64) -> crate::backtest::Trade {
    crate::backtest::Trade {
        ticker: String::new(),
        entry_ts: entry.into(),
        exit_ts: exit.into(),
        entry_price: 100.0,
        exit_price: 101.0,
        qty: 5.0,
        entries: 1,
        direction: dir.into(),
        exit_reason: "signal".into(),
        pnl,
        fees: 1.5,
        return_pct: 0.0,
        bars_held: 3,
        mae: 20.0,
        mfe: 40.0,
    }
}

#[test]
fn a_backtest_run_maps_on_its_own_strategy_clock() {
    let trades = vec![
        engine_trade("2026-01-05T15:00:00Z", "2026-01-05T18:00:00Z", "short", -4.0),
        engine_trade("2026-01-05T08:00:00Z", "2026-01-05T09:00:00Z", "long", 9.0),
        engine_trade("2026-01-06T08:00:00Z", "not a date", "long", 1.0),
    ];
    // The run's trades go through JSON exactly as the replay hands them over.
    let rows: Vec<EngineTrade> = serde_json::from_value(serde_json::to_value(&trades).unwrap()).unwrap();
    let meta = EngineMeta {
        settings: Some(settings()),
        classes: [("ES".to_string(), sources::class_of("future"))].into(),
        default_ticker: "ES".into(),
    };
    let set = sources::engine_set(SourceKind::Backtest, &rows, &meta);
    assert_eq!(set.currency, None, "a simulation names no currency");
    assert_eq!(set.trades.len(), 2);
    assert_eq!(set.unmeasured.len(), 1);
    assert_eq!(set.unmeasured[0].reason, "undated");
    let first = &set.trades[0];
    assert_eq!(first.lab.net, 9.0, "exit order");
    assert_eq!(first.lab.ticker, "ES");
    assert_eq!(first.lab.asset_class, "future");
    assert_eq!(first.lab.entry_local.unwrap().hour(), 10, "UTC+2 strategy clock");
    assert_eq!(first.notional, Some(1000.0), "100 x 5 x multiplier 2");
    assert_eq!(first.stop_pct, Some(2.0));
    assert_eq!(first.lab.risk, Some(20.0));
    assert_eq!(first.lab.mae_pct, Some(2.0));
    let short = &set.trades[1];
    assert_eq!(short.lab.side, "short");
    assert_eq!(short.stop_pct, None, "the short side has no stop");
    // Without settings nothing is derived from them.
    let bare = sources::engine_set(SourceKind::Backtest, &rows, &EngineMeta::default());
    assert_eq!((bare.trades[0].notional, bare.trades[0].stop_pct, bare.trades[0].lab.mae_pct), (None, None, None));
    assert_eq!(bare.trades[0].lab.entry_local.unwrap().hour(), 8);
}

#[test]
fn a_paper_trade_reads_as_the_engine_stored_it() {
    let stored = serde_json::to_value(engine_trade("2026-01-05T08:00:00Z", "2026-01-05T09:00:00Z", "long", 3.0)).unwrap();
    let mut row: EngineTrade = serde_json::from_value(stored).unwrap();
    row.ticker = "BTCUSDT".into();
    let set = sources::engine_set(SourceKind::Paper, &[row], &EngineMeta::default());
    assert_eq!(set.kind, SourceKind::Paper);
    assert_eq!(set.trades[0].lab.ticker, "BTCUSDT");
    assert_eq!(set.trades[0].lab.fees, 1.5);
    assert!(serde_json::from_value::<EngineTrade>(json!({ "ticker": "X" })).is_err(), "unreadable, not guessed");
}

#[test]
fn a_journal_book_keeps_the_lab_trades_and_its_unmeasured() {
    let lab: Vec<LabTrade> = by_hours(&[(9, 5.0, 3)]).into_iter().map(|t| t.lab).collect();
    let gone = vec![whatif::Unmeasured { id: Uuid::nil(), ticker: "X".into(), reason: "fx" }];
    let book = Book { trades: lab.clone(), raw: Default::default(), unmeasured: gone };
    let set = sources::from_book(book, "EUR");
    assert_eq!(set.kind, SourceKind::Journal);
    assert_eq!(set.currency.as_deref(), Some("EUR"));
    assert_eq!(set.trades.len(), 3);
    assert_eq!(set.trades[1].lab.id, lab[1].id);
    assert_eq!(set.unmeasured.len(), 1);
    assert_eq!(sources::class_of("equity"), "stock");
    assert_eq!(sources::class_of("fx"), "forex");
}
