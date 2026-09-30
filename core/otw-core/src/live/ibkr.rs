//! Interactive Brokers live bars, over the same local gateway the history comes from.
//!
//! The one provider that does not speak WebSocket, so it does not go through
//! [`super::run_feed`]: it drives an `ibapi` subscription on its own task and hands back the
//! same normalized [`LiveStream`] as everything else.
//!
//! Four things it has to get right, all of them consequences of IB being a broker rather
//! than a data vendor:
//!
//! - **The feed is tick-by-tick where IB serves it.** `reqTickByTickData(Last)` publishes
//!   each print as it happens, so the candle moves on the trade rather than stepping every
//!   five seconds, and the fold ([`super::agg::Bucketer`]) builds every timeframe from the
//!   prints exactly as it does for Coinbase. Where IB does not serve it (options, and indices
//!   outside CME) the five-second `reqRealTimeBars` stream stays, folded the same way.
//!   [`contract::live_source`] is the one place that choice lives, and it decides
//!   [`StreamConnector::grain`] with it.
//! - **Whatever the source, no timeframe is native.** A session is not 17280 epoch-aligned
//!   five-second bars and no more a count of prints, which is why daily and weekly buckets
//!   are anchored on IB's own stamps (see [`super::hub::anchored`]) rather than divided out
//!   of the epoch.
//! - **The same price basis as the history**, or the live candles would not continue the
//!   stored ones: cash forex and crypto have no usable tape at IB, so both are `MIDPOINT`
//!   there and the midpoint tick here (which carries no size, so those candles have no
//!   volume, the same way their stored bars do not). A five-second midpoint bar carries `-1`
//!   for volume, IB's "not applicable", floored to zero rather than charted as a negative.
//! - **It reuses the process-wide session**, not a second socket: IB counts market-data lines
//!   per account, and a second client id would only take another id from the band.
//!   Deliberately *not* through `Session::call`, whose rolling window is the 60-per-10-minutes
//!   historical limit; a streaming subscription is not a historical request and must not eat a
//!   download's place in that queue.
//! - **A refusal here is nearly always an entitlement**, not a bug: IB serves what the account
//!   subscribes to, and the live seat is exclusive per account. Those two are classified as
//!   [`LiveFault::Entitlement`] and [`LiveFault::Conflict`] so the user is told which of their
//!   own settings to change instead of watching a reconnect loop.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use ibapi::market_data::realtime::{BarSize, WhatToShow};
use ibapi::subscriptions::Subscription;
use ibapi::Client;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;

use crate::histdata::ibkr::contract::LiveSource;
use crate::histdata::ibkr::{contract, resolve, session};

use super::{
    FeedCmd, FeedHandle, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveSession,
    SessionRequest, SourceGrain, StreamConnector,
};

/// Market-data lines a plain IB account holds at once. IB never states the number over the
/// API, it only starts refusing requests once it is reached (error 322), so this is the
/// documented default: the account may have more (commissions and quote boosters raise it),
/// which is why crossing it is a warning and never a refusal on our side.
pub const LINE_CAP: usize = 100;

/// IB's only streaming bar size, for the instruments that have no tick-by-tick stream.
const SOURCE_SECS: i64 = 5;
/// Five-second bars arrive on the beat, so the close timer only has to cover a quiet market.
const GRACE: i64 = 3;

/// Historical ticks to replay before the live ones start. Zero: the chart already loaded its
/// history from the series endpoint and the hub seeds what it records, so asking IB to open
/// with a backlog would only duplicate rows it already has.
const TICK_BACKLOG: i32 = 0;

/// Whether IB may drop the size of a tick it considers unchanged. False: size is volume here,
/// and a fold that misses prints under-reports the candle's volume.
const IGNORE_SIZE: bool = false;

pub struct IbkrLive;

#[async_trait::async_trait]
impl StreamConnector for IbkrLive {
    /// One gateway per connector, and the connector is already part of the session key, so
    /// every instrument shares the single process-wide IB connection.
    fn socket_class(&self, _inst: &Instrument) -> Result<String> {
        Ok(String::new())
    }

    /// Prints where IB streams them, five-second bars where it does not. Either way the hub
    /// folds up to each pane's timeframe: IB publishes no candle we chart directly.
    fn grain(&self, inst: &Instrument) -> Result<SourceGrain> {
        Ok(match contract::live_source(&inst.asset_type) {
            LiveSource::Bars5s => SourceGrain::Bars { secs: SOURCE_SECS, grace: GRACE },
            LiveSource::LastTick | LiveSource::MidPointTick => SourceGrain::Trades,
        })
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(inst.symbol.clone())
    }

    /// Not a WebSocket: this drives one tick-by-tick (or five-second bar) subscription per
    /// instrument over the shared IB session, and the manager task below is what
    /// [`super::run_feed`] is for everyone else.
    ///
    /// **One market-data line per symbol, not per pane.** IB counts lines per account, so
    /// two timeframes of the same instrument ride one subscription and the fold happens in
    /// the hub.
    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        // The gateway address rides in the same map as a credential would (it is a setting,
        // not a secret), so this is the connector the user picked, not a global. Reaching it
        // here means a gateway that is down fails the session rather than each instrument.
        let session = session::get(req.secrets).await?;
        let (out_tx, out_rx) = mpsc::channel::<Result<FeedUpdate>>(512);
        let (cmd_tx, cmd_rx) = mpsc::channel::<FeedCmd>(32);
        tokio::spawn(manage(
            session,
            req.connector.to_string(),
            req.instruments.to_vec(),
            cmd_rx,
            out_tx,
        ));
        Ok(LiveSession {
            handle: FeedHandle::new(cmd_tx),
            stream: Box::pin(ReceiverStream::new(out_rx)),
        })
    }
}

/// Own the account's live subscriptions: one task per instrument, started and stopped as
/// panes come and go, all reporting onto one stream.
async fn manage(
    session: Arc<session::Session>,
    connector: String,
    initial: Vec<Instrument>,
    mut cmds: mpsc::Receiver<FeedCmd>,
    out: mpsc::Sender<Result<FeedUpdate>>,
) {
    let mut lines: HashMap<String, JoinHandle<()>> = HashMap::new();
    for inst in initial {
        start(&session, &connector, &inst, &out, &mut lines).await;
    }
    while let Some(cmd) = cmds.recv().await {
        match cmd {
            FeedCmd::Add(insts) => {
                for inst in insts {
                    if !lines.contains_key(&inst.symbol) {
                        start(&session, &connector, &inst, &out, &mut lines).await;
                    }
                }
            }
            FeedCmd::Remove(insts) => {
                for inst in insts {
                    if let Some(task) = lines.remove(&inst.symbol) {
                        task.abort();
                    }
                }
            }
        }
    }
    for (_, task) in lines {
        task.abort();
    }
}

/// Open one instrument's five-second stream. A refusal here belongs to that instrument
/// alone (an unknown contract, a market-data subscription the account does not hold), so it
/// travels as a tagged fault instead of ending the session the other panes are on.
async fn start(
    session: &Arc<session::Session>,
    connector: &str,
    inst: &Instrument,
    out: &mpsc::Sender<Result<FeedUpdate>>,
    lines: &mut HashMap<String, JoinHandle<()>>,
) {
    let channel = inst.symbol.clone();
    match subscribe(session, inst, connector, out).await {
        Ok(task) => {
            lines.insert(channel, task);
        }
        Err(e) => {
            let _ = out.send(Ok(FeedUpdate::fault(channel, &e))).await;
        }
    }
}

async fn subscribe(
    session: &Arc<session::Session>,
    inst: &Instrument,
    connector: &str,
    out: &mpsc::Sender<Result<FeedUpdate>>,
) -> Result<JoinHandle<()>> {
    // Settle the instrument before subscribing: a futures root lists on several exchanges,
    // and a live subscription to the wrong one is silence, not an error.
    let contract = resolve::contract_for(session.as_ref(), &inst.symbol, &inst.asset_type).await?;
    let (client, _) = session.client().await?;
    let line = Line {
        // The subscription is fed by the connection's message bus, so the connection has to
        // outlive it. The session registry holds one too, but a `reset` elsewhere would drop
        // that one mid-stream; this handle is the feed's own.
        connection: client.clone(),
        session: Arc::clone(session),
        channel: inst.symbol.clone(),
        symbol: inst.symbol.clone(),
        connector: connector.to_string(),
        out: out.clone(),
    };
    let fail = |e| explain(session.as_ref(), &inst.symbol, connector, e);
    // Three sources, one drain: only the line that turns an IB item into a [`FeedUpdate`]
    // differs, and each of them is the whole translation for that source.
    Ok(match contract::live_source(&inst.asset_type) {
        LiveSource::LastTick => {
            let sub = client
                .tick_by_tick_last(&contract, TICK_BACKLOG, IGNORE_SIZE)
                .await
                .map_err(fail)?;
            drain(sub, line, |l, t| FeedUpdate::trade(l, t.price, t.size.max(0.0), t.time))
        }
        LiveSource::MidPointTick => {
            let sub = client
                .tick_by_tick_midpoint(&contract, TICK_BACKLOG, IGNORE_SIZE)
                .await
                .map_err(fail)?;
            // A midpoint is a price with no trade behind it, so it carries no size: the
            // candle it folds into has no volume, exactly like its stored bars.
            drain(sub, line, |l, m| FeedUpdate::trade(l, m.mid_point, 0.0, m.time))
        }
        LiveSource::Bars5s => {
            let what = realtime_what_to_show(&inst.asset_type);
            let hours = contract::trading_hours(&inst.asset_type);
            let sub = client
                .realtime_bars(&contract, BarSize::Sec5, what, hours)
                .await
                .map_err(fail)?;
            drain(sub, line, |l, b| {
                FeedUpdate::bar(
                    l,
                    LiveEvent {
                        bar_ts: b.date,
                        open: b.open,
                        high: b.high,
                        low: b.low,
                        close: b.close,
                        // MIDPOINT and BID/ASK bars carry -1, IB's "not applicable".
                        volume: b.volume.max(0.0),
                        closed: true,
                        provider_ts: OffsetDateTime::now_utc(),
                    },
                )
            })
        }
    })
}

/// One instrument's live line: what its drain task needs once the subscription is open.
struct Line {
    connection: Arc<Client>,
    session: Arc<session::Session>,
    channel: String,
    symbol: String,
    connector: String,
    out: mpsc::Sender<Result<FeedUpdate>>,
}

/// Drain one IB subscription onto the feed until it ends, the connection dies, or the task is
/// aborted at teardown. Generic over the item: a print, a midpoint and a five-second bar are
/// three decoders of the same stream, and `into_update` is all that separates them.
fn drain<T, F>(mut sub: Subscription<T>, line: Line, into_update: F) -> JoinHandle<()>
where
    T: Send + 'static,
    F: Fn(String, T) -> FeedUpdate + Send + 'static,
{
    tokio::spawn(async move {
        let Line { connection, session, channel, symbol, connector, out } = line;
        let _connection = connection;
        while let Some(item) = sub.next().await {
            match item {
                Ok(v) => {
                    if out.send(Ok(into_update(channel.clone(), v))).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    // A socket that died (the Gateway restarts itself once a day) takes the
                    // whole session with it, because every other instrument rides the same
                    // connection; anything else is this instrument's own problem.
                    let dropped = session::dropped(&e);
                    if dropped {
                        session.reset().await;
                    }
                    let err = explain(&session, &symbol, &connector, e);
                    let _ = if dropped {
                        out.send(Err(err)).await
                    } else {
                        out.send(Ok(FeedUpdate::fault(channel.clone(), &err))).await
                    };
                    return;
                }
            }
        }
        // The subscription ended on its own: report it as this instrument's transport fault
        // so the hub retries it rather than leaving a pane silently frozen.
        let _ = out
            .send(Ok(FeedUpdate::fault(
                channel.clone(),
                &LiveError::err(
                    LiveFault::Transport,
                    format!("Interactive Brokers ended the {symbol} subscription"),
                ),
            )))
            .await;
    })
}

/// Streaming bars use their own, shorter `what_to_show` vocabulary. It mirrors the historical
/// choice exactly (`contract::what_to_show`) so the live candles continue the stored ones
/// instead of stepping onto a different price basis at the seam. Only the five-second path
/// asks for it: a tick-by-tick stream names its type in the request instead.
fn realtime_what_to_show(asset_type: &str) -> WhatToShow {
    match asset_type {
        "crypto" | "fx" => WhatToShow::MidPoint,
        _ => WhatToShow::Trades,
    }
}

/// Name the refusal. IB answers with a numbered message, and the number is the whole
/// diagnosis: 354 is a subscription to buy, 10197 is another IB session to close, 200 is a
/// ticker to fix. Everything else is treated as transport and retried.
fn explain(
    session: &session::Session,
    symbol: &str,
    connector: &str,
    e: ibapi::Error,
) -> anyhow::Error {
    classify(session.addr(), symbol, connector, e)
}

/// The classification itself, on the gateway's address rather than the session, so it is
/// testable without one.
fn classify(addr: &str, symbol: &str, connector: &str, e: ibapi::Error) -> anyhow::Error {
    let ibapi::Error::Message(code, message) = &e else {
        return LiveError::err(
            LiveFault::Transport,
            format!("{symbol} live: Interactive Brokers ({addr}) — {e}"),
        );
    };
    // Tick-by-tick refusals are classified on their words, not on their number. IB documents
    // the *limit* (as many simultaneous tick streams as the account has market-depth lines,
    // three on a plain account, sixty at most) but not the code it answers with, and the
    // 10180-range numbers are not in the public error table at all. The message always names
    // the feature, so that is what is read: a ceiling to free, or a contract IB does not
    // serve ticks for. Guessing a number would have shown the wrong sentence, or the raw one.
    let low = message.to_ascii_lowercase();
    if low.contains("tick-by-tick") || low.contains("tick by tick") {
        let hit_ceiling = ["max", "maximum", "reached", "limit", "exceed"]
            .iter()
            .any(|w| low.contains(w));
        return if hit_ceiling {
            LiveError::err(
                LiveFault::Conflict,
                format!(
                    "Interactive Brokers has no live tick stream left for {symbol} ({message}). \
                     An account may hold only as many at once as it has market-depth lines, \
                     three on a plain account: stop live on another Interactive Brokers chart, \
                     or ask IB for more lines"
                ),
            )
        } else {
            LiveError::err(
                LiveFault::Unsupported,
                format!(
                    "Interactive Brokers serves no tick-by-tick stream for {symbol} \
                     ({message}). Options and indices outside CME are the usual case; chart \
                     them from another provider, or download instead of streaming"
                ),
            )
        };
    }
    let (fault, text) = match *code {
        // The market-data ceiling, which is the other one: lines, not tick streams.
        101 => (
            LiveFault::Conflict,
            format!(
                "connector {connector}'s Interactive Brokers account is holding all the \
                 market-data lines it has ({message}). Stop live on another chart and this \
                 one starts"
            ),
        ),
        200 => (
            LiveFault::Symbol,
            format!(
                "Interactive Brokers has no contract matching {symbol} ({message}). Check the \
                 spelling, and add the currency for a non-US instrument, e.g. SAN:EUR"
            ),
        ),
        354 | 10089 | 10090 | 10091 => (
            LiveFault::Entitlement,
            format!(
                "connector {connector}'s Interactive Brokers account has no live market-data \
                 subscription for {symbol} ({message}). Subscriptions are bought in Client \
                 Portal under Settings → Market Data Subscriptions; the delayed data IB offers \
                 instead is not a live feed"
            ),
        ),
        10197 => (
            LiveFault::Conflict,
            format!(
                "Interactive Brokers gives one live market-data session per account and another \
                 one holds it. Close the TWS, mobile or web session using this account and this \
                 reconnects on its own ({message})"
            ),
        ),
        322 | 420 => (
            LiveFault::Conflict,
            format!(
                "Interactive Brokers is refusing further market-data requests on {addr} right \
                 now ({message}). Close a chart or wait for the current ones to expire"
            ),
        ),
        _ => (
            LiveFault::Transport,
            format!("{symbol} live: Interactive Brokers {code} — {message}"),
        ),
    };
    LiveError::err(fault, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{fault_message, fault_of};

    fn msg(code: i32, text: &str) -> anyhow::Error {
        classify("127.0.0.1:4002", "AAPL", "ib", ibapi::Error::Message(code, text.into()))
    }

    /// The tick-by-tick ceiling is small (three streams on a plain account) and it is the one
    /// limit a user will actually meet, so it has to arrive as a sentence naming the fix
    /// rather than as a raw IB line or a silent retry.
    #[test]
    fn the_tick_ceiling_reaches_the_user_in_words() {
        let e = msg(10190, "Max number (3) of tick-by-tick requests has been reached.");
        assert_eq!(fault_of(&e), LiveFault::Conflict);
        let said = fault_message(&e);
        assert!(said.contains("market-depth lines"), "{said}");
        assert!(said.contains("stop live on another"), "{said}");

        // The same refusal under any other number: IB does not publish these codes, so the
        // words are what is read.
        assert_eq!(fault_of(&msg(10189, "maximum tick by tick subscriptions reached")), LiveFault::Conflict);
    }

    /// A contract IB serves no ticks for is a different answer: nothing the user can free up,
    /// so the pane stops instead of retrying.
    #[test]
    fn a_contract_with_no_tick_stream_stops_rather_than_retries() {
        let e = msg(10189, "Failed to request tick-by-tick data. Not supported for this instrument.");
        assert_eq!(fault_of(&e), LiveFault::Unsupported);
        assert!(fault_of(&e).terminal());
        assert!(fault_message(&e).contains("no tick-by-tick stream"));
    }

    /// The other ceiling, the market-data lines one, keeps its own sentence: the two are
    /// counted separately at IB and freeing the wrong one changes nothing.
    #[test]
    fn the_market_data_ceiling_is_a_different_sentence() {
        let e = msg(101, "Max number of tickers has been reached.");
        assert_eq!(fault_of(&e), LiveFault::Conflict);
        assert!(fault_message(&e).contains("market-data lines"));
    }
}
