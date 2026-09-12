//! Live market data over provider WebSockets.
//!
//! This module extends Historical Data *forward*: the same datasets and the same REST
//! [`crate::histdata::Connector`]s used for history, plus a WS feed that streams the
//! forming bar. The **current (unclosed) bar is built server-side only** — if the browser
//! aggregated its own ticks in parallel, two clients would show two different candles.
//!
//! Layers:
//!   - [`LiveEvent`] — one normalized bar update: `{ bar_ts, o,h,l,c,v, closed, provider_ts }`.
//!     `bar_ts` is the bar's **open time as the exchange reports it** (ms UTC), the canonical
//!     idempotency key; every application of an event is an upsert on it.
//!   - [`Feed`] + [`run_feed`]: the transport. Connect one WS, send subscribe frames, answer
//!     pings, turn provider messages (candles, aggregates or individual trades)
//!     into tagged updates on an mpsc stream, and add or drop instruments on a socket that is
//!     already open. Reconnection is *not* here, it is the supervisor's.
//!   - [`StreamConnector`]: a provider's live capability. It says which socket an instrument
//!     belongs on, what grain that socket publishes, and opens it for a *set* of instruments.
//!     Registry in [`stream_connector_for`].
//!   - [`LiveFault`] — *why* a feed cannot run. A dropped socket is retried; a rejected key,
//!     a plan without streaming and an instrument the account is not subscribed to are not
//!     (retrying them is a loop that never ends and a message the user never sees). The
//!     supervisor stops on those and reports them, which is what the chart's banner shows.
//!   - [`hub::LiveHub`]: one supervised socket per connector and socket class, carrying every
//!     instrument the charts are watching on it. Per subscription it seeds via REST, overlap
//!     merges the buffered events (so there is no gap between REST and the stream), folds the
//!     timeframe, persists closed bars, backfills detected gaps, broadcasts every update, and
//!     reconnects from cold on drop.

use std::pin::Pin;

use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use otw_store::histdata::Bar;

pub mod agg;
mod alpaca;
mod binance;
mod coinbase;
pub mod hub;
pub mod ibkr;
mod kraken;
mod massive;

/// How many trailing bars the supervisor seeds from REST when a dataset has no stored
/// history yet (enough to warm up typical indicators without pulling a full download).
pub const SEED_BARS: i64 = 300;

// ── Faults ──────────────────────────────────────────────────────────────────────

/// Why a live feed stopped. The distinction that matters is [`LiveFault::terminal`]: a
/// dropped socket is worth reconnecting for, a rejected API key is not. Retrying a terminal
/// fault burns the provider's connection allowance forever and leaves the user staring at a
/// dot that never turns green, so the supervisor stops and says what to fix instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFault {
    /// The provider rejected the credentials.
    Auth,
    /// The credentials are fine, the plan does not include streaming.
    Plan,
    /// The plan is fine, this instrument (or this feed) is not part of the subscription.
    Entitlement,
    /// The provider does not know this symbol.
    Symbol,
    /// This provider cannot stream this asset type or timeframe.
    Unsupported,
    /// Another session is holding the seat: a connection-limit refusal, or IB's competing
    /// live session. It clears when the user closes the other one, so it is retried slowly.
    Conflict,
    /// Anything else: a dropped socket, a timeout, a malformed frame.
    Transport,
}

impl LiveFault {
    /// Whether retrying is pointless. A terminal fault needs a human (a key, a plan, a
    /// subscription, a different symbol), so the supervisor gives up and reports it.
    pub fn terminal(self) -> bool {
        matches!(
            self,
            LiveFault::Auth
                | LiveFault::Plan
                | LiveFault::Entitlement
                | LiveFault::Symbol
                | LiveFault::Unsupported
        )
    }

    /// Stable machine code, sent to the client so the UI can style and translate it.
    pub fn code(self) -> &'static str {
        match self {
            LiveFault::Auth => "auth",
            LiveFault::Plan => "plan",
            LiveFault::Entitlement => "entitlement",
            LiveFault::Symbol => "symbol",
            LiveFault::Unsupported => "unsupported",
            LiveFault::Conflict => "conflict",
            LiveFault::Transport => "transport",
        }
    }
}

/// A classified live failure. Travels as an `anyhow` error and is recovered by the
/// supervisor with `downcast_ref`, so every layer in between stays `Result<_>`.
#[derive(Debug, Clone)]
pub struct LiveError {
    pub fault: LiveFault,
    pub message: String,
}

impl LiveError {
    pub fn new(fault: LiveFault, message: impl Into<String>) -> Self {
        LiveError { fault, message: message.into() }
    }

    /// Build it already boxed as an `anyhow::Error`, which is what every call site wants.
    pub fn err(fault: LiveFault, message: impl Into<String>) -> anyhow::Error {
        anyhow::Error::new(LiveError::new(fault, message))
    }
}

impl std::fmt::Display for LiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LiveError {}

/// Classify an error chain: the [`LiveError`] someone put there, or `Transport` by default.
pub fn fault_of(e: &anyhow::Error) -> LiveFault {
    e.downcast_ref::<LiveError>().map(|l| l.fault).unwrap_or(LiveFault::Transport)
}

/// The sentence to show for an error chain: the classified message when there is one,
/// otherwise the chain's own words.
pub fn fault_message(e: &anyhow::Error) -> String {
    match e.downcast_ref::<LiveError>() {
        Some(l) => l.message.clone(),
        None => format!("{e:#}"),
    }
}

/// One normalized live bar update. Timestamps are UTC; `bar_ts` is the exchange's bar open
/// time (the idempotency key), `provider_ts` the event/emission time (used for lag metrics
/// and last-writer-wins when the same `bar_ts` arrives repeatedly).
#[derive(Debug, Clone)]
pub struct LiveEvent {
    pub bar_ts: OffsetDateTime,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    /// True once this is the bar's final value (persist + advance); false while it forms.
    pub closed: bool,
    pub provider_ts: OffsetDateTime,
}

impl LiveEvent {
    /// The persistable OHLCV bar (adjusted fields are always None for live crypto/fx).
    pub fn as_bar(&self) -> Bar {
        Bar {
            ts: self.bar_ts,
            open: self.open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self.volume,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        }
    }

    /// SSE payload for one event. `lag_ms` = now − provider_ts, the delivery delay we bear.
    pub fn to_wire(&self) -> Value {
        let now = OffsetDateTime::now_utc();
        let lag_ms = ((now - self.provider_ts).whole_milliseconds()).max(0);
        json!({
            "ts": self.bar_ts.format(&Rfc3339).unwrap_or_default(),
            "o": self.open,
            "h": self.high,
            "l": self.low,
            "c": self.close,
            "v": self.volume,
            "closed": self.closed,
            "lag_ms": lag_ms,
        })
    }
}

// ── Feed transport ──────────────────────────────────────────────────────────────

/// One instrument on a socket: what a provider needs in order to subscribe, and what the hub
/// routes updates back to.
///
/// The connector is not part of it (it is the *socket's* identity, not the instrument's) and
/// neither is the dataset: recording is the hub's business, the transport only reads.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Instrument {
    pub symbol: String,
    pub asset_type: String,
    pub timeframe: String,
}

impl Instrument {
    pub fn new(symbol: &str, asset_type: &str, timeframe: &str) -> Self {
        Instrument {
            symbol: symbol.to_string(),
            asset_type: asset_type.to_string(),
            timeframe: timeframe.to_string(),
        }
    }

    /// Identity inside one socket: an instrument at a timeframe. Two panes on the same
    /// symbol at two timeframes are two subscriptions sharing one channel.
    pub fn key(&self) -> String {
        format!("{}|{}|{}", self.symbol, self.asset_type, self.timeframe)
    }
}

/// What a socket publishes for an instrument, and therefore who folds the timeframe.
///
/// The fold used to live in each connector, which is why a second timeframe on the same
/// symbol needed a second socket. It is the hub's now: one channel feeds every timeframe
/// asked of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceGrain {
    /// Bars already at the asked timeframe: the provider publishes a channel per interval
    /// (Binance, Kraken), so nothing is folded.
    Native,
    /// Individual trades: every bar is built from them (Coinbase).
    Trades,
    /// Bars of a fixed grain, folded up to the timeframe (Alpaca and Massive publish one a
    /// minute, Interactive Brokers one every five seconds). `grace` is how long to wait past
    /// a bucket's end before the timer closes it.
    Bars { secs: i64, grace: i64 },
}

/// One inbound update, tagged with the channel it arrived on.
///
/// **The tag is what makes multiplexing possible.** One socket now carries N instruments and
/// only the provider knows how its own messages name them, so the connector labels every
/// update and the hub does the routing.
pub struct FeedUpdate {
    pub channel: String,
    pub payload: Payload,
}

/// What a live source says about one instrument: a bar (of whatever grain the socket
/// publishes), a single trade, or a refusal that concerns that instrument alone.
pub enum Payload {
    Bar(LiveEvent),
    Trade { price: f64, size: f64, ts: OffsetDateTime },
    /// This instrument cannot be served, but the socket is fine. A shared socket makes the
    /// distinction matter: an unknown symbol or a missing market-data subscription used to
    /// end the only feed it could end, and now it must not take the other panes down with
    /// it. A [`LiveFault::terminal`] one stops that subscription; the rest are retried with
    /// the session.
    Fault { fault: LiveFault, message: String },
}

impl FeedUpdate {
    pub fn bar(channel: impl Into<String>, ev: LiveEvent) -> Self {
        FeedUpdate { channel: channel.into(), payload: Payload::Bar(ev) }
    }
    pub fn trade(channel: impl Into<String>, price: f64, size: f64, ts: OffsetDateTime) -> Self {
        FeedUpdate { channel: channel.into(), payload: Payload::Trade { price, size, ts } }
    }
    /// A refusal that belongs to one instrument, classified the same way a session-level
    /// error is.
    pub fn fault(channel: impl Into<String>, err: &anyhow::Error) -> Self {
        FeedUpdate {
            channel: channel.into(),
            payload: Payload::Fault { fault: fault_of(err), message: fault_message(err) },
        }
    }
}

/// A provider's live socket, reduced to the few hooks [`run_feed`] needs. One instance holds
/// **many** instruments and stays open as panes come and go.
///
/// A provider that streams candles directly (Binance, Kraken) only implements `on_message`.
/// One that streams trades (Coinbase) tags them and lets the hub fold. One with a staged
/// handshake (Alpaca, Massive) queues its subscribe frames from `on_message` and hands them
/// over through `drain_sends`.
pub trait Feed: Send + 'static {
    /// The `wss://…` URL to connect.
    fn url(&self) -> String;
    /// Frames that add these instruments to the socket. Called once with the opening set and
    /// again whenever a pane joins. A feed whose handshake is not finished yet may queue them
    /// and return nothing, delivering them later through [`Feed::drain_sends`].
    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String>;
    /// Frames that drop them. Empty is valid: the hub has already stopped routing them, so
    /// an unsubscribe the provider does not support only costs bandwidth.
    fn unsubscribe(&mut self, _instruments: &[Instrument]) -> Vec<String> {
        Vec::new()
    }
    /// Parse one inbound text frame into zero or more tagged updates.
    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>>;
    /// Frames the feed wants written now, drained after the handshake and after every
    /// message. This is what a *staged* handshake needs: Alpaca and Massive both want `auth`
    /// first and accept `subscribe` only once they have answered.
    fn drain_sends(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// A live stream of tagged updates. `Err` ends the session (the supervisor reconnects).
pub type LiveStream = Pin<Box<dyn futures::Stream<Item = Result<FeedUpdate>> + Send>>;

/// What the hub asks a running socket to do between reconnects.
pub enum FeedCmd {
    Add(Vec<Instrument>),
    Remove(Vec<Instrument>),
}

/// Handle on a running socket: how the hub adds and drops instruments without reconnecting.
/// Dropping it ends the session's task, which is exactly what a reconnect wants.
pub struct FeedHandle {
    tx: mpsc::Sender<FeedCmd>,
}

impl FeedHandle {
    /// A handle onto a socket that manages itself (Interactive Brokers spawns one task per
    /// subscription rather than writing frames), and the constructor every driver uses.
    pub fn new(tx: mpsc::Sender<FeedCmd>) -> Self {
        FeedHandle { tx }
    }
    pub async fn add(&self, instruments: Vec<Instrument>) {
        let _ = self.tx.send(FeedCmd::Add(instruments)).await;
    }
    pub async fn remove(&self, instruments: Vec<Instrument>) {
        let _ = self.tx.send(FeedCmd::Remove(instruments)).await;
    }
}

/// One open socket: the updates it publishes and the handle that changes what it carries.
pub struct LiveSession {
    pub handle: FeedHandle,
    pub stream: LiveStream,
}

/// Connect a [`Feed`], drive it on its own task, and hand back the session. The task owns the
/// socket: it answers pings, forwards parsed updates, writes the frames the hub's add/remove
/// commands produce, and exits when the socket closes/errors **or** when the session is
/// dropped (the mpsc send then fails).
pub async fn run_feed<F: Feed>(mut feed: F, opening: &[Instrument]) -> Result<LiveSession> {
    let url = feed.url();
    let (ws, _) = connect_async(&url).await.with_context(|| format!("ws connect {url}"))?;
    let (tx, rx) = mpsc::channel::<Result<FeedUpdate>>(512);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<FeedCmd>(32);
    let mut first = feed.subscribe(opening);

    tokio::spawn(async move {
        let (mut write, mut read) = ws.split();
        first.extend(feed.drain_sends());
        for frame in first {
            if write.send(Message::Text(frame)).await.is_err() {
                return;
            }
        }
        loop {
            tokio::select! {
                msg = read.next() => match msg {
                    Some(Ok(Message::Text(t))) => {
                        if !forward(&tx, feed.on_message(t.as_str())).await { return; }
                    }
                    // A few providers send JSON as binary frames.
                    Some(Ok(Message::Binary(b))) => {
                        if let Ok(s) = std::str::from_utf8(&b) {
                            if !forward(&tx, feed.on_message(s)).await { return; }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if write.send(Message::Pong(p)).await.is_err() { return; }
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        let _ = tx.send(Err(anyhow!("ws read: {e}"))).await;
                        return;
                    }
                },
                cmd = cmd_rx.recv() => match cmd {
                    Some(FeedCmd::Add(insts)) => {
                        for frame in feed.subscribe(&insts) {
                            if write.send(Message::Text(frame)).await.is_err() { return; }
                        }
                    }
                    Some(FeedCmd::Remove(insts)) => {
                        for frame in feed.unsubscribe(&insts) {
                            if write.send(Message::Text(frame)).await.is_err() { return; }
                        }
                    }
                    // The hub let go of this session: the socket has no reason to stay open.
                    None => return,
                },
            }
            // Whatever the feed decided to say next (the subscribe frame after an auth ack,
            // a keepalive), written on this task so the socket has a single writer.
            for frame in feed.drain_sends() {
                if write.send(Message::Text(frame)).await.is_err() {
                    return;
                }
            }
        }
    });

    Ok(LiveSession { handle: FeedHandle::new(cmd_tx), stream: Box::pin(ReceiverStream::new(rx)) })
}

/// Push the result of a parse to the consumer. Returns false when the session should end
/// (consumer gone, or the parse itself failed).
async fn forward(tx: &mpsc::Sender<Result<FeedUpdate>>, parsed: Result<Vec<FeedUpdate>>) -> bool {
    match parsed {
        Ok(updates) => {
            for u in updates {
                if tx.send(Ok(u)).await.is_err() {
                    return false;
                }
            }
            true
        }
        Err(e) => {
            let _ = tx.send(Err(e)).await;
            false
        }
    }
}

// ── Provider registry ────────────────────────────────────────────────────────────

/// Everything a provider needs to open one socket. A struct rather than a parameter list
/// because the keyed providers need more than symbols: the credentials to authenticate with
/// and the connector's name, so a failure can say *which* of the user's keys was refused.
pub struct SessionRequest<'a> {
    /// Decrypted credentials merged over the connector's clear settings, the same map the
    /// REST connector reads, so an IB gateway address arrives here too.
    pub secrets: &'a std::collections::HashMap<String, String>,
    /// The connector's user-given name, for error messages the user has to act on.
    pub connector: &'a str,
    /// The instruments this socket opens with. All of one socket class (see
    /// [`StreamConnector::socket_class`]); more can be added later on the same socket.
    pub instruments: &'a [Instrument],
}

/// A provider that can open a live stream. Most speak WebSocket through [`run_feed`];
/// Interactive Brokers speaks its own socket protocol to a local gateway and builds the
/// session itself. Both hand back the same [`LiveSession`].
#[async_trait::async_trait]
pub trait StreamConnector: Send + Sync {
    /// Which socket an instrument belongs to on one connector. Instruments sharing a class
    /// share one connection; the default is one socket for everything.
    ///
    /// This is where a provider states what it cannot multiplex: Alpaca and Massive run one
    /// socket per market, and Kraken's OHLC rows do not name their interval, so its class is
    /// the interval itself.
    fn socket_class(&self, _inst: &Instrument) -> Result<String> {
        Ok(String::new())
    }
    /// What this socket publishes for that instrument, i.e. who folds the timeframe.
    fn grain(&self, inst: &Instrument) -> Result<SourceGrain>;
    /// The channel key the socket's updates for this instrument will carry.
    fn channel_of(&self, inst: &Instrument) -> Result<String>;
    /// Open the socket for a set of instruments of one class.
    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession>;
}


/// The live connector for a provider, or `None` if it has no live feed at all (the REST-only
/// `generic` providers — Yahoo, Alpha Vantage, EODHD).
pub fn stream_connector_for(provider: &str) -> Option<Box<dyn StreamConnector>> {
    match provider {
        "binance" => Some(Box::new(binance::BinanceLive)),
        "kraken" => Some(Box::new(kraken::KrakenLive)),
        "coinbase" => Some(Box::new(coinbase::CoinbaseLive)),
        "alpaca" => Some(Box::new(alpaca::AlpacaLive)),
        "massive" => Some(Box::new(massive::MassiveLive)),
        "ibkr" => Some(Box::new(ibkr::IbkrLive)),
        _ => None,
    }
}

/// The provider's declared live reach, or empty when it has no live transport. Live is not
/// a provider-wide yes/no once the keyed providers are in: Alpaca streams equities, crypto
/// and options but its daily candle is a session rather than 1440 epoch minutes, and IB
/// streams whatever the account is subscribed to. The capability table is the declaration,
/// the registry above is the transport, and a provider needs both.
fn stream_cap(provider: &str) -> Option<&'static crate::histdata::Capability> {
    stream_connector_for(provider)?;
    let cap = crate::histdata::connector_for(provider).ok()?.capability();
    (!cap.stream_asset_types.is_empty()).then_some(cap)
}

/// Asset types this provider can stream (empty = no live feed).
pub fn stream_asset_types(provider: &str) -> &'static [&'static str] {
    stream_cap(provider).map(|c| c.stream_asset_types).unwrap_or(&[])
}

/// Timeframes this provider can stream (empty = no live feed).
pub fn stream_timeframes(provider: &str) -> &'static [&'static str] {
    stream_cap(provider).map(|c| c.stream_timeframes).unwrap_or(&[])
}

/// What the socket actually publishes for this instrument, as a token for the pane to
/// translate: `native` (the provider sends this very candle), `trades`, or the length of the
/// source bar being folded (`5s`, `1m`).
///
/// The live dot says the feed is connected; it does not say what is forming. A daily candle
/// fed by minute bars moves once a minute and a weekly one looks frozen for days, which reads
/// as a stall unless the pane can name the grain it is waiting on.
pub fn source_grain_label(
    provider: &str,
    asset_type: &str,
    symbol: &str,
    timeframe: &str,
) -> Option<String> {
    let connector = stream_connector_for(provider)?;
    let inst = Instrument::new(symbol, asset_type, timeframe);
    Some(match connector.grain(&inst).ok()? {
        SourceGrain::Native => "native".to_string(),
        SourceGrain::Trades => "trades".to_string(),
        SourceGrain::Bars { secs, .. } => match secs {
            s if s % 3600 == 0 => format!("{}h", s / 3600),
            s if s % 60 == 0 => format!("{}m", s / 60),
            s => format!("{s}s"),
        },
    })
}

/// One line on what live costs with this provider — the plan or the subscription it needs.
/// Shown next to the live control, because an empty live chart is nearly always an
/// entitlement rather than a bug.
pub fn stream_note(provider: &str) -> &'static str {
    stream_cap(provider).map(|c| c.stream_note).unwrap_or("")
}

/// Validate an Alpaca connector's `feed` setting. Lives here rather than in the REST
/// connector because the setting exists for the live socket and nothing else.
pub fn alpaca_feed(config: &std::collections::HashMap<String, String>) -> Result<()> {
    alpaca::feed_of(config).map(|_| ())
}

/// Whether a provider exposes a live feed at all (drives the "Live" affordance in the UI).
pub fn stream_capable(provider: &str) -> bool {
    stream_cap(provider).is_some()
}

/// Whether a provider can stream this asset type at all. The timeframe-free half of
/// [`stream_capable_for`], for a symbol-search hit that has no timeframe yet.
pub fn streams_asset(provider: &str, asset_type: &str) -> bool {
    stream_asset_types(provider).contains(&asset_type)
}

/// Whether a provider can stream *this* instrument. Re-checked server-side before a
/// subscription starts, and mirrored in the UI so an impossible combination is greyed out
/// rather than failing on connect.
pub fn stream_capable_for(provider: &str, asset_type: &str, timeframe: &str) -> bool {
    match stream_cap(provider) {
        Some(c) => {
            c.stream_asset_types.contains(&asset_type) && c.stream_timeframes.contains(&timeframe)
        }
        None => false,
    }
}

/// Why this instrument cannot go live, as the sentence to show. `None` when it can.
pub fn stream_refusal(provider: &str, asset_type: &str, timeframe: &str) -> Option<String> {
    let Some(c) = stream_cap(provider) else {
        return Some(format!("{provider} has no live feed"));
    };
    if !c.stream_asset_types.contains(&asset_type) {
        return Some(format!(
            "{} streams {} — not {asset_type}",
            c.label,
            c.stream_asset_types.join(", ")
        ));
    }
    if !c.stream_timeframes.contains(&timeframe) {
        return Some(format!(
            "{} streams {} live — not {timeframe}",
            c.label,
            c.stream_timeframes.join(", ")
        ));
    }
    None
}

// ── Pure helpers (unit-tested) ────────────────────────────────────────────────────

/// Start (open time) of the bar containing `ts`, for a `tf_secs`-second timeframe. Bars are
/// bucketed on the UTC epoch, matching every exchange's candle alignment.
pub fn bar_start(ts: OffsetDateTime, tf_secs: i64) -> OffsetDateTime {
    let secs = ts.unix_timestamp();
    let start = secs - secs.rem_euclid(tf_secs);
    OffsetDateTime::from_unix_timestamp(start).expect("valid epoch")
}

/// Given the last closed bar's open time and a newly closed bar's open time, the open times
/// of any bars missing strictly between them (a technical gap to backfill). Empty when the
/// new bar is the immediate successor, a duplicate, or older (out-of-order).
pub fn missing_buckets(
    last_closed: OffsetDateTime,
    new_closed: OffsetDateTime,
    tf_secs: i64,
) -> Vec<OffsetDateTime> {
    let mut out = Vec::new();
    let mut t = last_closed.unix_timestamp() + tf_secs;
    let end = new_closed.unix_timestamp();
    while t < end {
        if let Ok(dt) = OffsetDateTime::from_unix_timestamp(t) {
            out.push(dt);
        }
        t += tf_secs;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn bar_start_buckets_on_epoch() {
        let t = datetime!(2026-07-23 19:23:37 UTC);
        assert_eq!(bar_start(t, 60), datetime!(2026-07-23 19:23:00 UTC));
        assert_eq!(bar_start(t, 300), datetime!(2026-07-23 19:20:00 UTC));
        assert_eq!(bar_start(t, 3600), datetime!(2026-07-23 19:00:00 UTC));
        // Already on a boundary → unchanged.
        assert_eq!(bar_start(datetime!(2026-07-23 19:00:00 UTC), 3600),
                   datetime!(2026-07-23 19:00:00 UTC));
    }

    #[test]
    fn a_fault_survives_the_anyhow_round_trip() {
        // The supervisor recovers the classification with `downcast_ref` through however
        // many `?` and `context` layers sit between it and the connector.
        let e: anyhow::Error = LiveError::err(LiveFault::Plan, "upgrade the plan");
        let wrapped = e.context("live session").context("supervisor");
        assert_eq!(fault_of(&wrapped), LiveFault::Plan);
        assert_eq!(fault_message(&wrapped), "upgrade the plan");
    }

    #[test]
    fn an_unclassified_error_is_transport_and_keeps_its_own_words() {
        let e = anyhow::anyhow!("socket closed");
        assert_eq!(fault_of(&e), LiveFault::Transport);
        assert!(fault_message(&e).contains("socket closed"));
    }

    #[test]
    fn only_the_faults_a_human_must_fix_are_terminal() {
        // Retrying these is a loop that never ends: they need a key, a plan, a subscription
        // or a different symbol.
        for f in [
            LiveFault::Auth,
            LiveFault::Plan,
            LiveFault::Entitlement,
            LiveFault::Symbol,
            LiveFault::Unsupported,
        ] {
            assert!(f.terminal(), "{f:?} should stop the supervisor");
        }
        // These clear on their own: a seat someone else is holding, a dropped socket.
        assert!(!LiveFault::Conflict.terminal());
        assert!(!LiveFault::Transport.terminal());
    }

    #[test]
    fn live_reach_matches_download_reach_where_there_is_a_transport() {
        // Every timeframe a streaming provider downloads, it also streams: the fold anchors
        // a session-shaped bucket on the provider's own bars instead of refusing it.
        for provider in ["binance", "kraken", "coinbase", "alpaca", "massive", "ibkr"] {
            let cap = crate::histdata::connector_for(provider).unwrap().capability();
            for tf in cap.timeframes {
                assert!(
                    cap.stream_timeframes.contains(tf),
                    "{provider} downloads {tf} but will not stream it"
                );
            }
        }
        assert!(stream_capable_for("binance", "crypto", "1d"));
        assert!(stream_capable_for("alpaca", "equity", "1d"));
        assert!(stream_capable_for("ibkr", "equity", "1w"));
        assert!(stream_capable_for("alpaca", "equity", "1w"));
        // Massive streams every market it downloads, futures cluster included.
        assert!(stream_capable_for("massive", "future", "1m"));
        // Reach is still narrower by *asset type*: Alpaca has no futures at all, and Yahoo
        // has no live transport.
        assert!(!stream_capable_for("alpaca", "future", "1m"));
        assert!(!stream_capable_for("yahoo", "equity", "1m"));
        // A refusal names which of the two it is.
        let why = stream_refusal("alpaca", "future", "1m").unwrap();
        assert!(why.contains("future"), "{why}");
        assert!(stream_refusal("yahoo", "equity", "1m").unwrap().contains("no live feed"));
    }

    #[test]
    fn gap_detection() {
        let a = datetime!(2026-07-23 19:00:00 UTC);
        // Immediate successor at 1m → no gap.
        assert!(missing_buckets(a, datetime!(2026-07-23 19:01:00 UTC), 60).is_empty());
        // Duplicate / older → no gap (idempotent / out-of-order).
        assert!(missing_buckets(a, a, 60).is_empty());
        assert!(missing_buckets(a, datetime!(2026-07-23 18:59:00 UTC), 60).is_empty());
        // Two buckets skipped → the two missing opens between them.
        assert_eq!(
            missing_buckets(a, datetime!(2026-07-23 19:03:00 UTC), 60),
            vec![
                datetime!(2026-07-23 19:01:00 UTC),
                datetime!(2026-07-23 19:02:00 UTC),
            ]
        );
    }
}

/// Live tests against the real (keyless) exchange WebSockets. Network-gated: they no-op
/// unless `OTW_LIVE_TEST=1`, so the normal `cargo test` stays offline and deterministic.
/// Run with: `OTW_LIVE_TEST=1 cargo test -p otw-core live_ -- --nocapture`.
#[cfg(test)]
mod net_tests {
    use super::*;
    use std::time::Duration;

    fn enabled() -> bool {
        std::env::var("OTW_LIVE_TEST").as_deref() == Ok("1")
    }

    /// Pull updates off a live socket until `max` collected or `budget` elapses.
    async fn collect(mut s: LiveStream, budget: Duration, max: usize) -> Vec<FeedUpdate> {
        let deadline = tokio::time::Instant::now() + budget;
        let mut out = Vec::new();
        while out.len() < max {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                break;
            }
            match tokio::time::timeout(left, s.next()).await {
                Ok(Some(Ok(e))) => out.push(e),
                Ok(Some(Err(e))) => panic!("stream error: {e:#}"),
                Ok(None) => break,
                Err(_) => break, // budget exhausted
            }
        }
        out
    }

    /// The bars among a batch of updates, with the channel they arrived on.
    fn bars(updates: &[FeedUpdate]) -> Vec<(&str, &LiveEvent)> {
        updates
            .iter()
            .filter_map(|u| match &u.payload {
                Payload::Bar(e) => Some((u.channel.as_str(), e)),
                _ => None,
            })
            .collect()
    }

    /// Open a keyless provider's socket the way the hub would. The keyed providers are not
    /// exercised here: they need the user's own credentials and an entitlement, and a test
    /// that silently passes because it was skipped is worse than no test.
    async fn open(provider: &str, instruments: &[Instrument]) -> LiveSession {
        let secrets = std::collections::HashMap::new();
        stream_connector_for(provider)
            .unwrap()
            .open(SessionRequest { secrets: &secrets, connector: provider, instruments })
            .await
            .unwrap()
    }

    /// OHLC sanity shared by every provider assertion.
    fn assert_valid(e: &LiveEvent) {
        assert!(e.open > 0.0 && e.high > 0.0 && e.low > 0.0 && e.close > 0.0, "prices > 0: {e:?}");
        assert!(e.high >= e.low, "high >= low: {e:?}");
        assert!(e.high >= e.open && e.high >= e.close, "high is the max: {e:?}");
        assert!(e.low <= e.open && e.low <= e.close, "low is the min: {e:?}");
        assert!(e.volume >= 0.0, "volume >= 0: {e:?}");
    }

    #[tokio::test]
    async fn live_binance_klines() {
        if !enabled() {
            return;
        }
        // 1s klines so a closed bar arrives within seconds (not a downloadable timeframe).
        let s = open("binance", &[Instrument::new("BTCUSDT", "crypto", "1s")]).await;
        let updates = collect(s.stream, Duration::from_secs(12), 40).await;
        let evs = bars(&updates);
        assert!(evs.len() >= 3, "expected several events, got {}", evs.len());
        assert!(evs.iter().any(|(_, e)| e.closed), "expected at least one closed bar");
        let mut prev = None;
        for (channel, e) in &evs {
            assert_valid(e);
            assert_eq!(*channel, "btcusdt@kline_1s", "every update names its channel");
            // 1s bars align to the second and never move backwards.
            assert_eq!(e.bar_ts.unix_timestamp() % 1, 0);
            if let Some(p) = prev {
                assert!(e.bar_ts >= p, "bar_ts monotonic non-decreasing");
            }
            prev = Some(e.bar_ts);
        }
    }

    /// The property the whole multiplexing change exists for: two instruments, one socket.
    #[tokio::test]
    async fn live_binance_two_symbols_share_one_socket() {
        if !enabled() {
            return;
        }
        let s = open(
            "binance",
            &[
                Instrument::new("BTCUSDT", "crypto", "1s"),
                Instrument::new("ETHUSDT", "crypto", "1s"),
            ],
        )
        .await;
        let updates = collect(s.stream, Duration::from_secs(15), 60).await;
        let evs = bars(&updates);
        assert!(evs.iter().any(|(c, _)| *c == "btcusdt@kline_1s"), "BTC bars on the socket");
        assert!(evs.iter().any(|(c, _)| *c == "ethusdt@kline_1s"), "ETH bars on the same socket");
    }

    #[tokio::test]
    async fn live_kraken_ohlc_snapshot() {
        if !enabled() {
            return;
        }
        let s = open("kraken", &[Instrument::new("BTC/USD", "crypto", "1m")]).await;
        // The snapshot alone yields many historical (closed) candles right away.
        let updates = collect(s.stream, Duration::from_secs(12), 50).await;
        let evs = bars(&updates);
        assert!(!evs.is_empty(), "expected snapshot events");
        assert!(evs.iter().any(|(_, e)| e.closed), "snapshot should mark historical bars closed");
        for (channel, e) in &evs {
            assert_valid(e);
            assert_eq!(*channel, "BTC/USD");
            assert_eq!(e.bar_ts.unix_timestamp() % 60, 0, "1m bars align to the minute");
        }
    }

    #[tokio::test]
    async fn coinbase_rest_seed_probe() {
        if !enabled() {
            return;
        }
        // Exercise the *REST* seed path (not WS) through the exact reqwest client + connector
        // the live seed uses, to isolate a seed 400 from a stream issue.
        let rest = crate::histdata::connector_for("coinbase").unwrap();
        let http = crate::histdata::client().unwrap();
        let now = OffsetDateTime::now_utc();
        let from = bar_start(now, 60) - time::Duration::seconds(60 * 298);
        let secrets = std::collections::HashMap::new();
        let r = rest
            .fetch_chunk(&http, &secrets, "BTC-USD", "crypto", "1m", from, now)
            .await;
        match r {
            Ok(c) => println!("coinbase REST seed OK: {} bars", c.bars.len()),
            Err(e) => panic!("coinbase REST seed FAILED: {e:#}"),
        }
    }

    #[tokio::test]
    async fn live_coinbase_trades_aggregate() {
        if !enabled() {
            return;
        }
        let s = open("coinbase", &[Instrument::new("BTC-USD", "crypto", "1m")]).await;
        let updates = collect(s.stream, Duration::from_secs(15), 20).await;
        assert!(!updates.is_empty(), "expected trades");
        // Coinbase publishes trades; the candle is the hub's, so fold them here the way it
        // would and check the result is the minute-aligned bar the chart draws.
        let mut agg = agg::Bucketer::new(60);
        let mut folded = Vec::new();
        for u in &updates {
            assert_eq!(u.channel, "BTC-USD");
            match u.payload {
                Payload::Trade { price, size, ts } => folded.extend(agg.trade(price, size, ts)),
                _ => panic!("coinbase publishes trades, not bars"),
            }
        }
        assert!(!folded.is_empty(), "trades should build a bar");
        for e in &folded {
            assert_valid(e);
            assert_eq!(e.bar_ts.unix_timestamp() % 60, 0, "1m buckets align to the minute");
        }
    }
}
