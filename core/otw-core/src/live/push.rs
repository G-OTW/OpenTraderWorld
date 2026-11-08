//! The live side of a push source: nothing upstream to open, the webhook feeds the hub.
//!
//! A chart on a push connector subscribes like any other; the session it opens carries no
//! socket. Bars and ticks posted to the connector's webhook reach the subscriptions through
//! [`super::hub::LiveHub::ingest`], and a tick folds into every subscribed timeframe through
//! the same [`super::agg::Bucketer`] a trade feed uses (hence [`SourceGrain::Trades`]).

use anyhow::Result;
use time::OffsetDateTime;
use tokio::sync::mpsc;

use super::{FeedHandle, Instrument, LiveError, LiveFault, LiveSession, SessionRequest, SourceGrain, StreamConnector};
use crate::histdata::kinds;

pub struct PushStream {
    provider: String,
}

impl PushStream {
    /// The live connector of a minted push source, or `None` for anything else.
    pub fn for_provider(provider: &str) -> Option<Self> {
        (kinds::kind_of(provider) == Some(kinds::push::KIND) && kinds::is_instance(provider))
            .then(|| PushStream { provider: provider.to_string() })
    }

    fn check(&self, inst: &Instrument) -> Result<()> {
        let spec = kinds::push_spec(&self.provider).map_err(|e| LiveError::err(LiveFault::Unsupported, format!("{e:#}")))?;
        if !spec.timeframes.contains(&inst.timeframe) {
            return Err(LiveError::err(
                LiveFault::Unsupported,
                format!("this push source declares {}, not {}", spec.timeframes.join(", "), inst.timeframe),
            ));
        }
        if !spec.mapping.asset_types.contains(&inst.asset_type) {
            return Err(LiveError::err(
                LiveFault::Unsupported,
                format!("this push source files {}, not {}", spec.mapping.asset_types.join(", "), inst.asset_type),
            ));
        }
        Ok(())
    }
}

/// Split a pushed batch at `now`: the bars whose period has ended (opened at or before
/// `now` minus one interval), which are history and may be stored, and how many are still
/// forming. A forming bar is broadcast to the charts watching it but never written: the next
/// push of the same period would correct it, and a stored half bar would sit in the dataset
/// as a finished one until then.
pub fn closed_bars(timeframe: &str, bars: Vec<otw_store::histdata::Bar>, now: OffsetDateTime) -> Result<(Vec<otw_store::histdata::Bar>, usize)> {
    let tf = crate::timeframe::Timeframe::parse(timeframe)?;
    let total = bars.len();
    let closed: Vec<_> = bars.into_iter().filter(|b| tf.closes_at(b.ts) <= now).collect();
    let forming = total - closed.len();
    Ok((closed, forming))
}

/// The channel a pushed tick for this instrument arrives on: every timeframe of a symbol
/// reads the same prints.
pub fn channel_of(symbol: &str, asset_type: &str) -> String {
    format!("{symbol}|{asset_type}")
}

#[async_trait::async_trait]
impl StreamConnector for PushStream {
    fn grain(&self, inst: &Instrument) -> Result<SourceGrain> {
        self.check(inst)?;
        Ok(SourceGrain::Trades)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        self.check(inst)?;
        Ok(channel_of(&inst.symbol, &inst.asset_type))
    }

    fn has_rest(&self) -> bool {
        false
    }

    /// A daily or weekly candle folded from pushed ticks starts at the connector's own
    /// midnight (Monday's, for a week), since there is no REST bar to anchor it on.
    fn anchor_offset(&self, _inst: &Instrument, tf_secs: i64, now: OffsetDateTime) -> Option<i64> {
        let spec = kinds::push_spec(&self.provider).ok()?;
        kinds::push::period_offset(&spec, tf_secs, now)
    }

    async fn open(&self, _req: SessionRequest<'_>) -> Result<LiveSession> {
        // Nothing to open and nothing to read: the stream stays quiet until the hub lets go
        // of it, and the handle's commands have nobody to tell.
        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        Ok(LiveSession { handle: FeedHandle::new(cmd_tx), stream: Box::pin(futures::stream::pending()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otw_store::histdata::Bar;
    use time::macros::datetime;

    fn bar(ts: OffsetDateTime) -> Bar {
        Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 0.0, adj_open: None, adj_high: None, adj_low: None, adj_close: None }
    }

    #[test]
    fn only_bars_whose_period_has_ended_are_kept() {
        let now = datetime!(2024-01-01 00:02:30 UTC);
        let bars = vec![
            bar(datetime!(2024-01-01 00:01:00 UTC)),
            bar(datetime!(2024-01-01 00:01:30 UTC)),
            bar(datetime!(2024-01-01 00:02:00 UTC)),
        ];
        // 00:01 closed at 00:02; 00:01:30 closes at 00:02:30, exactly now, so it is over too;
        // 00:02 is still forming until 00:03.
        let (closed, forming) = closed_bars("1m", bars, now).unwrap();
        assert_eq!(closed.len(), 2);
        assert_eq!(forming, 1);
        let (closed, forming) = closed_bars("1d", vec![bar(datetime!(2024-01-01 00:00:00 UTC))], now).unwrap();
        assert_eq!((closed.len(), forming), (0, 1));
        assert!(closed_bars("7x", Vec::new(), now).is_err());
    }
}
