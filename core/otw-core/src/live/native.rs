//! Live feeds of venues ported to the connector framework, served through the same
//! [`StreamConnector`] / [`Feed`] pair as every other provider, so the hub (seeding, folding,
//! anchoring, recording, reconnecting) cannot tell the difference.
//!
//! The venue's mapper says what to send and how to read what comes back; the socket itself is
//! still [`run_feed`]'s. Which grain a timeframe streams from (native candles, smaller candles
//! folded up, or trades) is the framework planner's answer over the venue's declared
//! capabilities, not a per-venue table.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use otw_connector_engine::{plan, Prefs, Request, Step};
use otw_connector_model::{Command, ConnectorError, CorrId, DataType, ErrorKind, Event, Timeframe};
use otw_connector_sdk::{Clock, Driver, InstrumentCache, MapCtx, Mapper, NativeRequest, RawFrame, SystemClock};
use time::OffsetDateTime;

use super::{
    run_feed, Feed, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveQuote, LiveSession, SessionRequest,
    SourceGrain, StreamConnector,
};

static NEXT_SUB: AtomicU64 = AtomicU64::new(1);

/// What a framework error means to the live supervisor.
fn fault(e: &ConnectorError) -> LiveFault {
    match e.kind {
        ErrorKind::Auth => LiveFault::Auth,
        ErrorKind::Permission => LiveFault::Entitlement,
        ErrorKind::NotFound => LiveFault::Symbol,
        ErrorKind::Unsupported => LiveFault::Unsupported,
        ErrorKind::RateLimited => LiveFault::Conflict,
        _ => LiveFault::Transport,
    }
}

fn live_err(e: ConnectorError) -> anyhow::Error {
    LiveError::err(fault(&e), e.message)
}

fn f(d: otw_connector_model::Decimal) -> Result<f64> {
    d.to_string().parse::<f64>().map_err(|e| anyhow!("number {d}: {e}"))
}

fn secs(t: otw_connector_model::Timestamp) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(t.secs()).map_err(|e| anyhow!("time {t}: {e}"))
}

fn millis(t: otw_connector_model::Timestamp) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(t.millis()) * 1_000_000).map_err(|e| anyhow!("time {t}: {e}"))
}

/// The data type a socket subscribes to for candles at `tf`, and who folds the rest.
fn source(caps: &otw_connector_model::Capabilities, tf: Timeframe, grace: i64) -> Result<(DataType, SourceGrain)> {
    let p = plan(&Request::LiveCandles { tf }, caps, &Prefs::default())
        .map_err(|u| LiveError::err(LiveFault::Unsupported, u.0))?;
    match p.steps.as_slice() {
        // A venue that publishes one grain only (Alpaca's minute) feeds the hub's fold at
        // every timeframe, its own included, so every pane on it closes the same way.
        [Step::Live(DataType::Candles { tf: t })] if *t == tf && caps.live.candle_timeframes.as_ref().is_some_and(|g| g.len() == 1) => {
            let secs = t.fixed_secs().ok_or_else(|| anyhow!("{t} has no fixed length to fold from"))?;
            Ok((DataType::Candles { tf }, SourceGrain::Bars { secs, grace }))
        }
        [Step::Live(DataType::Candles { tf: t })] if *t == tf => Ok((DataType::Candles { tf }, SourceGrain::Native)),
        [Step::Live(DataType::Candles { tf: t }), Step::Aggregate { .. }] => {
            let secs = t.fixed_secs().ok_or_else(|| anyhow!("{t} has no fixed length to fold from"))?;
            Ok((DataType::Candles { tf: *t }, SourceGrain::Bars { secs, grace }))
        }
        [Step::Live(DataType::Trades), Step::CandlesFromTrades { .. }] => Ok((DataType::Trades, SourceGrain::Trades)),
        other => Err(LiveError::err(
            LiveFault::Unsupported,
            format!("{}: {tf} would stream as {other:?}, which the live hub does not run", caps.label),
        )),
    }
}

fn label(dt: &DataType) -> String {
    match dt {
        DataType::Candles { tf } => tf.to_string(),
        DataType::Trades => "trades".into(),
        other => format!("{other:?}"),
    }
}

pub struct NativeStream<M: Mapper> {
    driver: Arc<Driver<M>>,
    /// How long past a bucket's end the hub waits before closing a folded bar.
    grace_secs: i64,
    /// Books a connector setting chooses between (the socket opened is the chosen one's).
    variants: Vec<crate::histdata::native::Variant<M>>,
    setting: Option<&'static str>,
}

impl<M: Mapper> NativeStream<M> {
    pub fn new(driver: Arc<Driver<M>>, grace_secs: i64) -> Self {
        NativeStream { driver, grace_secs, variants: Vec::new(), setting: None }
    }

    /// A connector whose setting picks the book; channels and grain are the first book's
    /// (the books stream the same timeframes).
    pub fn with_variants(setting: &'static str, variants: Vec<crate::histdata::native::Variant<M>>, grace_secs: i64) -> Option<Self> {
        let driver = variants.first()?.driver.clone();
        Some(NativeStream { driver, grace_secs, variants, setting: Some(setting) })
    }

    fn sub_of(&self, inst: &Instrument) -> Result<(otw_connector_model::InstrumentId, DataType, SourceGrain, String)> {
        let id = crate::histdata::native::instrument_of(self.driver.mapper(), &inst.symbol, &inst.asset_type)
            .map_err(|e| LiveError::err(LiveFault::Symbol, format!("{e:#}")))?;
        if inst.is_quote() {
            if self.quote_source(inst).is_none() {
                let label = &self.driver.mapper().capabilities().label;
                return Err(LiveError::err(LiveFault::Unsupported, format!("{label} streams no bid and ask")));
            }
            let channel = quote_channel(&id.native);
            return Ok((id, DataType::Quote, SourceGrain::Native, channel));
        }
        let tf: Timeframe = inst.timeframe.parse().map_err(|e| LiveError::err(LiveFault::Unsupported, format!("{e}")))?;
        // A market whose socket carries prints and no candles: every timeframe folds from them.
        if self.driver.mapper().live_trades_only(&id) {
            let channel = format!("{}|{}", id.native, label(&DataType::Trades));
            return Ok((id, DataType::Trades, SourceGrain::Trades, channel));
        }
        let (dt, grain) = source(self.driver.mapper().capabilities(), tf, self.grace_secs)?;
        let channel = format!("{}|{}", id.native, label(&dt));
        Ok((id, dt, grain, channel))
    }
}

#[async_trait::async_trait]
impl<M: Mapper> StreamConnector for NativeStream<M> {
    /// The adapter says what it cannot carry on one socket.
    fn socket_class(&self, inst: &Instrument) -> Result<String> {
        let (id, dt, _, _) = self.sub_of(inst)?;
        Ok(self.driver.mapper().socket_class(&id, &dt))
    }

    fn grain(&self, inst: &Instrument) -> Result<SourceGrain> {
        Ok(self.sub_of(inst)?.2)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(self.sub_of(inst)?.3)
    }

    /// A venue that pushes a quote channel is subscribed to it; one whose streamed candles
    /// carry both sides gives it for free, read off the candles already coming in.
    fn quote_source(&self, _inst: &Instrument) -> Option<&'static str> {
        let live = &self.driver.mapper().capabilities().live;
        if live.quote == otw_connector_model::LiveMode::Push {
            Some("native")
        } else if live.candle_sides {
            Some("candle_sides")
        } else {
            None
        }
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        let driver = if self.variants.is_empty() {
            self.driver.clone()
        } else {
            let label = self.driver.mapper().capabilities().label.clone();
            crate::histdata::native::pick(&self.variants, self.setting, &label, req.secrets)
                .map_err(|e| LiveError::err(LiveFault::Unsupported, format!("{e:#}")))?
                .driver
                .clone()
        };
        // A venue whose feed is keyed logs in with the connector's credentials.
        let signer = if driver.mapper().capabilities().auth == otw_connector_model::AuthKind::None {
            None
        } else {
            driver.mapper().signer(req.secrets).map_err(|e| LiveError::err(LiveFault::Auth, e.message))?
        };
        // A socket that rides the REST session needs it open before the first frame.
        if let Some(s) = &signer {
            driver.ensure_token(s.as_ref()).await.map_err(|e| LiveError::err(LiveFault::Auth, e.message))?;
        }
        if driver.mapper().live_requires_instruments() {
            driver
                .load_instruments_as(CorrId(NEXT_SUB.fetch_add(1, Ordering::Relaxed)), signer.as_deref())
                .await
                .map_err(live_err)?;
        }
        // A venue that hands out its socket's address per connection is asked for it first;
        // the answer lands in the feed's own session.
        let mut session = M::Session::default();
        if let Some(h) = driver.mapper().socket_bootstrap(CorrId(NEXT_SUB.fetch_add(1, Ordering::Relaxed)), &mut session) {
            let frame = driver.send_as(h, signer.as_deref()).await.map_err(live_err)?;
            let cache = driver.instruments().map_err(live_err)?;
            let ctx = MapCtx { received_ts: SystemClock.now(), instruments: &cache, connector: driver.connector() };
            driver.mapper().decode(&frame, &mut session, &ctx).map_err(live_err)?;
        }
        let feed = NativeFeed::with_session(driver, self.grace_secs, req.instruments, signer, session)?;
        run_feed(feed, req.instruments).await
    }
}

/// An instrument's quote channel.
fn quote_channel(native: &str) -> String {
    format!("{native}|quote")
}

struct Channel {
    /// `None`: a quote read off the candles, which costs no subscription of its own.
    sub_id: Option<CorrId>,
    /// Instrument keys riding this channel (several timeframes can fold from one).
    holders: HashSet<String>,
}

/// One socket: its own mapper session, the channels it carries, and the frames waiting to
/// be written.
pub(crate) struct NativeFeed<M: Mapper> {
    stream: NativeStream<M>,
    session: M::Session,
    url: String,
    channels: HashMap<String, Channel>,
    by_sub: HashMap<CorrId, String>,
    queued: Vec<String>,
    cache: InstrumentCache,
    /// The forming candle per channel, for a venue with no closed flag: the engine closes it
    /// when the next period starts.
    forming: HashMap<String, LiveEvent>,
    /// The account's credentials, for a venue whose socket logs in with a message.
    signer: Option<Arc<dyn otw_connector_sdk::Signer>>,
    /// The socket speaks MessagePack (the adapter says so for the path it opens).
    msgpack: bool,
}

impl<M: Mapper> NativeFeed<M> {
    pub(crate) fn new(driver: Arc<Driver<M>>, grace_secs: i64, opening: &[Instrument]) -> Result<Self> {
        Self::with_session(driver, grace_secs, opening, None, M::Session::default())
    }

    /// A feed holding the account's credentials from the start (a venue that signs every
    /// frame signs the opening subscriptions too), on a session the socket's bootstrap
    /// answer has already filled.
    pub(crate) fn with_session(
        driver: Arc<Driver<M>>,
        grace_secs: i64,
        opening: &[Instrument],
        signer: Option<Arc<dyn otw_connector_sdk::Signer>>,
        session: M::Session,
    ) -> Result<Self> {
        let cache = driver.instruments().map_err(live_err)?;
        let mut feed = NativeFeed {
            stream: NativeStream::new(driver, grace_secs),
            session,
            url: String::new(),
            channels: HashMap::new(),
            by_sub: HashMap::new(),
            queued: Vec::new(),
            cache,
            forming: HashMap::new(),
            signer,
            msgpack: false,
        };
        let frames = feed.join(opening)?;
        if feed.url.is_empty() {
            return Err(anyhow!("{}: the adapter opened no socket", feed.stream.driver.mapper().capabilities().label));
        }
        feed.queued.extend(frames);
        Ok(feed)
    }

    pub(crate) fn with_signer(mut self, signer: Option<Arc<dyn otw_connector_sdk::Signer>>) -> Self {
        self.signer = signer;
        self
    }

    /// The login frame the signer writes for this socket.
    fn login(&self) -> Result<String> {
        let signer = self.signer.as_ref().ok_or_else(|| LiveError::err(LiveFault::Auth, "this feed logs in with the connector's key and none is set"))?;
        let frame = signer
            .ws_login(SystemClock.now())
            .map_err(live_err)?
            .ok_or_else(|| anyhow!("the venue asked for a socket login its signer does not write"))?;
        String::from_utf8(frame.to_vec()).map_err(|_| anyhow!("login frame is not text"))
    }

    /// A frame the venue authenticates per message, completed by the signer.
    fn signed(&self, payload: bytes::Bytes) -> Result<String> {
        let signer = self.signer.as_ref().ok_or_else(|| LiveError::err(LiveFault::Auth, "this feed signs its frames with the connector's key and none is set"))?;
        let frame = signer.sign_frame(payload, SystemClock.now()).map_err(live_err)?;
        String::from_utf8(frame.to_vec()).map_err(|_| anyhow!("signed frame is not text"))
    }

    fn encode(&mut self, cmds: &[Command]) -> Result<Vec<String>> {
        if cmds.is_empty() {
            return Ok(Vec::new());
        }
        let driver = self.stream.driver.clone();
        let ctx = MapCtx { received_ts: SystemClock.now(), instruments: &self.cache, connector: driver.connector() };
        let reqs = driver.mapper().encode_batch(cmds, &mut self.session, &ctx).map_err(live_err)?;
        let mut frames = Vec::new();
        for r in reqs {
            match r {
                NativeRequest::WsOpen { base, path, .. } => {
                    if !self.url.is_empty() {
                        return Err(anyhow!("the adapter asked for a second socket on a single-socket feed"));
                    }
                    self.msgpack = driver.mapper().ws_msgpack(&path);
                    self.url = format!("{}{path}", driver.base_url(base).map_err(live_err)?);
                }
                NativeRequest::WsOpenUrl { url, .. } => {
                    if !self.url.is_empty() {
                        return Err(anyhow!("the adapter asked for a second socket on a single-socket feed"));
                    }
                    self.url = url;
                }
                NativeRequest::WsSend { payload, .. } => {
                    frames.push(String::from_utf8(payload.to_vec()).map_err(|_| anyhow!("adapter wrote a non-text frame"))?);
                }
                NativeRequest::WsSigned { payload, .. } => frames.push(self.signed(payload)?),
                NativeRequest::WsClose { .. } => {}
                NativeRequest::WsLogin { .. } => frames.push(self.login()?),
                NativeRequest::Http(_) => return Err(anyhow!("an HTTP request on the socket path")),
            }
        }
        Ok(frames)
    }

    /// Subscribe what is not carried yet; a timeframe folding from a channel already open
    /// costs nothing upstream.
    fn join(&mut self, instruments: &[Instrument]) -> Result<Vec<String>> {
        let mut cmds = Vec::new();
        for inst in instruments {
            let (id, dt, _, channel) = self.stream.sub_of(inst)?;
            if let Some(c) = self.channels.get_mut(&channel) {
                c.holders.insert(inst.key());
                continue;
            }
            if dt == DataType::Quote && self.stream.quote_source(inst) == Some("candle_sides") {
                self.channels.insert(channel, Channel { sub_id: None, holders: HashSet::from([inst.key()]) });
                continue;
            }
            let sub_id = CorrId(NEXT_SUB.fetch_add(1, Ordering::Relaxed));
            self.channels.insert(channel.clone(), Channel { sub_id: Some(sub_id), holders: HashSet::from([inst.key()]) });
            self.by_sub.insert(sub_id, channel);
            cmds.push(Command::Subscribe { sub_id, instrument: id, data_type: dt });
        }
        self.encode(&cmds)
    }

    fn leave(&mut self, instruments: &[Instrument]) -> Result<Vec<String>> {
        let mut cmds = Vec::new();
        for inst in instruments {
            let Ok((_, _, _, channel)) = self.stream.sub_of(inst) else { continue };
            let Some(c) = self.channels.get_mut(&channel) else { continue };
            c.holders.remove(&inst.key());
            if c.holders.is_empty() {
                self.forming.remove(&channel);
                let sub_id = c.sub_id;
                self.channels.remove(&channel);
                if let Some(sub_id) = sub_id {
                    self.by_sub.remove(&sub_id);
                    cmds.push(Command::Unsubscribe { sub_id });
                }
            }
        }
        self.encode(&cmds)
    }
}

impl<M: Mapper> NativeFeed<M> {
    /// A venue with no closed flag: a newer period closes the one in hand, an older row is
    /// not news.
    fn roll(&mut self, channel: String, ev: LiveEvent) -> Vec<FeedUpdate> {
        let mut out = Vec::new();
        match self.forming.get(&channel) {
            Some(cur) if ev.bar_ts < cur.bar_ts => return out,
            Some(cur) if ev.bar_ts > cur.bar_ts => {
                let mut closed = cur.clone();
                closed.closed = true;
                out.push(FeedUpdate::bar(channel.clone(), closed));
            }
            _ => {}
        }
        self.forming.insert(channel.clone(), ev.clone());
        out.push(FeedUpdate::bar(channel, ev));
        out
    }
}

impl<M: Mapper> Feed for NativeFeed<M> {
    fn url(&self) -> String {
        self.url.clone()
    }

    fn subscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        match self.join(instruments) {
            Ok(frames) => frames,
            Err(e) => {
                tracing::warn!("live subscribe refused: {e:#}");
                Vec::new()
            }
        }
    }

    fn unsubscribe(&mut self, instruments: &[Instrument]) -> Vec<String> {
        self.leave(instruments).unwrap_or_default()
    }

    fn on_message(&mut self, text: &str) -> Result<Vec<FeedUpdate>> {
        self.on_binary(text.as_bytes())
    }

    fn msgpack(&self) -> bool {
        self.msgpack
    }

    /// Text and binary frames alike go to the adapter as bytes: it knows its encoding.
    fn on_binary(&mut self, payload: &[u8]) -> Result<Vec<FeedUpdate>> {
        let driver = self.stream.driver.clone();
        let frame = RawFrame::Ws {
            conn_key: otw_connector_sdk::ConnKey("main".into()),
            payload: bytes::Bytes::copy_from_slice(payload),
        };
        let ctx = MapCtx { received_ts: SystemClock.now(), instruments: &self.cache, connector: driver.connector() };
        let events = driver.mapper().decode(&frame, &mut self.session, &ctx).map_err(live_err)?;
        let mut out = Vec::new();
        for env in events {
            let Some(channel) = env.corr_id.and_then(|c| self.by_sub.get(&c)).cloned() else {
                if let Event::Error(e) = &env.event {
                    tracing::warn!("live {}: {}", driver.mapper().capabilities().provider, e.message);
                }
                continue;
            };
            match env.event {
                Event::Candle(c) => {
                    // Both sides on the candle: the forming one's closes are the quote, for
                    // whoever holds this instrument's quote channel.
                    if let Some(sides) = c.sides.filter(|_| !c.is_final) {
                        let qc = quote_channel(&c.instrument.native);
                        if self.channels.contains_key(&qc) {
                            let at = millis(env.exchange_ts.unwrap_or(env.received_ts))?;
                            if let Some(q) = LiveQuote::new(f(sides.bid.close.0)?, f(sides.ask.close.0)?, at) {
                                out.push(FeedUpdate::quote(qc, q));
                            }
                        }
                    }
                    let ev = LiveEvent {
                        bar_ts: secs(c.open_ts)?,
                        open: f(c.open.0)?,
                        high: f(c.high.0)?,
                        low: f(c.low.0)?,
                        close: f(c.close.0)?,
                        volume: f(c.volume.0)?,
                        closed: c.is_final,
                        // The venue's event time, to the millisecond so the lag reads true;
                        // when it sends none, when we heard it.
                        provider_ts: millis(env.exchange_ts.unwrap_or(env.received_ts))?,
                    };
                    if driver.mapper().capabilities().live.candles_final_flag {
                        out.push(FeedUpdate::bar(channel, ev));
                    } else {
                        out.extend(self.roll(channel, ev));
                    }
                }
                Event::Trade(t) => {
                    // To the millisecond: trades of one second keep their order in the fold.
                    let ts = millis(env.exchange_ts.unwrap_or(env.received_ts))?;

                    out.push(FeedUpdate::trade(channel, f(t.price.0)?, f(t.qty.0)?, ts));
                }
                Event::Quote(q) => {
                    let at = millis(env.exchange_ts.unwrap_or(env.received_ts))?;
                    if let Some(q) = LiveQuote::new(f(q.bid.0)?, f(q.ask.0)?, at) {
                        out.push(FeedUpdate::quote(channel, q));
                    }
                }
                Event::SubscriptionFail { error } => out.push(FeedUpdate::fault(channel, &live_err(error))),
                Event::Error(e) => tracing::warn!("live {}: {}", driver.mapper().capabilities().provider, e.message),
                _ => {}
            }
        }
        Ok(out)
    }

    fn drain_sends(&mut self) -> Vec<String> {
        let driver = self.stream.driver.clone();
        let ctx = MapCtx { received_ts: SystemClock.now(), instruments: &self.cache, connector: driver.connector() };
        for r in driver.mapper().keepalive(&mut self.session, &ctx) {
            match r {
                NativeRequest::WsSend { payload, .. } => {
                    if let Ok(t) = String::from_utf8(payload.to_vec()) {
                        self.queued.push(t);
                    }
                }
                NativeRequest::WsLogin { .. } => match self.login() {
                    Ok(t) => self.queued.push(t),
                    Err(e) => tracing::warn!("live login: {e:#}"),
                },
                NativeRequest::WsSigned { payload, .. } => match self.signed(payload) {
                    Ok(t) => self.queued.push(t),
                    Err(e) => tracing::warn!("live signed frame: {e:#}"),
                },
                _ => {}
            }
        }
        std::mem::take(&mut self.queued)
    }
}

// ── Registered venues ─────────────────────────────────────────────────────────

pub fn binance() -> Option<NativeStream<otw_connector_providers::binance::Binance>> {
    Some(NativeStream::new(crate::histdata::native::binance_driver()?, 2))
}

pub fn bitget() -> Option<NativeStream<otw_connector_providers::bitget::Bitget>> {
    NativeStream::with_variants("market", crate::histdata::native::bitget_variants()?, 2)
}

pub fn gateio() -> Option<NativeStream<otw_connector_providers::gateio::Gateio>> {
    NativeStream::with_variants("market", crate::histdata::native::gateio_variants()?, 2)
}

pub fn mexc() -> Option<NativeStream<otw_connector_providers::mexc::Mexc>> {
    NativeStream::with_variants("market", crate::histdata::native::mexc_variants()?, 2)
}

pub fn bitfinex() -> Option<NativeStream<otw_connector_providers::bitfinex::Bitfinex>> {
    NativeStream::with_variants("market", crate::histdata::native::bitfinex_variants()?, 2)
}

pub fn deribit() -> Option<NativeStream<otw_connector_providers::deribit::Deribit>> {
    Some(NativeStream::new(crate::histdata::native::deribit_driver()?, 2))
}

pub fn kucoin() -> Option<NativeStream<otw_connector_providers::kucoin::Kucoin>> {
    Some(NativeStream::new(crate::histdata::native::kucoin_driver()?, 2))
}

pub fn kucoin_futures() -> Option<NativeStream<otw_connector_providers::kucoin::Kucoin>> {
    Some(NativeStream::new(crate::histdata::native::kucoin_futures_driver()?, 2))
}

pub fn bybit() -> Option<NativeStream<otw_connector_providers::bybit::Bybit>> {
    NativeStream::with_variants("market", crate::histdata::native::bybit_variants()?, 2)
}

pub fn bybit_eu() -> Option<NativeStream<otw_connector_providers::bybit::Bybit>> {
    Some(NativeStream::new(crate::histdata::native::bybit_eu_driver()?, 2))
}

pub fn okx() -> Option<NativeStream<otw_connector_providers::okx::Okx>> {
    Some(NativeStream::new(crate::histdata::native::okx_driver()?, 2))
}

pub fn coinbase() -> Option<NativeStream<otw_connector_providers::coinbase::Coinbase>> {
    Some(NativeStream::new(crate::histdata::native::coinbase_driver()?, 2))
}

/// Alpaca: one-minute bars, the `feed` setting picks the socket's entitlement.
pub fn alpaca() -> Option<NativeStream<otw_connector_providers::alpaca::Alpaca>> {
    NativeStream::with_variants("feed", crate::histdata::native::alpaca_variants()?, 4)
}

/// A minute aggregate is published a few seconds after its minute.
pub fn massive() -> Option<NativeStream<otw_connector_providers::massive::Massive>> {
    Some(NativeStream::new(crate::histdata::native::massive_driver()?, 8))
}

/// Capital.com: the socket rides the REST session of the platform the `env` setting picks.
pub fn capitalcom() -> Option<NativeStream<otw_connector_providers::capitalcom::CapitalCom>> {
    NativeStream::with_variants("env", crate::histdata::native::capitalcom_variants()?, 2)
}

pub fn kraken() -> Option<NativeStream<otw_connector_providers::kraken::Kraken>> {
    Some(NativeStream::new(crate::histdata::native::kraken_driver()?, 2))
}

pub fn binance_futures() -> Option<NativeStream<otw_connector_providers::binance::Binance>> {
    Some(NativeStream::new(crate::histdata::native::binance_futures_driver()?, 2))
}

pub(crate) mod ibkr;

/// Interactive Brokers live through the framework's gateway adapter.
pub fn ibkr() -> Option<ibkr::NativeIbkrLive> {
    Some(ibkr::NativeIbkrLive::new(otw_connector_providers::ibkr::gateway::IbkrGateway::new().ok()?))
}

#[cfg(test)]
mod parity_tests;

#[cfg(test)]
mod quote_tests;
