//! Property tests of the what-if lab's pure scenarios (TESTING.md §5): reversing, resizing and
//! re-charging a closed book must keep the journal's P&L arithmetic consistent.
//!
//! Money is `f64` here as in production; equalities are held to a relative 1e-9 of the
//! amounts involved (floating-point noise, not a rounding rule: these functions never round,
//! only `compute_fee` does, to 4 or 6 decimals, which the fee checks account for).

use super::*;
use proptest::prelude::*;
use time::macros::datetime;

fn money() -> impl Strategy<Value = f64> {
    (-5_000_000i64..5_000_000).prop_map(|c| c as f64 / 100.0)
}

fn fee() -> impl Strategy<Value = f64> {
    (0i64..20_000).prop_map(|c| c as f64 / 100.0)
}

/// (net, fees, hours after the first entry, entry quantity, entry price) of one closed trade.
fn book() -> impl Strategy<Value = Vec<LabTrade>> {
    prop::collection::vec((money(), fee(), 0i64..24 * 20, 1i64..10_000, 1i64..1_000_000), 0..30).prop_map(|rows| {
        let t0 = datetime!(2026-01-05 08:00 UTC);
        rows.into_iter()
            .enumerate()
            .map(|(i, (net, fees, h, q, p))| {
                let entry = t0 + Duration::hours(h);
                let exit = entry + Duration::minutes(30 + i as i64);
                LabTrade {
                    id: Uuid::from_u128(i as u128 + 1),
                    ticker: "ACME".into(),
                    asset_class: "stock".into(),
                    side: "long".into(),
                    exit_at: exit,
                    entry_local: Some(entry),
                    exit_local: exit,
                    qty: q as f64 / 100.0,
                    entry_price: Some(p as f64 / 100.0),
                    net,
                    fees,
                    risk: None,
                    fx: 1.0,
                    regime: None,
                    mae_pct: None,
                    mfe_pct: None,
                }
            })
            .collect()
    })
}

fn sims(steps: &[Step]) -> Vec<Option<Sim>> {
    steps.iter().map(|s| s.sim).collect()
}

fn as_is(trades: &[LabTrade]) -> Vec<Option<Sim>> {
    trades.iter().map(|t| Some(Sim { net: t.net, fees: t.fees })).collect()
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-9 * scale.abs().max(1.0)
}

fn schedule(amount: f64, kind: &str, per: &str) -> FeeSchedule {
    FeeSchedule { id: Uuid::nil(), name: "alt".into(), amount, amount_kind: kind.into(), per: per.into(), currency: "USD".into(), position: 0.0 }
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

    /// Flipping a trade twice gives the trade back: the same net, the same fees.
    #[test]
    fn reversing_twice_is_the_identity(trades in book()) {
        let all = vec![true; trades.len()];
        let (once, _) = reverse(&trades, &all);
        let flipped: Vec<LabTrade> = trades
            .iter()
            .zip(&once)
            .map(|(t, s)| LabTrade { net: s.sim.unwrap().net, fees: s.sim.unwrap().fees, ..t.clone() })
            .collect();
        let (twice, _) = reverse(&flipped, &all);
        for (t, s) in trades.iter().zip(&twice) {
            let sim = s.sim.unwrap();
            prop_assert!(close(sim.net, t.net, t.net.abs() + t.fees), "{} -> {}", t.net, sim.net);
            prop_assert_eq!(sim.fees, t.fees);
        }
    }

    /// A trade and its reverse share the same entry and exit, so their gross cancels and
    /// together they lose exactly both commissions: fees can only take P&L away.
    #[test]
    fn a_trade_and_its_reverse_lose_exactly_both_fees(trades in book()) {
        let (steps, _) = reverse(&trades, &vec![true; trades.len()]);
        for (t, s) in trades.iter().zip(&steps) {
            let both = t.net + s.sim.unwrap().net;
            prop_assert!(close(both, -2.0 * t.fees, t.net.abs() + t.fees), "{} + {} != -2 x {}", t.net, s.sim.unwrap().net, t.fees);
            prop_assert!(both <= 1e-9 * t.net.abs().max(1.0));
        }
    }

    /// Size x1 without a cap changes nothing, and size xk scales the book's net and fees by k.
    #[test]
    fn size_is_linear_without_a_cap(trades in book(), k in 50i64..=300) {
        let k = k as f64 / 100.0;
        let base = summarize(&trades, &as_is(&trades));
        let (one, _) = size(&trades, &SizeParams { factor: 1.0, daily_loss_cap: None });
        prop_assert_eq!(summarize(&trades, &sims(&one)), base.clone());
        let (scaled, _) = size(&trades, &SizeParams { factor: k, daily_loss_cap: None });
        let s = summarize(&trades, &sims(&scaled));
        let scale: f64 = trades.iter().map(|t| t.net.abs() + t.fees).sum();
        prop_assert!(close(s.net, base.net * k, scale * k), "{} vs {} x {k}", s.net, base.net);
        prop_assert!(close(s.fees, base.fees * k, scale * k));
        prop_assert!(close(s.max_drawdown, base.max_drawdown * k, scale * k));
    }

    /// A daily loss cap only ever removes trades: every trade it keeps is the uncapped trade,
    /// and it never takes more trades than the uncapped run.
    #[test]
    fn a_daily_loss_cap_only_removes_trades(trades in book(), cap in 1i64..500_000) {
        let cap = cap as f64 / 100.0;
        let (free, _) = size(&trades, &SizeParams { factor: 1.0, daily_loss_cap: None });
        let (capped, detail) = size(&trades, &SizeParams { factor: 1.0, daily_loss_cap: Some(cap) });
        let mut skipped = 0;
        for (a, b) in free.iter().zip(&capped) {
            match b.sim {
                Some(sim) => prop_assert_eq!(Some(sim), a.sim),
                None => skipped += 1,
            }
        }
        let Detail::Size { trades_skipped, .. } = detail else { unreachable!() };
        prop_assert_eq!(skipped, trades_skipped);
    }

    /// Re-charging the book: the net moves by exactly what was paid minus what the schedule
    /// charges, and a dearer schedule never leaves a better net.
    #[test]
    fn a_dearer_fee_schedule_never_improves_the_net(
        trades in book(), lo in 0i64..1_000, extra in 0i64..1_000, pct in any::<bool>(), per_unit in any::<bool>(),
    ) {
        let kind = if pct { "pct" } else { "fixed" };
        let per = if per_unit { "unit" } else { "trade" };
        let (lo, hi) = (lo as f64 / 100.0, (lo + extra) as f64 / 100.0);
        let fx = vec![1.0; trades.len()];
        let (cheap, _) = fees(&trades, &schedule(lo, kind, per), None, &fx, 0);
        let (dear, _) = fees(&trades, &schedule(hi, kind, per), None, &fx, 0);
        for ((t, c), d) in trades.iter().zip(&cheap).zip(&dear) {
            let (c, d) = (c.sim.unwrap(), d.sim.unwrap());
            let scale = t.net.abs() + t.fees + c.fees;
            prop_assert!(close(c.net, t.net + t.fees - c.fees, scale), "net {} != {} + {} - {}", c.net, t.net, t.fees, c.fees);
            prop_assert!(c.fees >= 0.0);
            prop_assert!(d.net <= c.net + 1e-9 * scale, "{} fee {hi} gives {} > {} at {lo}", kind, d.net, c.net);
        }
    }

    /// The summary's books close: the curve ends on the net, drawdown is never negative and
    /// never more than the whole swing of the curve, and wins plus the rest are the sample.
    #[test]
    fn the_summary_adds_up(trades in book()) {
        let s = summarize(&trades, &as_is(&trades));
        let net: f64 = trades.iter().map(|t| t.net).sum();
        let scale: f64 = trades.iter().map(|t| t.net.abs()).sum();
        prop_assert_eq!(s.n, trades.len());
        prop_assert!(close(s.net, net, scale));
        prop_assert!(close(s.curve.last().copied().unwrap_or(0.0), s.net, scale));
        prop_assert!(s.max_drawdown >= 0.0);
        let peak = s.curve.iter().copied().fold(0.0_f64, f64::max);
        let low = s.curve.iter().copied().fold(0.0_f64, f64::min);
        prop_assert!(s.max_drawdown <= peak - low + 1e-9 * scale.max(1.0));
        if let Some(w) = s.win_rate {
            prop_assert!((0.0..=100.0).contains(&w));
        }
    }
}

/// A book of flat trades (zero net, zero fees) is a zero-return period: nothing earned, no
/// drawdown, no win, and no profit factor to report (no loss to divide by).
#[test]
fn a_flat_book_is_a_zero_return_period() {
    let t0 = datetime!(2026-02-02 10:00 UTC);
    let trades: Vec<LabTrade> = (0..5)
        .map(|i| LabTrade {
            id: Uuid::from_u128(i + 1),
            ticker: "ACME".into(),
            asset_class: "stock".into(),
            side: "long".into(),
            exit_at: t0 + Duration::days(i as i64),
            entry_local: None,
            exit_local: t0 + Duration::days(i as i64),
            qty: 1.0,
            entry_price: Some(100.0),
            net: 0.0,
            fees: 0.0,
            risk: None,
            fx: 1.0,
            regime: None,
            mae_pct: None,
            mfe_pct: None,
        })
        .collect();
    let s = summarize(&trades, &as_is(&trades));
    assert_eq!(s.net, 0.0);
    assert_eq!(s.max_drawdown, 0.0);
    assert_eq!(s.win_rate, Some(0.0));
    assert_eq!(s.expectancy, Some(0.0));
    assert_eq!(s.profit_factor, None);
    assert_eq!(s.curve, vec![0.0; 5]);
}
