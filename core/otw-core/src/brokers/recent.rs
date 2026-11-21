//! An account's recent activity for the chart's order panel: the orders filled since the
//! start of the user's day, and for each open position the order that opened it.
//!
//! Both come from the broker's fills ([`super::Broker::executions`]), grouped by order. A
//! cancelled order that never filled has no fill and is not listed here.
//!
//! **The opening order** is found by walking the instrument's fills back from now, undoing
//! each one from the position the broker holds: the fill before which the position was flat
//! (or on the other side) opened it. When the fills run out first, the position was opened
//! before the window the broker answers and the opening is unknown, never guessed.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use time::OffsetDateTime;

use super::{Execution, Position};

/// A quantity this small is flat: venues report quantities as floats.
const EPS: f64 = 1e-9;

/// One order as its fills tell it: the sum of their sizes and fees, the size-weighted price,
/// the time of the first one.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FilledOrder {
    /// The broker's order id; empty when it reports fills without one (each fill is then its
    /// own row).
    pub order_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub symbol: String,
    /// "buy" | "sell".
    pub side: String,
    pub qty: f64,
    pub price: f64,
    pub fee: f64,
    pub fee_currency: String,
    pub currency: String,
    /// Sub-account id, when the broker has several.
    pub account: String,
}

/// The order that opened an open position.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Opening {
    pub symbol: String,
    /// "long" | "short", the position's.
    pub side: String,
    pub account: String,
    /// `None` when the position was opened before the fills the broker answers.
    pub order: Option<FilledOrder>,
}

/// The broker's spelling without punctuation or case: `BTC-USD` is `BTCUSD`.
pub fn bare(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase()
}

/// Whether a fill belongs to a position: same instrument, and same sub-account where both
/// name one.
fn of_position(f: &Execution, p: &Position) -> bool {
    bare(&f.symbol) == bare(&p.symbol) && (f.account.is_empty() || p.account.is_empty() || f.account == p.account)
}

/// Fills merged into their orders, newest first.
pub fn orders_of(fills: &[Execution]) -> Vec<FilledOrder> {
    let mut by: Vec<(String, Vec<&Execution>)> = Vec::new();
    for f in fills {
        let key = if f.order_id.is_empty() {
            format!("fill:{}", f.id)
        } else {
            format!("{}|{}|{}", f.account, f.symbol, f.order_id)
        };
        match by.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(f),
            None => by.push((key, vec![f])),
        }
    }
    let mut out: Vec<FilledOrder> = by
        .into_iter()
        .map(|(_, v)| {
            let qty: f64 = v.iter().map(|f| f.qty).sum();
            let notional: f64 = v.iter().map(|f| f.qty * f.price).sum();
            let first = v.iter().min_by_key(|f| f.at).copied().unwrap_or(v[0]);
            FilledOrder {
                order_id: first.order_id.clone(),
                at: first.at,
                symbol: first.symbol.clone(),
                side: first.side.clone(),
                qty,
                price: if qty > EPS { notional / qty } else { first.price },
                fee: v.iter().map(|f| f.fee).sum(),
                fee_currency: first.fee_currency.clone(),
                currency: first.currency.clone(),
                account: first.account.clone(),
            }
        })
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}

/// The fill that opened `p`, walking its instrument's fills back from now. `None` when they
/// run out with the position still open.
pub fn opening_fill<'a>(p: &Position, fills: &'a [Execution]) -> Option<&'a Execution> {
    let dir = if p.side == "short" { -1.0 } else { 1.0 };
    let mut mine: Vec<&Execution> = fills.iter().filter(|f| of_position(f, p)).collect();
    mine.sort_by(|a, b| b.at.cmp(&a.at));
    // The position in its own direction: positive while it is open.
    let mut pos = p.qty.abs();
    for f in mine {
        let signed = if f.side == "buy" { f.qty } else { -f.qty } * dir;
        let before = pos - signed;
        if before <= EPS && pos > EPS {
            return Some(f);
        }
        pos = before;
    }
    None
}

/// The whole opening order of `p`: every fill of the order whose fill opened it.
pub fn opening_of(p: &Position, fills: &[Execution]) -> Opening {
    let order = opening_fill(p, fills).map(|f| {
        let same: Vec<Execution> = if f.order_id.is_empty() {
            vec![f.clone()]
        } else {
            fills.iter().filter(|g| g.order_id == f.order_id && g.symbol == f.symbol && g.account == f.account).cloned().collect()
        };
        orders_of(&same).remove(0)
    });
    Opening { symbol: p.symbol.clone(), side: p.side.clone(), account: p.account.clone(), order }
}

/// How long an opening found stays answered without reading the venue again. The key carries
/// the position's size, so a fill that changes it asks again at once.
const OPENING_TTL: Duration = Duration::from_secs(60 * 60);

type OpeningSlot = Mutex<HashMap<String, (Instant, Opening)>>;

fn openings() -> &'static OpeningSlot {
    static S: OnceLock<OpeningSlot> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The cache key of a position's opening: the account, the instrument, its side and size.
pub fn opening_key(account: uuid::Uuid, p: &Position) -> String {
    format!("{account}|{}|{}|{}|{}", p.account, bare(&p.symbol), p.side, p.qty)
}

pub fn cached_opening(key: &str) -> Option<Opening> {
    let mut s = openings().lock().ok()?;
    s.retain(|_, (at, _)| at.elapsed() < OPENING_TTL);
    s.get(key).map(|(_, o)| o.clone())
}

/// Kept only once found: an unknown opening is asked again on the next read.
pub fn keep_opening(key: String, o: &Opening) {
    if o.order.is_none() {
        return;
    }
    if let Ok(mut s) = openings().lock() {
        s.insert(key, (Instant::now(), o.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn fill(id: &str, order: &str, at: OffsetDateTime, side: &str, qty: f64, price: f64) -> Execution {
        Execution {
            id: id.into(),
            order_id: order.into(),
            client_id: String::new(),
            at,
            symbol: "BTCUSDT".into(),
            side: side.into(),
            qty,
            price,
            fee: 0.1,
            fee_currency: "USDT".into(),
            currency: "USDT".into(),
            asset_class: "crypto".into(),
            multiplier: 1.0,
            venue: String::new(),
            account: String::new(),
        }
    }

    fn position(side: &str, qty: f64) -> Position {
        Position {
            symbol: "BTC-USDT".into(),
            side: side.into(),
            qty,
            avg_price: None,
            fill_price: None,
            currency: "USDT".into(),
            asset_class: "crypto".into(),
            multiplier: 1.0,
            venue: String::new(),
            account: String::new(),
            native_id: None,
            market_price: None,
            unrealized_pnl: None,
            opened_at: None,
            entry_fee: None,
            fee_currency: String::new(),
            leverage: None,
            margin_mode: None,
        }
    }

    #[test]
    fn fills_of_one_order_merge_at_their_weighted_price() {
        let fills = [
            fill("1", "A", datetime!(2026-11-10 09:00 UTC), "buy", 1.0, 100.0),
            fill("2", "A", datetime!(2026-11-10 09:01 UTC), "buy", 3.0, 104.0),
            fill("3", "B", datetime!(2026-11-10 10:00 UTC), "sell", 2.0, 110.0),
        ];
        let o = orders_of(&fills);
        assert_eq!(o.len(), 2);
        assert_eq!(o[0].order_id, "B", "newest first");
        assert_eq!(o[1].qty, 4.0);
        assert_eq!(o[1].price, 103.0);
        assert!((o[1].fee - 0.2).abs() < 1e-12);
        assert_eq!(o[1].at, datetime!(2026-11-10 09:00 UTC));
    }

    #[test]
    fn the_opening_is_the_order_after_the_last_flat() {
        // Long 1 then out, then long 2 (order C), added 1 (D), trimmed 1 (E): holds 2.
        let fills = [
            fill("1", "A", datetime!(2026-11-01 09:00 UTC), "buy", 1.0, 90.0),
            fill("2", "B", datetime!(2026-11-02 09:00 UTC), "sell", 1.0, 95.0),
            fill("3", "C", datetime!(2026-11-03 09:00 UTC), "buy", 1.5, 100.0),
            fill("4", "C", datetime!(2026-11-03 09:00:05 UTC), "buy", 0.5, 101.0),
            fill("5", "D", datetime!(2026-11-04 09:00 UTC), "buy", 1.0, 105.0),
            fill("6", "E", datetime!(2026-11-05 09:00 UTC), "sell", 1.0, 108.0),
        ];
        let o = opening_of(&position("long", 2.0), &fills);
        let order = o.order.expect("opened inside the window");
        assert_eq!(order.order_id, "C");
        assert_eq!(order.qty, 2.0);
        assert_eq!(order.at, datetime!(2026-11-03 09:00 UTC));
    }

    #[test]
    fn a_short_opens_on_a_sell_and_a_reversal_counts() {
        // Long 1, then one sell of 3 reverses to short 2.
        let fills = [
            fill("1", "A", datetime!(2026-11-01 09:00 UTC), "buy", 1.0, 90.0),
            fill("2", "B", datetime!(2026-11-02 09:00 UTC), "sell", 3.0, 95.0),
        ];
        let o = opening_of(&position("short", 2.0), &fills);
        assert_eq!(o.order.map(|o| o.order_id).as_deref(), Some("B"));
    }

    #[test]
    fn an_opening_before_the_window_is_unknown() {
        // Holds 3 but the window shows only an add of 1.
        let fills = [fill("1", "A", datetime!(2026-11-01 09:00 UTC), "buy", 1.0, 90.0)];
        assert!(opening_of(&position("long", 3.0), &fills).order.is_none());
    }

    #[test]
    fn another_instrument_never_opens_it() {
        let mut f = fill("1", "A", datetime!(2026-11-01 09:00 UTC), "buy", 1.0, 90.0);
        f.symbol = "ETHUSDT".into();
        assert!(opening_of(&position("long", 1.0), &[f]).order.is_none());
    }
}
