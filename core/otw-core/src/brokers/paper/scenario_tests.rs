//! Trading-desk scenarios for the parts of the paper broker the HTTP API cannot reach offline:
//! a fill needs a live price feed, so matching, the position fold and the contract maths are
//! driven here directly with prices written by hand and expectations worked out on paper.
//!
//! Conventions asserted are the market's, not the code's: a buy limit never pays more than its
//! limit, a sell never receives less; a stop is a market order once touched; a position's
//! average cost is volume weighted and a closing fill realizes against it; a future's point
//! value and tick come from the contract specification (ES: 50 USD a point, d("0.25") tick).

use std::time::Instant;

use otw_store::paper_broker::{Fill, Held};
use time::OffsetDateTime;
use uuid::Uuid;

use super::matching::{decide, slip, Commission, Decision, Resting, Tick};
use super::*;

fn d(v: &str) -> Decimal {
    v.parse().unwrap()
}

fn at() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH
}

fn bars(lo: Decimal, hi: Decimal, last: Decimal) -> Tick {
    Tick { lo, hi, last, book: None }
}

fn book(bid: Decimal, ask: Decimal) -> Tick {
    let mid = (bid + ask) / Decimal::TWO;
    Tick { lo: mid, hi: mid, last: mid, book: Some((bid, ask)) }
}

fn order(buy: bool, kind: &str, limit: Option<Decimal>, stop: Option<Decimal>) -> Resting<'_> {
    Resting { buy, kind, limit, stop, triggered: false, trail: None, best: None }
}

fn fill(ticker: &str, buy: bool, qty: Decimal, price: Decimal, mult: Decimal) -> Fill {
    Fill {
        id: Uuid::nil(),
        account_id: Uuid::nil(),
        order_id: Uuid::nil(),
        at: at(),
        connector_id: Uuid::nil(),
        provider: "massive".into(),
        asset_type: "future".into(),
        ticker: ticker.into(),
        side: if buy { "buy" } else { "sell" }.into(),
        qty,
        price,
        fee: d("0"),
        currency: "USD".into(),
        multiplier: mult,
        realized: d("0"),
        fx: d("1"),
    }
}

// ── positions: average cost, realized P&L, flips ─────────────────────────────────────────

#[test]
fn scaling_in_prices_the_position_at_the_volume_weighted_average() {
    let mut h = Held::default();
    assert_eq!(h.apply(true, d("10"), d("100"), d("0"), d("1"), at()), d("0"));
    assert_eq!(h.apply(true, d("30"), d("104"), d("0"), d("1"), at()), d("0"));
    // (10 x 100 + 30 x 104) / 40 = 103.
    assert_eq!((h.avg, h.qty), (d("103"), d("40")), "{h:?}");
}

#[test]
fn a_partial_close_realizes_against_the_average_with_the_point_value() {
    // ES: 2 at 5000 then 2 at 5010 is 4 at 5005.
    let mut h = Held::default();
    h.apply(true, d("2"), d("5000"), d("2.5"), d("50"), at());
    h.apply(true, d("2"), d("5010"), d("2.5"), d("50"), at());
    assert_eq!(h.avg, d("5005"), "{h:?}");
    // Sell 1 at 5020: (5020 - 5005) x 1 x 50 = 750.
    let realized = h.apply(false, d("1"), d("5020"), d("2.5"), d("50"), at());
    assert_eq!(realized, d("750"));
    // The average cost of what is left does not move on a close.
    assert_eq!((h.qty, h.avg), (d("3"), d("5005")), "{h:?}");
    // d("5") of entry commission on 4 contracts: 3 contracts keep three quarters of it.
    assert_eq!(h.entry_fee, d("3.75"));
}

#[test]
fn a_losing_close_realizes_a_negative_result() {
    let mut h = Held::default();
    h.apply(true, d("1"), d("5000"), d("0"), d("50"), at());
    // (4990 - 5000) x 50 = -500.
    assert_eq!(h.apply(false, d("1"), d("4990"), d("0"), d("50"), at()), d("-500"));
    assert!(h.flat());
}

#[test]
fn one_order_through_zero_closes_the_long_and_opens_the_short_at_its_own_price() {
    let mut h = Held::default();
    h.apply(true, d("25"), d("103"), d("0"), d("50"), at());
    // Sell 40 at 101 against a long of 25 at 103: 25 close at (101 - 103) x 25 x 50 = -2500,
    // the other 15 open a short at 101, not at the old average.
    let realized = h.apply(false, d("40"), d("101"), d("4"), d("50"), at());
    assert_eq!(realized, d("-2500"));
    assert_eq!((h.qty, h.avg), (d("-15"), d("101")), "{h:?}");
    // The commission splits by quantity: 15 of 40 belongs to the new short.
    assert_eq!(h.entry_fee, d("1.5"));
}

#[test]
fn a_short_realizes_when_the_price_falls_and_flat_forgets_the_cost() {
    let mut h = Held::default();
    h.apply(false, d("10"), d("100"), d("0"), d("1"), at());
    h.apply(false, d("10"), d("110"), d("0"), d("1"), at());
    assert_eq!((h.avg, h.qty), (d("105"), d("-20")), "{h:?}");
    // Cover 5 at 95: (105 - 95) x 5 = 50.
    assert_eq!(h.apply(true, d("5"), d("95"), d("0"), d("1"), at()), d("50"));
    // Cover the other 15 at 120: (105 - 120) x 15 = -225, then nothing is held.
    assert_eq!(h.apply(true, d("15"), d("120"), d("0"), d("1"), at()), d("-225"));
    assert_eq!(h, Held::default());
}

#[test]
fn float_dust_does_not_leave_a_phantom_position() {
    // 0.1 + 0.1 + 0.1 is 0.30000000000000004 in binary; the ledger is decimal, so it is 0.3.
    let mut h = Held::default();
    for _ in 0..3 {
        h.apply(true, d("0.1"), d("100"), d("0"), d("1"), at());
    }
    h.apply(false, d("0.3"), d("101"), d("0"), d("1"), at());
    assert!(h.flat(), "{h:?}");
    assert_eq!(h, Held::default());
}

#[test]
fn the_account_holds_one_line_per_open_instrument_in_the_order_first_traded() {
    let fills = vec![
        fill("ES", true, d("1"), d("5000"), d("50")),
        fill("NQ", false, d("2"), d("18000"), d("20")),
        fill("ES", false, d("1"), d("5010"), d("50")),
        fill("CL", true, d("3"), d("80"), d("1000")),
    ];
    let held = holdings(&fills);
    let names: Vec<&str> = held.iter().map(|h| h.ticker.as_str()).collect();
    // ES was closed by its own sell and drops out.
    assert_eq!(names, vec!["NQ", "CL"]);
    assert_eq!((held[0].held.qty, held[1].held.qty), (d("-2"), d("3")));
}

#[test]
fn a_position_is_marked_at_the_last_price_with_the_contract_multiplier() {
    let native = native_id("massive", "future", "SCENARIO-SHORT");
    set_mark(&native, |_| Mark { last: d("4990"), at: Instant::now(), book: None });
    let mut h = Held::default();
    h.apply(false, d("2"), d("5000"), d("0"), d("50"), at());
    let hold = Holding {
        native,
        ticker: "SCENARIO-SHORT".into(),
        asset_type: "future".into(),
        provider: "massive".into(),
        connector_id: Uuid::nil(),
        currency: "USD".into(),
        multiplier: d("50"),
        held: h,
    };
    let p = hold.position();
    assert_eq!((p.side.as_str(), p.qty), ("short", 2.0));
    // Short 2 at 5000, market 4990: (4990 - 5000) x -2 x 50 = +1000.
    assert_eq!(p.unrealized_pnl, Some(1000.0), "{p:?}");
    assert_eq!(p.market_price, Some(4990.0));
    // With no print yet a position is marked at its own cost: no unrealized result is invented.
    let bare = Holding { native: native_id("massive", "future", "SCENARIO-NO-PRINT"), ..hold };
    assert_eq!(bare.mark(), d("5000"));
    assert_eq!(bare.position().unrealized_pnl, None);
}

// ── contract specifications and margin ───────────────────────────────────────────────────

fn spec_facts(ticker: &str) -> Result<Facts> {
    let meta = crate::futures_specs::complete("massive", "future", ticker, None)?;
    facts_from("future", ticker, meta.as_ref())
}

#[test]
fn futures_are_booked_with_their_specified_tick_point_value_and_currency() {
    for (ticker, mult, tick) in [("ES", d("50"), d("0.25")), ("MES", d("5"), d("0.25")), ("NQ", d("20"), d("0.25")), ("CL", d("1000"), d("0.01")), ("6E", d("125000"), d("0.00005"))] {
        let f = spec_facts(ticker).unwrap_or_else(|e| panic!("{ticker}: {e:#}"));
        assert_eq!((f.currency.as_str(), f.multiplier, f.tick), ("USD", mult, Some(tick)), "{ticker}");
        // No margin is ever invented from the specification.
        assert_eq!(f.margin, Margin::Missing, "{ticker}");
    }
}

#[test]
fn every_spelling_of_a_contract_reads_as_its_root() {
    for t in ["ESZ6", "ES=F", "@ES", "/ES", "ES.202612@CME:USD", "ES@CME"] {
        let f = spec_facts(t).unwrap_or_else(|e| panic!("{t}: {e:#}"));
        assert_eq!(f.multiplier, d("50"), "{t}");
    }
}

#[test]
fn an_unknown_contract_is_refused_not_guessed() {
    for t in ["ZZNOPE", "ES@NOWHERE", "NOTAROOT.202612"] {
        let e = spec_facts(t).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("instrument info"), "{t}: {e:?}");
    }
}

#[test]
fn a_future_ties_up_its_stated_margin_per_contract_whatever_the_price() {
    let m = Margin::PerContract(d("15000"));
    // 6 contracts: 90 000, at 5 000 or at 9 000 a point alike; short or long alike.
    assert_eq!(m.of(d("6"), d("5000"), d("50"), d("1")), d("90000"));
    assert_eq!(m.of(d("-6"), d("9000"), d("50"), d("1")), d("90000"));
    // 7 contracts break a 100 000 account.
    assert!(m.of(d("7"), d("5000"), d("50"), d("1")) > d("100000"));
    // Counted at full value instead: 1 ES at 5000 is 250 000.
    assert_eq!(Margin::Notional.of(d("1"), d("5000"), d("50"), d("1")), d("250000"));
    // A stock on a 2x account: 1 000 at 200 ties up 100 000.
    assert_eq!(Margin::Leveraged.of(d("1000"), d("200"), d("1"), d("2")), d("100000"));
    // Leverage below 1 is never a discount.
    assert_eq!(Margin::Leveraged.of(d("10"), d("100"), d("1"), d("0.1")), d("1000"));
}

#[test]
fn the_tick_a_ticket_reports_is_the_exact_decimal() {
    assert_eq!(tick_decimal(Some(0.25)).map(|d| d.to_string()), Some("0.25".into()));
    assert_eq!(tick_decimal(Some(0.00005)).map(|d| d.to_string()), Some("0.00005".into()));
    assert_eq!(tick_decimal(None), None);
}

// ── commission ───────────────────────────────────────────────────────────────────────────

#[test]
fn commission_scales_with_the_point_value_only_on_the_percent_leg() {
    // 2 ES at 5000: per unit d("2.25") x 2 = d("4.5"), plus d("0.001")% of 2 x 5000 x 50 = 5.
    let c = Commission { per_order: d("0"), per_unit: d("2.25"), pct: d("0.001"), min: d("0") };
    assert_eq!(c.fee(d("2"), d("5000"), d("50")), d("9.5"));
    // A minimum lifts a small fill and leaves a large one alone.
    let c = Commission { per_unit: d("0.005"), min: d("1"), ..Default::default() };
    assert_eq!(c.fee(d("10"), d("50"), d("1")), d("1"));
    assert_eq!(c.fee(d("1000"), d("50"), d("1")), d("5"));
    // No commission configured costs nothing.
    assert_eq!(Commission::default().fee(d("1000"), d("5000"), d("50")), d("0"));
}

// ── matching: market, limit, stop, stop-limit ────────────────────────────────────────────

#[test]
fn a_buy_limit_never_pays_more_than_its_limit_and_a_sell_never_receives_less() {
    let limit = d("100");
    let buy = order(true, "limit", Some(limit), None);
    let sell = order(false, "limit", Some(limit), None);
    // Sweep a grid of bars and books, with and without slippage configured.
    let mut px = d("90");
    while px <= d("110") {
        for spread in [d("0"), d("0.5"), d("2")] {
            let tick_sets = [
                bars(px - spread, px + spread, px),
                bars(px - spread, px + spread, px + spread),
                bars(px - spread, px + spread, px - spread),
                book(px - spread / d("2"), px + spread / d("2")),
            ];
            for t in tick_sets {
                for bps in [d("0"), d("50")] {
                    if let Some(f) = decide(&buy, &t, bps).fill {
                        assert!(f <= limit, "buy limit {limit} filled at {f} on {t:?}");
                    }
                    if let Some(f) = decide(&sell, &t, bps).fill {
                        assert!(f >= limit, "sell limit {limit} filled at {f} on {t:?}");
                    }
                }
            }
        }
        px += d("0.25");
    }
}

#[test]
fn a_buy_limit_below_the_market_rests_until_the_market_comes_down_to_it() {
    let buy = order(true, "limit", Some(d("95")), None);
    // The market trades 96..99 all session: nothing.
    assert_eq!(decide(&buy, &bars(d("96"), d("99"), d("97")), d("0")).fill, None);
    // A book whose ask is still above: nothing, even though a trade printed at 95.
    let t = Tick { lo: d("95"), hi: d("99"), last: d("95"), book: Some((d("94.9"), d("95.1"))) };
    assert_eq!(decide(&buy, &t, d("0")).fill, None);
    // The ask reaches it: filled at the limit when the ask equals it.
    assert_eq!(decide(&buy, &book(d("94.5"), d("95")), d("0")).fill, Some(d("95")));
}

#[test]
fn market_orders_cross_the_spread_and_pay_the_configured_slippage() {
    let b = decide(&order(true, "market", None, None), &book(d("99"), d("101")), d("25")).fill.unwrap_or_default();
    let s = decide(&order(false, "market", None, None), &book(d("99"), d("101")), d("25")).fill.unwrap_or_default();
    // 25 bps: the buy pays d("0.25")% above the ask, the sell gives d("0.25")% below the bid.
    assert_eq!(b, d("101.2525"));
    assert_eq!(s, d("98.7525"));
    assert_eq!(slip(d("100"), true, d("-10")), d("100"), "a negative slippage is never a discount");
}

#[test]
fn a_stop_waits_for_the_touch_then_trades_at_the_market_never_better_than_the_stop() {
    let sell_stop = order(false, "stop", None, Some(d("95")));
    let buy_stop = order(true, "stop", None, Some(d("105")));
    // Untouched.
    assert_eq!(decide(&sell_stop, &bars(d("96"), d("99"), d("97")), d("0")).fill, None);
    assert_eq!(decide(&buy_stop, &bars(d("100"), d("104"), d("103")), d("0")).fill, None);
    // Touched and back: filled at the stop.
    assert_eq!(decide(&sell_stop, &bars(d("94"), d("99"), d("98")), d("0")).fill, Some(d("95")));
    assert_eq!(decide(&buy_stop, &bars(d("100"), d("106"), d("101")), d("0")).fill, Some(d("105")));
    // Gap through: the fill is where the market is, a sell below its stop, a buy above.
    assert_eq!(decide(&sell_stop, &bars(d("89"), d("91"), d("90")), d("0")).fill, Some(d("90")));
    assert_eq!(decide(&buy_stop, &bars(d("110"), d("112"), d("111")), d("0")).fill, Some(d("111")));
    // Never better than the stop, with or without slippage.
    for bps in [d("0"), d("30")] {
        for last in [d("80"), d("90"), d("95"), d("96")] {
            if let Some(f) = decide(&sell_stop, &bars(d("80"), d("99"), last), bps).fill {
                assert!(f <= d("95"), "sell stop 95 filled at {f}");
            }
        }
        for last in [d("105"), d("106"), d("120")] {
            if let Some(f) = decide(&buy_stop, &bars(d("101"), d("125"), last), bps).fill {
                assert!(f >= d("105"), "buy stop 105 filled at {f}");
            }
        }
    }
}

#[test]
fn a_stop_limit_triggers_once_then_rests_as_a_limit() {
    let mut o = order(true, "stop_limit", Some(d("106")), Some(d("105")));
    // Below the stop: nothing, not even a trigger.
    assert_eq!(decide(&o, &bars(d("100"), d("104"), d("103")), d("0")), Decision::default());
    // The market gaps over stop and limit together: triggered, but the limit is out of reach.
    assert_eq!(decide(&o, &bars(d("108"), d("109"), d("108.5")), d("0")), Decision { trigger: true, fill: None, best: None });
    o.triggered = true;
    // Still out of reach: no new trigger, no fill.
    assert_eq!(decide(&o, &bars(d("107"), d("108"), d("107.5")), d("0")), Decision::default());
    // The market comes back to the limit: now it fills, at the better of last and limit.
    assert_eq!(decide(&o, &bars(d("105.5"), d("107"), d("105.8")), d("0")).fill, Some(d("105.8")));
    assert_eq!(decide(&o, &bars(d("105.5"), d("107"), d("106.5")), d("0")).fill, Some(d("106")));
    // A sell stop-limit mirrors it: stop 95, limit 94.
    let s = order(false, "stop_limit", Some(d("94")), Some(d("95")));
    assert_eq!(decide(&s, &bars(d("90"), d("93"), d("91")), d("0")), Decision { trigger: true, fill: None, best: None });
    // Stop touched with the limit in reach on the same print: sold at the better of last and limit.
    assert_eq!(decide(&s, &bars(d("94"), d("96"), d("95.5")), d("0")), Decision { trigger: true, fill: Some(d("95.5")), best: None });
}

#[test]
fn an_order_missing_the_price_its_type_needs_never_fills() {
    assert_eq!(decide(&order(true, "limit", None, None), &bars(d("1"), d("1000"), d("5")), d("0")), Decision::default());
    assert_eq!(decide(&order(true, "stop", None, None), &bars(d("1"), d("1000"), d("5")), d("0")), Decision::default());
    assert_eq!(decide(&order(true, "stop_limit", Some(d("5")), None), &bars(d("1"), d("1000"), d("5")), d("0")), Decision::default());
    assert_eq!(decide(&order(true, "iceberg", Some(d("5")), None), &bars(d("1"), d("1000"), d("5")), d("0")), Decision::default());
}

// ── time in force ────────────────────────────────────────────────────────────────────────

#[test]
fn a_day_order_placed_one_second_before_midnight_lives_one_second() {
    let late = time::macros::datetime!(2026-10-22 23:59:59 UTC);
    assert_eq!(expiry(TifKind::Day, None, late).unwrap(), Some(time::macros::datetime!(2026-10-23 00:00 UTC)));
    // Placed exactly at midnight it lasts the whole new day, not zero seconds.
    let midnight = time::macros::datetime!(2026-10-23 00:00 UTC);
    assert_eq!(expiry(TifKind::Day, None, midnight).unwrap(), Some(time::macros::datetime!(2026-10-24 00:00 UTC)));
}

#[test]
fn good_till_cancelled_and_immediate_or_cancel_carry_no_expiry() {
    let now = time::macros::datetime!(2026-10-22 15:30 UTC);
    assert_eq!(expiry(TifKind::Gtc, None, now).unwrap(), None);
    assert_eq!(expiry(TifKind::Ioc, None, now).unwrap(), None);
    // A good-till-date keeps its exact instant.
    let ms = (now.unix_timestamp() + 3600) * 1000 + 250;
    let at = expiry(TifKind::Gtd, Some(ms), now).unwrap().unwrap();
    assert_eq!(at.unix_timestamp_nanos() / 1_000_000, ms as i128);
}

#[test]
fn covering_a_short_to_flat_takes_risk_off() {
    // Buying 1 back on a 1-lot short leaves nothing: risk only goes down.
    assert!(takes_risk_off(d("-1"), d("0")));
    assert!(takes_risk_off(d("1"), d("0")));
    assert!(takes_risk_off(d("-3"), d("-1")));
    // A cover in tenths lands on flat exactly: a decimal sum leaves no dust on either side.
    assert!(takes_risk_off(d("-0.3"), d("-0.3") + d("0.1") + d("0.1") + d("0.1")));
    assert!(!takes_risk_off(d("-0.3"), d("0.000000000001")));
    // Growing, or flipping through zero to the other side, is new risk.
    assert!(!takes_risk_off(d("-1"), d("-2")));
    assert!(!takes_risk_off(d("-1"), d("0.5")));
    assert!(!takes_risk_off(d("0"), d("1")));
}

#[test]
fn a_price_between_two_ticks_is_refused_like_a_venue_does() {
    let es = Some(d("0.25"));
    assert!(on_tick(d("5000.25"), es));
    assert!(!on_tick(d("5000.10"), es));
    assert!(on_tick(d("1400.01"), Some(d("0.01"))));
    assert!(!on_tick(d("1400.005"), Some(d("0.01"))));
    assert!(on_tick(d("0.00000013"), Some(d("0.00000001"))));
    // No stated tick: any price stands.
    assert!(on_tick(d("95.13"), None));
    assert!(venue_refusal("futures", es, Some(d("1")), &[(Some(d("5000.10")), "limit price")]).is_some());
    assert!(venue_refusal("futures", es, Some(d("1")), &[(Some(d("5000.25")), "limit price"), (None, "stop price")]).is_none());
}

#[test]
fn a_contract_trades_in_whole_units_a_crypto_does_not() {
    assert!(venue_refusal("futures", Some(d("0.25")), Some(d("1.5")), &[]).is_some());
    assert!(venue_refusal("futures", Some(d("0.25")), Some(d("2")), &[]).is_none());
    assert!(venue_refusal("crypto", Some(d("0.01")), Some(d("0.015")), &[]).is_none());
}
