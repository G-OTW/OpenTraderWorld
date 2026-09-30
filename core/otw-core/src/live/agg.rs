//! Bucketing N source updates into one bar of the timeframe the user is looking at.
//!
//! Only two providers stream the exact candle we want (Binance and Kraken publish a channel
//! per interval). Every other live source publishes one grain and one grain only — Coinbase
//! individual trades, Alpaca and Massive one-minute bars, Interactive Brokers five-second
//! bars — so the timeframe on the chart is built here rather than in each connector.
//!
//! Two rules make the result match what the REST history would have stored:
//!
//! - **A bucket is `floor(ts / tf)` on the UTC epoch**, the alignment every exchange candle
//!   already uses and the one [`super::bar_start`] applies, so a live bar and a downloaded
//!   bar for the same period carry the same timestamp and the store's upsert reconciles them.
//!   *Unless the provider says otherwise*: an equity daily bar is a session, stamped at the
//!   exchange's midnight, not the UTC one, and a weekly bar starts on the session's Monday.
//!   [`Bucketer::anchor`] takes that alignment from the provider's own bars (the REST seed,
//!   a backfill) and shifts every bucket onto it, which is what lets a daily or weekly chart
//!   go live at all: without it the fold would invent a second row per period.
//! - **A bucket closes on evidence, not on a clock, whenever there is evidence**: the source
//!   update that completes it (a five-second bar ending on the boundary, a trade belonging to
//!   the next bucket) closes it immediately. The timer in [`Bucketer::tick`] is the fallback
//!   for a market that simply went quiet, because a feed that only publishes on activity
//!   would otherwise leave the last bar of a lull forming forever.
//!
//! Empty buckets are never invented: a period with no trade emits nothing, and the
//! supervisor's gap detection backfills it from REST, where the provider says whether the
//! market was closed or merely still.

use std::time::Duration;

use time::OffsetDateTime;

use super::LiveEvent;

/// Default grace after a bucket's end before the close timer fires. Absorbs both slightly
/// out-of-order trade stamps and the delay a provider takes to publish the bar that closes
/// the period (Massive publishes a minute aggregate a few seconds after the minute).
pub const DEFAULT_GRACE: i64 = 2;

/// How often the close timer is polled. Well under any timeframe we serve, so a bucket
/// closes within a fraction of a second of its grace expiring.
pub const TICK: Duration = Duration::from_millis(500);

/// Folds source updates into `tf_secs` buckets, emitting the forming bar on every update and
/// a closed one at each boundary.
pub struct Bucketer {
    tf_secs: i64,
    /// Seconds after a bucket's end before the timer closes it.
    grace: i64,
    /// Where inside the period a bucket starts, as seconds into the UTC epoch cycle. `0` is
    /// the epoch alignment every intraday feed already uses; a session daily bar sets it to
    /// the exchange midnight the provider stamps its own bars with.
    offset: i64,
    /// The forming bucket, if any.
    cur: Option<LiveEvent>,
    /// Newest bucket already emitted closed, so a late update cannot resurrect it.
    last_closed: Option<OffsetDateTime>,
}

impl Bucketer {
    pub fn new(tf_secs: i64) -> Self {
        Bucketer { tf_secs, grace: DEFAULT_GRACE, offset: 0, cur: None, last_closed: None }
    }

    /// Same, with a longer wait before the timer closes a bucket. Use it for a feed that
    /// publishes its source bar after the period it covers has already ended.
    pub fn with_grace(tf_secs: i64, grace: i64) -> Self {
        Bucketer { tf_secs, grace, offset: 0, cur: None, last_closed: None }
    }

    /// Align every future bucket on `ts`, a bar timestamp the provider itself produced.
    /// Idempotent and cheap, so it is called on each seed and each backfill: a market that
    /// changes its UTC offset (a DST switch between two sessions) re-aligns on the next one.
    pub fn anchor(&mut self, ts: OffsetDateTime) {
        self.offset = ts.unix_timestamp().rem_euclid(self.tf_secs);
    }

    /// Same alignment, taken from a remembered offset instead of a bar. This is what lets a
    /// second daily pane on an instrument the hub already anchored open without spending a
    /// REST request of its own.
    pub fn anchor_at(&mut self, offset: i64) {
        self.offset = offset.rem_euclid(self.tf_secs);
    }

    /// Where this bucketer currently thinks the period starts, as seconds into the
    /// timeframe cycle. Cached by the hub so the anchor outlives the subscription.
    pub fn offset(&self) -> i64 {
        self.offset
    }

    /// Adopt `bar` as the bucket in progress instead of folding it in from nothing. Used for
    /// the period already underway when a subscription starts: the provider's own partial
    /// bar for the current session is the honest open/high/low, where a fold that began
    /// mid-period would report the range since we connected as the range of the day.
    pub fn prime(&mut self, bar: LiveEvent) {
        self.offset = bar.bar_ts.unix_timestamp().rem_euclid(self.tf_secs);
        self.last_closed = Some(bar.bar_ts - time::Duration::seconds(self.tf_secs));
        self.cur = Some(LiveEvent { closed: false, ..bar });
    }

    /// Start of the bucket `ts` belongs to, on this bucketer's alignment.
    fn bucket_of(&self, ts: OffsetDateTime) -> OffsetDateTime {
        super::bar_start(ts - time::Duration::seconds(self.offset), self.tf_secs)
            + time::Duration::seconds(self.offset)
    }

    /// Fold one trade. Volume adds up, the close follows the last price.
    pub fn trade(&mut self, price: f64, size: f64, ts: OffsetDateTime) -> Vec<LiveEvent> {
        self.fold(
            self.bucket_of(ts),
            LiveEvent {
                bar_ts: OffsetDateTime::UNIX_EPOCH, // rewritten by `fold`
                open: price,
                high: price,
                low: price,
                close: price,
                volume: size,
                closed: false,
                provider_ts: ts,
            },
            false,
        )
    }

    /// Fold one source bar covering `src_secs` from `src.bar_ts`.
    ///
    /// `src.closed` says whether the *source* bar is final; the bucket closes when the source
    /// bar reaches its end (an update that lands exactly on the boundary needs no timer) or
    /// when a later bucket arrives. A source bar finer than the timeframe therefore keeps the
    /// bucket forming and only the last one of the period closes it.
    pub fn bar(&mut self, src: LiveEvent, src_secs: i64) -> Vec<LiveEvent> {
        let bucket = self.bucket_of(src.bar_ts);
        // The source bar completes the bucket when its own end reaches the bucket's end.
        let completes = src.closed
            && src.bar_ts.unix_timestamp() + src_secs >= bucket.unix_timestamp() + self.tf_secs;
        self.fold(bucket, src, completes)
    }

    /// Close the forming bucket once its period plus the grace has elapsed, for a feed that
    /// publishes nothing while the market is quiet.
    pub fn tick(&mut self, now: OffsetDateTime) -> Vec<LiveEvent> {
        let Some(cur) = &self.cur else { return Vec::new() };
        let end = cur.bar_ts.unix_timestamp() + self.tf_secs;
        if now.unix_timestamp() < end + self.grace {
            return Vec::new();
        }
        let mut closed = self.cur.take().unwrap();
        closed.closed = true;
        self.last_closed = Some(closed.bar_ts);
        vec![closed]
    }

    /// The one place a bucket is opened, extended or closed. `part` carries the incoming
    /// values (its `bar_ts` is ignored — `bucket` is where it lands); `completes` closes the
    /// bucket right after folding.
    fn fold(&mut self, bucket: OffsetDateTime, part: LiveEvent, completes: bool) -> Vec<LiveEvent> {
        // An update for a bucket we already finalized is a straggler, not a correction: the
        // series has moved on and rewriting a closed candle would make the chart flicker
        // backwards. REST backfill is what fixes a bar that was wrong.
        if self.last_closed.is_some_and(|lc| bucket <= lc) {
            return Vec::new();
        }
        let mut out = Vec::new();
        // A part for a later bucket closes the one in hand first: the previous period is
        // over, whatever the timer thinks.
        if let Some(cur) = &self.cur {
            if bucket > cur.bar_ts {
                let mut done = self.cur.take().unwrap();
                done.closed = true;
                self.last_closed = Some(done.bar_ts);
                out.push(done);
            }
        }
        match &mut self.cur {
            Some(cur) => {
                cur.high = cur.high.max(part.high);
                cur.low = cur.low.min(part.low);
                cur.close = part.close;
                cur.volume += part.volume;
                cur.provider_ts = part.provider_ts;
            }
            None => {
                self.cur = Some(LiveEvent { bar_ts: bucket, closed: false, ..part });
            }
        }
        if completes {
            let mut done = self.cur.take().unwrap();
            done.closed = true;
            self.last_closed = Some(done.bar_ts);
            out.push(done);
        } else {
            out.push(self.cur.clone().unwrap());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    /// A remembered offset aligns a fold exactly like the bar it came from, which is what
    /// lets a daily pane open without spending a REST request to re-learn the session.
    #[test]
    fn a_remembered_offset_aligns_like_the_bar_it_came_from() {
        // A New York session bar: midnight ET, five hours off the UTC epoch boundary.
        let session = datetime!(2026-01-05 05:00:00 UTC);
        let mut from_bar = Bucketer::new(86_400);
        from_bar.anchor(session);
        let mut from_cache = Bucketer::new(86_400);
        from_cache.anchor_at(from_bar.offset());
        assert_eq!(from_bar.offset(), 5 * 3600);
        // Anything inside that session lands on its open, not on the UTC midnight between.
        let midday = datetime!(2026-01-05 17:30:00 UTC);
        assert_eq!(from_cache.bucket_of(midday), session);
        assert_eq!(from_cache.bucket_of(midday), from_bar.bucket_of(midday));
        // And the day before the session opens still belongs to the previous one.
        assert_eq!(
            from_cache.bucket_of(datetime!(2026-01-05 02:00:00 UTC)),
            datetime!(2026-01-04 05:00:00 UTC)
        );
    }

    fn src(ts: OffsetDateTime, o: f64, h: f64, l: f64, c: f64, v: f64) -> LiveEvent {
        LiveEvent { bar_ts: ts, open: o, high: h, low: l, close: c, volume: v, closed: true, provider_ts: ts }
    }

    #[test]
    fn one_minute_bars_pass_through_at_one_minute() {
        let mut b = Bucketer::new(60);
        let t = datetime!(2026-07-23 19:00:00 UTC);
        let out = b.bar(src(t, 10.0, 12.0, 9.0, 11.0, 100.0), 60);
        // The source bar is the whole bucket: closed at once, no timer involved.
        assert_eq!(out.len(), 1);
        assert!(out[0].closed);
        assert_eq!(out[0].bar_ts, t);
        assert_eq!((out[0].open, out[0].high, out[0].low, out[0].close), (10.0, 12.0, 9.0, 11.0));
        assert_eq!(out[0].volume, 100.0);
    }

    #[test]
    fn five_one_minute_bars_make_one_five_minute_bar() {
        let mut b = Bucketer::new(300);
        let base = datetime!(2026-07-23 19:00:00 UTC);
        let mut last = None;
        for i in 0..5 {
            let t = base + time::Duration::minutes(i);
            let price = 10.0 + i as f64;
            let out = b.bar(src(t, price, price + 1.0, price - 1.0, price, 10.0), 60);
            assert_eq!(out.len(), 1);
            last = Some(out[0].clone());
            // Only the fifth minute completes the bucket.
            assert_eq!(out[0].closed, i == 4, "minute {i}");
        }
        let done = last.unwrap();
        assert_eq!(done.bar_ts, base);
        assert_eq!(done.open, 10.0); // first minute's open
        assert_eq!(done.close, 14.0); // last minute's close
        assert_eq!(done.high, 15.0); // max over the five
        assert_eq!(done.low, 9.0); // min over the five
        assert_eq!(done.volume, 50.0); // volume adds up
    }

    #[test]
    fn a_later_bucket_closes_the_one_in_hand() {
        let mut b = Bucketer::new(300);
        let base = datetime!(2026-07-23 19:00:00 UTC);
        b.bar(src(base, 10.0, 10.0, 10.0, 10.0, 1.0), 60);
        // Nothing traded for the rest of the period; the next bar belongs to 19:05.
        let out = b.bar(src(base + time::Duration::minutes(5), 20.0, 20.0, 20.0, 20.0, 1.0), 60);
        assert_eq!(out.len(), 2);
        assert!(out[0].closed && out[0].bar_ts == base && out[0].close == 10.0);
        assert!(!out[1].closed && out[1].bar_ts == base + time::Duration::minutes(5));
    }

    #[test]
    fn the_timer_closes_a_bucket_a_quiet_market_left_open() {
        let mut b = Bucketer::new(60);
        let base = datetime!(2026-07-23 19:00:00 UTC);
        b.trade(10.0, 1.0, base + time::Duration::seconds(3));
        // Still inside the period, and inside the grace right after it: nothing closes.
        assert!(b.tick(base + time::Duration::seconds(30)).is_empty());
        assert!(b.tick(base + time::Duration::seconds(61)).is_empty());
        let out = b.tick(base + time::Duration::seconds(63));
        assert_eq!(out.len(), 1);
        assert!(out[0].closed && out[0].bar_ts == base);
    }

    #[test]
    fn a_straggler_cannot_reopen_a_closed_bucket() {
        let mut b = Bucketer::new(60);
        let base = datetime!(2026-07-23 19:00:00 UTC);
        b.bar(src(base, 10.0, 10.0, 10.0, 10.0, 1.0), 60); // closes 19:00
        let out = b.bar(src(base, 99.0, 99.0, 99.0, 99.0, 1.0), 60);
        assert!(out.is_empty(), "a repeat of a closed bucket is dropped, not re-emitted");
    }

    #[test]
    fn a_daily_bucket_follows_the_provider_session_not_the_utc_day() {
        // Alpaca and Massive stamp a US daily bar at midnight New York (05:00 UTC in winter).
        let session_open = datetime!(2026-01-05 05:00:00 UTC);
        let mut b = Bucketer::new(86_400);
        b.anchor(session_open);
        // A minute bar from that afternoon belongs to the session that opened at 05:00,
        // not to the UTC day that started at 00:00.
        let t = datetime!(2026-01-05 19:30:00 UTC);
        let out = b.bar(src(t, 10.0, 11.0, 9.0, 10.5, 5.0), 60);
        assert_eq!(out[0].bar_ts, session_open);
        assert!(!out[0].closed);
        // A minute bar before the anchor time belongs to the *previous* session.
        let mut b2 = Bucketer::new(86_400);
        b2.anchor(session_open);
        let out = b2.bar(src(datetime!(2026-01-05 02:00:00 UTC), 1.0, 1.0, 1.0, 1.0, 1.0), 60);
        assert_eq!(out[0].bar_ts, session_open - time::Duration::days(1));
    }

    #[test]
    fn priming_keeps_the_session_open_and_extends_it() {
        let session_open = datetime!(2026-01-05 05:00:00 UTC);
        let mut b = Bucketer::new(86_400);
        // The REST seed's partial daily bar: the session is already three hours old.
        b.prime(src(session_open, 100.0, 104.0, 99.0, 103.0, 1_000.0));
        // One minute bar later in the same session extends it, it does not restart it.
        let out = b.bar(
            src(datetime!(2026-01-05 19:30:00 UTC), 103.0, 106.0, 102.0, 105.0, 50.0),
            60,
        );
        let bar = &out[0];
        assert_eq!(bar.bar_ts, session_open);
        assert_eq!(bar.open, 100.0, "the day's open survives, not the minute's");
        assert_eq!(bar.high, 106.0);
        assert_eq!(bar.low, 99.0);
        assert_eq!(bar.close, 105.0);
        assert_eq!(bar.volume, 1_050.0);
        assert!(!bar.closed);
        // The next session closes it and opens the next bucket on the same alignment.
        let next = session_open + time::Duration::days(1);
        let out = b.bar(src(next, 105.0, 105.0, 105.0, 105.0, 1.0), 60);
        assert!(out[0].closed && out[0].bar_ts == session_open && out[0].close == 105.0);
        assert_eq!(out[1].bar_ts, next);
    }

    #[test]
    fn an_unanchored_bucketer_still_divides_the_epoch() {
        let mut b = Bucketer::new(86_400);
        let out = b.bar(
            src(datetime!(2026-01-05 19:30:00 UTC), 10.0, 10.0, 10.0, 10.0, 1.0),
            60,
        );
        assert_eq!(out[0].bar_ts, datetime!(2026-01-05 00:00:00 UTC));
    }

    #[test]
    fn trades_build_the_bar_in_order() {
        let mut b = Bucketer::new(60);
        let base = datetime!(2026-07-23 19:00:00 UTC);
        b.trade(10.0, 2.0, base);
        b.trade(12.0, 1.0, base + time::Duration::seconds(10));
        let out = b.trade(9.0, 3.0, base + time::Duration::seconds(20));
        let bar = &out[0];
        assert_eq!((bar.open, bar.high, bar.low, bar.close), (10.0, 12.0, 9.0, 9.0));
        assert_eq!(bar.volume, 6.0);
        assert!(!bar.closed);
    }
}
