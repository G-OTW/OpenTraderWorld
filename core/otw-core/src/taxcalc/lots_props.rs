//! Property tests of lot matching (TESTING.md §5): the invariants every matching method must
//! hold whatever the fills are.
//!
//! Money here is `f64` (the production type), so equalities are checked to a relative
//! 1e-9 of the notional involved: that is floating-point noise, far under a cent on any
//! realistic book, and never a way to hide a rounding rule (this module has none: it never
//! rounds, the report does).

use super::*;
use proptest::prelude::*;
use time::macros::datetime;

const METHODS: [Method; 3] = [Method::Average, Method::Fifo, Method::UkPool];

/// One generated execution: side, quantity and price on a grid of cents, day offset.
#[derive(Debug, Clone, Copy)]
struct Gen {
    buy: bool,
    qty: f64,
    price: f64,
    day: i64,
    fee: f64,
}

fn qty() -> impl Strategy<Value = f64> {
    (1i64..100_000).prop_map(|c| c as f64 / 100.0)
}

fn price() -> impl Strategy<Value = f64> {
    (1i64..10_000_000).prop_map(|c| c as f64 / 100.0)
}

fn fee() -> impl Strategy<Value = f64> {
    (0i64..10_000).prop_map(|c| c as f64 / 100.0)
}

fn fill(i: usize, g: Gen, rate: f64) -> Fill {
    Fill {
        id: format!("f{i:04}"),
        symbol: "ACME".into(),
        asset_class: "stock".into(),
        bucket: "capital",
        valuation: Valuation::Spot,
        at: datetime!(2025-01-02 10:00 UTC) + Duration::days(g.day) + Duration::minutes(i as i64),
        buy: g.buy,
        qty: g.qty,
        gross: g.qty * g.price,
        fee: g.fee,
        currency: "USD".into(),
        rate: Some(rate),
    }
}

fn fills(gens: &[Gen], rate: f64) -> Vec<Fill> {
    gens.iter().enumerate().map(|(i, g)| fill(i, *g, rate)).collect()
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-9 * scale.abs().max(1.0)
}

fn total_gain(o: &Outcome) -> f64 {
    o.disposals.iter().map(|d| d.gain).sum()
}

fn notional(gens: &[Gen]) -> f64 {
    gens.iter().map(|g| g.qty * g.price + g.fee).sum()
}

/// A long-only book that never sells more than it holds: buys, then partial sales of what is
/// open, each sale a fraction (in percent) of the position.
fn long_book() -> impl Strategy<Value = Vec<Gen>> {
    prop::collection::vec((any::<bool>(), qty(), price(), 1u32..=100, fee()), 1..24).prop_map(|steps| {
        let mut out = Vec::new();
        let mut held = 0.0_f64;
        for (i, (want_buy, q, p, pct, f)) in steps.into_iter().enumerate() {
            let day = i as i64 * 3;
            if want_buy || held <= 0.0 {
                out.push(Gen { buy: true, qty: q, price: p, day, fee: f });
                held += q;
            } else {
                let sell = (held * pct as f64 / 100.0 * 100.0).floor() / 100.0;
                if sell <= 0.0 {
                    continue;
                }
                out.push(Gen { buy: false, qty: sell, price: p, day, fee: f });
                held -= sell;
            }
        }
        out
    })
}

fn without_fees(gens: &[Gen]) -> Vec<Gen> {
    gens.iter().map(|g| Gen { fee: 0.0, ..*g }).collect()
}

/// A fixed seed by default, so every run explores the same cases (TESTING.md section 0, rule 5);
/// `PROPTEST_RNG_SEED=<n>` explores others. A failure is saved under `proptest-regressions/`
/// and replayed first on the next run: commit that file with the fix.
fn config(cases: u32) -> ProptestConfig {
    let mut c = ProptestConfig::with_cases(cases);
    if c.rng_seed == proptest::test_runner::RngSeed::Random {
        c.rng_seed = proptest::test_runner::RngSeed::Fixed(20_261_107);
    }
    c
}

proptest! {
    #![proptest_config(config(256))]

    /// Bought and sold back at the same price, no fees, in one or several sales: nothing is
    /// realized, under every method and whatever the tax rate is.
    #[test]
    fn round_trip_at_the_same_price_without_fees_realizes_nothing(
        q in qty(), p in price(), pieces in 1usize..6, rate in 1i64..400,
    ) {
        let rate = rate as f64 / 100.0;
        let mut gens = vec![Gen { buy: true, qty: q, price: p, day: 0, fee: 0.0 }];
        let piece = q / pieces as f64;
        for k in 0..pieces {
            let last = k + 1 == pieces;
            let sold: f64 = gens[1..].iter().map(|g| g.qty).sum();
            gens.push(Gen { buy: false, qty: if last { q - sold } else { piece }, price: p, day: 40 + k as i64, fee: 0.0 });
        }
        for m in METHODS {
            let out = match_fills(fills(&gens, rate), m, false, None);
            prop_assert!(out.errors.is_empty(), "{m:?}: {:?}", out.errors);
            prop_assert!(close(total_gain(&out), 0.0, q * p * rate), "{m:?}: gain {}", total_gain(&out));
            prop_assert!(out.open.is_empty(), "{m:?}: {:?}", out.open);
        }
    }

    /// Commissions are a cost: the same book with its fees never realizes more than without.
    #[test]
    fn fees_never_increase_the_realized_gain(gens in long_book()) {
        let scale = notional(&gens);
        for m in METHODS {
            let with = match_fills(fills(&gens, 1.0), m, false, None);
            let without = match_fills(fills(&without_fees(&gens), 1.0), m, false, None);
            prop_assert!(with.errors.is_empty() && without.errors.is_empty(), "{m:?}: {:?} {:?}", with.errors, without.errors);
            prop_assert!(total_gain(&with) <= total_gain(&without) + 1e-9 * scale, "{m:?}: {} > {}", total_gain(&with), total_gain(&without));
        }
    }

    /// The pooled cost of a position is a weighted average of what its units cost: per unit,
    /// it lies between the cheapest and the dearest purchase (fees off, tax rate 1).
    #[test]
    fn average_cost_per_unit_lies_between_the_purchase_prices(gens in long_book()) {
        let gens = without_fees(&gens);
        let buys: Vec<f64> = gens.iter().filter(|g| g.buy).map(|g| g.price).collect();
        let (lo, hi) = buys.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(*p), b.max(*p)));
        for m in [Method::Average, Method::UkPool] {
            let out = match_fills(fills(&gens, 1.0), m, false, None);
            for pos in &out.open {
                let per_unit = pos.basis / pos.qty;
                prop_assert!(per_unit >= lo * (1.0 - 1e-9) && per_unit <= hi * (1.0 + 1e-9), "{m:?}: {per_unit} outside [{lo}, {hi}]");
            }
        }
    }

    /// A cash account (no short): whatever the order of the fills, the open quantity is never
    /// negative, and it is the purchases minus the quantity the sales actually matched. A sale
    /// past the position is an error, never a short or a silent partial match.
    #[test]
    fn open_quantity_is_buys_minus_matched_sales_and_never_negative(
        steps in prop::collection::vec((any::<bool>(), qty(), price()), 1..20),
    ) {
        let gens: Vec<Gen> = steps.iter().enumerate().map(|(i, (b, q, p))| Gen { buy: *b, qty: *q, price: *p, day: i as i64 * 2, fee: 0.0 }).collect();
        let bought: f64 = gens.iter().filter(|g| g.buy).map(|g| g.qty).sum();
        let mut held = 0.0_f64;
        let mut oversold = false;
        for g in &gens {
            if g.buy { held += g.qty } else if g.qty > held + 1e-9 { oversold = true; held = 0.0 } else { held -= g.qty }
        }
        for m in [Method::Average, Method::Fifo] {
            let out = match_fills(fills(&gens, 1.0), m, false, None);
            let open: f64 = out.open.iter().map(|p| p.qty).sum();
            let matched: f64 = out.disposals.iter().map(|d| d.qty).sum();
            prop_assert!(open >= 0.0, "{m:?}: open {open}");
            prop_assert!(out.disposals.iter().all(|d| !d.short), "{m:?}: a short in a cash account");
            prop_assert!(close(open, bought - matched, bought), "{m:?}: open {open} != {bought} - {matched}");
            prop_assert_eq!(oversold, !out.errors.is_empty(), "{:?}: errors {:?}", m, out.errors);
        }
    }

    /// A split N:1 multiplies every quantity by N and leaves every amount alone: the gains, the
    /// open basis and the matched value do not move; the open quantity is N times larger.
    #[test]
    fn a_split_changes_quantities_not_money(gens in long_book(), n in 2u32..=20) {
        let n = n as f64;
        let split: Vec<Gen> = gens.iter().map(|g| Gen { qty: g.qty * n, price: g.price / n, ..*g }).collect();
        let scale = notional(&gens);
        for m in METHODS {
            let a = match_fills(fills(&gens, 1.0), m, false, None);
            let b = match_fills(fills(&split, 1.0), m, false, None);
            prop_assert!(a.errors.is_empty() && b.errors.is_empty(), "{m:?}: {:?} {:?}", a.errors, b.errors);
            prop_assert!(close(total_gain(&a), total_gain(&b), scale), "{m:?}: {} vs {}", total_gain(&a), total_gain(&b));
            let qa: f64 = a.open.iter().map(|p| p.qty).sum();
            let qb: f64 = b.open.iter().map(|p| p.qty).sum();
            let ba: f64 = a.open.iter().map(|p| p.basis).sum();
            let bb: f64 = b.open.iter().map(|p| p.basis).sum();
            prop_assert!(close(qa * n, qb, qb), "{m:?}: open {qa} x {n} != {qb}");
            prop_assert!(close(ba, bb, scale), "{m:?}: basis {ba} vs {bb}");
        }
    }

    /// The same book booked in another currency: every amount is x times larger in B and every
    /// rate x times smaller (A to B to tax is A to tax). The tax-currency result is the same,
    /// so the route through a second currency adds nothing and loses nothing.
    #[test]
    fn rebooking_in_a_second_currency_leaves_the_tax_result_unchanged(
        gens in long_book(), rate in 50i64..200, x in 1i64..20_000,
    ) {
        let (rate, x) = (rate as f64 / 100.0, x as f64 / 100.0);
        let scale = notional(&gens) * rate;
        for m in METHODS {
            let a = match_fills(fills(&gens, rate), m, false, None);
            let mut in_b = fills(&gens, rate / x);
            for f in &mut in_b {
                f.gross *= x;
                f.fee *= x;
                f.currency = "JPY".into();
            }
            let b = match_fills(in_b, m, false, None);
            prop_assert!(close(total_gain(&a), total_gain(&b), scale), "{m:?}: {} vs {}", total_gain(&a), total_gain(&b));
            let ba: f64 = a.open.iter().map(|p| p.basis).sum();
            let bb: f64 = b.open.iter().map(|p| p.basis).sum();
            prop_assert!(close(ba, bb, scale), "{m:?}: basis {ba} vs {bb}");
        }
    }

    /// With one rate for every fill, conversion is a scale: the tax gain is the native gain
    /// times the rate, and a rate of 1 (the tax currency itself) is the identity.
    #[test]
    fn one_rate_for_every_fill_scales_the_native_gain(gens in long_book(), rate in 1i64..500) {
        let rate = rate as f64 / 100.0;
        let scale = notional(&gens) * rate.max(1.0);
        for m in METHODS {
            let out = match_fills(fills(&gens, rate), m, false, None);
            for d in &out.disposals {
                prop_assert!(close(d.gain, d.native_gain * rate, scale), "{m:?}: {} vs {} x {rate}", d.gain, d.native_gain);
            }
            let unit = match_fills(fills(&gens, 1.0), m, false, None);
            for d in &unit.disposals {
                prop_assert!(close(d.gain, d.native_gain, scale), "{m:?}: rate 1 moved the gain");
            }
        }
    }
}

/// Known case, by hand: 10 @ 100 and 10 @ 120 pooled is 20 @ 110; selling 5 @ 130 with a 2.00
/// commission realizes 5 x 130 - 2 - 5 x 110 = 98, and the 15 left keep the 110 basis.
#[test]
fn average_cost_known_case() {
    let gens = [
        Gen { buy: true, qty: 10.0, price: 100.0, day: 0, fee: 0.0 },
        Gen { buy: true, qty: 10.0, price: 120.0, day: 5, fee: 0.0 },
        Gen { buy: false, qty: 5.0, price: 130.0, day: 10, fee: 2.0 },
    ];
    let out = match_fills(fills(&gens, 1.0), Method::Average, false, None);
    assert_eq!(out.disposals.len(), 1);
    assert!((out.disposals[0].gain - 98.0).abs() < 1e-9, "{}", out.disposals[0].gain);
    assert_eq!(out.open[0].qty, 15.0);
    assert!((out.open[0].basis - 1650.0).abs() < 1e-9, "{}", out.open[0].basis);
}

/// Known case, by hand: the same fills under FIFO match the sale against the first lot (100),
/// realizing 5 x 130 - 2 - 5 x 100 = 148; the 15 left are 5 @ 100 and 10 @ 120 = 1700.
#[test]
fn fifo_known_case() {
    let gens = [
        Gen { buy: true, qty: 10.0, price: 100.0, day: 0, fee: 0.0 },
        Gen { buy: true, qty: 10.0, price: 120.0, day: 5, fee: 0.0 },
        Gen { buy: false, qty: 5.0, price: 130.0, day: 10, fee: 2.0 },
    ];
    let out = match_fills(fills(&gens, 1.0), Method::Fifo, false, None);
    assert!((out.disposals[0].gain - 148.0).abs() < 1e-9, "{}", out.disposals[0].gain);
    assert!((out.open[0].basis - 1700.0).abs() < 1e-9, "{}", out.open[0].basis);
}
