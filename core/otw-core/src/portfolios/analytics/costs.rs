//! `costs` — income collected, cost paid, and what the cost was worth.
//!
//! "Net performance after fees" is a **counterfactual, not a subtraction**. Fees already sit
//! inside cost basis, proceeds and cash, so the curve is net by construction and subtracting
//! them again would double-count. What the screen owes is the curve the book *would* have
//! had without them, and the gap between the two.

use serde_json::json;

use super::{Analysis, Block, Needs, Sample, Substrate};
use crate::quant;

pub struct Costs;

impl Analysis for Costs {
    fn key(&self) -> &'static str {
        "costs"
    }
    fn needs(&self) -> Needs {
        Needs::BOOK | Needs::CURVE
    }

    fn run(&self, s: &Substrate<'_>) -> Block {
        let Some(b) = s.book.as_ref() else {
            return Block::unavailable(&["no_ledger"]);
        };
        let window = s.curve.as_ref().map(|c| {
            c.slice(s.window.from.map(|d| d.to_string()).as_deref(), s.window.to.map(|d| d.to_string()).as_deref())
        });

        // The drag, and the curve without it. A fee is a return the book did not make, so it
        // is added back at the value it was paid out of, not as a flat percentage.
        let (fee_free_return, drag_bps, avg_net_worth, days) = match window.as_ref() {
            Some(c) if c.len() > 1 => {
                let mut adjusted = Vec::with_capacity(c.rets.len());
                for i in 1..c.net_worth.len() {
                    let prev = c.net_worth[i - 1];
                    // Positive, never `abs()`: the same rule the curve's own returns follow.
                    let give_back = if prev > 1e-9 { c.fees[i] / prev } else { 0.0 };
                    adjusted.push(c.rets[i - 1] + give_back);
                }
                let eq = quant::compound(&adjusted);
                let avg = quant::mean(&c.net_worth);
                let paid: f64 = c.fees.iter().sum();
                let years = c.years().max(1e-9);
                // Annual drag against the average money at work: a fee is only meaningful
                // next to the size of the book that paid it.
                let bps = if avg.abs() > 1e-9 { paid / avg / years * 10_000.0 } else { 0.0 };
                (eq.last().map(|v| v - 1.0), Some(bps), Some(avg), c.len())
            }
            _ => (None, None, None, window.as_ref().map(|c| c.len()).unwrap_or(0)),
        };

        let realized_return = window.as_ref().and_then(|c| c.total_return());
        let mut block = Block::ok(json!({
            "currency": b.currency,
            "income": b.income,
            "costs": b.costs,
            // Lifetime figures from the ledger, and the window's own from the curve.
            "window_fees": window.as_ref().map(|c| c.fees.iter().sum::<f64>() + 0.0),
            "window_income": window.as_ref().map(|c| c.income.iter().sum::<f64>() + 0.0),
            "avg_net_worth": avg_net_worth,
            "drag_bps": drag_bps,
            "net_return_pct": realized_return.map(|r| r * 100.0),
            "gross_return_pct": fee_free_return.map(|r| r * 100.0),
            "fee_cost_pct": match (realized_return, fee_free_return) {
                (Some(net), Some(gross)) => Some((gross - net) * 100.0),
                _ => None,
            },
            "income_pct_of_net_worth": (b.net_worth.abs() > 1e-9)
                .then(|| b.income.total / b.net_worth * 100.0),
        }));
        if days > 0 {
            let sample = Sample::rows(days);
            block = block.with(match window.as_ref() {
                Some(c) if !c.is_empty() => {
                    sample.span(c.dates[0].clone(), c.dates[c.len() - 1].clone())
                }
                _ => sample,
            });
        }
        block
    }
}
