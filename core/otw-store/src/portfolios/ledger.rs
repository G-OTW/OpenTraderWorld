//! The position engine: units, lots, cost basis and realized result per asset.
//!
//! Pure. The caller converts every amount into the portfolio's currency at its own date and
//! hands the rows over oldest first; nothing here reads a database or a rate. The book, the
//! history rebuild and the lots view all walk the ledger through [`Ledger::apply`], so a
//! split or a transfer means the same thing on every screen.
//!
//! - **Method** decides which lot a sale consumes and at what cost. `Average` keeps every
//!   open lot at the pool's average cost (the book's convention and the French PMP); `Fifo`
//!   consumes the oldest lot at its own cost; `UkPool` is the section 104 pool, which is an
//!   average for the lots left open.
//! - **A split** multiplies every lot's units by the ratio and leaves its cost alone, so the
//!   cost per unit divides by the same ratio and nothing is realized.
//! - **A spin-off** moves a share of each parent lot's cost to the child, with the parent
//!   lot's acquisition date, and changes no parent unit.
//! - **A transfer** removes or adds lots with their cost and their date. Nothing is realized.

use std::collections::HashMap;

use serde::Serialize;
use time::Date;
use uuid::Uuid;

/// Quantities below this are rounding dust, not a position.
const EPS: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Average,
    Fifo,
    UkPool,
}

impl Method {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "average" => Some(Self::Average),
            "fifo" => Some(Self::Fifo),
            "uk_pool" => Some(Self::UkPool),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Average => "average",
            Self::Fifo => "fifo",
            Self::UkPool => "uk_pool",
        }
    }
    fn pooled(self) -> bool {
        !matches!(self, Self::Fifo)
    }
}

/// One open acquisition.
#[derive(Debug, Clone, Serialize)]
pub struct Lot {
    /// The day the units were first acquired. A transferred or spun-off lot keeps its
    /// origin's date: that is what its holding age is measured from.
    #[serde(serialize_with = "iso")]
    pub opened: Date,
    pub qty: f64,
    /// Total cost of the units still open, portfolio currency.
    pub cost: f64,
}

fn iso<S: serde::Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&d.to_string())
}

/// One asset's running position.
#[derive(Debug, Clone, Default)]
pub struct Position {
    pub lots: Vec<Lot>,
    pub realized: f64,
}

impl Position {
    pub fn qty(&self) -> f64 {
        self.lots.iter().map(|l| l.qty).sum()
    }
    pub fn basis(&self) -> f64 {
        self.lots.iter().map(|l| l.cost).sum()
    }

    /// Every open lot at the pool's average cost.
    fn reaverage(&mut self) {
        let (q, c) = (self.qty(), self.basis());
        if q > EPS {
            for l in &mut self.lots {
                l.cost = l.qty * c / q;
            }
        }
    }

    fn add(&mut self, lot: Lot, method: Method) {
        if lot.qty <= EPS {
            return;
        }
        self.lots.push(lot);
        self.lots.sort_by_key(|l| l.opened);
        if method.pooled() {
            self.reaverage();
        }
    }

    /// Take `qty` units out, oldest lot first, and return what was taken. Never more than
    /// is held: a sale past the position is a missing buy, and the extra is dropped.
    fn take(&mut self, qty: f64) -> Vec<Lot> {
        let mut left = qty;
        let mut out = Vec::new();
        for l in self.lots.iter_mut() {
            if left <= EPS {
                break;
            }
            let q = l.qty.min(left);
            let cost = if l.qty > EPS { l.cost * q / l.qty } else { 0.0 };
            out.push(Lot { opened: l.opened, qty: q, cost });
            l.qty -= q;
            l.cost -= cost;
            left -= q;
        }
        self.lots.retain(|l| l.qty > EPS);
        out
    }

    /// The lots a removal of `qty` would take, without taking them.
    pub fn peek(&self, qty: f64) -> Vec<Lot> {
        self.clone().take(qty)
    }
}

/// One ledger row that moves units, amounts already in the portfolio's currency.
#[derive(Debug, Clone)]
pub struct LedgerOp {
    pub asset_id: Uuid,
    pub side: String,
    pub date: Date,
    pub qty: f64,
    /// `quantity × price` in the portfolio currency. On a transfer it is the carried cost.
    pub gross: f64,
    pub fee: f64,
    /// New units per old unit (split and reverse split).
    pub ratio: Option<f64>,
    /// Spin-off: the child asset, its units, and the share of cost (0..1) it takes.
    pub child: Option<(Uuid, f64, f64)>,
    /// Transfer in: the original acquisition day.
    pub lot_date: Option<Date>,
}

/// Kinds the engine reads. Everything else moves cash only.
pub fn moves_units(side: &str) -> bool {
    matches!(side, "buy" | "sell" | "split" | "reverse_split" | "spin_off" | "transfer_out" | "transfer_in")
}

/// Every asset's position, walked under one method.
#[derive(Debug, Clone)]
pub struct Ledger {
    pub method: Method,
    pub positions: HashMap<Uuid, Position>,
}

impl Ledger {
    pub fn new(method: Method) -> Self {
        Self { method, positions: HashMap::new() }
    }

    pub fn position(&self, id: &Uuid) -> Option<&Position> {
        self.positions.get(id)
    }

    /// Apply one row. Returns the realized result it booked (zero for everything but a sale).
    pub fn apply(&mut self, op: &LedgerOp) -> f64 {
        let method = self.method;
        match op.side.as_str() {
            "buy" => {
                let p = self.positions.entry(op.asset_id).or_default();
                p.add(Lot { opened: op.date, qty: op.qty, cost: op.gross + op.fee }, method);
                0.0
            }
            "sell" => {
                let p = self.positions.entry(op.asset_id).or_default();
                let taken = p.take(op.qty);
                let cost: f64 = taken.iter().map(|l| l.cost).sum();
                let r = (op.gross - op.fee) - cost;
                p.realized += r;
                r
            }
            "split" | "reverse_split" => {
                if let Some(ratio) = op.ratio.filter(|r| *r > 0.0) {
                    if let Some(p) = self.positions.get_mut(&op.asset_id) {
                        for l in &mut p.lots {
                            l.qty *= ratio;
                        }
                    }
                }
                0.0
            }
            "spin_off" => {
                let Some((child, child_qty, share)) = op.child else { return 0.0 };
                let share = share.clamp(0.0, 1.0);
                let Some(parent) = self.positions.get_mut(&op.asset_id) else { return 0.0 };
                let parent_qty = parent.qty();
                if parent_qty <= EPS {
                    return 0.0;
                }
                let mut moved = Vec::with_capacity(parent.lots.len());
                for l in &mut parent.lots {
                    let cost = l.cost * share;
                    l.cost -= cost;
                    moved.push(Lot { opened: l.opened, qty: child_qty * l.qty / parent_qty, cost });
                }
                let c = self.positions.entry(child).or_default();
                for lot in moved {
                    c.add(lot, method);
                }
                0.0
            }
            "transfer_out" => {
                if let Some(p) = self.positions.get_mut(&op.asset_id) {
                    p.take(op.qty);
                }
                0.0
            }
            "transfer_in" => {
                let p = self.positions.entry(op.asset_id).or_default();
                p.add(Lot { opened: op.lot_date.unwrap_or(op.date), qty: op.qty, cost: op.gross }, method);
                0.0
            }
            _ => 0.0,
        }
    }
}

/// Units held on each day an operation touched, for the oversell check: a sale is refused
/// when it would take any later day below zero. Splits multiply, so this cannot be a sum.
pub fn min_qty_from(ops: &[LedgerOp], asset: Uuid, from: Date) -> f64 {
    let mut sorted: Vec<&LedgerOp> = ops.iter().collect();
    sorted.sort_by_key(|o| o.date);
    let mut held = 0.0_f64;
    let mut worst = f64::INFINITY;
    let mut i = 0;
    while i < sorted.len() {
        let day = sorted[i].date;
        while i < sorted.len() && sorted[i].date == day {
            let o = sorted[i];
            match o.side.as_str() {
                "buy" | "transfer_in" if o.asset_id == asset => held += o.qty,
                "sell" | "transfer_out" if o.asset_id == asset => held -= o.qty,
                "split" | "reverse_split" if o.asset_id == asset => held *= o.ratio.unwrap_or(1.0),
                "spin_off" if o.child.is_some_and(|c| c.0 == asset) => {
                    held += o.child.map(|c| c.1).unwrap_or(0.0)
                }
                _ => {}
            }
            i += 1;
        }
        if day >= from {
            worst = worst.min(held);
        }
    }
    if worst.is_finite() { worst } else { 0.0 }
}

/// Units of `asset` held at the close of `day`.
pub fn qty_on(ops: &[LedgerOp], asset: Uuid, day: Date) -> f64 {
    let mut l = Ledger::new(Method::Fifo);
    let mut sorted: Vec<&LedgerOp> = ops.iter().filter(|o| o.date <= day).collect();
    sorted.sort_by_key(|o| o.date);
    for o in sorted {
        l.apply(o);
    }
    l.position(&asset).map(|p| p.qty()).unwrap_or(0.0)
}

/// Days a lot opened on `opened` has been held on `today`.
pub fn holding_days(opened: Date, today: Date) -> i64 {
    (today - opened).whole_days()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn d(m: Month, day: u8) -> Date {
        Date::from_calendar_date(2026, m, day).unwrap()
    }
    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }
    fn op(side: &str, asset: Uuid, date: Date, qty: f64, gross: f64) -> LedgerOp {
        LedgerOp { asset_id: asset, side: side.into(), date, qty, gross, fee: 0.0, ratio: None, child: None, lot_date: None }
    }
    fn split(asset: Uuid, date: Date, ratio: f64) -> LedgerOp {
        LedgerOp { ratio: Some(ratio), ..op(if ratio >= 1.0 { "split" } else { "reverse_split" }, asset, date, 0.0, 0.0) }
    }

    #[test]
    fn a_split_halves_cost_per_unit_and_doubles_units() {
        let a = id(1);
        let mut l = Ledger::new(Method::Fifo);
        l.apply(&op("buy", a, d(Month::January, 2), 10.0, 1000.0));
        l.apply(&split(a, d(Month::March, 1), 2.0));
        let p = l.position(&a).unwrap();
        assert!((p.qty() - 20.0).abs() < 1e-9);
        assert!((p.basis() - 1000.0).abs() < 1e-9);
        assert!((p.basis() / p.qty() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_reverse_split_divides_units_and_keeps_cost() {
        let a = id(1);
        let mut l = Ledger::new(Method::Average);
        l.apply(&op("buy", a, d(Month::January, 2), 100.0, 500.0));
        l.apply(&split(a, d(Month::March, 1), 0.1));
        let p = l.position(&a).unwrap();
        assert!((p.qty() - 10.0).abs() < 1e-9);
        assert!((p.basis() - 500.0).abs() < 1e-9);
    }

    /// Realized P/L is the same whether the sale is booked in pre-split or post-split units.
    #[test]
    fn realized_is_invariant_under_a_split() {
        let a = id(1);
        let mut before = Ledger::new(Method::Fifo);
        before.apply(&op("buy", a, d(Month::January, 2), 10.0, 1000.0));
        let r1 = before.apply(&op("sell", a, d(Month::April, 1), 5.0, 750.0));

        let mut after = Ledger::new(Method::Fifo);
        after.apply(&op("buy", a, d(Month::January, 2), 10.0, 1000.0));
        after.apply(&split(a, d(Month::March, 1), 2.0));
        let r2 = after.apply(&op("sell", a, d(Month::April, 1), 10.0, 750.0));
        assert!((r1 - 250.0).abs() < 1e-9);
        assert!((r1 - r2).abs() < 1e-9);
        assert!((before.position(&a).unwrap().basis() - after.position(&a).unwrap().basis()).abs() < 1e-9);
    }

    #[test]
    fn a_spin_off_moves_its_share_of_cost_and_keeps_parent_units() {
        let (parent, child) = (id(1), id(2));
        let mut l = Ledger::new(Method::Fifo);
        l.apply(&op("buy", parent, d(Month::January, 2), 10.0, 1000.0));
        let mut s = op("spin_off", parent, d(Month::June, 1), 0.0, 0.0);
        s.child = Some((child, 5.0, 0.2));
        l.apply(&s);
        let p = l.position(&parent).unwrap();
        let c = l.position(&child).unwrap();
        assert!((p.qty() - 10.0).abs() < 1e-9);
        assert!((p.basis() - 800.0).abs() < 1e-9);
        assert!((c.qty() - 5.0).abs() < 1e-9);
        assert!((c.basis() - 200.0).abs() < 1e-9);
        assert_eq!(c.lots[0].opened, d(Month::January, 2), "the child keeps the parent's date");
        assert!((p.basis() + c.basis() - 1000.0).abs() < 1e-9, "no cost is created or lost");
    }

    #[test]
    fn fifo_consumes_the_oldest_lots_across_partial_sells() {
        let a = id(1);
        let mut l = Ledger::new(Method::Fifo);
        l.apply(&op("buy", a, d(Month::January, 2), 10.0, 100.0));
        l.apply(&op("buy", a, d(Month::February, 2), 10.0, 200.0));
        let r1 = l.apply(&op("sell", a, d(Month::March, 2), 6.0, 120.0));
        assert!((r1 - (120.0 - 60.0)).abs() < 1e-9);
        let r2 = l.apply(&op("sell", a, d(Month::April, 2), 6.0, 150.0));
        // 4 left of lot 1 at 10, then 2 of lot 2 at 20.
        assert!((r2 - (150.0 - 40.0 - 40.0)).abs() < 1e-9);
        let p = l.position(&a).unwrap();
        assert_eq!(p.lots.len(), 1);
        assert_eq!(p.lots[0].opened, d(Month::February, 2));
        assert!((p.lots[0].qty - 8.0).abs() < 1e-9);
        assert!((p.lots[0].cost - 160.0).abs() < 1e-9);
    }

    #[test]
    fn average_prices_every_lot_at_the_pool_cost() {
        let a = id(1);
        let mut l = Ledger::new(Method::Average);
        l.apply(&op("buy", a, d(Month::January, 2), 10.0, 100.0));
        l.apply(&op("buy", a, d(Month::February, 2), 10.0, 200.0));
        let r = l.apply(&op("sell", a, d(Month::March, 2), 5.0, 100.0));
        assert!((r - (100.0 - 75.0)).abs() < 1e-9);
        let p = l.position(&a).unwrap();
        for lot in &p.lots {
            assert!((lot.cost / lot.qty - 15.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_transfer_moves_lots_intact_and_realizes_nothing() {
        let a = id(1);
        let b = id(9);
        let mut src = Ledger::new(Method::Fifo);
        src.apply(&op("buy", a, d(Month::January, 2), 10.0, 100.0));
        src.apply(&op("buy", a, d(Month::February, 2), 10.0, 300.0));
        let moved = src.position(&a).unwrap().peek(15.0);
        let carried: f64 = moved.iter().map(|l| l.cost).sum();
        let mut out = op("transfer_out", a, d(Month::March, 1), 15.0, carried);
        out.fee = 0.0;
        src.apply(&out);

        let mut dst = Ledger::new(Method::Fifo);
        for lot in &moved {
            let mut i = op("transfer_in", b, d(Month::March, 1), lot.qty, lot.cost);
            i.lot_date = Some(lot.opened);
            dst.apply(&i);
        }
        let (s, t) = (src.position(&a).unwrap(), dst.position(&b).unwrap());
        assert_eq!(s.realized, 0.0);
        assert_eq!(t.realized, 0.0);
        assert!((s.qty() + t.qty() - 20.0).abs() < 1e-9, "units are conserved");
        assert!((s.basis() + t.basis() - 400.0).abs() < 1e-9, "cost is conserved");
        assert_eq!(t.lots[0].opened, d(Month::January, 2), "a moved lot keeps its date");
        assert_eq!(t.lots.len(), 2);
    }

    #[test]
    fn the_oversell_floor_follows_splits() {
        let a = id(1);
        let ops = vec![
            op("buy", a, d(Month::January, 2), 10.0, 100.0),
            split(a, d(Month::February, 1), 3.0),
            op("sell", a, d(Month::March, 1), 25.0, 0.0),
        ];
        assert!((min_qty_from(&ops, a, d(Month::March, 1)) - 5.0).abs() < 1e-9);
        let mut over = ops.clone();
        over.push(op("sell", a, d(Month::March, 2), 6.0, 0.0));
        assert!(min_qty_from(&over, a, d(Month::March, 2)) < 0.0);
    }
}
