//! Event invariants (spec Appendix B). Used by the conformance suite on every adapter, and
//! by the live pipeline to flag, not silently drop, what a venue gets wrong.

use otw_connector_model::{BookLevel, Candle, Envelope, Event, HistoryItems, Quote, Trade};

use crate::candles::bucket_start;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule: &'static str,
    pub detail: String,
}

fn v(rule: &'static str, detail: impl Into<String>) -> Violation {
    Violation { rule, detail: detail.into() }
}

pub fn candle(c: &Candle) -> Vec<Violation> {
    let mut out = Vec::new();
    if c.low > c.open.min(c.close) || c.high < c.open.max(c.close) || c.low > c.high {
        out.push(v("candle.ohlc", format!("{} o{} h{} l{} c{}", c.open_ts, c.open, c.high, c.low, c.close)));
    }
    if c.volume.is_sign_negative() {
        out.push(v("candle.volume", format!("{} volume {}", c.open_ts, c.volume)));
    }
    if c.tf.fixed_secs().is_some() && !c.tf.is_session_shaped() && bucket_start(c.tf, c.open_ts) != Some(c.open_ts) {
        out.push(v("candle.aligned", format!("{} is not on a {} boundary", c.open_ts, c.tf)));
    }
    out
}

pub fn quote(q: &Quote) -> Vec<Violation> {
    let mut out = Vec::new();
    if q.bid > q.ask {
        out.push(v("quote.crossed", format!("bid {} > ask {}", q.bid, q.ask)));
    }
    if q.bid_qty.is_some_and(|x| x.is_sign_negative()) || q.ask_qty.is_some_and(|x| x.is_sign_negative()) {
        out.push(v("quote.qty", "negative size"));
    }
    out
}

pub fn trade(t: &Trade) -> Vec<Violation> {
    let mut out = Vec::new();
    if t.price.is_zero() || t.price.is_sign_negative() {
        out.push(v("trade.price", format!("price {}", t.price)));
    }
    if t.qty.is_zero() || t.qty.is_sign_negative() {
        out.push(v("trade.qty", format!("qty {}", t.qty)));
    }
    out
}

pub fn book_snapshot(bids: &[BookLevel], asks: &[BookLevel]) -> Vec<Violation> {
    let mut out = Vec::new();
    if bids.windows(2).any(|w| w[0].price <= w[1].price) {
        out.push(v("book.bids_order", "bids not strictly descending"));
    }
    if asks.windows(2).any(|w| w[0].price >= w[1].price) {
        out.push(v("book.asks_order", "asks not strictly ascending"));
    }
    if bids.iter().chain(asks).any(|l| l.qty.is_zero()) {
        out.push(v("book.zero_qty", "zero quantity in a snapshot"));
    }
    if let (Some(b), Some(a)) = (bids.first(), asks.first()) {
        if b.price >= a.price {
            out.push(v("book.crossed", format!("best bid {} >= best ask {}", b.price, a.price)));
        }
    }
    out
}

/// A history answer: each candle sound, sorted, no duplicate open time.
pub fn history(items: &HistoryItems) -> Vec<Violation> {
    let mut out = Vec::new();
    match items {
        HistoryItems::Candles(cs) => {
            for c in cs {
                out.extend(candle(c));
            }
            if cs.windows(2).any(|w| w[0].open_ts >= w[1].open_ts) {
                out.push(v("history.order", "candles not strictly increasing in time"));
            }
        }
        HistoryItems::Trades(ts) => {
            for t in ts {
                out.extend(trade(t));
            }
        }
    }
    out
}

/// Live events: the venue's stamp within five minutes of when we received it.
pub fn envelope(e: &Envelope) -> Vec<Violation> {
    let mut out = match &e.event {
        Event::Candle(c) => candle(c),
        Event::Quote(q) => quote(q),
        Event::Trade(t) => trade(t),
        Event::BookSnapshot { bids, asks, .. } => book_snapshot(bids, asks),
        Event::HistoryChunk { items, .. } => history(items),
        _ => Vec::new(),
    };
    let live = matches!(e.event, Event::Trade(_) | Event::Quote(_) | Event::BookDelta { .. } | Event::BookSnapshot { .. });
    if let (true, Some(x)) = (live, e.exchange_ts) {
        let skew = (x.nanos() - e.received_ts.nanos()).abs();
        if skew > 300_000_000_000 {
            out.push(v("time.skew", format!("exchange {} vs received {}", x, e.received_ts)));
        }
    }
    out
}
