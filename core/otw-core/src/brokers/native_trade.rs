//! Orders on venues ported to the connector framework, through the shared trader
//! ([`otw_connector_engine::trading::Trader`]) and the venue's [`TradeMapper`]: a venue adds
//! orders by writing its mapper and fixtures, nothing here.
//!
//! The account is read as before (the wrapped read broker, every book it reads); each order
//! goes to the book of its instrument:
//! - **The instrument's book.** The chart's data connector names it when it is the venue's own
//!   (its `market` setting, handed over as [`super::INSTRUMENT_BOOK`]); otherwise the one book
//!   that lists the symbol. A symbol two books list (Bybit `BTCUSDT`, spot pair and perpetual)
//!   with no connector to say which is refused with the fix, never guessed.
//! - **Ids.** An order is `BOOK:SYMBOL:venue id` and a position `BOOK:SYMBOL:side`: a cancel, a
//!   change and a close go to the book named there.
//! - **Live below E4.** Refused, except on the live canary and, at E2D, on an account whose owner
//!   acknowledged that the broker was never tested against the venue (the core marks it,
//!   [`super::LIVE_UNTESTED`]); there the venue's own check runs before the first order of the
//!   session where the venue has one. A sandbox account (the venue's testnet) takes orders at
//!   any level, as on Binance.
//! - **The protocol is the trader's**: rules before I/O, one turn per instrument, a lost answer
//!   looked up and never resent ([`super::OrderUnknown`]), a ticket never left half placed.
//!   Exact decimals only (`float_arithmetic` denied here, as in the trader).

#![deny(clippy::float_arithmetic)]

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Result};
use otw_connector_engine::trading::{Account, AttachedExit, TicketOptions, TradeError, Trader};
use otw_connector_model::{Decimal, DryRunCap, ErrorKind, ExecLevel, Mapped, OrderAmend, OrderCaps, OrderRequest, Side};
use otw_connector_sdk::trade::{ClientRef, ExitKind, TradeMapper};
use otw_connector_sdk::{Driver, Signer};

use super::native::{NativeBroker, NativeMultiBroker};
use super::{AccountSummary, Broker, BrokerCapability, ExecQuery, Execution, Holding, Lookup, MintedId, OpenOrder, OrderTicket, OrderUnknown, Placed, Position, Quote, QuoteSized, StoredLeg};

/// An answer from the venue that nobody mapped (an unknown status word, an unreadable answer),
/// on a call that names no order leg.
#[derive(Debug, Clone)]
pub struct UnmappedAnswer(pub String);

impl std::fmt::Display for UnmappedAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnmappedAnswer {}

/// One book that takes orders: its name (as the data connector's `market` and the account's
/// `books` setting spell it), its read broker (the account's driver, key and hints) and its
/// trader (shared by every account on the book).
pub struct TradeBook<M: TradeMapper + Clone> {
    pub name: &'static str,
    pub broker: NativeBroker<M>,
    pub trader: Arc<Trader<M>>,
}

/// A venue account read by `read` and traded on `books`, each order on its instrument's book.
pub struct NativeTradingBroker<M: TradeMapper + Clone, R: Broker> {
    read: R,
    books: Vec<TradeBook<M>>,
    /// The setting naming the books the account reads (`books`): which private streams it
    /// follows.
    setting: &'static str,
    /// Other spellings of a book's name the venue's data connector (`market`) or the account's
    /// `books` setting accepts, each with the book it names (`futures` for `usdt-futures`).
    aliases: &'static [(&'static str, &'static str)],
}

fn lock_scope(key: &str, sandbox: bool, venue: &str) -> String {
    let h = hex::encode(otw_connector_sdk::signer::sha256(key.trim().as_bytes()));
    format!("{venue}:{sandbox}:{}", &h[..12])
}

/// The settings that name an account's key, the first one set: `api_key` on most venues,
/// Coinbase's `key_name` (its CDP key's name). Only hashed ([`lock_scope`]), never logged.
const KEY_IDENTITY: &[&str] = &["api_key", "key_name"];

fn key_identity(s: &HashMap<String, String>) -> &str {
    KEY_IDENTITY.iter().find_map(|k| s.get(*k).map(|v| v.trim()).filter(|v| !v.is_empty())).unwrap_or("")
}

/// `BOOK`, `SYMBOL` and the rest of an id OTW follows (`BOOK:SYMBOL:rest`), the symbol free to
/// hold colons (Bitfinex `tBTCF0:USTF0`, `tAVAX:USD`): the book ends at the first colon; the rest
/// starts at a tagged id (`cond:`, `tpsl:`, which carry colons of their own), else after the last
/// colon (a venue id, a position's side).
fn split_book_id(id: &str) -> Option<(&str, &str, &str)> {
    let (book, rest) = id.trim().split_once(':')?;
    let tagged = [":cond:", ":tpsl:"].iter().filter_map(|t| rest.find(t)).min();
    let cut = tagged.or_else(|| rest.rfind(':'))?;
    let (symbol, tail) = (&rest[..cut], &rest[cut + 1..]);
    (!book.is_empty() && !symbol.is_empty() && !tail.is_empty()).then_some((book, symbol, tail))
}

impl<M: TradeMapper + Clone, R: Broker> NativeTradingBroker<M, R> {
    pub fn new(read: R, setting: &'static str, books: Vec<TradeBook<M>>) -> Self {
        NativeTradingBroker { read, books, setting, aliases: &[] }
    }

    /// The other spellings of the books' names, as the data connector's `market` setting takes
    /// them (`(alias, book)`).
    pub fn aliases(mut self, aliases: &'static [(&'static str, &'static str)]) -> Self {
        self.aliases = aliases;
        self
    }

    /// A book name as written in a setting, an alias resolved to the book it names.
    fn book_name<'a>(&self, name: &'a str) -> &'a str {
        let name = name.trim();
        self.aliases.iter().find(|(a, _)| a.eq_ignore_ascii_case(name)).map(|(_, b)| *b).unwrap_or(name)
    }

    fn label(&self) -> &'static str {
        self.read.capability().label
    }

    fn named(&self, name: &str) -> Option<&TradeBook<M>> {
        let name = self.book_name(name);
        self.books.iter().find(|b| b.name.eq_ignore_ascii_case(name))
    }

    /// `BOOK:SYMBOL:id`, the id an order is followed by.
    fn split_id<'a>(&self, id: &'a str) -> Result<(&TradeBook<M>, &'a str, &'a str)> {
        let Some((book, symbol, venue_id)) = split_book_id(id) else {
            return Err(anyhow!("order id {id:?} is not BOOK:SYMBOL:id: read the account again"));
        };
        let b = self.named(book).ok_or_else(|| anyhow!("{} takes no orders on its {book} book", self.label()))?;
        Ok((b, symbol, venue_id))
    }

    /// The book of `ticker`: the one the chart's data connector names, else the one book that
    /// lists it. Two books listing it with nothing to choose between them is refused.
    async fn book_for(&self, s: &HashMap<String, String>, ticker: &str) -> Result<&TradeBook<M>> {
        if let Some(hint) = s.get(super::INSTRUMENT_BOOK).map(|v| v.trim()).filter(|v| !v.is_empty()) {
            return self.named(hint).ok_or_else(|| anyhow!("{} takes no orders on its {hint} book from OpenTraderWorld", self.label()));
        }
        let mut listing = Vec::new();
        for b in &self.books {
            let (d, signer, scope) = self.session(b, s)?;
            let symbol = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
            let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
            match b.trader.rules(&d, &a, &symbol).await {
                Ok(_) => listing.push(b),
                Err(TradeError::Refused(e)) if e.kind == ErrorKind::NotFound => {}
                Err(e) => return Err(self.fail(b, s, e)),
            }
        }
        match listing.as_slice() {
            [one] => Ok(one),
            [] => Err(anyhow!("{} lists {ticker} on none of the books it takes orders on", self.label())),
            several => Err(anyhow!(
                "{ticker} trades on {} books ({}): open the chart on a {} data connector set to the one you mean, so the order goes to it",
                self.label(),
                several.iter().map(|b| b.name).collect::<Vec<_>>().join(" and "),
                self.label()
            )),
        }
    }

    /// The book a leg was sent on: the one its order id or position id names
    /// (`BOOK:SYMBOL:...`), else the instrument's.
    async fn book_of_ticker(&self, s: &HashMap<String, String>, ticker: &str) -> Result<(&TradeBook<M>, String)> {
        if let Some((book, symbol, _)) = split_book_id(ticker) {
            if let Some(b) = self.named(book) {
                return Ok((b, symbol.to_string()));
            }
        }
        let b = self.book_for(s, ticker).await?;
        let symbol = b.broker.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
        Ok((b, symbol))
    }

    fn level(b: &TradeBook<M>) -> ExecLevel {
        b.broker.caps().orders.level
    }

    /// Live orders below E4 only on the canary, or at E2D on an account the core marked.
    fn check_env(&self, b: &TradeBook<M>, s: &HashMap<String, String>) -> Result<bool> {
        let live = !b.broker.mapper().is_sandbox(s);
        let level = Self::level(b);
        if !live || level >= ExecLevel::E4 || s.get(super::LIVE_CANARY).is_some_and(|v| v == "1") {
            return Ok(live);
        }
        if level == ExecLevel::E2D && s.get(super::LIVE_UNTESTED).is_some_and(|v| v == "1") {
            return Ok(live);
        }
        if level == ExecLevel::E2D {
            return Err(anyhow!(
                "{} orders reach a live account only once live trading is turned on for it with the acknowledgement that this broker was never tested against the venue; otherwise set the account to the testnet",
                self.label()
            ));
        }
        Err(anyhow!("{} orders go to the testnet only until its order path is rated E4 (it is {level:?}). Set the account to the testnet.", self.label()))
    }

    /// The account's driver, key and per-account scope on book `b`.
    fn session(&self, b: &TradeBook<M>, s: &HashMap<String, String>) -> Result<(Arc<Driver<M>>, Arc<dyn Signer>, String)> {
        let d = b.broker.on(s)?;
        let signer = b.broker.signer(s)?.ok_or_else(|| anyhow!("{} needs an API key", self.label()))?;
        let scope = lock_scope(key_identity(s), d.mapper().is_sandbox(s), d.mapper().capabilities().venue.as_str());
        Ok((d, signer, scope))
    }

    fn fail(&self, b: &TradeBook<M>, s: &HashMap<String, String>, e: TradeError) -> anyhow::Error {
        match e {
            TradeError::Refused(c) => b.broker.hint(s, c),
            TradeError::Unknown { keys, message, alert } if !keys.is_empty() => anyhow::Error::new(OrderUnknown { keys, message, alert }),
            TradeError::Unknown { message, alert: true, .. } => anyhow::Error::new(UnmappedAnswer(message)),
            TradeError::Unknown { message, .. } => anyhow!(message),
        }
    }

    fn requests(&self, d: &Driver<M>, ticket: &OrderTicket) -> Result<Vec<OrderRequest>> {
        let id = d.mapper().instrument_id(&ticket.ticker).map_err(anyhow::Error::new)?;
        ticket.legs.iter().map(|l| l.request(&id)).collect()
    }

    /// The book a read row belongs to, by the venue label it carries.
    fn book_of_row(&self, venue: &str) -> Option<&TradeBook<M>> {
        self.books.iter().find(|b| b.broker.caps().account.venue_label.as_deref() == Some(venue))
    }

    /// The books the account reads, by its setting (every one when it is empty).
    fn read_books(&self, s: &HashMap<String, String>) -> Vec<&TradeBook<M>> {
        let raw = s.get(self.setting).map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
        if raw.is_empty() {
            return self.books.iter().collect();
        }
        let parts: Vec<&str> = raw.split(',').map(|p| self.book_name(p)).collect();
        self.books.iter().filter(|b| parts.contains(&b.name)).collect()
    }

    /// Show the exits the venue attached to an entry as it lists them: the conditional orders a
    /// position's take profit or stop loss creates (`stopOrderType` `TakeProfit`/`StopLoss`),
    /// linked to their entry by `parentOrderLinkId` while it still works, reduce-only.
    async fn mark_exits(&self, b: &TradeBook<M>, s: &HashMap<String, String>, rows: &mut [OpenOrder]) {
        let Ok((d, signer, scope)) = self.session(b, s) else { return };
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        let venue = match b.trader.open_orders(&d, &a, None).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("{} {}: attached exits not read ({e}), shown as plain orders", self.label(), b.name);
                return;
            }
        };
        for o in rows.iter_mut() {
            let Some(v) = venue.iter().find(|v| v.venue_id == o.id) else { continue };
            let Some(kind) = v.exit else { continue };
            o.order_type = match kind {
                ExitKind::TakeProfit => "TakeProfit".into(),
                ExitKind::StopLoss => "StopLoss".into(),
            };
            o.reduce_only = true;
            o.parent_id = v
                .parent_client_id
                .as_deref()
                .and_then(|p| venue.iter().find(|e| e.client_id.as_deref() == Some(p)))
                .map(|e| format!("{}:{}:{}", b.name, e.symbol, e.venue_id));
        }
    }

    /// One book's private streams (one per socket the venue has for it, merged), or why it has
    /// none.
    async fn stream(&self, b: &TradeBook<M>, s: &HashMap<String, String>) -> Result<Option<futures::stream::BoxStream<'static, super::AccountEvent>>> {
        let (d, signer, scope) = self.session(b, s)?;
        let sockets = d.mapper().private_sockets(s);
        if sockets.is_empty() {
            return Ok(None);
        }
        if let Err(e) = signer.ws_login(d.venue_now()) {
            tracing::info!("{} {}: no private stream ({}), read on the cycle", self.label(), b.name, e.message);
            return Ok(None);
        }
        let mut streams = Vec::new();
        for (i, socket) in sockets.into_iter().enumerate() {
            let follow = super::native_trade_events::Follow {
                book: b.name,
                driver: d.clone(),
                signer: signer.clone(),
                trader: b.trader.clone(),
                scope: scope.clone(),
                settings: s.clone(),
                socket,
                venue: b.broker.caps().account.venue_label.clone().unwrap_or_else(|| self.label().to_string()),
                asset_class: self.read.capability().asset_classes.first().copied().unwrap_or("").to_string(),
                prefill: i == 0,
            };
            streams.push(super::native_trade_events::follow(follow).await.map_err(|e| b.broker.hint(s, e))?);
        }
        Ok(Some(if streams.len() == 1 { streams.remove(0) } else { Box::pin(futures::stream::select_all(streams)) }))
    }
}

#[async_trait::async_trait]
impl<M: TradeMapper + Clone, R: Broker> Broker for NativeTradingBroker<M, R> {
    fn capability(&self) -> &'static BrokerCapability {
        self.read.capability()
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        self.read.validate_config(config)
    }

    async fn test(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<String> {
        self.read.test(client, s).await
    }

    fn chart_fills(&self) -> bool {
        self.read.chart_fills()
    }

    async fn executions(&self, client: &reqwest::Client, s: &HashMap<String, String>, q: &ExecQuery) -> Result<Vec<Execution>> {
        self.read.executions(client, s, q).await
    }

    async fn symbols(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<String>> {
        self.read.symbols(client, s).await
    }

    /// Every position the account holds, as read; those on a book that takes orders close by
    /// `BOOK:SYMBOL:side`.
    async fn positions(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<Position>> {
        let mut rows = self.read.positions(client, s).await?;
        for p in rows.iter_mut() {
            if let Some(b) = self.book_of_row(&p.venue) {
                p.native_id = Some(format!("{}:{}:{}", b.name, p.symbol, p.side));
            }
        }
        Ok(rows)
    }

    async fn held_asset(&self, s: &HashMap<String, String>, ticker: &str) -> Result<Option<Decimal>> {
        self.read.held_asset(s, ticker).await
    }

    /// Every working order, as read; those on a book that takes orders under the ids orders are
    /// followed by (`BOOK:SYMBOL:venue id`; a conditional one `BOOK:SYMBOL:cond:venue id`, a
    /// position's take profit or stop loss `BOOK:SYMBOL:tpsl:...`).
    async fn orders(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<OpenOrder>> {
        let mut rows = self.read.orders(client, s).await?;
        for b in self.books.iter().filter(|b| b.broker.caps().orders.brackets) {
            let conditional = rows.iter().any(|o| o.stop_price.is_some() && self.book_of_row(&o.venue).is_some_and(|x| x.name == b.name));
            if conditional {
                self.mark_exits(b, s, &mut rows).await;
            }
        }
        for o in rows.iter_mut() {
            if self.split_id(&o.id).is_ok() {
                continue;
            }
            if let Some(b) = self.book_of_row(&o.venue) {
                o.id = format!("{}:{}:{}", b.name, o.symbol, o.id);
            }
        }
        Ok(rows)
    }

    async fn marks(&self, s: &HashMap<String, String>, ids: &[String]) -> Result<HashMap<String, f64>> {
        self.read.marks(s, ids).await
    }

    async fn account(&self, s: &HashMap<String, String>, symbol: Option<&str>) -> Result<Vec<AccountSummary>> {
        self.read.account(s, symbol).await
    }

    /// The venue's private stream of every book the account reads that takes orders, merged;
    /// the read broker's (none, for Bybit) when none has one (no stream on this host, a key
    /// whose socket login is not documented): the account is then read on the cycle.
    async fn events(&self, s: &HashMap<String, String>) -> Result<Option<futures::stream::BoxStream<'static, super::AccountEvent>>> {
        let mut streams = Vec::new();
        for b in self.read_books(s) {
            match self.stream(b, s).await {
                Ok(Some(st)) => streams.push(st),
                Ok(None) => {}
                Err(e) if streams.is_empty() && self.read_books(s).len() == 1 => return Err(e),
                Err(e) => tracing::warn!("{} {}: private stream not opened ({e:#}), read on the cycle", self.label(), b.name),
            }
        }
        if streams.is_empty() {
            return self.read.events(s).await;
        }
        Ok(Some(Box::pin(futures::stream::select_all(streams))))
    }

    /// The chart just opened this instrument on the account: read its rules and position mode
    /// on its book now, so an order on it waits for neither.
    async fn symbol_for(&self, s: &HashMap<String, String>, ticker: &str, asset_type: &str) -> Result<Option<String>> {
        let Ok(b) = self.book_for(s, ticker).await else { return self.read.symbol_for(s, ticker, asset_type).await };
        let Ok((d, signer, scope)) = self.session(b, s) else { return self.read.symbol_for(s, ticker, asset_type).await };
        let symbol = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        if let Err(e) = b.trader.warm(&d, &a, &symbol).await {
            tracing::info!("{}: {symbol} not warmed ({e}), read on its first order", self.label());
        }
        Ok(Some(symbol))
    }

    /// After a restart: the attached exits OTW follows are known again from their stored ids.
    async fn resume(&self, s: &HashMap<String, String>, legs: &[StoredLeg]) -> Result<()> {
        for l in legs {
            let Some(id) = l.id.as_deref().filter(|i| i.contains(":tpsl:")) else { continue };
            let Ok((b, symbol, venue_id)) = self.split_id(id) else { continue };
            let Some(x) = AttachedExit::parse(symbol, venue_id) else { continue };
            let (_, signer, scope) = self.session(b, s)?;
            let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
            b.trader.register_exit(&a, &l.key, x);
        }
        Ok(())
    }

    /// A venue whose client ids OTW mints (Bitfinex): one per key, minted before anything
    /// leaves, for the caller to store. Nothing, and no read, for every other venue.
    async fn mint_client_ids(&self, s: &HashMap<String, String>, ticker: &str, keys: &[String]) -> Result<Vec<MintedId>> {
        if !self.books.iter().any(|b| b.broker.mapper().mints_client_ids()) {
            return Ok(Vec::new());
        }
        let (b, _) = self.book_of_ticker(s, ticker).await?;
        let (d, signer, scope) = self.session(b, s)?;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        let mut out = Vec::new();
        for k in keys {
            if let Some(r) = b.trader.mint(&d, &a, k).map_err(|e| self.fail(b, s, e))? {
                out.push(MintedId { key: k.clone(), id: r.id, at_ms: r.at_ms.unwrap_or_default(), ticker: ticker.to_string(), order_id: None });
            }
        }
        Ok(out)
    }

    /// After a restart: each stored minted id held again by its book's trader.
    async fn resume_client_ids(&self, s: &HashMap<String, String>, ids: &[MintedId]) -> Result<()> {
        for m in ids {
            let found = match m.order_id.as_deref().map(|i| self.split_id(i)) {
                Some(Ok((b, _, venue_id))) => Some((b, Some(venue_id.to_string()))),
                _ => self.book_of_ticker(s, &m.ticker).await.ok().map(|(b, _)| (b, None)),
            };
            let Some((b, venue_id)) = found else { continue };
            let (_, signer, scope) = self.session(b, s)?;
            let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
            b.trader.hold_client(&a, &m.key, ClientRef { id: m.id.clone(), at_ms: Some(m.at_ms) }, venue_id.as_deref());
        }
        Ok(())
    }

    /// A book sized in the quote currency (Deribit inverse contracts): its currency, its coin
    /// and its step, from the venue's rules.
    async fn quote_sized(&self, s: &HashMap<String, String>, ticker: &str) -> Result<Option<QuoteSized>> {
        let Ok(b) = self.book_for(s, ticker).await else { return Ok(None) };
        let (d, signer, scope) = self.session(b, s)?;
        let symbol = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        let r = b.trader.rules(&d, &a, &symbol).await.map_err(|e| self.fail(b, s, e))?;
        if r.size_unit != otw_connector_model::SizeUnit::Quote {
            return Ok(None);
        }
        let spec = d.instruments().ok().and_then(|c| c.get(&r.instrument).cloned());
        Ok(Some(QuoteSized {
            quote: spec.as_ref().map(|i| i.quote.clone()).unwrap_or_default(),
            base: spec.and_then(|i| i.base).unwrap_or_default(),
            step: r.lot_step,
        }))
    }

    async fn holdings(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<Holding>> {
        self.read.holdings(client, s).await
    }

    async fn quotes(&self, client: &reqwest::Client, s: &HashMap<String, String>, symbols: &[String]) -> Result<Vec<Quote>> {
        self.read.quotes(client, s, symbols).await
    }

    async fn place(&self, s: &HashMap<String, String>, ticket: &OrderTicket) -> Result<Vec<Placed>> {
        let b = self.book_for(s, &ticket.ticker).await?;
        let live = self.check_env(b, s)?;
        let (d, signer, scope) = self.session(b, s)?;
        let requests = self.requests(&d, ticket)?;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        let opt = TicketOptions { reference: ticket.reference, dry_run_first: live && Self::level(b) == ExecLevel::E2D };
        let placed = b.trader.place(&d, &a, &requests, &opt).await.map_err(|e| self.fail(b, s, e))?;
        for note in placed.iter().filter_map(|p| p.note.as_deref()) {
            tracing::info!("{}: {note}", self.label());
        }
        let tick = b.trader.rules(&d, &a, &requests[0].instrument.native).await.ok().and_then(|r| r.tick);
        Ok(placed.into_iter().map(|p| Placed { key: p.key, id: format!("{}:{}:{}", b.name, p.symbol, p.venue_id), tick }).collect())
    }

    /// A working order in place (where the venue declares it), or an attached exit's price on the
    /// position it guards.
    async fn modify(&self, s: &HashMap<String, String>, id: &str, change: &OrderAmend) -> Result<Option<Placed>> {
        let (b, symbol, venue_id) = self.split_id(id)?;
        self.check_env(b, s)?;
        let (d, signer, scope) = self.session(b, s)?;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        if let Some(x) = AttachedExit::parse(symbol, venue_id) {
            let price = match x.kind {
                ExitKind::TakeProfit => change.limit.or(change.stop),
                ExitKind::StopLoss => change.stop.or(change.limit),
            };
            let price = price.ok_or_else(|| anyhow!("an attached take profit or stop loss changes its price only"))?;
            b.trader.change_exit(&d, &a, &x, Some(price.value())).await.map_err(|e| self.fail(b, s, e))?;
            return Ok(None);
        }
        // A venue that changes an order by replacing it answers the new order: followed under it.
        let moved = b.trader.modify(&d, &a, symbol, venue_id, change).await.map_err(|e| self.fail(b, s, e))?;
        Ok(moved.map(|p| Placed { key: p.key, id: format!("{}:{}:{}", b.name, p.symbol, p.venue_id), tick: None }))
    }

    /// An order already final at the venue (filled before the cancel reached it) is no error.
    async fn cancel(&self, s: &HashMap<String, String>, id: &str) -> Result<()> {
        let (b, symbol, venue_id) = self.split_id(id)?;
        let (d, signer, scope) = self.session(b, s)?;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        // An attached exit is removed from the position it guards.
        if let Some(x) = AttachedExit::parse(symbol, venue_id) {
            return b.trader.change_exit(&d, &a, &x, None).await.map_err(|e| self.fail(b, s, e));
        }
        b.trader.cancel(&d, &a, symbol, venue_id).await.map(|_| ()).map_err(|e| self.fail(b, s, e))
    }

    /// The kill switch, on every book that takes orders: each one is tried, and a failure on
    /// one is said after the others were cancelled.
    async fn cancel_all(&self, s: &HashMap<String, String>) -> Result<usize> {
        let (mut n, mut failed) = (0, Vec::new());
        for b in &self.books {
            let (d, signer, scope) = self.session(b, s)?;
            let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
            match b.trader.cancel_all(&d, &a).await {
                Ok(k) => n += k,
                Err(e) => failed.push(format!("{}: {:#}", b.name, self.fail(b, s, e))),
            }
        }
        if failed.is_empty() {
            Ok(n)
        } else {
            Err(anyhow!("{} cancelled {n} order(s), and could not cancel on {}", self.label(), failed.join("; ")))
        }
    }

    /// The order sent under `key`: on its instrument's book when that can be told, else on every
    /// book that lists the symbol (a client id is the account's once, so it is on one of them).
    async fn lookup(&self, s: &HashMap<String, String>, ticker: &str, _asset_type: &str, key: &str) -> Result<Lookup> {
        let books: Vec<&TradeBook<M>> = match self.book_for(s, ticker).await {
            Ok(b) => vec![b],
            Err(_) => self.books.iter().collect(),
        };
        for b in books {
            let (d, signer, scope) = self.session(b, s)?;
            let symbol = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
            let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
            // An attached exit is followed through its entry, under the id it was placed with.
            if let Some(x) = b.trader.exit_of(&a, key) {
                let Some(o) = b.trader.exit_status(&d, &a, &x).await.map_err(|e| self.fail(b, s, e))? else { return Ok(Lookup::NotFound) };
                let status = o.status.known().copied().ok_or_else(|| anyhow!("{}: the exit {key} is in a state OpenTraderWorld does not know", self.label()))?;
                let fills_as = (!o.venue_id.starts_with("tpsl:")).then(|| o.venue_id.clone());
                return Ok(Lookup::Found { id: format!("{}:{}:{}", b.name, x.symbol, x.id()), status, filled_qty: o.filled_qty, fills_as });
            }
            let Some(o) = b.trader.lookup(&d, &a, &symbol, key).await.map_err(|e| self.fail(b, s, e))? else { continue };
            return match o.status {
                Mapped::Known(status) => Ok(Lookup::Found { id: format!("{}:{}:{}", b.name, b.trader.followed_name(&d, &a, &symbol), o.venue_id), status, filled_qty: o.filled_qty, fills_as: Some(o.venue_id) }),
                Mapped::Unknown(w) => Err(anyhow::Error::new(UnmappedAnswer(format!("{}: order {key} is in a state OpenTraderWorld does not know ({w})", self.label())))),
            };
        }
        Ok(Lookup::NotFound)
    }

    /// At the market, reduce-only, the size the venue holds now: `BOOK:SYMBOL:long` /
    /// `BOOK:SYMBOL:short` as the position was read, or `SYMBOL` on the instrument's book.
    async fn close_position(&self, s: &HashMap<String, String>, native_id: &str, _account: &str, key: &str) -> Result<Placed> {
        let native_id = native_id.trim();
        // `BOOK:SYMBOL:side` as the position was read; anything whose first part names no book
        // is a bare symbol (which may hold colons, Bitfinex `tBTCF0:USTF0`).
        let (b, symbol, side) = match split_book_id(native_id).filter(|(book, _, _)| self.named(book).is_some()) {
            Some((book, symbol, side)) => {
                let b = self.named(book).ok_or_else(|| anyhow!("{} takes no orders on its {book} book", self.label()))?;
                let side = match side {
                    "long" => Side::Buy,
                    "short" => Side::Sell,
                    other => return Err(anyhow!("a position is closed by BOOK:SYMBOL:long or BOOK:SYMBOL:short, not {other:?}")),
                };
                (b, symbol, Some(side))
            }
            None if !native_id.is_empty() => (self.book_for(s, native_id).await?, native_id, None),
            None => return Err(anyhow!("a position is closed by BOOK:SYMBOL:side, as it was read ({native_id:?} is not)")),
        };
        self.check_env(b, s)?;
        let (d, signer, scope) = self.session(b, s)?;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        let p = b.trader.close_position(&d, &a, symbol, side, None, key).await.map_err(|e| self.fail(b, s, e))?;
        Ok(Placed { key: p.key, id: format!("{}:{}:{}", b.name, p.symbol, p.venue_id), tick: None })
    }

    async fn dry_run(&self, s: &HashMap<String, String>, ticket: &OrderTicket) -> Result<String> {
        let b = self.book_for(s, &ticket.ticker).await?;
        let (d, signer, scope) = self.session(b, s)?;
        let requests = self.requests(&d, ticket)?;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        b.trader.dry_run(&d, &a, &requests, ticket.reference).await.map_err(|e| self.fail(b, s, e))
    }

    fn has_dry_run(&self) -> bool {
        self.books.iter().any(|b| b.broker.caps().orders.dry_run != DryRunCap::None)
    }

    async fn is_live(&self, s: &HashMap<String, String>) -> Result<bool> {
        Ok(self.books.first().is_none_or(|b| !b.broker.mapper().is_sandbox(s)))
    }

    /// What the books that take orders declare, together, at the lowest level among them: the
    /// broker's level and what an account offers before an instrument is chosen. A ticket on an
    /// instrument takes its book's caps ([`Broker::order_caps_on`]).
    fn order_caps(&self) -> Option<OrderCaps> {
        let mut it = self.books.iter().map(|b| b.broker.caps().orders.clone());
        let mut c = it.next()?;
        for o in it {
            c.types.extend(o.types);
            c.tif.extend(o.tif);
            c.triggers.extend(o.triggers);
            c.trail.extend(o.trail);
            c.amend |= o.amend;
            c.brackets |= o.brackets;
            c.position_exits |= o.position_exits;
            c.oca |= o.oca;
            c.reduce_only |= o.reduce_only;
            c.post_only |= o.post_only;
            c.client_id &= o.client_id;
            c.cancel_all_native &= o.cancel_all_native;
            c.position_mode = c.position_mode.or(o.position_mode);
            if c.dry_run == DryRunCap::None {
                c.dry_run = o.dry_run;
            }
            c.level = c.level.min(o.level);
        }
        Some(c)
    }

    /// The caps of the instrument's book: spot caps on a spot pair, linear caps on a contract.
    async fn order_caps_on(&self, s: &HashMap<String, String>, ticker: &str, _asset_type: &str) -> Result<Option<OrderCaps>> {
        Ok(Some(self.book_for(s, ticker).await?.broker.caps().orders.clone()))
    }

    /// The leverage and margin mode the account set on the instrument, read only (as on Binance
    /// USDⓈ-M): held from the last positions read or stream frame, read once when nothing is.
    /// `None` on a book with no positions (spot).
    async fn leverage(&self, s: &HashMap<String, String>, ticker: &str) -> Result<Option<(Option<Decimal>, Option<String>)>> {
        let b = self.book_for(s, ticker).await?;
        let (d, signer, scope) = self.session(b, s)?;
        let symbol = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        b.trader.leverage(&d, &a, &symbol).await.map_err(|e| self.fail(b, s, e))
    }

    /// The instrument's grid, from the venue's rules on its book: its tick, size step and
    /// smallest size as the venue states them (the shared snapping mechanism, each venue's own
    /// values through its mapper). A venue with no separate market-order grid states none.
    async fn grid(&self, s: &HashMap<String, String>, provider: Option<&str>, ticker: &str, asset_type: &str) -> Result<Option<super::Grid>> {
        let Ok(b) = self.book_for(s, ticker).await else { return self.read.grid(s, provider, ticker, asset_type).await };
        let (d, signer, scope) = self.session(b, s)?;
        let symbol = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
        let a = Account { settings: s, signer: signer.as_ref(), scope: &scope };
        let r = b.trader.rules(&d, &a, &symbol).await.map_err(|e| self.fail(b, s, e))?;
        Ok(Some(super::Grid { tick: r.tick, ladder: Vec::new(), lot_step: r.lot_step, min_qty: r.min_qty, market_step: None, market_min_qty: None }))
    }

    async fn qty_step(&self, s: &HashMap<String, String>, ticker: &str) -> Result<Option<Decimal>> {
        Ok(self.grid(s, None, ticker, "").await?.and_then(|g| g.lot_step))
    }
}

// ── Registered venues ─────────────────────────────────────────────────────────

static BYBIT_SPOT: OnceLock<Arc<Trader<otw_connector_providers::bybit::Bybit>>> = OnceLock::new();
static BYBIT_LINEAR: OnceLock<Arc<Trader<otw_connector_providers::bybit::Bybit>>> = OnceLock::new();
static BYBIT_EU_SPOT: OnceLock<Arc<Trader<otw_connector_providers::bybit::Bybit>>> = OnceLock::new();

fn shared<M: TradeMapper>(cell: &'static OnceLock<Arc<Trader<M>>>) -> Arc<Trader<M>> {
    cell.get_or_init(|| Arc::new(Trader::new())).clone()
}

/// Bybit: read over its four books as before; orders on spot and linear, each on its
/// instrument's book.
pub fn bybit() -> Option<NativeTradingBroker<otw_connector_providers::bybit::Bybit, NativeMultiBroker<otw_connector_providers::bybit::Bybit>>> {
    let read = super::native::bybit()?;
    let d = crate::histdata::native::bybit_drivers()?;
    Some(NativeTradingBroker::new(
        read,
        "books",
        vec![
            TradeBook { name: "spot", broker: NativeBroker::new(d.spot, &super::caps::BYBIT), trader: shared(&BYBIT_SPOT) },
            TradeBook { name: "linear", broker: NativeBroker::new(d.linear, &super::caps::BYBIT), trader: shared(&BYBIT_LINEAR) },
        ],
    ))
}

/// Bybit EU: read as before (its spot book, a bybit.eu key); orders on that spot book through
/// the Bybit spot mapper on the EU host, polled only (no EU private stream is documented).
pub fn bybit_eu() -> Option<NativeTradingBroker<otw_connector_providers::bybit::Bybit, NativeBroker<otw_connector_providers::bybit::Bybit>>> {
    let read = super::native::bybit_eu()?;
    let d = crate::histdata::native::bybit_eu_driver()?;
    Some(NativeTradingBroker::new(read, "books", vec![TradeBook { name: "spot", broker: NativeBroker::new(d, &super::caps::BYBIT_EU), trader: shared(&BYBIT_EU_SPOT) }]))
}

#[cfg(test)]
#[path = "native_trade_tests.rs"]
mod tests;
