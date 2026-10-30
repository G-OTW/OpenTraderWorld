//! Interactive Brokers market data over a local IB Gateway or TWS (the TWS socket API).
//!
//! The one venue with no vendor URL and no key: the user runs IB Gateway (or TWS) and ticks
//! "Enable ActiveX and Socket Clients"; the connector carries an **address** (`host`,
//! `port`), stored in the clear because an address that cannot be read back cannot be
//! diagnosed. The client id is ours ([`session`]) and is never asked for.
//!
//! An adapter rather than a [`otw_connector_sdk::Mapper`]: the transport is the `ibapi`
//! client over one long-lived socket per gateway, with its own pacing (60 historical requests
//! per ten minutes per account). What it answers is translated by the pure [`mapper`]; the
//! adapter is driven by method calls in canonical terms (an instrument id, a timeframe, a
//! range) and answers canonical values. The same session serves OTW's derivatives tools,
//! which read [`session`], [`contract`] and [`resolve`] directly.
//!
//! The market rides the instrument id's venue ([`mapper::VENUES`]): a ticker alone cannot say
//! whether `BTC` is a share or a coin.

pub mod account;
pub mod contract;
pub mod events;
pub mod mapper;
pub mod margin;
pub mod marks;
pub mod orders;
pub mod resolve;
pub mod session;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, Candle, Capabilities, CapsError, ConnectorError, ConnectorId, DerivedFrom, Envelope, ErrorKind, Event, InstrumentId, InstrumentSpec, Price, Qty, Quote, TimeRange,
    Timeframe, Trade,
};
use tokio::sync::mpsc;

use contract::LiveSource;

const CAPS_TOML: &str = include_str!("../gateway.toml");
/// Safety valve on the slicing loop: a worker chunk needs a handful of IB requests, never an
/// unbounded number.
const MAX_SLICES: usize = 24;
/// Historical ticks replayed before the live ones start: none, the chart already holds them.
const TICK_BACKLOG: i32 = 0;
/// IB may drop the size of a tick it considers unchanged; size is volume here, so never.
const IGNORE_SIZE: bool = false;
/// How long a first bid/ask tick is waited for.
const QUOTE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
/// Updates a live line may queue before its reader catches up.
const LINE_BUFFER: usize = 512;

/// Which of IB's series a history reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Series {
    /// The market's own basis: trades, or the midpoint where IB has no tape (forex, crypto).
    Native,
    Bid,
    Ask,
}

#[derive(Clone)]
pub struct IbkrGateway {
    caps: Capabilities,
}

/// One instrument's live stream. Dropping it stops the subscription.
pub struct LiveLine {
    pub updates: mpsc::Receiver<Result<Envelope, ConnectorError>>,
    pub source: LiveSource,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for LiveLine {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn connector() -> ConnectorId {
    ConnectorId("ibkr".into())
}

impl IbkrGateway {
    pub fn new() -> Result<Self, CapsError> {
        Ok(IbkrGateway { caps: Capabilities::from_toml(CAPS_TOML)? })
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// The id of an OTW ticker in a market. The ticker is kept as typed (in capitals): the
    /// contract is built from it, and a future is resolved against the gateway before use.
    pub fn instrument_for(&self, symbol: &str, market: Option<AssetClass>) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim().to_uppercase();
        if s.is_empty() || s.chars().any(char::is_control) {
            return Err(ConnectorError::not_found(format!("Interactive Brokers: bad symbol `{symbol}`")));
        }
        let asset = match market {
            None | Some(AssetClass::Equity | AssetClass::Etf) => "equity",
            Some(AssetClass::Spot) => "crypto",
            Some(AssetClass::Forex) => "fx",
            Some(AssetClass::Index) => "index",
            Some(AssetClass::Future) => "future",
            Some(AssetClass::Option) => "option",
            Some(other) => return Err(ConnectorError::unsupported(format!("Interactive Brokers connector does not serve {other:?}"))),
        };
        Ok(InstrumentId::new(mapper::venue_of(asset), &s))
    }

    /// What IB last said about its link to its servers ([`session::Link`]), followed for the
    /// life of the process.
    pub async fn link(&self, settings: &HashMap<String, String>) -> Result<tokio::sync::watch::Receiver<session::Link>, ConnectorError> {
        Ok(session::get(settings).await?.link())
    }

    /// The latest live mark of each held contract that has one ([`marks`]), by contract id.
    pub async fn marks(&self, settings: &HashMap<String, String>, contract_ids: &[i32]) -> Result<HashMap<i32, f64>, ConnectorError> {
        let s = session::get(settings).await?;
        Ok(marks::marks(&s, contract_ids).await)
    }

    /// The money of the gateway's accounts ([`account::summary`]).
    pub async fn account_summary(&self, settings: &HashMap<String, String>) -> Result<Vec<account::Figure>, ConnectorError> {
        let s = session::get(settings).await?;
        account::summary(&s).await
    }

    /// Follow the account live ([`events`]): its order messages and position changes, on one
    /// feed per gateway.
    pub async fn follow(&self, settings: &HashMap<String, String>) -> Result<tokio::sync::broadcast::Receiver<events::Event>, ConnectorError> {
        Ok(events::follow(&session::get(settings).await?).await)
    }

    /// Whether the gateway is signed in to a live account (any IB account not numbered `D…`, or
    /// none named): orders on it move real money.
    pub async fn is_live(&self, settings: &HashMap<String, String>) -> Result<bool, ConnectorError> {
        let accounts = orders::accounts(&*session::get(settings).await?).await?;
        Ok(accounts.is_empty() || accounts.iter().any(|a| !orders::paper(a)))
    }

    /// Whether the gateway connection still answers ([`session::Session::probe`]).
    pub async fn probe(&self, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        session::get(settings).await?.probe().await
    }

    /// Send orders on one instrument (execution, v2): checked against the declared order
    /// capabilities, refused on a live account below E4, resolved on this gateway, kept on the
    /// contract's tick and size grid, dry run first on a live account's first order of the
    /// connection. Answers each order's IB id, and the tick a moved line has to snap to.
    pub async fn place_orders(
        &self,
        settings: &HashMap<String, String>,
        orders: &[otw_connector_model::OrderRequest],
        now: otw_connector_model::Timestamp,
    ) -> Result<(Vec<otw_connector_model::OrderAck>, Option<otw_connector_model::Decimal>), orders::PlaceError> {
        otw_connector_model::check_orders(&self.caps, orders, now)?;
        let s = session::get(settings).await?;
        orders::check_execution(&s, &self.caps, settings).await?;
        let id = &orders[0].instrument;
        let c = orders::tradable(resolve::contract_for(&s, &id.native, mapper::asset_type(id)).await?, &id.native)?;
        let rules = orders::rules_of(&s, &c).await?;
        margin::dry_run(&s, &c, orders).await?;
        margin::check(&s, &c, orders).await?;
        Ok((orders::place(&s, &c, orders, &rules).await?, rules.tick))
    }

    /// The ticket checked as `place_orders` checks it and sent to IB as a what-if, on paper and
    /// live alike: the dry run a maintainer runs before the live canary. Nothing is placed.
    pub async fn dry_run_orders(
        &self,
        settings: &HashMap<String, String>,
        orders: &[otw_connector_model::OrderRequest],
        now: otw_connector_model::Timestamp,
    ) -> Result<(), ConnectorError> {
        otw_connector_model::check_orders(&self.caps, orders, now)?;
        let s = session::get(settings).await?;
        let id = &orders[0].instrument;
        let c = orders::tradable(resolve::contract_for(&s, &id.native, mapper::asset_type(id)).await?, &id.native)?;
        let rules = orders::rules_of(&s, &c).await?;
        for o in orders {
            orders::on_grid(&rules, o, &orders::name_of(&c))?;
        }
        margin::what_if_all(&s, &c, orders).await
    }

    /// The order sent under client id `key`, as IB lists it today ([`orders::lookup`]).
    pub async fn lookup_order(&self, settings: &HashMap<String, String>, key: &str) -> Result<Option<orders::Seen>, ConnectorError> {
        orders::lookup(&*session::get(settings).await?, key).await
    }

    /// Change a working order in place: prices, quantity, time in force.
    pub async fn amend_order(
        &self,
        settings: &HashMap<String, String>,
        id: &str,
        change: otw_connector_model::OrderAmend,
        now: otw_connector_model::Timestamp,
    ) -> Result<(), ConnectorError> {
        otw_connector_model::check_amend(&self.caps, &change, now)?;
        let s = session::get(settings).await?;
        orders::check_execution(&s, &self.caps, settings).await?;
        orders::amend(&s, id, change).await
    }

    /// Cancel a working order. Never gated: cancelling is the safe direction.
    pub async fn cancel_order(&self, settings: &HashMap<String, String>, id: &str) -> Result<(), ConnectorError> {
        orders::cancel(&*session::get(settings).await?, id).await
    }

    /// Close the account's position in this contract (IB's id) at the market.
    pub async fn close_position(
        &self,
        settings: &HashMap<String, String>,
        contract_id: i32,
        account: &str,
        client_order_id: &str,
        now: otw_connector_model::Timestamp,
    ) -> Result<otw_connector_model::OrderAck, orders::PlaceError> {
        orders::close(&session::get(settings).await?, &self.caps, settings, contract_id, account, client_order_id, now).await
    }

    /// The kill switch: cancel every working order, and answer how many there were.
    pub async fn cancel_all_orders(&self, settings: &HashMap<String, String>) -> Result<usize, ConnectorError> {
        orders::cancel_all(&*session::get(settings).await?).await
    }

    /// A connector's settings name a gateway address.
    pub fn check_settings(&self, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        session::address(settings).map(|_| ())
    }

    /// A fresh handshake, reported: reuse would describe a socket opened minutes ago and hide
    /// the setting the user just changed.
    pub async fn test(&self, settings: &HashMap<String, String>) -> Result<String, ConnectorError> {
        let s = session::get(settings).await?;
        s.reset().await;
        let (client, client_id) = s.client().await?;
        let when = client.connection_time().map(|t| t.date().to_string()).unwrap_or_else(|| "unknown".into());
        Ok(format!("Connected to {} as client {client_id}. TWS API server version {}, gateway date {when}.", s.addr(), client.server_version()))
    }

    /// One bar series over `[from, to)`, walked in the windows IB accepts at this bar size,
    /// sorted and unique by stamp. The instrument is settled before the first bar is asked.
    pub async fn history(&self, settings: &HashMap<String, String>, id: &InstrumentId, tf: Timeframe, range: TimeRange, series: Series) -> Result<Vec<Candle>, ConnectorError> {
        let asset = mapper::asset_type(id);
        let size = contract::bar_size(tf)?;
        let tf_secs = tf.fixed_secs().ok_or_else(|| ConnectorError::unsupported(format!("Interactive Brokers has no {tf} bar")))?;
        let hours = contract::trading_hours(asset);
        let what = match series {
            Series::Native => contract::what_to_show(asset),
            Series::Bid => ibapi::market_data::historical::WhatToShow::Bid,
            Series::Ask => ibapi::market_data::historical::WhatToShow::Ask,
        };
        let max_window = contract::max_window_secs(tf);
        let s = session::get(settings).await?;
        let c = resolve::contract_for(&s, &id.native, asset).await?;
        let mut out = Vec::new();
        let mut cursor = range.from;
        let mut slices = 0;
        while cursor < range.to {
            slices += 1;
            if slices > MAX_SLICES {
                // A short answer would leave a hole the worker's cursor steps over: refused.
                return Err(ConnectorError::invalid(format!("{} {tf}: this window needs more than {MAX_SLICES} Interactive Brokers requests, download a shorter range", id.native)));
            }
            let slice_to = cursor.plus_secs(max_window).min(range.to);
            // IB asks for an end plus a duration, never a start.
            let span = contract::duration_back(slice_to.secs().saturating_sub(cursor.secs()), tf_secs);
            let end = slice_to.to_offset_datetime().ok_or_else(|| ConnectorError::invalid(format!("time {slice_to} out of range")))?;
            let (cc, w) = (c.clone(), what);
            let data = s
                .call(&format!("{} {tf}", id.native), move |client| {
                    let cc = cc.clone();
                    async move { client.historical_data(&cc, Some(end), span, size, Some(w), hours).await }
                })
                .await?;
            for b in &data.bars {
                out.push(mapper::candle(id, tf, b)?);
            }
            cursor = slice_to;
        }
        // A bar can land in two adjacent windows.
        out.sort_by_key(|c| c.open_ts);
        out.dedup_by_key(|c| c.open_ts);
        Ok(out)
    }

    /// The market right now: the first tick of IB's bid/ask stream.
    pub async fn quote(&self, settings: &HashMap<String, String>, id: &InstrumentId) -> Result<Quote, ConnectorError> {
        let s = session::get(settings).await?;
        let c = resolve::contract_for(&s, &id.native, mapper::asset_type(id)).await?;
        let (client, _) = s.client().await?;
        let ticker = &id.native;
        let mut sub = client
            .tick_by_tick_bid_ask(&c, 0, true)
            .await
            .map_err(|e| ConnectorError::new(mapper::request_error(ticker, &e).kind, format!("{ticker}: Interactive Brokers refused the bid/ask stream: {e}")))?;
        match tokio::time::timeout(QUOTE_WAIT, sub.next()).await {
            Ok(Some(Ok(t))) if t.bid_price > 0.0 && t.ask_price > 0.0 => {
                Ok(Quote { instrument: id.clone(), bid: Price(mapper::dec(t.bid_price)?), ask: Price(mapper::dec(t.ask_price)?), bid_qty: None, ask_qty: None })
            }
            Ok(Some(Err(e))) => Err(ConnectorError::new(mapper::request_error(ticker, &e).kind, format!("{ticker}: Interactive Brokers bid/ask: {e}"))),
            _ => Err(ConnectorError::new(ErrorKind::Transient, format!("{ticker}: Interactive Brokers sent no bid/ask within 5 s"))),
        }
    }

    /// Instruments matching a query, in IB's order. `market` narrows the kinds (an ETF is STK
    /// to IB, so it keeps the equities). Futures come from the contract database, not from
    /// IB's symbol samples, which answer with underlyings.
    pub async fn search(&self, settings: &HashMap<String, String>, query: &str, market: Option<AssetClass>, limit: usize) -> Result<Vec<InstrumentSpec>, ConnectorError> {
        let needle = query.trim().to_string();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let s = session::get(settings).await?;
        if market == Some(AssetClass::Future) {
            return search_futures(&s, &needle, limit).await;
        }
        let hits = s
            .call("symbol search", move |client| {
                let needle = needle.clone();
                async move { client.matching_symbols(&needle).await }
            })
            .await?;
        let wanted = |kind: &str| match market {
            None => true,
            Some(AssetClass::Equity | AssetClass::Etf) => kind == "equity",
            Some(m) => mapper::class_of(kind) == m,
        };
        Ok(mapper::search_hits(&hits).into_iter().filter(|(k, _)| wanted(k)).map(|(_, spec)| spec).take(limit).collect())
    }

    /// One instrument's live stream: prints where IB streams them, the midpoint where it has
    /// no tape, five-second bars where it serves no ticks ([`contract::live_source`]). Not
    /// through the session's pacing: a streaming subscription is not a historical request.
    pub async fn stream(&self, settings: &HashMap<String, String>, id: &InstrumentId, connector_name: &str) -> Result<LiveLine, ConnectorError> {
        let asset = mapper::asset_type(id);
        let s = session::get(settings).await?;
        // A futures root lists on several exchanges, and a live subscription to the wrong one
        // is silence, not an error.
        let c = resolve::contract_for(&s, &id.native, asset).await?;
        let (client, _) = s.client().await?;
        let source = contract::live_source(asset);
        let (tx, rx) = mpsc::channel(LINE_BUFFER);
        let fail = |e: ibapi::Error| mapper::live_error(s.addr(), &id.native, connector_name, &e);
        let line = Line { connection: client.clone(), session: s.clone(), symbol: id.native.clone(), connector: connector_name.to_string(), tx };
        let instrument = id.clone();
        let task = match source {
            LiveSource::LastTick => {
                let sub = client.tick_by_tick_last(&c, TICK_BACKLOG, IGNORE_SIZE).await.map_err(fail)?;
                drain(sub, line, move |t| {
                    Ok(Envelope::new(connector(), None, mapper::ts(t.time)?, Event::Trade(Trade { instrument: instrument.clone(), id: None, price: Price(mapper::dec(t.price)?), qty: Qty(mapper::dec(t.size.max(0.0))?), aggressor: None })).at(mapper::ts(t.time)?))
                })
            }
            LiveSource::MidPointTick => {
                let sub = client.tick_by_tick_midpoint(&c, TICK_BACKLOG, IGNORE_SIZE).await.map_err(fail)?;
                // A midpoint has no trade behind it, so no size: its candles have no volume,
                // exactly like the stored midpoint bars.
                drain(sub, line, move |m| {
                    Ok(Envelope::new(connector(), None, mapper::ts(m.time)?, Event::Trade(Trade { instrument: instrument.clone(), id: None, price: Price(mapper::dec(m.mid_point)?), qty: Qty(otw_connector_model::Decimal::ZERO), aggressor: None }))
                        .at(mapper::ts(m.time)?)
                        .derived(DerivedFrom::Mid))
                })
            }
            LiveSource::Bars5s => {
                let what = contract::realtime_what_to_show(asset);
                let sub = client.realtime_bars(&c, ibapi::market_data::realtime::BarSize::Sec5, what, contract::trading_hours(asset)).await.map_err(fail)?;
                let tf = Timeframe { n: 5, unit: otw_connector_model::TfUnit::Second };
                drain(sub, line, move |b| {
                    let bar = ibapi::market_data::historical::Bar { date: b.date, open: b.open, high: b.high, low: b.low, close: b.close, volume: b.volume, wap: b.wap, count: b.count };
                    let mut candle = mapper::candle(&instrument, tf, &bar)?;
                    // A five-second bar arrives once its five seconds are over.
                    candle.is_final = true;
                    Ok(Envelope::new(connector(), None, candle.open_ts, Event::Candle(candle)))
                })
            }
        };
        Ok(LiveLine { updates: rx, source, task })
    }

    /// One instrument's best bid and ask: `reqMktData`, ticks 1 (bid) and 2 (ask). That is a
    /// market-data line like any watched instrument, not one of the account's few
    /// tick-by-tick slots (a second tick-by-tick stream would halve the live charts an
    /// account can open). Delayed ticks (66, 67) are not a live quote and are ignored; a
    /// refusal (354, no subscription) fails this line alone.
    pub async fn stream_quotes(&self, settings: &HashMap<String, String>, id: &InstrumentId, connector_name: &str) -> Result<LiveLine, ConnectorError> {
        use ibapi::contracts::tick_types::TickType;
        use ibapi::market_data::realtime::TickTypes;
        let asset = mapper::asset_type(id);
        let s = session::get(settings).await?;
        let c = resolve::contract_for(&s, &id.native, asset).await?;
        let (client, _) = s.client().await?;
        let fail = |e: ibapi::Error| mapper::live_error(s.addr(), &id.native, connector_name, &e);
        let sub = client.market_data(&c).subscribe().await.map_err(fail)?;
        let (tx, rx) = mpsc::channel(LINE_BUFFER);
        let line = Line { connection: client.clone(), session: s.clone(), symbol: id.native.clone(), connector: connector_name.to_string(), tx };
        let instrument = id.clone();
        let (mut bid, mut ask) = (None::<f64>, None::<f64>);
        let task = drain_some(sub, line, move |t: TickTypes| {
            let (kind, price) = match t {
                TickTypes::Price(p) => (p.tick_type, p.price),
                TickTypes::PriceSize(p) => (p.price_tick_type, p.price),
                _ => return None,
            };
            // IB sends -1 (or 0) for a side with nothing on it.
            let price = (price > 0.0).then_some(price);
            match kind {
                TickType::Bid => bid = price,
                TickType::Ask => ask = price,
                _ => return None,
            }
            let (Some(b), Some(a)) = (bid, ask) else { return None };
            let now = otw_connector_model::Timestamp::from_offset_datetime(time::OffsetDateTime::now_utc())?;
            Some((|| {
                let q = Quote { instrument: instrument.clone(), bid: Price(mapper::dec(b)?), ask: Price(mapper::dec(a)?), bid_qty: None, ask_qty: None };
                Ok(Envelope::new(connector(), None, now, Event::Quote(q)))
            })())
        });
        Ok(LiveLine { updates: rx, source: contract::live_source(asset), task })
    }
}

async fn search_futures(s: &session::Session, needle: &str, limit: usize) -> Result<Vec<InstrumentSpec>, ConnectorError> {
    // A half-typed ticker is not an error to show under the search box.
    let Ok(mut partial) = contract::build(needle, "future") else {
        return Ok(Vec::new());
    };
    // With no month a root builds the continuous series, one contract rather than a list.
    if partial.security_type == ibapi::contracts::SecurityType::ContinuousFuture {
        partial.security_type = ibapi::contracts::SecurityType::Future;
    }
    let details = match s
        .call("futures search", move |client| {
            let partial = partial.clone();
            async move { client.contract_details(&partial).await }
        })
        .await
    {
        Ok(d) => d,
        Err(e) if mapper::unknown_contract(&e) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    // Once per trading class; identity is the contract id. By month: the front one first.
    let mut found: Vec<ibapi::contracts::Contract> = details.into_iter().map(|d| d.contract).collect();
    found.sort_by(|a, b| (&a.last_trade_date_or_contract_month, &a.exchange.0, a.contract_id).cmp(&(&b.last_trade_date_or_contract_month, &b.exchange.0, b.contract_id)));
    found.dedup_by_key(|c| c.contract_id);
    Ok(found.iter().take(limit).map(mapper::future_hit).collect())
}

/// What a line's drain task holds once the subscription is open.
struct Line {
    /// The subscription is fed by the connection's message bus, so the connection outlives it
    /// even if a `reset` elsewhere drops the session's copy.
    connection: Arc<ibapi::Client>,
    session: Arc<session::Session>,
    symbol: String,
    connector: String,
    tx: mpsc::Sender<Result<Envelope, ConnectorError>>,
}

/// Drain one subscription until it ends, the socket dies, or the line is dropped. A socket
/// that died takes the session with it (code [`mapper::DROPPED`]); anything else is this
/// instrument's own refusal.
fn drain<T, F>(sub: ibapi::subscriptions::Subscription<T>, line: Line, translate: F) -> tokio::task::JoinHandle<()>
where
    T: Send + 'static,
    F: Fn(T) -> Result<Envelope, ConnectorError> + Send + 'static,
{
    drain_some(sub, line, move |v| Some(translate(v)))
}

/// [`drain`] for a stream where not every item is news (`None` is skipped), and whose
/// translation keeps state between items.
fn drain_some<T, F>(mut sub: ibapi::subscriptions::Subscription<T>, line: Line, mut translate: F) -> tokio::task::JoinHandle<()>
where
    T: Send + 'static,
    F: FnMut(T) -> Option<Result<Envelope, ConnectorError>> + Send + 'static,
{
    tokio::spawn(async move {
        let Line { connection, session, symbol, connector, tx } = line;
        let _connection = connection;
        while let Some(item) = sub.next().await {
            match item {
                Ok(v) => {
                    let Some(env) = translate(v) else { continue };
                    if tx.send(env).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    if session::dropped(&e) {
                        session.reset().await;
                    }
                    let _ = tx.send(Err(mapper::live_error(session.addr(), &symbol, &connector, &e))).await;
                    return;
                }
            }
        }
        // Ended on its own: a transport fault, so the hub retries rather than freezing a pane.
        let _ = tx.send(Err(ConnectorError::new(ErrorKind::Transient, format!("Interactive Brokers ended the {symbol} subscription")))).await;
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_parse_and_markets_are_ids() {
        let g = IbkrGateway::new().unwrap();
        assert_eq!(g.capabilities().provider, "ibkr");
        assert_eq!(g.instrument_for(" mnqz6 ", Some(AssetClass::Future)).unwrap(), InstrumentId::new("IBKR_FUTURE", "MNQZ6"));
        assert_eq!(g.instrument_for("AAPL", None).unwrap().venue.0, "IBKR");
        assert!(g.instrument_for("X", Some(AssetClass::Bond)).is_err() && g.instrument_for(" ", None).is_err());
        assert!(g.check_settings(&HashMap::new()).is_err());
    }
}
