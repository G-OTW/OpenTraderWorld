//! Candle folding: smaller candles into larger ones, trades into candles.
//!
//! Buckets are UTC: intraday by epoch, a day at UTC midnight, a week on Monday, a month on
//! the 1st. A venue whose day opens elsewhere (an exchange session) is anchored by OTW's live
//! hub, not here; `bucket_start` refuses nothing, so a caller folding session-shaped bars
//! must know it wants UTC ones.

use otw_connector_model::{Candle, InstrumentId, Money, Price, Qty, TfUnit, Timeframe, Timestamp, Trade};

const DAY: i64 = 86_400;
/// 1970-01-01 was a Thursday; Monday-aligned weeks start 4 days later in epoch terms.
const MONDAY_OFFSET: i64 = 4 * DAY;

/// Open time of the `tf` bucket containing `ts`.
pub fn bucket_start(tf: Timeframe, ts: Timestamp) -> Option<Timestamp> {
    let s = ts.secs();
    match tf.unit {
        TfUnit::Week => {
            let len = 7 * DAY * i64::from(tf.n);
            Timestamp::from_secs((s - MONDAY_OFFSET).div_euclid(len) * len + MONDAY_OFFSET)
        }
        TfUnit::Month => {
            let t = ts.to_offset_datetime()?;
            let months = i64::from(t.year()) * 12 + i64::from(u8::from(t.month())) - 1;
            let n = i64::from(tf.n);
            let start = months.div_euclid(n) * n;
            let year = i32::try_from(start.div_euclid(12)).ok()?;
            let month = time::Month::try_from(u8::try_from(start.rem_euclid(12) + 1).ok()?).ok()?;
            let d = time::Date::from_calendar_date(year, month, 1).ok()?;
            Timestamp::from_offset_datetime(d.midnight().assume_utc())
        }
        _ => {
            let len = tf.fixed_secs()?;
            Timestamp::from_secs(s.div_euclid(len) * len)
        }
    }
}

/// Open time of the bucket after the one opening at `start`.
pub fn bucket_end(tf: Timeframe, start: Timestamp) -> Option<Timestamp> {
    match tf.unit {
        TfUnit::Month => {
            let t = start.to_offset_datetime()?;
            let months = i64::from(t.year()) * 12 + i64::from(u8::from(t.month())) - 1 + i64::from(tf.n);
            let year = i32::try_from(months.div_euclid(12)).ok()?;
            let month = time::Month::try_from(u8::try_from(months.rem_euclid(12) + 1).ok()?).ok()?;
            let d = time::Date::from_calendar_date(year, month, 1).ok()?;
            Timestamp::from_offset_datetime(d.midnight().assume_utc())
        }
        _ => Some(start.plus_secs(tf.fixed_secs()?)),
    }
}

fn sum_opt<T: Copy + std::ops::Add<Output = T>>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        _ => None,
    }
}

fn merge_into(acc: &mut Candle, c: &Candle) {
    acc.high = acc.high.max(c.high);
    acc.low = acc.low.min(c.low);
    acc.close = c.close;
    acc.volume = acc.volume + c.volume;
    acc.quote_volume = sum_opt(acc.quote_volume, c.quote_volume);
    acc.trades = match (acc.trades, c.trades) {
        (Some(x), Some(y)) => Some(x + y),
        _ => None,
    };
    acc.is_final = acc.is_final && c.is_final;
    // Both sides fold like the mid; a bucket missing them anywhere has none.
    acc.sides = match (acc.sides, c.sides) {
        (Some(a), Some(b)) => Some(otw_connector_model::Sides { bid: fold_side(a.bid, b.bid), ask: fold_side(a.ask, b.ask) }),
        _ => None,
    };
}

fn fold_side(a: otw_connector_model::Ohlc, b: otw_connector_model::Ohlc) -> otw_connector_model::Ohlc {
    otw_connector_model::Ohlc { open: a.open, high: a.high.max(b.high), low: a.low.min(b.low), close: b.close }
}

/// Fold sorted candles into `to`-sized ones. A bucket is final when every candle in it is
/// final and the bucket is closed: either a later bucket follows, or its last candle ends
/// exactly where the bucket does.
pub fn aggregate(candles: &[Candle], to: Timeframe) -> Vec<Candle> {
    let mut out: Vec<Candle> = Vec::new();
    let mut last_end: Option<Timestamp> = None;
    for c in candles {
        let Some(start) = bucket_start(to, c.open_ts) else { continue };
        match out.last_mut() {
            Some(acc) if acc.open_ts == start => merge_into(acc, c),
            _ => {
                let mut n = c.clone();
                n.tf = to;
                n.open_ts = start;
                out.push(n);
            }
        }
        last_end = bucket_end(c.tf, c.open_ts);
    }
    if let (Some(last), Some(end)) = (out.last_mut(), last_end) {
        if bucket_end(to, last.open_ts) != Some(end) {
            last.is_final = false;
        }
    }
    out
}

/// [`aggregate`], each bucket stamped as the capabilities say: its start, or its first
/// source candle's own stamp (a session the venue opens at the exchange's midnight).
pub fn aggregate_stamped(candles: &[Candle], to: Timeframe, stamp: otw_connector_model::AggregateStamp) -> Vec<Candle> {
    let mut out = aggregate(candles, to);
    if stamp == otw_connector_model::AggregateStamp::FirstBar {
        // `aggregate` keeps buckets in order and each one starts from its first candle.
        let mut firsts = Vec::with_capacity(out.len());
        let mut last: Option<Timestamp> = None;
        for c in candles {
            let b = bucket_start(to, c.open_ts);
            if b != last {
                firsts.push(c.open_ts);
                last = b;
            }
        }
        for (o, first) in out.iter_mut().zip(firsts) {
            o.open_ts = first;
        }
    }
    out
}

/// Trades into candles, live. Emits the forming candle on every trade and the closed one
/// when the clock passes the bucket end plus a grace window for late prints. A trade that
/// arrives for the bucket just closed corrects it (re-emitted, still final); anything older
/// is counted and dropped.
pub struct TradeFolder {
    pub instrument: InstrumentId,
    pub tf: Timeframe,
    pub grace_ns: i64,
    forming: Option<Candle>,
    closed: Option<Candle>,
    pub dropped_late: u64,
}

impl TradeFolder {
    pub fn new(instrument: InstrumentId, tf: Timeframe) -> Self {
        TradeFolder { instrument, tf, grace_ns: 2_000_000_000, forming: None, closed: None, dropped_late: 0 }
    }

    fn fresh(&self, start: Timestamp, price: Price, qty: Qty) -> Candle {
        Candle {
            instrument: self.instrument.clone(),
            tf: self.tf,
            open_ts: start,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: qty,
            quote_volume: Some(price * qty),
            trades: Some(1),
            is_final: false,
            adjusted: None,
            sides: None,
        }
    }

    fn apply(c: &mut Candle, price: Price, qty: Qty) {
        c.high = c.high.max(price);
        c.low = c.low.min(price);
        c.close = price;
        c.volume = c.volume + qty;
        c.quote_volume = c.quote_volume.map(|q| q + price * qty);
        c.trades = c.trades.map(|n| n + 1);
    }

    /// Feed one trade stamped `ts` (exchange time). Returns the candles to publish.
    pub fn on_trade(&mut self, t: &Trade, ts: Timestamp) -> Vec<Candle> {
        let Some(start) = bucket_start(self.tf, ts) else { return vec![] };
        let mut out = Vec::new();
        if let Some(closed) = self.closed.as_mut() {
            if closed.open_ts == start {
                Self::apply(closed, t.price, t.qty);
                out.push(closed.clone());
                return out;
            }
        }
        match self.forming.as_mut() {
            Some(f) if f.open_ts == start => {
                Self::apply(f, t.price, t.qty);
                out.push(f.clone());
            }
            Some(f) if start < f.open_ts => {
                self.dropped_late += 1;
            }
            _ => {
                if let Some(mut prev) = self.forming.take() {
                    prev.is_final = true;
                    out.push(prev.clone());
                    self.closed = Some(prev);
                }
                let c = self.fresh(start, t.price, t.qty);
                out.push(c.clone());
                self.forming = Some(c);
            }
        }
        out
    }

    /// Close the forming candle once `now` is past its end plus the grace window.
    pub fn on_timer(&mut self, now: Timestamp) -> Vec<Candle> {
        let Some(f) = &self.forming else { return vec![] };
        let Some(end) = bucket_end(self.tf, f.open_ts) else { return vec![] };
        if now < end.plus_nanos(self.grace_ns) {
            return vec![];
        }
        let mut prev = f.clone();
        prev.is_final = true;
        self.forming = None;
        self.closed = Some(prev.clone());
        vec![prev]
    }
}

/// Mid of a quote, for candles folded from quotes (`DerivedFrom::Mid`).
pub fn mid(bid: Price, ask: Price) -> Price {
    Price((bid.0 + ask.0) / otw_connector_model::Decimal::TWO)
}

/// Zero helpers, so a caller does not reach into the newtypes.
pub fn zero_volume() -> (Qty, Option<Money>) {
    (Qty::ZERO, Some(Money::ZERO))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_model::Decimal;
    use proptest::prelude::*;

    fn inst() -> InstrumentId {
        InstrumentId::new("T", "X")
    }

    fn p(v: i64) -> Price {
        Price(Decimal::from(v))
    }

    fn q(v: i64) -> Qty {
        Qty(Decimal::from(v))
    }

    fn ts(s: i64) -> Timestamp {
        Timestamp::from_secs(s).unwrap()
    }

    #[test]
    fn buckets_are_utc_and_weeks_start_monday() {
        // 2024-01-03 (a Wednesday) 12:34:56 UTC.
        let t = ts(1_704_285_296);
        assert_eq!(bucket_start(Timeframe::H1, t), Some(ts(1_704_283_200)));
        assert_eq!(bucket_start(Timeframe::D1, t), Some(ts(1_704_240_000)));
        // Monday 2024-01-01.
        assert_eq!(bucket_start(Timeframe::W1, t), Some(ts(1_704_067_200)));
        assert_eq!(bucket_start(Timeframe::MO1, t), Some(ts(1_704_067_200)));
        // Feb 2024 ends Mar 1st (leap year).
        let feb = ts(1_706_745_600);
        assert_eq!(bucket_end(Timeframe::MO1, feb), Some(ts(1_709_251_200)));
    }

    fn trade(price: i64, qty: i64) -> Trade {
        Trade { instrument: inst(), id: None, price: p(price), qty: q(qty), aggressor: None }
    }

    #[test]
    fn trade_folder_closes_on_boundary_and_corrects_a_late_print() {
        let mut f = TradeFolder::new(inst(), Timeframe::M1);
        f.on_trade(&trade(10, 1), ts(60));
        f.on_trade(&trade(12, 1), ts(90));
        assert!(f.on_timer(ts(121)).is_empty(), "inside the grace window");
        let closed = f.on_timer(ts(123));
        assert_eq!(closed.len(), 1);
        assert!(closed[0].is_final);
        assert_eq!((closed[0].open, closed[0].high, closed[0].close), (p(10), p(12), p(12)));
        let corrected = f.on_trade(&trade(9, 1), ts(119));
        assert_eq!(corrected[0].low, p(9));
        assert!(corrected[0].is_final);
        f.on_trade(&trade(11, 1), ts(130));
        f.on_trade(&trade(1, 1), ts(10));
        assert_eq!(f.dropped_late, 1);
    }

    fn fold_trades(trades: &[(i64, i64, i64)], tf: Timeframe) -> Vec<Candle> {
        let mut f = TradeFolder::new(inst(), tf);
        let mut out: Vec<Candle> = Vec::new();
        for (t, price, qty) in trades {
            for c in f.on_trade(&trade(*price, *qty), ts(*t)) {
                match out.iter_mut().find(|x| x.open_ts == c.open_ts) {
                    Some(x) => *x = c,
                    None => out.push(c),
                }
            }
        }
        for c in f.on_timer(ts(i64::from(u32::MAX))) {
            if let Some(x) = out.iter_mut().find(|x| x.open_ts == c.open_ts) {
                *x = c;
            }
        }
        out
    }

    proptest! {
        /// Trades folded to 1m then aggregated to 5m equal the same trades folded to 5m.
        #[test]
        fn aggregation_matches_direct_folding(raw in proptest::collection::vec((0i64..3_600, 1i64..1_000, 1i64..50), 1..200)) {
            let mut trades = raw.clone();
            trades.sort_by_key(|t| t.0);
            let via_1m = aggregate(&fold_trades(&trades, Timeframe::M1), Timeframe::M5);
            let direct = fold_trades(&trades, Timeframe::M5);
            prop_assert_eq!(via_1m.len(), direct.len());
            for (a, b) in via_1m.iter().zip(direct.iter()) {
                prop_assert_eq!((a.open_ts, a.open, a.high, a.low, a.close, a.volume, a.trades), (b.open_ts, b.open, b.high, b.low, b.close, b.volume, b.trades));
            }
        }

        /// Aggregating in two steps equals aggregating in one.
        #[test]
        fn aggregation_is_associative(raw in proptest::collection::vec((0i64..86_400, 1i64..1_000, 1i64..50), 1..300)) {
            let mut trades = raw.clone();
            trades.sort_by_key(|t| t.0);
            let m1 = fold_trades(&trades, Timeframe::M1);
            let two = aggregate(&aggregate(&m1, Timeframe::M15), Timeframe::H1);
            let one = aggregate(&m1, Timeframe::H1);
            prop_assert_eq!(two, one);
        }
    }
}
