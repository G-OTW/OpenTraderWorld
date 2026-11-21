use super::*;
use time::macros::datetime;

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn fill(id: &str, min: i64, buy: bool, qty: &str, price: &str, fee: &str) -> Fill {
    Fill {
        id: id.into(),
        at: datetime!(2026-11-20 10:00 UTC) + Duration::minutes(min),
        symbol: "ES".into(),
        buy,
        qty: d(qty),
        price: d(price),
        fee: d(fee),
        multiplier: Decimal::ONE,
    }
}

fn trade(key: &str, pnl: &str) -> Trade {
    Trade { key: key.into(), closed_at: datetime!(2026-11-20 10:00 UTC), pnl: d(pnl), notional: d("1000") }
}

#[test]
fn a_round_trip_nets_its_fees() {
    let t = trades(&[fill("a", 0, true, "2", "100", "1"), fill("b", 1, false, "2", "105", "1")], &HashMap::new());
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].pnl, d("8"));
    assert_eq!(t[0].notional, d("200"));
    assert_eq!(t[0].key, "ES:a");
}

#[test]
fn partial_exits_close_one_trade_when_flat() {
    let f = [fill("a", 0, true, "2", "100", "0"), fill("b", 1, false, "1", "90", "0"), fill("c", 2, false, "1", "95", "0")];
    let t = trades(&f, &HashMap::new());
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].pnl, d("-15"));
}

#[test]
fn a_reversal_closes_then_opens() {
    let f = [fill("a", 0, true, "1", "100", "0"), fill("b", 1, false, "3", "90", "0"), fill("c", 2, true, "2", "80", "0")];
    let t = trades(&f, &HashMap::new());
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].pnl, d("-10"));
    assert_eq!(t[1].pnl, d("20"));
    assert_eq!(t[1].key, "ES:b");
}

#[test]
fn a_position_held_before_the_session_is_not_counted() {
    let seed = HashMap::from([("ES".to_string(), d("1"))]);
    let f = [fill("a", 0, false, "1", "90", "0"), fill("b", 1, true, "1", "100", "0"), fill("c", 2, false, "1", "99", "0")];
    let t = trades(&f, &seed);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].key, "ES:b");
}

#[test]
fn consecutive_losses_fire_on_each_run() {
    let f = Facts { trades: vec![trade("1", "-1"), trade("2", "-1"), trade("3", "2"), trade("4", "-1"), trade("5", "-1"), trade("6", "-1"), trade("7", "-1")], ..Default::default() };
    let fires = evaluate(&Rule::LosingTrades { n: 2, consecutive: true }, &f).unwrap();
    let keys: Vec<_> = fires.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["c:2", "c:5", "c:7"]);
}

#[test]
fn losses_over_the_session_count_every_one() {
    let f = Facts { trades: vec![trade("1", "-1"), trade("2", "3"), trade("3", "-1"), trade("4", "-1")], ..Default::default() };
    let fires = evaluate(&Rule::LosingTrades { n: 3, consecutive: false }, &f).unwrap();
    assert_eq!(fires, vec![Fire { key: "l:3".into(), detail: "3".into() }]);
}

#[test]
fn trades_in_a_sliding_window() {
    let t0 = datetime!(2026-11-20 10:00 UTC);
    let entries = [0, 10, 20, 70, 75, 80, 85].iter().map(|m| t0 + Duration::minutes(*m)).collect();
    let f = Facts { entries, session_start: Some(t0), ..Default::default() };
    let fires = evaluate(&Rule::Trades { n: 3, minutes: 60, rolling: true }, &f).unwrap();
    // The third entry within the hour fires; the next firing waits for a window past it.
    assert_eq!(fires.len(), 2);
    assert_eq!(fires[0].key, format!("r:{}", ms(t0 + Duration::minutes(20))));
    assert_eq!(fires[1].key, format!("r:{}", ms(t0 + Duration::minutes(80))));
}

#[test]
fn trades_in_fixed_windows_from_the_session_start() {
    let t0 = datetime!(2026-11-20 00:00 UTC);
    let entries = [5, 10, 59, 61, 62, 119].iter().map(|m| t0 + Duration::minutes(*m)).collect();
    let f = Facts { entries, session_start: Some(t0), ..Default::default() };
    let fires = evaluate(&Rule::Trades { n: 3, minutes: 60, rolling: false }, &f).unwrap();
    let keys: Vec<_> = fires.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["w:0", "w:1"]);
}

#[test]
fn drawdown_in_percent_and_amount() {
    let f = Facts { equity_start: Some(d("10000")), equity_now: Some(d("9700")), currency: "USD".into(), ..Default::default() };
    assert_eq!(evaluate(&Rule::Drawdown { value: d("3"), pct: true }, &f).unwrap().len(), 1);
    assert!(evaluate(&Rule::Drawdown { value: d("4"), pct: true }, &f).unwrap().is_empty());
    assert_eq!(evaluate(&Rule::Drawdown { value: d("250"), pct: false }, &f).unwrap()[0].detail, "-300 USD");
    let blind = Facts::default();
    assert!(evaluate(&Rule::Drawdown { value: d("1"), pct: false }, &blind).is_err());
}

#[test]
fn one_trade_loss_against_notional_or_equity() {
    let f = Facts { trades: vec![trade("1", "-30"), trade("2", "-5")], equity_start: Some(d("10000")), ..Default::default() };
    let on_notional = evaluate(&Rule::TradeLoss { value: d("2"), pct: true, of: Of::Notional }, &f).unwrap();
    assert_eq!(on_notional.len(), 1);
    assert_eq!(on_notional[0].key, "t:1");
    assert!(evaluate(&Rule::TradeLoss { value: d("1"), pct: true, of: Of::Equity }, &f).unwrap().is_empty());
    assert_eq!(evaluate(&Rule::TradeLoss { value: d("4"), pct: false, of: Of::Notional }, &f).unwrap().len(), 2);
}

#[test]
fn fees_reached() {
    let f = Facts { fees: d("12.5"), currency: "USD".into(), ..Default::default() };
    assert_eq!(evaluate(&Rule::Fees { value: d("12") }, &f).unwrap()[0].detail, "12.5 USD");
    assert!(evaluate(&Rule::Fees { value: d("13") }, &f).unwrap().is_empty());
}

#[test]
fn rules_read_from_json_and_validated() {
    let r = Rule::parse(&json!({ "kind": "trade_loss", "value": 1.5, "pct": true, "of": "equity" })).unwrap();
    assert_eq!(r, Rule::TradeLoss { value: d("1.5"), pct: true, of: Of::Equity });
    assert!(Rule::parse(&json!({ "kind": "fees", "value": 1, "extra": 1 })).is_err());
    assert!(Rule::Drawdown { value: d("120"), pct: true }.validate().is_err());
    assert!(Rule::Trades { n: 10, minutes: 0, rolling: true }.validate().is_err());
}

#[test]
fn a_session_starts_at_the_day_or_the_last_reset() {
    let now = datetime!(2026-11-20 15:00 UTC);
    let day = store::Settings { reset: "day".into(), timezone: "UTC".into(), updated_at: None };
    assert_eq!(session_start(&day, None, now), datetime!(2026-11-20 00:00 UTC));
    assert_eq!(session_start(&day, Some(datetime!(2026-11-20 12:00 UTC)), now), datetime!(2026-11-20 12:00 UTC));
    assert_eq!(session_start(&day, Some(datetime!(2026-11-19 12:00 UTC)), now), datetime!(2026-11-20 00:00 UTC));
    let manual = store::Settings { reset: "manual".into(), timezone: "UTC".into(), updated_at: Some(datetime!(2026-11-01 00:00 UTC)) };
    assert_eq!(session_start(&manual, None, now), datetime!(2026-11-01 00:00 UTC));
}
