//! `book` — what the portfolio is, right now.
//!
//! Needs the ledger and nothing else: no bars, no connector, no history. It is the block a
//! fresh install renders on its first day, and the reason the registry lets an analysis
//! declare what it needs instead of assuming market data is there.

use serde_json::json;

use super::{Analysis, Block, Needs, Sample, Substrate};

pub struct Book;

impl Analysis for Book {
    fn key(&self) -> &'static str {
        "book"
    }
    fn needs(&self) -> Needs {
        Needs::BOOK
    }

    fn run(&self, s: &Substrate<'_>) -> Block {
        let Some(b) = s.book.as_ref() else {
            return Block::unavailable(&["no_ledger"]);
        };

        // A book worth less than nothing has a debt, not an allocation. Every share would be
        // a percentage of a negative base, and taking the absolute value to make it printable
        // is how a margin call renders as a balanced portfolio.
        if b.net_worth < 0.0 {
            return Block::unavailable(&["negative_equity"]);
        }

        // Allocation is over **net worth**, cash included. A book that is 40% cash is not a
        // book whose equities are 100% of it, and the donut that says so is lying about the
        // only thing the user is looking at it for.
        let base = b.net_worth.max(1e-9);
        let mut by_class: std::collections::BTreeMap<&str, f64> = std::collections::BTreeMap::new();
        for p in &b.positions {
            if let Some(mv) = p.market_value {
                if p.quantity > 0.0 {
                    *by_class.entry(p.asset.asset_class.as_str()).or_default() += mv;
                }
            }
        }
        let mut allocation: Vec<_> = by_class
            .into_iter()
            .map(|(class, value)| json!({ "bucket": class, "value": value, "pct": value / base * 100.0 }))
            .collect();
        if b.cash_total.abs() > 1e-9 {
            allocation.push(json!({
                "bucket": "cash",
                "value": b.cash_total,
                "pct": b.cash_total / base * 100.0,
            }));
        }

        let open = b.positions.iter().filter(|p| p.quantity > 0.0).count();
        Block::ok(json!({
            "currency": b.currency,
            "net_worth": b.net_worth,
            "invested": b.market_value,
            "cost_basis": b.cost_basis,
            "cash_total": b.cash_total,
            "cash": b.cash,
            // False means the ledger has no deposit and no withdrawal, so there is no cash
            // account and no invested/cash split to draw. Said out loud rather than shown as
            // a book that is 100% deployed, which is a different claim.
            "cash_tracked": b.cash_tracked,
            // What share of the book is not deployed. The roadmap's "invested vs cash" is
            // this one number, and it only exists because the ledger learned about cash.
            "cash_pct": b.cash_total / base * 100.0,
            "unrealized": b.unrealized,
            "realized": b.realized,
            "income": b.income,
            "open_positions": open,
            "unpriced": b.unpriced,
            "allocation": allocation,
        }))
        .with(Sample::rows(open))
    }
}
