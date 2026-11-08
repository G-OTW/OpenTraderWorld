//! What a position ties up at a US broker under the standard strategy-based rules (Reg T and
//! FINRA Rule 4210, as the Cboe Margin Manual sets them out). An estimate: a broker can ask
//! for more, and portfolio margin works differently.
//!
//! - A long option is paid in full.
//! - An uncovered short pays its premium plus 20% of the underlying (15% on a broad index)
//!   less what it is out of the money, at least 10% of the underlying for a call and of the
//!   strike for a put.
//! - A short covered by a long of the same kind expiring no sooner needs the distance between
//!   the strikes, and nothing when the long is the safer strike. A short call on shares held
//!   (a short put on shares sold) needs nothing more than the shares.
//! - A short call and a short put on one expiry cannot both lose: the larger side counts, plus
//!   the premium of the other when it is uncovered.
//! - Shares take half their value.

use serde::Serialize;
use time::Date;

use super::strategy::{Kind, Leg};
use super::MULTIPLIER;

const EQUITY_RATE: f64 = 0.20;
const INDEX_RATE: f64 = 0.15;
const MINIMUM_RATE: f64 = 0.10;
const STOCK_RATE: f64 = 0.50;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Margin {
    /// The rules' requirement, proceeds of the short options not yet taken off.
    pub requirement: f64,
    /// What leaves the account's buying power: the requirement net of the premium received,
    /// or plus the premium paid.
    pub buying_power: f64,
}

#[derive(Debug, Clone, Copy)]
struct Open {
    strike: f64,
    expiry: Date,
    price: f64,
    /// Contracts, always positive; what is left to pair.
    qty: f64,
}

fn side(legs: &[Leg], kind: Kind, short: bool) -> Vec<Open> {
    legs.iter()
        .filter(|l| l.kind == kind && (l.qty < 0.0) == short && l.qty != 0.0)
        .filter_map(|l| Some(Open { strike: l.strike, expiry: l.expiry?, price: l.price, qty: l.qty.abs() }))
        .collect()
}

fn uncovered(call: bool, o: &Open, spot: f64, rate: f64) -> f64 {
    let (out_of_money, floor) = if call {
        ((o.strike - spot).max(0.0), MINIMUM_RATE * spot)
    } else {
        ((spot - o.strike).max(0.0), MINIMUM_RATE * o.strike)
    };
    o.price + (rate * spot - out_of_money).max(floor)
}

/// One side's requirement on `expiry`, and the premium of what stayed uncovered there.
struct SideTotal {
    requirement: f64,
    uncovered_premium: f64,
}

/// The shorts of one kind paired against the longs, tightest first, then against the shares
/// left, then uncovered. `longs` and `shares` carry what stays free to the next expiry.
fn requirement(call: bool, shorts: &[Open], longs: &mut [Open], shares: &mut f64, spot: f64, rate: f64) -> SideTotal {
    let mut total = SideTotal { requirement: 0.0, uncovered_premium: 0.0 };
    for short in shorts {
        let mut left = short.qty;
        let distance = |l: &Open| if call { (l.strike - short.strike).max(0.0) } else { (short.strike - l.strike).max(0.0) };
        while left > 1e-9 {
            let Some(long) = longs
                .iter_mut()
                .filter(|l| l.qty > 1e-9 && l.expiry >= short.expiry)
                .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            else {
                break;
            };
            let n = left.min(long.qty);
            total.requirement += distance(long) * n * MULTIPLIER;
            long.qty -= n;
            left -= n;
        }
        let covered = left.min(*shares / MULTIPLIER);
        *shares -= covered * MULTIPLIER;
        left -= covered;
        if left > 1e-9 {
            total.requirement += uncovered(call, short, spot, rate) * left * MULTIPLIER;
            total.uncovered_premium += short.price * left * MULTIPLIER;
        }
    }
    total
}

/// `net_premium` as the strategy reports it: positive for a credit. `index` for a broad index.
pub fn estimate(legs: &[Leg], spot: f64, index: bool, net_premium: f64) -> Margin {
    let rate = if index { INDEX_RATE } else { EQUITY_RATE };
    let (mut long_calls, mut long_puts) = (side(legs, Kind::Call, false), side(legs, Kind::Put, false));
    let (short_calls, short_puts) = (side(legs, Kind::Call, true), side(legs, Kind::Put, true));
    let shares: f64 = legs.iter().filter(|l| l.kind == Kind::Stock).map(|l| l.qty).sum();
    let (mut held, mut sold) = (shares.max(0.0), (-shares).max(0.0));

    let mut expiries: Vec<Date> = short_calls.iter().chain(&short_puts).map(|o| o.expiry).collect();
    expiries.sort();
    expiries.dedup();
    let mut options = 0.0;
    for e in expiries {
        let on = |v: &[Open]| v.iter().copied().filter(|o| o.expiry == e).collect::<Vec<_>>();
        let calls = requirement(true, &on(&short_calls), &mut long_calls, &mut held, spot, rate);
        let puts = requirement(false, &on(&short_puts), &mut long_puts, &mut sold, spot, rate);
        options += if calls.requirement >= puts.requirement {
            calls.requirement + puts.uncovered_premium
        } else {
            puts.requirement + calls.uncovered_premium
        };
    }
    let stock = shares.abs() * spot * STOCK_RATE;
    let required = options + stock;
    Margin { requirement: required, buying_power: (required - net_premium).max(0.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const NEAR: Date = date!(2026 - 12 - 18);
    const FAR: Date = date!(2027 - 01 - 15);

    fn opt(kind: Kind, strike: f64, qty: f64, price: f64) -> Leg {
        Leg { kind, strike, expiry: Some(NEAR), qty, price, iv: Some(0.2) }
    }

    fn net(legs: &[Leg]) -> f64 {
        legs.iter().filter(|l| l.kind != Kind::Stock).map(|l| -l.qty * l.price * MULTIPLIER).sum()
    }

    fn run(legs: &[Leg]) -> Margin {
        estimate(legs, 100.0, false, net(legs))
    }

    #[test]
    fn a_long_option_is_paid_in_full() {
        assert_eq!(run(&[opt(Kind::Call, 100.0, 2.0, 3.0)]), Margin { requirement: 0.0, buying_power: 600.0 });
    }

    #[test]
    fn an_uncovered_short_follows_the_rule_and_its_floor() {
        // At the money: 20% of 100 plus the premium.
        assert_eq!(run(&[opt(Kind::Call, 100.0, -1.0, 3.0)]), Margin { requirement: 2300.0, buying_power: 2000.0 });
        // 15 out of the money: 20 - 15 = 5, under the 10% floor.
        assert_eq!(run(&[opt(Kind::Call, 115.0, -1.0, 0.5)]).requirement, 1050.0);
        // A put's floor is 10% of its strike.
        assert_eq!(run(&[opt(Kind::Put, 80.0, -1.0, 0.5)]).requirement, 850.0);
        // In the money the whole 20% counts.
        assert_eq!(run(&[opt(Kind::Put, 110.0, -1.0, 12.0)]).requirement, 3200.0);
    }

    #[test]
    fn a_broad_index_takes_fifteen_percent() {
        let legs = [opt(Kind::Call, 100.0, -1.0, 3.0)];
        assert_eq!(estimate(&legs, 100.0, true, net(&legs)).requirement, 1800.0);
    }

    #[test]
    fn a_credit_spread_ties_up_its_most_loss() {
        let legs = [opt(Kind::Put, 95.0, -1.0, 2.0), opt(Kind::Put, 90.0, 1.0, 1.0)];
        assert_eq!(run(&legs), Margin { requirement: 500.0, buying_power: 400.0 });
    }

    #[test]
    fn a_debit_spread_is_only_its_debit() {
        let legs = [opt(Kind::Call, 100.0, 1.0, 3.0), opt(Kind::Call, 105.0, -1.0, 1.0)];
        assert_eq!(run(&legs), Margin { requirement: 0.0, buying_power: 200.0 });
    }

    #[test]
    fn an_iron_condor_counts_its_wider_side_once() {
        let legs = [
            opt(Kind::Put, 90.0, 1.0, 0.5),
            opt(Kind::Put, 95.0, -1.0, 1.2),
            opt(Kind::Call, 105.0, -1.0, 1.1),
            opt(Kind::Call, 115.0, 1.0, 0.3),
        ];
        let m = run(&legs);
        assert_eq!(m.requirement, 1000.0);
        assert!((m.buying_power - (1000.0 - 150.0)).abs() < 1e-9, "{m:?}");
    }

    #[test]
    fn a_short_strangle_counts_the_larger_side_plus_the_other_premium() {
        let legs = [opt(Kind::Call, 105.0, -1.0, 2.0), opt(Kind::Put, 95.0, -1.0, 1.5)];
        // Call: 2 + 20 - 5 = 17; put: 1.5 + 20 - 5 = 16.5. 1700 + 150.
        assert_eq!(run(&legs).requirement, 1850.0);
    }

    #[test]
    fn a_covered_call_needs_only_the_shares() {
        let stock = Leg { kind: Kind::Stock, strike: 0.0, expiry: None, qty: 100.0, price: 100.0, iv: None };
        let legs = [stock, opt(Kind::Call, 105.0, -1.0, 2.0)];
        assert_eq!(run(&legs), Margin { requirement: 5000.0, buying_power: 4800.0 });
        // Two calls on one lot: the second is uncovered.
        let two = [stock, opt(Kind::Call, 105.0, -2.0, 2.0)];
        assert_eq!(run(&two).requirement, 5000.0 + 1700.0);
    }

    #[test]
    fn a_calendar_covers_only_when_the_long_lasts_longer() {
        let far = |kind, qty, price| Leg { expiry: Some(FAR), ..opt(kind, 100.0, qty, price) };
        // Short near, long far: covered.
        assert_eq!(run(&[opt(Kind::Call, 100.0, -1.0, 2.0), far(Kind::Call, 1.0, 4.0)]).requirement, 0.0);
        // Short far, long near: the long is gone first, the short is uncovered.
        assert_eq!(run(&[far(Kind::Call, -1.0, 4.0), opt(Kind::Call, 100.0, 1.0, 2.0)]).requirement, 2400.0);
    }
}
