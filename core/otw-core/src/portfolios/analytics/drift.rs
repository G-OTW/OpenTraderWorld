//! `drift` — current allocation against the target, and the trades that close the gap.
//!
//! Display only. Nothing here places an order, and the trade list is a suggestion the user
//! executes at their broker, which is why it is expressed in money and not in a ticket.

use std::collections::BTreeMap;

use serde_json::json;

use super::{Analysis, Block, Needs, Sample, Substrate};

pub struct Drift;

/// The dimension v1 buckets on. `sector`, `region` and `currency` are the same table and the
/// same code with another value here.
const DIMENSION: &str = "asset_class";

impl Analysis for Drift {
    fn key(&self) -> &'static str {
        "drift"
    }
    fn needs(&self) -> Needs {
        Needs::BOOK | Needs::TARGETS
    }

    fn run(&self, s: &Substrate<'_>) -> Block {
        let (Some(b), Some(targets)) = (s.book.as_ref(), s.targets.as_ref()) else {
            return Block::unavailable(&["no_ledger"]);
        };
        let targets: Vec<_> = targets.iter().filter(|t| t.dimension == DIMENSION).collect();
        if targets.is_empty() {
            // No target is not a drift of zero. A book with no plan has nothing to be off.
            return Block::unavailable(&["no_targets"]);
        }
        if b.net_worth.abs() < 1e-9 {
            return Block::unavailable(&["empty_book"]);
        }
        // A negative base flips the sign of every row at once: a bucket holding real money
        // reads as under-allocated, and the rebalancing plan tells the user to sell it.
        if b.net_worth < 0.0 {
            return Block::unavailable(&["negative_equity"]);
        }

        // Cash is a bucket, never an asset row: it comes from the ledger walk, and a
        // synthetic cash asset would pollute the positions table and the donut.
        let mut current: BTreeMap<String, f64> = BTreeMap::new();
        for p in &b.positions {
            if p.quantity > 0.0 {
                if let Some(mv) = p.market_value {
                    *current.entry(p.asset.asset_class.clone()).or_default() += mv;
                }
            }
        }
        if b.cash_total.abs() > 1e-9 {
            *current.entry("cash".into()).or_default() += b.cash_total;
        }

        let base = b.net_worth;
        let mut rows = Vec::new();
        let mut trades = Vec::new();
        let mut worst = 0.0_f64;
        for t in &targets {
            let value = current.remove(&t.bucket).unwrap_or(0.0);
            let pct = value / base * 100.0;
            let deviation = pct - t.target_pct;
            let out_of_band = deviation.abs() > t.band_pct;
            worst = worst.max(deviation.abs());
            let delta_value = (t.target_pct - pct) / 100.0 * base;
            rows.push(json!({
                "bucket": t.bucket,
                "value": value,
                "current_pct": pct,
                "target_pct": t.target_pct,
                "deviation_pct": deviation,
                "band_pct": t.band_pct,
                "out_of_band": out_of_band,
                "delta_value": delta_value,
            }));
            if out_of_band {
                trades.push(json!({
                    "bucket": t.bucket,
                    "side": if delta_value > 0.0 { "buy" } else { "sell" },
                    "amount": delta_value.abs(),
                    // Which lines inside the bucket, pro rata of what is held there. The
                    // bucket is the decision; the split inside it is arithmetic.
                    "legs": bucket_legs(b, &t.bucket, delta_value),
                }));
            }
        }

        // What is held that no target names. Reported as unallocated rather than folded into
        // "other" or silently treated as a zero target, both of which hide a real hole in the
        // plan.
        let unallocated: Vec<_> = current
            .into_iter()
            .filter(|(_, v)| v.abs() > 1e-9)
            .map(|(bucket, value)| json!({
                "bucket": bucket,
                "value": value,
                "current_pct": value / base * 100.0,
            }))
            .collect();

        let total_target: f64 = targets.iter().map(|t| t.target_pct).sum();
        Block::ok(json!({
            "currency": b.currency,
            "dimension": DIMENSION,
            "net_worth": base,
            "rows": rows,
            "unallocated": unallocated,
            "trades": trades,
            "worst_deviation_pct": worst,
            "in_band": trades.is_empty(),
            "target_total_pct": total_target,
        }))
        .with(Sample::rows(targets.len()))
    }
}

/// Split a bucket-level move across the lines that make it up, pro rata of what is held.
///
/// A sell can only come out of what is there; a buy into an empty bucket has no line to name
/// and returns nothing, which is the honest answer to "buy 7% of bonds" when the book holds
/// no bond.
fn bucket_legs(b: &otw_store::portfolios::Book, bucket: &str, delta: f64) -> Vec<serde_json::Value> {
    let held: Vec<_> = b
        .positions
        .iter()
        .filter(|p| p.asset.asset_class == bucket && p.quantity > 0.0)
        .filter_map(|p| p.market_value.map(|mv| (p, mv)))
        .collect();
    let total: f64 = held.iter().map(|(_, mv)| *mv).sum();
    if total <= 1e-9 {
        return Vec::new();
    }
    held.into_iter()
        .map(|(p, mv)| {
            let amount = delta * (mv / total);
            json!({
                "asset_id": p.asset.id,
                "symbol": p.asset.symbol,
                "amount": amount.abs(),
                "side": if amount > 0.0 { "buy" } else { "sell" },
                "units": p.price.filter(|px| *px > 0.0).map(|px| (amount / px).abs()),
            })
        })
        .collect()
}
