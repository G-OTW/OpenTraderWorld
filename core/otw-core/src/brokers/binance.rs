//! Binance spot and USDⓈ-M futures accounts. The read side is the framework's
//! ([`NativeBroker`]); orders are sent here, with the requests the adapter builds
//! ([`otw_connector_providers::binance::trade`]) signed by the account's key.
//!
//! - **Test environment only below E4.** An account set to live is refused before anything is
//!   sent: Spot Demo Mode and the futures testnet are where these orders go until the order
//!   path is rated E4, as IB's paper accounts are for the gateway.
//! - **One id, one order.** A client order id this process already sent for the key is
//!   refused: Binance only refuses a duplicate while the first is still working.
//! - **A lost answer is looked up, never resent.** A submission that got no answer is asked
//!   back by its client id; found, it is placed.
//! - **No lag on a ticket.** The venue clock is read once every few minutes, not per order;
//!   margin and balance are the venue's to refuse, synchronously, with its reason.
//! - **USDⓈ-M exits are linked here.** Binance attaches no exit to an entry: a ticket's exits
//!   go out with it as reduce-only orders of the entry's size, and OTW cancels what is left
//!   ([`link`]), taken back up after a restart from the stored legs ([`Broker::resume`]).
//! - **The venue's filters before anything leaves.** Tick, lot, price range, market lot,
//!   notional, price band (against the reference price read then) and open-order caps, from
//!   `exchangeInfo`, kept a day and reloaded after a filter refusal. A refused ticket names
//!   the filter and the nearest values that pass; nothing is adjusted.
//! - **Order buckets are refused, not queued.** The `X-MBX-ORDER-COUNT-*` headers of each order
//!   answer are held against the account's `rateLimit/order` buckets; a 429 holds new orders,
//!   and a 418 every call, until its `Retry-After`.
//! - **Live gates** (live accounts only, refused below E4 anyway): before the first live order of
//!   a session the key's rights are read (`apiRestrictions`: trading on, withdrawals off, IP
//!   restricted) and the order is dry run on Binance's test route.
//! - **USDⓈ-M dead man's switch.** While OTW has orders working on a futures symbol it keeps
//!   `countdownCancelAll` armed on a heartbeat: if OTW goes away, Binance cancels them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use bytes::Bytes;
use otw_connector_model::{
    check_amend, check_orders, ConnectorError, Decimal, ErrorKind, InstrumentSpec, ExecLevel, Mapped, OrderAmend, OrderCaps, OrderOpts, OrderRequest, OrderStatus, OrderType, Qty, Side, TimeInForce,
    Timestamp,
};
use otw_connector_providers::binance::market::{Kind, Market};
use otw_connector_providers::binance::trade::{self, OrderRef, VenueOrder};
use otw_connector_providers::binance::Binance;
use otw_connector_sdk::{Driver, HttpRequest, Mapper, Signer};

use super::native::{next_id, NativeBroker};
use super::{AccountSummary, Broker, BrokerCapability, ExecQuery, Execution, Holding, OpenOrder, OrderTicket, Placed, Position, Quote, StoredLeg};

/// How long a venue clock reading is trusted (`recvWindow` is 20 s).
const CLOCK: Duration = Duration::from_secs(300);

/// The code a placing call's error carries when its answer was lost and the order could not
/// be looked up: the leg is unknown, not refused ([`BinanceBroker::fail_placing`]).
const LOST: &str = "otw_lost";

/// How often a USDⓈ-M bracket is read while OTW links it.
const LINK_POLL: Duration = Duration::from_secs(5);
/// Reads in a row that may fail before a bracket is let go.
const LINK_MISSES: u32 = 120;

/// Client order ids sent, by environment and key.
static SENT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// How long a symbol's filters are trusted (spec 4.2: refreshed daily).
const RULES_TTL: Duration = Duration::from_secs(86_400);
/// How long the account's order buckets are trusted.
const LIMITS_TTL: Duration = Duration::from_secs(3_600);
/// The USDⓈ-M dead man's switch: armed for this long, refreshed every [`HEARTBEAT`].
const COUNTDOWN_MS: u64 = 120_000;
const HEARTBEAT: Duration = Duration::from_secs(30);

/// When filters were last read: per symbol on spot, per market and environment's list on USDⓈ-M.
static RULES: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(Default::default);

/// The order buckets by key: their limits, and the counts the last order answer stated.
#[derive(Default)]
struct Budget {
    limits: Option<(Vec<trade::OrderLimit>, Instant)>,
    counts: Vec<(i64, u64)>,
    at: Option<Instant>,
}
static BUDGET: LazyLock<Mutex<HashMap<String, Budget>>> = LazyLock::new(Default::default);

/// Rate-limit holds by host (Binance limits by IP): until when, and whether it is a ban (418,
/// every call) or a 429 (new orders only).
static HOLD: LazyLock<Mutex<HashMap<String, (Instant, bool)>>> = LazyLock::new(Default::default);

/// Keys whose rights were checked, and dry run passed, this session (live only).
static LIVE_CHECKED: LazyLock<Mutex<HashMap<String, (bool, bool)>>> = LazyLock::new(Default::default);

/// USDⓈ-M symbols whose dead man's switch is armed, by key.
static ARMED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// USDⓈ-M stops replaced under a new client id ([`trade::next_client_id`]): the old id to the
/// new, so a link that follows the old one follows the new.
static RENAMED: LazyLock<Mutex<HashMap<String, OrderRef>>> = LazyLock::new(Default::default);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One queue per account and symbol for changes and cancels: a spot list is cancelled and
/// placed again whole to change one of its orders, so two changes at once would each work on
/// what the other is replacing. Fair (first come, first served), so an automation's changes
/// go out in the order it asked them.
static ORDER_QUEUES: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = LazyLock::new(Default::default);

/// Spot pairs' base and quote assets per market and symbol, as the venue answered them.
static PAIRS: LazyLock<Mutex<HashMap<String, Option<(String, String)>>>> = LazyLock::new(Default::default);

/// USDⓈ-M position mode by environment and key, and when it was read: hedge mode on or off
/// changes how every order names its side, and the account can switch it at any time.
static HEDGE: LazyLock<Mutex<HashMap<String, (bool, Instant)>>> = LazyLock::new(Default::default);
/// Spot orders being changed by `cancelReplace`, by Binance order number: the stream reports
/// the cancelled half under the same client id as the new order, which is not the order
/// ending ([`replaced_half`]).
static REPLACING: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(Default::default);
/// How long a change's cancelled half is waited for on the stream.
const REPLACING_TTL: Duration = Duration::from_secs(60);

#[cfg(test)]
pub(super) fn mark_replacing(venue_id: &str) {
    lock(&REPLACING).insert(venue_id.to_string(), Instant::now());
}

/// Whether a cancel the stream reports is the old half of a spot change (taken once): the new
/// order's own event follows under the same id, so the order did not end.
pub(super) fn replaced_half(venue_id: &str) -> bool {
    let mut m = lock(&REPLACING);
    m.retain(|_, at| at.elapsed() < REPLACING_TTL);
    m.remove(venue_id).is_some()
}

pub struct BinanceBroker {
    inner: NativeBroker<Binance>,
    /// When each environment's clock was last read.
    synced: Mutex<HashMap<bool, Instant>>,
}

pub fn binance() -> Option<BinanceBroker> {
    Some(BinanceBroker { inner: super::native::binance_native()?, synced: Mutex::default() })
}

pub fn binance_futures() -> Option<BinanceBroker> {
    Some(BinanceBroker { inner: super::native::binance_futures_native()?, synced: Mutex::default() })
}

fn now() -> Result<Timestamp> {
    Timestamp::from_offset_datetime(time::OffsetDateTime::now_utc()).ok_or_else(|| anyhow!("the clock is out of range"))
}

fn f(d: Decimal) -> f64 {
    d.to_string().parse().unwrap_or(0.0)
}

/// A working order as the book lists it.
fn open_order_of(o: &VenueOrder, currency: String, venue: String) -> OpenOrder {
    OpenOrder {
        id: o.r.id(),
        symbol: o.r.symbol.clone(),
        side: if o.side == Side::Buy { "buy".into() } else { "sell".into() },
        order_type: o.kind.clone(),
        qty: f(o.qty),
        filled_qty: f(o.filled),
        limit_price: o.limit.map(f),
        stop_price: o.stop.map(f),
        trail_start: o.trail_start.map(f),
        trail_rate: o.trail_rate.map(f),
        currency,
        asset_class: "crypto".into(),
        placed_at: o.placed_ms.and_then(|ms| time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).ok()),
        venue,
        account: String::new(),
        parent_id: None,
        reduce_only: o.reduce_only || o.close_position,
    }
}

impl BinanceBroker {
    fn market(&self) -> &'static Market {
        // Both environments run the same mapper.
        match self.inner.caps().provider.as_str() {
            "binance_futures" => &otw_connector_providers::binance::market::USDM,
            _ => &otw_connector_providers::binance::market::SPOT,
        }
    }

    fn label(&self) -> &'static str {
        self.inner.capability().label
    }

    fn fail(&self, s: &HashMap<String, String>, e: ConnectorError) -> anyhow::Error {
        self.inner.hint(s, e)
    }

    /// Orders go to the test environment until the order path is rated E4, except on the one
    /// account a maintainer names for the live canary.
    fn check_env(&self, s: &HashMap<String, String>) -> Result<()> {
        let level = self.inner.caps().orders.level;
        if level >= ExecLevel::E4 || self.inner.wants_sandbox(s) || s.get(super::LIVE_CANARY).is_some_and(|v| v == "1") {
            return Ok(());
        }
        Err(anyhow!(
            "{} orders go to the testnet only until its order path is rated E4 (it is {level:?}). Set the account's environment to testnet.",
            self.label(),
        ))
    }

    /// The account's driver and key, with a venue clock no older than [`CLOCK`].
    async fn key(&self, s: &HashMap<String, String>) -> Result<(Arc<Driver<Binance>>, Arc<dyn Signer>)> {
        let d = self.inner.on(s)?;
        let signer = self.inner.signer(s)?.ok_or_else(|| anyhow!("{} needs an API key", self.label()))?;
        let env = self.inner.wants_sandbox(s);
        let fresh = self.synced.lock().unwrap_or_else(|e| e.into_inner()).get(&env).is_some_and(|t| t.elapsed() < CLOCK);
        if !fresh {
            d.sync_time(next_id()).await.map_err(|e| self.fail(s, e))?;
            self.synced.lock().unwrap_or_else(|e| e.into_inner()).insert(env, Instant::now());
        }
        Ok((d, signer))
    }

    /// One signed call: the body, or Binance's refusal.
    async fn send(&self, d: &Driver<Binance>, signer: &dyn Signer, req: HttpRequest) -> Result<Bytes, ConnectorError> {
        send_on(self.market(), d, signer, req).await
    }

    /// The instrument's terms, kept once read. Spot reads this one symbol (a few KB); USDⓈ-M,
    /// whose list takes no symbol, the whole list once. A symbol the venue does not list is an
    /// error, never a near match.
    async fn spec(&self, d: &Driver<Binance>, symbol: &str) -> Result<InstrumentSpec> {
        let id = otw_connector_model::InstrumentId::new(self.market().venue, symbol);
        if let Some(spec) = d.instruments().ok().and_then(|c| c.get(&id).cloned()) {
            return Ok(spec);
        }
        self.read_terms(d, &id).await?;
        d.instruments()
            .ok()
            .and_then(|c| c.get(&id).cloned())
            .ok_or_else(|| anyhow!("{} does not list {symbol}: check its spelling (BTCUSDT)", self.label()))
    }

    /// Read `id`'s terms from the venue into the cache: this one symbol on spot (`-1121`, a
    /// symbol Binance does not know, leaves it out), the whole list on USDⓈ-M.
    async fn read_terms(&self, d: &Driver<Binance>, id: &otw_connector_model::InstrumentId) -> Result<()> {
        if self.market().kind != Kind::Spot {
            d.load_instruments(next_id()).await.map_err(anyhow::Error::new)?;
            return Ok(());
        }
        match d.resolve_instruments(next_id(), std::slice::from_ref(id), None).await {
            Err(e) if e.provider_code.as_deref() != Some("-1121") => Err(anyhow::Error::new(e)),
            _ => Ok(()),
        }
    }

    /// A spot pair's base and quote assets: from the instrument list when this process holds
    /// it, else from the venue for this one symbol (never the whole list), kept once read. A
    /// symbol Binance refuses as unknown (a chart of another venue) is kept as having none.
    async fn pair(&self, d: &Driver<Binance>, signer: &dyn Signer, symbol: &str) -> Option<(String, String)> {
        let id = otw_connector_model::InstrumentId::new(self.market().venue, symbol);
        if let Some(spec) = d.instruments().ok().and_then(|c| c.get(&id).cloned()) {
            return Some((spec.base?, spec.quote));
        }
        let key = format!("{}:{symbol}", self.market().name);
        if let Some(known) = lock(&PAIRS).get(&key) {
            return known.clone();
        }
        match self.send(d, signer, trade::symbol_info(symbol, next_id())).await.and_then(|b| trade::decode_symbol_assets(&b)) {
            Ok(pair) => {
                lock(&PAIRS).insert(key, pair.clone());
                pair
            }
            Err(e) => {
                if e.provider_code.as_deref() == Some("-1121") {
                    lock(&PAIRS).insert(key, None);
                }
                tracing::warn!("binance: assets of {symbol} not read: {e}");
                None
            }
        }
    }

    /// The market, environment and key an account trades under, for what this process keeps
    /// per key.
    /// Wait for this account's turn on `symbol` ([`ORDER_QUEUES`]).
    async fn queue(&self, s: &HashMap<String, String>, symbol: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let q = lock(&ORDER_QUEUES).entry(format!("{}:{symbol}", self.scope(s))).or_default().clone();
        q.lock_owned().await
    }

    fn scope(&self, s: &HashMap<String, String>) -> String {
        let key = hex::encode(otw_connector_sdk::signer::sha256(s.get("api_key").map(String::as_str).unwrap_or("").trim().as_bytes()));
        format!("{}:{}:{}", self.market().name, self.inner.wants_sandbox(s), &key[..12])
    }

    /// Whether the USDⓈ-M account holds long and short apart (hedge mode), read again after
    /// [`CLOCK`]. Spot has no position mode.
    async fn hedge(&self, d: &Driver<Binance>, signer: &dyn Signer, s: &HashMap<String, String>) -> Result<bool> {
        if self.market().kind != Kind::UsdM {
            return Ok(false);
        }
        let scope = self.scope(s);
        if let Some((on, at)) = HEDGE.lock().unwrap_or_else(|e| e.into_inner()).get(&scope) {
            if at.elapsed() < CLOCK {
                return Ok(*on);
            }
        }
        let on = trade::decode_dual(&self.send(d, signer, trade::position_mode(next_id())).await.map_err(|e| self.fail(s, e))?).map_err(anyhow::Error::new)?;
        HEDGE.lock().unwrap_or_else(|e| e.into_inner()).insert(scope, (on, Instant::now()));
        Ok(on)
    }

    /// Refuse a client id this key was already sent from this process (spec 2.1).
    fn claim(&self, s: &HashMap<String, String>, orders: &[OrderRequest]) -> Result<()> {
        let scope = self.scope(s);
        let mut sent = SENT.lock().unwrap_or_else(|e| e.into_inner());
        for o in orders {
            if sent.contains(&format!("{scope}:{}", o.client_order_id)) {
                return Err(anyhow!("client order id {} was already sent to Binance: a new order needs a new id, so nothing is placed twice", o.client_order_id));
            }
        }
        for o in orders {
            sent.insert(format!("{scope}:{}", o.client_order_id));
        }
        Ok(())
    }

    async fn query(&self, d: &Driver<Binance>, signer: &dyn Signer, r: &OrderRef) -> Result<VenueOrder, ConnectorError> {
        query_on(self.market(), d, signer, r).await
    }

    /// Send one order. A submission that got no answer is looked up by its client id before
    /// anything is said: found, it was placed.
    async fn place_one(&self, d: &Driver<Binance>, signer: &dyn Signer, o: &OrderRequest, reduce_only: bool, hedge: bool, scope: &str) -> Result<OrderRef, ConnectorError> {
        let m = self.market();
        let r = trade::placed_ref(m, o);
        self.answered(d, signer, trade::place(m, o, reduce_only, hedge, next_id())?, &r, &o.client_order_id, scope, 1).await.map(|_| r)
    }

    /// The account's order buckets, read once an hour; `None` when they could not be read (the
    /// venue then counts alone).
    async fn limits(&self, d: &Driver<Binance>, signer: &dyn Signer, scope: &str) -> Option<Vec<trade::OrderLimit>> {
        if let Some((l, at)) = lock(&BUDGET).get(scope).and_then(|b| b.limits.clone()) {
            if at.elapsed() < LIMITS_TTL {
                return Some(l);
            }
        }
        match self.send(d, signer, trade::order_limits(self.market(), next_id())).await.and_then(|b| trade::decode_order_limits(&b)) {
            Ok(l) => {
                lock(&BUDGET).entry(scope.to_string()).or_default().limits = Some((l.clone(), Instant::now()));
                Some(l)
            }
            Err(e) => {
                // Not asked again before the hour is out.
                tracing::warn!("binance: order buckets not read, the venue counts alone: {e}");
                lock(&BUDGET).entry(scope.to_string()).or_default().limits = Some((Vec::new(), Instant::now()));
                None
            }
        }
    }

    /// Send one placing call (an order, a spot order list, a change) adding `adds` orders to
    /// the account's buckets. Refused before sending when the counts the last answer stated
    /// leave no room (spec 8: refused, not queued). A call that got no answer is looked up by
    /// `r`, one order it places, before anything is said: found, it was placed.
    #[allow(clippy::too_many_arguments)]
    async fn answered(&self, d: &Driver<Binance>, signer: &dyn Signer, req: HttpRequest, r: &OrderRef, what: &str, scope: &str, adds: u64) -> Result<(), ConnectorError> {
        let m = self.market();
        if let Some(limits) = self.limits(d, signer, scope).await {
            let (counts, age) = lock(&BUDGET).get(scope).map(|b| (b.counts.clone(), b.at.map(|t| t.elapsed()))).unwrap_or_default();
            if let Some(age) = age {
                trade::check_budget(m, &limits, &counts, age.as_millis().try_into().unwrap_or(i64::MAX), adds)?;
            }
        }
        let o_id = what;
        match send_frame(m, d, signer, req).await {
            Ok((_, counts)) => {
                if !counts.is_empty() {
                    let mut b = lock(&BUDGET);
                    let b = b.entry(scope.to_string()).or_default();
                    b.counts = counts;
                    b.at = Some(Instant::now());
                }
                Ok(())
            }
            Err(e) if e.kind == ErrorKind::Transient => match self.query(d, signer, r).await {
                Ok(_) => Ok(()),
                Err(q) if trade::unknown_order(&q) => Err(e),
                Err(_) => Err(ConnectorError::new(
                    ErrorKind::Transient,
                    format!("{}: no answer to order {o_id}, and it could not be looked up: it is looked up again until the venue answers, do not send it again ({})", m.name, e.message),
                )
                .with_code(LOST)),
            },
            Err(e) => Err(e),
        }
    }

    /// A placing call's failure: one whose answer was lost and could not be looked up leaves
    /// the legs `keys` unknown (spec 6.3), looked up by client id until the venue answers;
    /// anything else is the venue's refusal, with the hint for the account.
    fn fail_placing(&self, s: &HashMap<String, String>, e: ConnectorError, keys: &[&OrderRequest]) -> anyhow::Error {
        if e.provider_code.as_deref() == Some(LOST) {
            return anyhow::Error::new(super::OrderUnknown { keys: keys.iter().map(|o| o.client_order_id.clone()).collect(), message: e.message, alert: false });
        }
        self.fail(s, e)
    }

    /// Cancel orders a refused ticket already placed: a ticket is never left half placed.
    async fn undo(&self, d: &Driver<Binance>, signer: &dyn Signer, placed: &[OrderRef]) {
        for r in placed {
            if let Err(c) = self.send(d, signer, trade::cancel(self.market(), r, next_id())).await {
                tracing::warn!("binance: undoing {} after a refused leg: {c}", r.id());
            }
        }
    }

    /// Spot: change one order of a list (an OTO's entry or exit, an OCO's half). Binance
    /// cancels a whole list with any of its orders, so a cancel-replace would drop the others:
    /// the list is cancelled and placed again whole, under the same client ids, with the
    /// change. `false` when the order is the last one of its list still working: it changes
    /// on its own. A list partly filled (an entry with fills) is refused.
    #[allow(clippy::too_many_arguments)]
    async fn move_in_list(&self, s: &HashMap<String, String>, d: &Driver<Binance>, signer: &dyn Signer, cur: &VenueOrder, list: i64, change: &OrderAmend, scope: &str) -> Result<bool> {
        let m = self.market();
        let ids = trade::decode_order_list(&self.send(d, signer, trade::order_list(list, next_id())).await.map_err(|e| self.fail(s, e))?).map_err(anyhow::Error::new)?;
        let mut live = Vec::new();
        for id in ids {
            let r = OrderRef { symbol: cur.r.symbol.clone(), client_id: id, algo: false };
            let o = self.query(d, signer, &r).await.map_err(|e| self.fail(s, e))?;
            if !matches!(o.status, Mapped::Known(st) if st.is_final()) {
                live.push(o);
            }
        }
        if live.len() < 2 {
            return Ok(false);
        }
        if live.iter().any(|o| o.filled > Decimal::ZERO) {
            return Err(anyhow!("order {} is part of a list already partly filled: cancel it and place it again", cur.r.id()));
        }
        let changed = |o: &VenueOrder| (o.r.client_id == cur.r.client_id).then_some(change);
        // An OTO (OTOCO): the working entry, its exits pending until it fills. Else an OCO pair.
        let pending = |o: &VenueOrder| o.status == Mapped::Known(OrderStatus::PendingNew);
        let mut reqs = Vec::with_capacity(live.len());
        if let Some(entry) = live.iter().find(|o| !pending(o)).filter(|_| live.iter().any(pending)) {
            let e = trade::spot_request(m, entry, changed(entry)).map_err(anyhow::Error::new)?;
            let old_qty = entry.qty;
            for x in live.iter().filter(|o| pending(o)) {
                let mut r = trade::spot_request(m, x, changed(x)).map_err(anyhow::Error::new)?;
                // Exits sized as the entry follow a new entry size.
                if changed(x).and_then(|a| a.qty).is_none() && x.qty == old_qty {
                    r.qty = e.qty;
                }
                r.parent = Some(e.client_order_id.clone());
                reqs.push(r);
            }
            reqs.insert(0, e);
        } else {
            for o in &live {
                let mut r = trade::spot_request(m, o, changed(o)).map_err(anyhow::Error::new)?;
                r.oca_group = Some(list.to_string());
                reqs.push(r);
            }
        }
        let req = trade::place_list(m, &reqs, next_id()).map_err(anyhow::Error::new)?.ok_or_else(|| anyhow!("order {} is part of a list Binance cannot place again", cur.r.id()))?;
        // Their cancels on the stream are the old halves of the change, not the orders ending.
        for o in &live {
            lock(&REPLACING).insert(o.venue_id.clone(), Instant::now());
        }
        if let Err(e) = self.send(d, signer, trade::cancel_list(&cur.r.symbol, list, next_id())).await {
            for o in &live {
                lock(&REPLACING).remove(&o.venue_id);
            }
            return Err(anyhow!("Binance refused to cancel the list to change it ({}): it stands as it was", self.fail(s, e)));
        }
        let first = &reqs[0];
        self.answered(d, signer, req, &trade::placed_ref(m, first), &first.client_order_id, scope, reqs.len() as u64).await.map_err(|e| {
            self.reload_rules(s, &cur.r.symbol, &e);
            anyhow!(
                "the list was cancelled to change it, but Binance refused it placed again ({}): nothing of it works on {} now, place it again",
                self.fail(s, e),
                cur.r.symbol
            )
        })?;
        Ok(true)
    }

    /// A spot ticket: one order, or an order list Binance links itself (OCO, OTO, OTOCO). A
    /// market entry's exits cannot wait on the book for it, so they go out once it filled, as
    /// one order or an OCO, built before the entry leaves so a shape Binance cannot link is
    /// refused first; exits refused then leave the bought balance without them, and the error
    /// says so.
    async fn place_spot(&self, s: &HashMap<String, String>, d: &Driver<Binance>, signer: &dyn Signer, orders: &[OrderRequest]) -> Result<Vec<(String, OrderRef)>> {
        let m = self.market();
        let scope = self.scope(s);
        let refs = |list: &[OrderRequest]| list.iter().map(|o| (o.client_order_id.clone(), trade::placed_ref(m, o))).collect::<Vec<_>>();
        let entry = &orders[0];
        // A market entry's exits stand alone once it filled: two of them are one OCO.
        let exits: Vec<OrderRequest> = orders[1..]
            .iter()
            .filter(|o| o.parent.is_some())
            .map(|o| OrderRequest { parent: None, oca_group: Some(entry.client_order_id.clone()), ..o.clone() })
            .collect();
        if entry.order_type == OrderType::Market && !exits.is_empty() {
            let (exit_req, adds) = match exits.as_slice() {
                [one] => (trade::place(m, &OrderRequest { oca_group: None, ..one.clone() }, false, false, next_id()).map_err(anyhow::Error::new)?, 1),
                list => (trade::place_list(m, list, next_id()).map_err(anyhow::Error::new)?.ok_or_else(|| anyhow!("Binance spot links an OCO pair, or one entry with one or two exits"))?, 2),
            };
            self.live_gate(s, d, signer, Some(entry), false).await?;
            let r = self.place_one(d, signer, entry, false, false, &scope).await.map_err(|e| self.fail_placing(s, e, &[entry]))?;
            let filled = self.query(d, signer, &r).await.map(|o| o.status == Mapped::Known(OrderStatus::Filled)).unwrap_or(false);
            if !filled {
                return Err(anyhow!("the market entry {} did not fill at once, so its exits were not placed: check the account", entry.client_order_id));
            }
            if let Err(e) = self.answered(d, signer, exit_req, &trade::placed_ref(m, &exits[0]), &exits[0].client_order_id, &scope, adds).await {
                if e.provider_code.as_deref() == Some(LOST) {
                    return Err(self.fail_placing(s, e, &exits.iter().collect::<Vec<_>>()));
                }
                return Err(anyhow!("the market entry {} filled, but Binance refused its exits ({}): the balance stands without them", entry.client_order_id, self.fail(s, e)));
            }
            let mut out = refs(std::slice::from_ref(entry));
            out.extend(refs(&exits));
            return Ok(out);
        }
        self.live_gate(s, d, signer, Some(entry), false).await?;
        match trade::place_list(m, orders, next_id()).map_err(anyhow::Error::new)? {
            Some(req) => {
                // An OCO adds 2 orders to the buckets, an OTO 2, an OTOCO 3.
                self.answered(d, signer, req, &trade::placed_ref(m, entry), &entry.client_order_id, &scope, orders.len() as u64)
                    .await
                    .map_err(|e| self.fail_placing(s, e, &orders.iter().collect::<Vec<_>>()))?;
                Ok(refs(orders))
            }
            None => {
                let r = self.place_one(d, signer, entry, false, false, &scope).await.map_err(|e| self.fail_placing(s, e, &[entry]))?;
                Ok(vec![(entry.client_order_id.clone(), r)])
            }
        }
    }

    /// Before the first live order of a session (spec 9, 12.2): the key's rights
    /// (`apiRestrictions`: trading on for this market, withdrawals off, IP restricted), then
    /// `first` dry run on Binance's test route. Skipped on the test environments: Spot Demo
    /// Mode serves no `/sapi`, and a testnet order is its own dry run. A ticket whose first
    /// order has no test route (an order list's, a USDⓈ-M conditional order) is dry run on its
    /// next ticket that has one.
    async fn live_gate(&self, s: &HashMap<String, String>, d: &Driver<Binance>, signer: &dyn Signer, first: Option<&OrderRequest>, reduce_only: bool) -> Result<()> {
        if self.inner.wants_sandbox(s) {
            return Ok(());
        }
        let m = self.market();
        let scope = self.scope(s);
        let (rights, dry) = lock(&LIVE_CHECKED).get(&scope).copied().unwrap_or_default();
        if !rights {
            let k = trade::decode_rights(&self.send(d, signer, trade::api_restrictions(next_id())).await.map_err(|e| self.fail(s, e))?).map_err(anyhow::Error::new)?;
            trade::check_rights(m, &k).map_err(anyhow::Error::new)?;
            lock(&LIVE_CHECKED).insert(scope.clone(), (true, false));
        }
        let Some(o) = first.filter(|_| !dry) else { return Ok(()) };
        let hedge = self.hedge(d, signer, s).await?;
        let alone = OrderRequest { parent: None, oca_group: None, ..o.clone() };
        let Some(test) = trade::order_test(&trade::place(m, &alone, reduce_only, hedge, next_id()).map_err(anyhow::Error::new)?) else { return Ok(()) };
        self.send(d, signer, test).await.map_err(|e| anyhow!("{}: the dry run on Binance's test route refused this order, nothing was sent ({})", m.name, self.fail(s, e)))?;
        lock(&LIVE_CHECKED).insert(scope, (true, true));
        Ok(())
    }

    /// A symbol's filters, as the venue states them ([`InstrumentSpec::rules`]), read again
    /// once a day and after a refusal that names a filter (spot: this symbol's, USDⓈ-M: the
    /// list). `None` when they could not be read: the venue still applies them, and says
    /// which one refused.
    async fn rules(&self, d: &Driver<Binance>, _signer: &dyn Signer, s: &HashMap<String, String>, symbol: &str) -> Option<trade::Rules> {
        let key = self.rules_key(s, symbol);
        // First use: the terms this process read count from now.
        let stale = lock(&RULES).entry(key.clone()).or_insert_with(Instant::now).elapsed() >= RULES_TTL;
        if stale {
            match self.read_terms(d, &otw_connector_model::InstrumentId::new(self.market().venue, symbol)).await {
                Ok(()) => {
                    lock(&RULES).insert(key, Instant::now());
                }
                Err(e) => tracing::warn!("binance: filters of {symbol} not read again, the ones held stand: {e:#}"),
            }
        }
        match self.spec(d, symbol).await {
            Ok(spec) => Some(trade::rules_of(&spec)),
            Err(e) => {
                tracing::warn!("binance: filters of {symbol} not read, the venue checks alone: {e:#}");
                None
            }
        }
    }

    /// Read the instrument list again on the next order after the venue refused one on a
    /// filter (spec 4.2).
    fn reload_rules(&self, s: &HashMap<String, String>, symbol: &str, e: &ConnectorError) {
        let filter = matches!(e.provider_code.as_deref(), Some("-1013" | "-4013" | "-4014" | "-4016" | "-4023" | "-4024" | "-4164" | "-4005"));
        if filter {
            if let Some(old) = Instant::now().checked_sub(RULES_TTL) {
                lock(&RULES).insert(self.rules_key(s, symbol), old);
            }
        }
    }

    /// What [`RULES`] dates: spot reads a symbol's filters on their own, USDⓈ-M the list.
    fn rules_key(&self, s: &HashMap<String, String>, symbol: &str) -> String {
        let env = format!("{}:{}", self.market().name, self.inner.wants_sandbox(s));
        if self.market().kind == Kind::Spot { format!("{env}:{symbol}") } else { env }
    }

    /// What the band and a market order's notional are measured against: spot's reference
    /// price, else its average price; USDⓈ-M's mark price. `None` when it could not be read.
    async fn reference(&self, d: &Driver<Binance>, signer: &dyn Signer, symbol: &str) -> Option<trade::Reference> {
        let m = self.market();
        let first = match m.kind {
            Kind::Spot => trade::reference_price(symbol, next_id()),
            Kind::UsdM => trade::mark_price(symbol, next_id()),
        };
        match self.send(d, signer, first).await {
            Ok(b) => match trade::decode_reference(m, &b) {
                Ok(Some(r)) => return Some(r),
                Ok(None) => {}
                Err(e) => tracing::warn!("binance: {e}"),
            },
            // -2043: the symbol never had a reference price.
            Err(e) if e.provider_code.as_deref() == Some("-2043") => {}
            Err(e) => tracing::warn!("binance: reference price of {symbol} not read: {e}"),
        }
        if m.kind != Kind::Spot {
            return None;
        }
        self.send(d, signer, trade::avg_price(symbol, next_id())).await.and_then(|b| trade::decode_avg_price(&b)).map_err(|e| tracing::warn!("binance: average price of {symbol} not read: {e}")).ok()
    }

    /// Orders working on `symbol` now, and how many of them are conditional.
    async fn open_count(&self, d: &Driver<Binance>, signer: &dyn Signer, symbol: &str) -> Result<(usize, usize), ConnectorError> {
        let m = self.market();
        // Both books at once on USDⓈ-M: a ticket waits on one round trip, not two.
        let (plain, algo) = tokio::join!(self.send(d, signer, trade::open_on(m, symbol, next_id())), async {
            if m.kind == Kind::UsdM { Some(self.send(d, signer, trade::open_algo_on(symbol, next_id())).await) } else { None }
        });
        let mut rows = trade::decode_orders(&plain?)?;
        if let Some(algo) = algo {
            rows.extend(trade::decode_orders(&algo?)?);
        }
        Ok((rows.len(), rows.iter().filter(|o| trade::is_algo(o)).count()))
    }

    /// The ticket against the instrument's terms and the venue's filters, before anything
    /// leaves. `orders` with the reduce-only OTW forces on each.
    async fn pre_trade(&self, s: &HashMap<String, String>, d: &Driver<Binance>, signer: &dyn Signer, spec: &InstrumentSpec, orders: &[(&OrderRequest, bool)]) -> Result<()> {
        let m = self.market();
        for (o, _) in orders {
            trade::check_client_id(&o.client_order_id).map_err(anyhow::Error::new)?;
            trade::check_rules(spec, o).map_err(anyhow::Error::new)?;
        }
        let Some(rules) = self.rules(d, signer, s, &spec.id.native).await else { return Ok(()) };
        // The reference price and the open-order count are read together: the ticket waits on
        // one round trip for both.
        let needs_reference = orders.iter().any(|(o, _)| trade::needs_reference(m, &rules, o));
        let counts = rules.max_orders.is_some() || rules.max_algo_orders.is_some();
        let (reference, open) = tokio::join!(
            async { if needs_reference { self.reference(d, signer, &spec.id.native).await } else { None } },
            async { if counts { Some(self.open_count(d, signer, &spec.id.native).await) } else { None } },
        );
        for (o, reduce) in orders {
            trade::check_filters(m, &rules, o, *reduce, reference).map_err(anyhow::Error::new)?;
        }
        if let Some(open) = open {
            match open {
                Ok((n, algo)) => {
                    let adds = orders.iter().filter(|(o, _)| o.order_type.triggered()).count();
                    trade::check_open(&rules, &spec.id.native, n, algo, orders.len(), adds).map_err(anyhow::Error::new)?;
                }
                Err(e) => tracing::warn!("binance: working orders on {} not counted, the venue counts alone: {e}", spec.id.native),
            }
        }
        Ok(())
    }

    /// USDⓈ-M: keep `countdownCancelAll` armed on `symbol` while OTW has orders working there
    /// (spec 7.3): refreshed every [`HEARTBEAT`] for [`COUNTDOWN_MS`], stopped once nothing
    /// works on the symbol. If OTW goes away, Binance cancels every order on it, OTW's or not.
    /// The account setting `cancel_on_disconnect = off` leaves it unarmed.
    fn arm(&self, s: &HashMap<String, String>, d: &Arc<Driver<Binance>>, signer: &Arc<dyn Signer>, symbol: &str) {
        let m = self.market();
        if m.kind != Kind::UsdM || s.get("cancel_on_disconnect").is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "off" | "false" | "no" | "0")) {
            return;
        }
        let key = format!("{}:{symbol}", self.scope(s));
        if !lock(&ARMED).insert(key.clone()) {
            return;
        }
        let (d, signer, symbol) = (d.clone(), signer.clone(), symbol.to_string());
        tokio::spawn(async move {
            loop {
                if let Err(e) = send_on(m, &d, signer.as_ref(), trade::countdown(&symbol, COUNTDOWN_MS, next_id())).await {
                    tracing::warn!("binance: dead man's switch on {symbol} not refreshed: {e}");
                }
                tokio::time::sleep(HEARTBEAT).await;
                let working = async {
                    let mut n = trade::decode_orders(&send_on(m, &d, signer.as_ref(), trade::open_on(m, &symbol, next_id())).await?)?.len();
                    n += trade::decode_orders(&send_on(m, &d, signer.as_ref(), trade::open_algo_on(&symbol, next_id())).await?)?.len();
                    Ok::<usize, ConnectorError>(n)
                };
                // Unread, the countdown stays armed: Binance cancels if OTW cannot reach it.
                if let Ok(0) = working.await {
                    lock(&ARMED).remove(&key);
                    if let Err(e) = send_on(m, &d, signer.as_ref(), trade::countdown(&symbol, 0, next_id())).await {
                        tracing::warn!("binance: dead man's switch on {symbol} not stopped: {e}");
                    }
                    // An order placed meanwhile armed it again: the stop must not undo that.
                    if lock(&ARMED).contains(&key) {
                        let _ = send_on(m, &d, signer.as_ref(), trade::countdown(&symbol, COUNTDOWN_MS, next_id())).await;
                    }
                    return;
                }
            }
        });
    }

    /// Every working order, both books on USDⓈ-M.
    async fn working(&self, d: &Driver<Binance>, signer: &dyn Signer) -> Result<Vec<VenueOrder>, ConnectorError> {
        let m = self.market();
        let mut out = trade::decode_orders(&self.send(d, signer, trade::open_orders(m, next_id())).await?)?;
        if m.kind == Kind::UsdM {
            out.extend(trade::decode_orders(&self.send(d, signer, trade::open_algo(next_id())).await?)?);
        }
        Ok(out)
    }

    /// What a change leaves must still sit on the instrument's tick and lot.
    fn check_change(spec: &InstrumentSpec, cur: &VenueOrder, a: &OrderAmend) -> Result<(), ConnectorError> {
        let probe = OrderRequest { limit: a.limit, stop: a.stop, ..Self::probe(spec, cur, a) };
        trade::check_rules(spec, &probe)
    }

    /// The order a change leaves, for the filters: its size and prices after the change, as a
    /// limit when it carries a limit price (the price the filters value it at).
    fn probe(spec: &InstrumentSpec, cur: &VenueOrder, a: &OrderAmend) -> OrderRequest {
        let limit = a.limit.or(cur.limit.map(otw_connector_model::Price::new));
        OrderRequest {
            client_order_id: cur.r.client_id.clone(),
            instrument: spec.id.clone(),
            side: cur.side,
            order_type: if limit.is_some() && !cur.r.algo { OrderType::Limit } else { OrderType::Stop },
            qty: a.qty.unwrap_or(Qty::new(cur.qty)),
            limit,
            stop: a.stop.or(cur.stop.map(otw_connector_model::Price::new)),
            tif: TimeInForce::Gtc,
            parent: None,
            oca_group: None,
            account: None,
            opts: OrderOpts { close_position: cur.close_position, ..Default::default() },
        }
    }
}

/// One signed call: the body, or Binance's refusal.
async fn send_on(m: &Market, d: &Driver<Binance>, signer: &dyn Signer, req: HttpRequest) -> Result<Bytes, ConnectorError> {
    send_frame(m, d, signer, req).await.map(|(body, _)| body)
}

/// Drop every rate-limit hold: a test's mock venue holds its port, which the next test's may reuse.
#[cfg(test)]
pub(super) fn clear_holds() {
    lock(&HOLD).clear();
}

/// A rate-limit hold in force on this host for `req`: a ban stops every call, a 429 new orders.
fn held(host: &str, req: &HttpRequest) -> Option<Duration> {
    let mut holds = lock(&HOLD);
    let (until, ban) = *holds.get(host)?;
    let left = until.checked_duration_since(Instant::now());
    if left.is_none() {
        holds.remove(host);
    }
    left.filter(|_| ban || trade::places(req))
}

/// One call, with what an order answer counts in the account's buckets
/// (`X-MBX-ORDER-COUNT-*`). A rate-limit hold in force refuses it before it is sent; a 429 or
/// 418 answer sets one until its `Retry-After`.
async fn send_frame(m: &Market, d: &Driver<Binance>, signer: &dyn Signer, req: HttpRequest) -> Result<(Bytes, Vec<(i64, u64)>), ConnectorError> {
    let host = d.base_url("rest").unwrap_or_default();
    if let Some(left) = held(&host, &req) {
        let secs = left.as_secs() + 1;
        return Err(ConnectorError::new(ErrorKind::RateLimited, format!("{}: Binance asked to wait before {} (rate limit): try again in {secs} s", m.name, if trade::places(&req) { "new orders" } else { "any call" }))
            .with_retry_after_ms(left.as_millis().try_into().unwrap_or(u64::MAX)));
    }
    let frame = d.send_as(req, Some(signer)).await?;
    if let Some(mut e) = trade::refused(&frame, m.name) {
        if let otw_connector_sdk::RawFrame::Http { status: status @ (418 | 429), .. } = &frame {
            if let Some(ms) = e.retry_after_ms {
                lock(&HOLD).insert(host, (Instant::now() + Duration::from_millis(ms), *status == 418));
                // Refused with when to try again (spec T30), the hold keeps that time on this side.
                let what = if *status == 418 { "any call" } else { "new orders" };
                e.message = format!("{}: Binance holds {what} for {} s", e.message, ms.div_ceil(1000));
            }
        }
        return Err(e);
    }
    let counts = trade::order_counts(&frame);
    match frame {
        otw_connector_sdk::RawFrame::Http { body, .. } => Ok((body, counts)),
        _ => Err(ConnectorError::new(ErrorKind::Transient, format!("{}: no answer", m.name))),
    }
}

async fn query_on(m: &Market, d: &Driver<Binance>, signer: &dyn Signer, r: &OrderRef) -> Result<VenueOrder, ConnectorError> {
    trade::decode_order(&send_on(m, d, signer, trade::query(m, r, next_id())).await?)
}

/// The order a USDⓈ-M stop became, after changes under new client ids.
fn follow(mut r: OrderRef) -> OrderRef {
    let renamed = lock(&RENAMED);
    while let Some(next) = renamed.get(&r.id()) {
        r = next.clone();
    }
    r
}

/// What OTW linked on top of the venue among an account's still-working stored legs: each
/// entry with the exits hung on it, and each group of legs on no entry (an OCO pair, or exits
/// whose entry no longer works: filled, they are an OCO now). `(entry, legs)`, by OTW id; a leg
/// the venue never acknowledged, or alone, links nothing.
fn links_of(legs: &[StoredLeg]) -> Vec<(Option<OrderRef>, Vec<OrderRef>)> {
    let working: Vec<(&StoredLeg, OrderRef)> = legs.iter().filter_map(|l| Some((l, OrderRef::parse(l.id.as_deref()?).ok()?))).collect();
    let by_key: HashMap<&str, &OrderRef> = working.iter().map(|(l, r)| (l.key.as_str(), r)).collect();
    let mut out: Vec<(Option<OrderRef>, Vec<OrderRef>)> = Vec::new();
    let mut entries: Vec<&str> = Vec::new();
    let mut groups: Vec<(String, Vec<OrderRef>)> = Vec::new();
    for (l, r) in &working {
        match l.parent.as_deref() {
            Some(p) if by_key.contains_key(p) => {
                if !entries.contains(&p) {
                    entries.push(p);
                }
            }
            // No working entry: grouped with its siblings (same entry, else same group).
            Some(p) => push(&mut groups, format!("parent:{p}"), r.clone()),
            None => {
                if let Some(g) = l.group.as_deref().filter(|g| !g.is_empty()) {
                    // An entry is linked with its exits, not with a group of its own.
                    if !working.iter().any(|(x, _)| x.parent.as_deref() == Some(l.key.as_str())) {
                        push(&mut groups, format!("group:{g}"), r.clone());
                    }
                }
            }
        }
    }
    for e in entries {
        let exits: Vec<OrderRef> = working.iter().filter(|(l, _)| l.parent.as_deref() == Some(e)).map(|(_, r)| r.clone()).collect();
        out.push((by_key.get(e).map(|r| (*r).clone()), exits));
    }
    out.extend(groups.into_iter().filter(|(_, rs)| rs.len() > 1).map(|(_, rs)| (None, rs)));
    out
}

fn push(groups: &mut Vec<(String, Vec<OrderRef>)>, key: String, r: OrderRef) {
    match groups.iter_mut().find(|(k, _)| *k == key) {
        Some((_, rs)) => rs.push(r),
        None => groups.push((key, vec![r])),
    }
}

/// A USDⓈ-M ticket with exits, as Binance takes it: the entry first, then each exit as a plain
/// order on the other side, sent reduce-only. The link is OTW's to keep ([`link`]).
fn unlink(orders: &[OrderRequest]) -> Result<Vec<OrderRequest>> {
    let entry = orders.first().ok_or_else(|| anyhow!("no order to send"))?;
    if entry.parent.is_some() {
        return Err(anyhow!("an exit must follow the entry it is attached to"));
    }
    let mut out = vec![OrderRequest { oca_group: None, ..entry.clone() }];
    for o in &orders[1..] {
        if o.parent.as_deref() != Some(entry.client_order_id.as_str()) {
            return Err(anyhow!("Binance USDⓈ-M exits hang on the ticket's one entry"));
        }
        if o.side == entry.side {
            return Err(anyhow!("an exit is on the other side of its entry"));
        }
        out.push(OrderRequest { parent: None, oca_group: None, ..o.clone() });
    }
    Ok(out)
}

/// Why a USDⓈ-M bracket is refused before anything leaves, if it is: Binance turns down a
/// reduce-only exit with nothing to reduce (`-2022`), so behind a resting entry, on an account
/// holding nothing on the symbol in the entry's direction, the exits could not rest and the
/// entry would go out bare. A market entry has filled by the time its exits are sent.
fn flat_bracket(symbol: &str, entry: &OrderRequest, held: &[Position]) -> Option<String> {
    if entry.order_type == OrderType::Market {
        return None;
    }
    let side = if entry.side == Side::Buy { "long" } else { "short" };
    let holds = held.iter().any(|p| p.symbol.eq_ignore_ascii_case(symbol) && p.side == side && p.qty.abs() > 0.0);
    (!holds).then(|| {
        format!("Binance USDⓈ-M takes no take-profit or stop-loss with an entry while the account holds no {side} {symbol}: send the entry alone, then the exits once it has filled")
    })
}

/// OTW's half of a USDⓈ-M bracket or OCO pair, which Binance does not link: an entry gone
/// without a fill takes its exits with it, and once the entry has filled (a pair has none),
/// the first exit to fill cancels the others. Read every [`LINK_POLL`] until nothing is left to
/// link. Held in this process, and taken back up after a restart from the stored legs
/// ([`links_of`]); a bracket's exits are reduce-only, so they can close the position but never
/// open one.
async fn link(m: &'static Market, d: Arc<Driver<Binance>>, signer: Arc<dyn Signer>, entry: Option<OrderRef>, mut exits: Vec<OrderRef>) {
    let done = |st: &Mapped<OrderStatus>| matches!(st, Mapped::Known(st) if st.is_final());
    let cancel = |r: OrderRef| {
        let (d, signer) = (d.clone(), signer.clone());
        async move {
            match send_on(m, &d, signer.as_ref(), trade::cancel(m, &r, next_id())).await {
                Ok(_) => {}
                Err(e) if trade::unknown_order(&e) => {}
                Err(e) => tracing::warn!("binance: {} stays working, its bracket could not cancel it: {e}", r.id()),
            }
        }
    };
    let (mut entry_open, mut filled, mut misses) = (entry.is_some(), entry.is_none(), 0u32);
    let name = entry.as_ref().or(exits.first()).map(OrderRef::id).unwrap_or_default();
    let mut entry = entry;
    'read: while !exits.is_empty() {
        tokio::time::sleep(LINK_POLL).await;
        // A stop changed meanwhile works under its new id.
        entry = entry.map(follow);
        exits = exits.into_iter().map(follow).collect();
        if misses >= LINK_MISSES {
            tracing::warn!("binance: link of {name} let go after {misses} failed reads; its orders stay as they are");
            return;
        }
        if let Some(entry) = entry.as_ref().filter(|_| entry_open) {
            match query_on(m, &d, signer.as_ref(), entry).await {
                Ok(o) => {
                    // A stop entry that fired reads as filled with no quantity of its own.
                    filled |= o.filled > Decimal::ZERO || o.status == Mapped::Known(OrderStatus::Filled);
                    entry_open = !done(&o.status);
                }
                Err(_) => {
                    misses += 1;
                    continue;
                }
            }
        }
        if !entry_open && !filled {
            for r in exits {
                cancel(r).await;
            }
            return;
        }
        let mut states = Vec::with_capacity(exits.len());
        for r in &exits {
            match query_on(m, &d, signer.as_ref(), r).await {
                Ok(o) => states.push(o.status),
                // Binance forgets a cancelled stop after a few days.
                Err(e) if trade::unknown_order(&e) => states.push(Mapped::Known(OrderStatus::Cancelled)),
                Err(_) => {
                    misses += 1;
                    continue 'read;
                }
            }
        }
        misses = 0;
        // Before the entry fills, an exit that fired had nothing to close: it is not the one
        // that ends the trade.
        if filled && states.contains(&Mapped::Known(OrderStatus::Filled)) {
            for (r, st) in exits.into_iter().zip(states) {
                if !done(&st) {
                    cancel(r).await;
                }
            }
            return;
        }
        exits = exits.into_iter().zip(states).filter(|(_, st)| !done(st)).map(|(r, _)| r).collect();
    }
}

#[async_trait::async_trait]
impl Broker for BinanceBroker {
    fn capability(&self) -> &'static BrokerCapability {
        self.inner.capability()
    }

    /// What the venue takes, and on USDⓈ-M what OTW links on top of it: a ticket's exits and
    /// OCO pairs ([`link`]).
    fn order_caps(&self) -> Option<OrderCaps> {
        let mut caps = self.inner.caps().orders.clone();
        if self.market().kind == Kind::UsdM {
            caps.brackets = true;
            caps.oca = true;
        }
        Some(caps)
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        self.inner.validate_config(config)
    }

    /// Every order of the ticket that Binance's test route serves goes there, after the checks a
    /// place runs (capabilities, terms, filters) and, on a live key, its rights. Conditional
    /// USDⓈ-M orders have no test route and are named as unchecked.
    async fn dry_run(&self, s: &HashMap<String, String>, ticket: &OrderTicket) -> Result<String> {
        let (d, signer) = self.key(s).await?;
        let id = d.mapper().instrument_id(&ticket.ticker).map_err(anyhow::Error::new)?;
        let requests: Vec<OrderRequest> = ticket.legs.iter().map(|l| l.request(&id)).collect::<Result<_>>()?;
        let m = self.market();
        let spec = self.spec(&d, &id.native).await?;
        let bracket = m.kind == Kind::UsdM && requests.iter().any(|o| o.parent.is_some());
        let requests = if bracket { unlink(&requests)? } else { requests };
        check_orders(self.inner.caps(), &requests, now()?).map_err(anyhow::Error::new)?;
        let legs: Vec<(&OrderRequest, bool)> = requests.iter().enumerate().map(|(i, o)| (o, bracket && i > 0)).collect();
        self.pre_trade(s, &d, signer.as_ref(), &spec, &legs).await?;
        if !self.inner.wants_sandbox(s) {
            let k = trade::decode_rights(&self.send(&d, signer.as_ref(), trade::api_restrictions(next_id())).await.map_err(|e| self.fail(s, e))?).map_err(anyhow::Error::new)?;
            trade::check_rights(m, &k).map_err(anyhow::Error::new)?;
        }
        let hedge = self.hedge(&d, signer.as_ref(), s).await?;
        let (mut tested, mut unchecked) = (0, Vec::new());
        for (o, reduce) in &legs {
            let alone = OrderRequest { parent: None, oca_group: None, ..(*o).clone() };
            match trade::order_test(&trade::place(m, &alone, *reduce, hedge, next_id()).map_err(anyhow::Error::new)?) {
                Some(test) => {
                    self.send(&d, signer.as_ref(), test).await.map_err(|e| anyhow!("{}: the test route refused {} ({})", m.name, o.client_order_id, self.fail(s, e)))?;
                    tested += 1;
                }
                None => unchecked.push(o.client_order_id.clone()),
            }
        }
        let rest = if unchecked.is_empty() { String::new() } else { format!("; no test route for {}", unchecked.join(", ")) };
        Ok(format!("{}: {tested} order(s) accepted by the test route, nothing placed{rest}", m.name))
    }

    async fn is_live(&self, s: &HashMap<String, String>) -> Result<bool> {
        Ok(!self.inner.wants_sandbox(s))
    }

    async fn test(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<String> {
        self.inner.test(client, s).await
    }

    async fn executions(&self, client: &reqwest::Client, s: &HashMap<String, String>, q: &ExecQuery) -> Result<Vec<Execution>> {
        self.inner.executions(client, s, q).await
    }

    async fn symbols(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<String>> {
        self.inner.symbols(client, s).await
    }

    async fn holdings(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<Holding>> {
        self.inner.holdings(client, s).await
    }

    async fn quotes(&self, client: &reqwest::Client, s: &HashMap<String, String>, symbols: &[String]) -> Result<Vec<Quote>> {
        self.inner.quotes(client, s, symbols).await
    }

    /// Spot: the pair's base asset, free and locked (`BTC` on `BTCUSDT`). A pair whose assets
    /// cannot be read is an error, never a guess from its spelling.
    async fn held_asset(&self, s: &HashMap<String, String>, ticker: &str) -> Result<Option<Decimal>> {
        if self.market().kind != Kind::Spot {
            return Ok(None);
        }
        let (d, signer) = self.key(s).await?;
        let symbol = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
        let Some((base, _)) = self.pair(&d, signer.as_ref(), &symbol).await else {
            return Err(anyhow!("{}: the assets of {symbol} could not be read", self.label()));
        };
        let balances = trade::decode_spot_balances(&self.send(&d, signer.as_ref(), trade::spot_account(next_id())).await.map_err(|e| self.fail(s, e))?).map_err(anyhow::Error::new)?;
        Ok(Some(balances.into_iter().find(|(a, _, _)| *a == base).map(|(_, free, locked)| free + locked).unwrap_or_default()))
    }

    /// An order by its client id: spot's book, USDⓈ-M's order book then its algo book (a
    /// conditional order lives there). Unknown to every book it could be in: not found.
    async fn lookup(&self, s: &HashMap<String, String>, ticker: &str, _asset_type: &str, key: &str) -> Result<super::Lookup> {
        let (d, signer) = self.key(s).await?;
        let symbol = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?.native;
        let books: &[bool] = if self.market().kind == Kind::UsdM { &[false, true] } else { &[false] };
        for &algo in books {
            let r = OrderRef { symbol: symbol.clone(), client_id: key.to_string(), algo };
            match self.query(&d, signer.as_ref(), &r).await {
                Ok(o) => {
                    let status = o.status.known().copied().ok_or_else(|| anyhow!("{}: order {key} has a status OTW does not know", self.label()))?;
                    return Ok(super::Lookup::Found { id: r.id(), status, filled_qty: o.filled });
                }
                Err(e) if trade::unknown_order(&e) => {}
                Err(e) => return Err(self.fail(s, e)),
            }
        }
        Ok(super::Lookup::NotFound)
    }

    /// The venue's positions, each closable by its symbol.
    async fn positions(&self, client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<Position>> {
        let mut rows = self.inner.positions(client, s).await?;
        // Hedge mode lists a long and a short on one symbol: each closes on its own.
        let both: HashSet<String> = rows.iter().filter(|p| rows.iter().filter(|q| q.symbol == p.symbol).count() > 1).map(|p| p.symbol.clone()).collect();
        for p in &mut rows {
            p.native_id = Some(if both.contains(&p.symbol) { format!("{}:{}", p.symbol, p.side.to_uppercase()) } else { p.symbol.clone() });
        }
        // USDⓈ-M: the leverage and margin mode the account set on each symbol, as read; a
        // position is still listed when they cannot be.
        if self.market().kind == Kind::UsdM && !rows.is_empty() {
            let (d, signer) = self.key(s).await?;
            match self.send(&d, signer.as_ref(), trade::symbol_config(next_id())).await.and_then(|b| trade::decode_symbol_config(&b)) {
                Ok(cfg) => {
                    for p in &mut rows {
                        if let Some(c) = cfg.get(&p.symbol) {
                            p.leverage = c.leverage;
                            p.margin_mode = c.margin_mode.clone();
                        }
                    }
                }
                Err(e) => tracing::warn!("binance: leverage and margin mode not read: {e}"),
            }
        }
        Ok(rows)
    }

    /// Working orders under the ids orders are sent by (`SYMBOL:client id`), USDⓈ-M stops
    /// included (the algo book, which `openOrders` leaves out).
    async fn orders(&self, _client: &reqwest::Client, s: &HashMap<String, String>) -> Result<Vec<OpenOrder>> {
        let (d, signer) = self.key(s).await?;
        let rows = self.working(&d, signer.as_ref()).await.map_err(|e| self.fail(s, e))?;
        let venue = self.inner.caps().account.venue_label.clone().unwrap_or_else(|| self.label().to_string());
        let mut out = Vec::with_capacity(rows.len());
        for o in &rows {
            // Best effort: an order is still an order without its currency.
            let currency = self.spec(&d, &o.r.symbol).await.map(|sp| sp.quote).unwrap_or_default();
            out.push(open_order_of(o, currency, venue.clone()));
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    async fn place(&self, s: &HashMap<String, String>, ticket: &OrderTicket) -> Result<Vec<Placed>> {
        self.check_env(s)?;
        let (d, signer) = self.key(s).await?;
        let id = d.mapper().instrument_id(&ticket.ticker).map_err(anyhow::Error::new)?;
        let requests: Vec<OrderRequest> = ticket.legs.iter().map(|l| l.request(&id)).collect::<Result<_>>()?;
        let m = self.market();
        let spec = self.spec(&d, &id.native).await?;
        let tick = spec.tick_size.map(|t| t.value());
        let done = |placed: Vec<(String, OrderRef)>| Ok(placed.into_iter().map(|(key, r)| Placed { key, id: r.id(), tick }).collect());
        if m.kind == Kind::Spot {
            check_orders(self.inner.caps(), &requests, now()?).map_err(anyhow::Error::new)?;
            let legs: Vec<(&OrderRequest, bool)> = requests.iter().map(|o| (o, false)).collect();
            self.pre_trade(s, &d, signer.as_ref(), &spec, &legs).await?;
            self.claim(s, &requests)?;
            return match self.place_spot(s, &d, signer.as_ref(), &requests).await {
                Ok(placed) => done(placed),
                Err(e) => {
                    if let Some(c) = e.downcast_ref::<ConnectorError>() {
                        self.reload_rules(s, &id.native, c);
                    }
                    Err(e)
                }
            };
        }
        // USDⓈ-M links nothing itself: exits and OCO pairs go out as plain orders, and OTW
        // links them.
        let bracket = requests.iter().any(|o| o.parent.is_some());
        let pair = !bracket && requests.len() == 2 && requests[0].oca_group.is_some() && requests[0].oca_group == requests[1].oca_group;
        if bracket {
            let http = super::client()?;
            let held = self.positions(&http, s).await?;
            if let Some(why) = flat_bracket(&id.native, &requests[0], &held) {
                return Err(anyhow!(why));
            }
        }
        let requests = if bracket {
            unlink(&requests)?
        } else if pair {
            requests.into_iter().map(|o| OrderRequest { oca_group: None, ..o }).collect()
        } else {
            requests
        };
        check_orders(self.inner.caps(), &requests, now()?).map_err(anyhow::Error::new)?;
        let legs: Vec<(&OrderRequest, bool)> = requests.iter().enumerate().map(|(i, o)| (o, bracket && i > 0)).collect();
        self.pre_trade(s, &d, signer.as_ref(), &spec, &legs).await?;
        let hedge = self.hedge(&d, signer.as_ref(), s).await?;
        // The dry run takes the first order the test route serves (conditional ones it does not).
        let probe = requests.iter().enumerate().find(|(_, o)| !trade::placed_ref(m, o).algo);
        self.live_gate(s, &d, signer.as_ref(), probe.map(|(_, o)| o), probe.is_some_and(|(i, _)| bracket && i > 0)).await?;
        self.claim(s, &requests)?;
        let scope = self.scope(s);
        let mut placed: Vec<OrderRef> = Vec::new();
        for (i, o) in requests.iter().enumerate() {
            let reduce = bracket && i > 0;
            match self.place_one(&d, signer.as_ref(), o, reduce, hedge, &scope).await {
                Ok(r) => placed.push(r),
                Err(e) => {
                    self.undo(&d, signer.as_ref(), &placed).await;
                    self.reload_rules(s, &id.native, &e);
                    if !placed.is_empty() || e.provider_code.as_deref() == Some(LOST) {
                        self.arm(s, &d, &signer, &id.native);
                    }
                    return Err(self.fail_placing(s, e, &[o]));
                }
            }
        }
        self.arm(s, &d, &signer, &id.native);
        if bracket {
            tokio::spawn(link(m, d.clone(), signer.clone(), Some(placed[0].clone()), placed[1..].to_vec()));
        } else if pair {
            tokio::spawn(link(m, d.clone(), signer.clone(), None, placed.clone()));
        }
        done(requests.iter().map(|o| o.client_order_id.clone()).zip(placed).collect())
    }

    /// A USDⓈ-M stop has no change in place: the changed stop goes out first under the next
    /// client id ([`trade::next_client_id`], so its OTW id changes), and the old one is
    /// cancelled once it works, so a stop works throughout. Refused, the old one stands.
    async fn modify(&self, s: &HashMap<String, String>, id: &str, change: &OrderAmend) -> Result<Option<Placed>> {
        self.check_env(s)?;
        let asked = OrderRef::parse(id).map_err(anyhow::Error::new)?;
        check_amend(self.inner.caps(), change, now()?).map_err(anyhow::Error::new)?;
        let (d, signer) = self.key(s).await?;
        let m = self.market();
        // Read once it is this change's turn: a change before it may have replaced its list,
        // or renamed its stop.
        let _turn = self.queue(s, &asked.symbol).await;
        let r = follow(asked);
        let cur = self.query(&d, signer.as_ref(), &r).await.map_err(|e| self.fail(s, e))?;
        if matches!(cur.status, Mapped::Known(st) if st.is_final()) {
            return Err(anyhow!("order {id} is not working any more"));
        }
        let spec = self.spec(&d, &r.symbol).await?;
        Self::check_change(&spec, &cur, change).map_err(anyhow::Error::new)?;
        if let Some(rules) = self.rules(&d, signer.as_ref(), s, &r.symbol).await {
            let probe = Self::probe(&spec, &cur, change);
            let reference = if trade::needs_reference(m, &rules, &probe) { self.reference(&d, signer.as_ref(), &r.symbol).await } else { None };
            trade::check_filters(m, &rules, &probe, trade::reduces(&cur), reference).map_err(anyhow::Error::new)?;
        }
        self.live_gate(s, &d, signer.as_ref(), None, false).await?;
        let scope = self.scope(s);
        if m.kind == Kind::UsdM && r.algo {
            let new = trade::replacement(m, &cur, change, &trade::next_client_id(&r.client_id)).map_err(anyhow::Error::new)?;
            let hedge = cur.position_side.as_deref().is_some_and(|p| p != "BOTH");
            self.claim(s, std::slice::from_ref(&new))?;
            let nr = self
                .place_one(&d, signer.as_ref(), &new, false, hedge, &scope)
                .await
                .map_err(|e| anyhow!("Binance refused the change ({}); the stop {id} stands as it was", self.fail(s, e)))?;
            // Both work for this moment; the old one goes now.
            match self.send(&d, signer.as_ref(), trade::cancel(m, &r, next_id())).await {
                Ok(_) => {}
                Err(e) if trade::unknown_order(&e) => {
                    // Gone meanwhile: if it fired, the new one must not fire again.
                    let fired = self.query(&d, signer.as_ref(), &r).await.map(|o| o.status == Mapped::Known(OrderStatus::Filled)).unwrap_or(true);
                    if fired {
                        let undone = self.send(&d, signer.as_ref(), trade::cancel(m, &nr, next_id())).await;
                        return Err(match undone {
                            Ok(_) => anyhow!("stop {id} fired before it could be changed; its replacement was cancelled"),
                            Err(c) => anyhow!("stop {id} fired before it could be changed, and its replacement {} could not be cancelled ({c}): cancel it", nr.id()),
                        });
                    }
                }
                Err(e) => return Err(anyhow!("the changed stop works as {}, but {id} could not be cancelled ({}): both work, cancel {id}", nr.id(), self.fail(s, e))),
            }
            let placed = Placed { key: nr.client_id.clone(), id: nr.id(), tick: spec.tick_size.map(|t| t.value()) };
            lock(&RENAMED).insert(r.id(), nr);
            return Ok(Some(placed));
        }
        if let (Kind::Spot, Some(list)) = (m.kind, cur.list) {
            if self.move_in_list(s, &d, signer.as_ref(), &cur, list, change, &scope).await? {
                return Ok(None);
            }
        }
        let req = trade::amend(m, &cur, change, next_id()).map_err(anyhow::Error::new)?;
        if m.kind == Kind::Spot {
            lock(&REPLACING).insert(cur.venue_id.clone(), Instant::now());
        }
        self.answered(&d, signer.as_ref(), req, &r, &r.client_id, &scope, 1).await.map_err(|e| {
            lock(&REPLACING).remove(&cur.venue_id);
            self.reload_rules(s, &r.symbol, &e);
            self.fail(s, e)
        })?;
        Ok(None)
    }

    /// An order Binance no longer knows as working (`-2011`) is looked up: final (cancelled,
    /// expired, or filled before the cancel reached it) is done, as at IB (spec 6.2); the
    /// reconciliation cycle and the account stream show how it ended. A USDⓈ-M stop changed
    /// under a new id is cancelled under that one.
    async fn cancel(&self, s: &HashMap<String, String>, id: &str) -> Result<()> {
        let asked = OrderRef::parse(id).map_err(anyhow::Error::new)?;
        let (d, signer) = self.key(s).await?;
        // After any change in flight on the symbol: a list it is placing again is cancelled whole.
        let _turn = self.queue(s, &asked.symbol).await;
        let r = follow(asked);
        let e = match self.send(&d, signer.as_ref(), trade::cancel(self.market(), &r, next_id())).await {
            Ok(_) => return Ok(()),
            Err(e) => e,
        };
        if !trade::unknown_order(&e) {
            return Err(self.fail(s, e));
        }
        match self.query(&d, signer.as_ref(), &r).await {
            Ok(o) => match o.status {
                Mapped::Known(st) if st.is_final() => Ok(()),
                _ => Err(self.fail(s, e)),
            },
            Err(_) => Err(self.fail(s, e)),
        }
    }

    /// Every working order, every symbol, both books.
    async fn cancel_all(&self, s: &HashMap<String, String>) -> Result<usize> {
        let (d, signer) = self.key(s).await?;
        let rows = self.working(&d, signer.as_ref()).await.map_err(|e| self.fail(s, e))?;
        let mut symbols: Vec<&str> = rows.iter().map(|o| o.r.symbol.as_str()).collect();
        symbols.sort_unstable();
        symbols.dedup();
        for sym in symbols {
            for req in trade::cancel_symbol(self.market(), sym, next_id()) {
                match self.send(&d, signer.as_ref(), req).await {
                    Ok(_) => {}
                    // One book of the symbol had nothing working.
                    Err(e) if trade::unknown_order(&e) => {}
                    Err(e) => return Err(self.fail(s, e)),
                }
            }
        }
        Ok(rows.len())
    }

    /// The chart just opened this instrument on the account: read the venue clock and the
    /// instrument's terms now, so the first ticket waits for neither.
    async fn symbol_for(&self, s: &HashMap<String, String>, ticker: &str, _asset_type: &str) -> Result<Option<String>> {
        let (d, _) = self.key(s).await?;
        let id = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?;
        self.spec(&d, &id.native).await?;
        Ok(Some(id.native))
    }

    /// `LOT_SIZE`'s step, from the instrument list [`Broker::symbol_for`] just read.
    async fn qty_step(&self, s: &HashMap<String, String>, ticker: &str) -> Result<Option<Decimal>> {
        let (d, _) = self.key(s).await?;
        let id = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?;
        Ok(self.spec(&d, &id.native).await?.lot_size.map(|q| q.value()).filter(|v| *v > Decimal::ZERO))
    }

    async fn leverage(&self, s: &HashMap<String, String>, ticker: &str) -> Result<Option<(Option<Decimal>, Option<String>)>> {
        if self.market().kind != Kind::UsdM {
            return Ok(None);
        }
        let (d, signer) = self.key(s).await?;
        let id = d.mapper().instrument_id(ticker).map_err(anyhow::Error::new)?;
        let cfg = trade::decode_symbol_config(&self.send(&d, signer.as_ref(), trade::symbol_config(next_id())).await?)?;
        Ok(cfg.get(&id.native).map(|c| (c.leverage, c.margin_mode.clone())))
    }

    /// USDⓈ-M: the brackets and OCO pairs OTW was linking before a restart, linked again from
    /// the stored legs still working, and the dead man's switch armed again on their symbols.
    /// Spot order lists are linked at the venue: nothing to do.
    async fn resume(&self, s: &HashMap<String, String>, legs: &[StoredLeg]) -> Result<()> {
        if self.market().kind != Kind::UsdM || legs.is_empty() {
            return Ok(());
        }
        let (d, signer) = self.key(s).await?;
        let mut symbols: Vec<String> = legs.iter().filter_map(|l| OrderRef::parse(l.id.as_deref()?).ok()).map(|r| r.symbol).collect();
        symbols.sort_unstable();
        symbols.dedup();
        for sym in &symbols {
            self.arm(s, &d, &signer, sym);
        }
        for (entry, exits) in links_of(legs) {
            tokio::spawn(link(self.market(), d.clone(), signer.clone(), entry, exits));
        }
        Ok(())
    }

    /// A futures position at the market, reduce-only: it can shrink the position, never turn it.
    async fn close_position(&self, s: &HashMap<String, String>, native_id: &str, _account: &str, key: &str) -> Result<Placed> {
        if self.market().kind == Kind::Spot {
            return Err(anyhow!("a Binance spot balance is not a position: sell it with an order"));
        }
        self.check_env(s)?;
        let (d, signer) = self.key(s).await?;
        // `SYMBOL`, or `SYMBOL:LONG` / `SYMBOL:SHORT` for one side of a hedge-mode pair.
        let (symbol, wanted) = match native_id.trim().split_once(':') {
            Some((sym, side)) => (sym, Some(side.to_ascii_uppercase())),
            None => (native_id.trim(), None),
        };
        let id = d.mapper().instrument_id(symbol).map_err(anyhow::Error::new)?;
        let rows = trade::decode_positions(&self.send(&d, signer.as_ref(), trade::position(&id.native, next_id())).await.map_err(|e| self.fail(s, e))?)
            .map_err(anyhow::Error::new)?;
        let held: Vec<&(String, Decimal)> = rows.iter().filter(|(side, amt)| !amt.is_zero() && wanted.as_ref().is_none_or(|w| w == side)).collect();
        let amt = match held.as_slice() {
            [] => return Err(anyhow!("no position on {symbol} to close")),
            [(_, amt)] => *amt,
            _ => return Err(anyhow!("{symbol} is held both long and short (hedge mode): close each side from its own row")),
        };
        let hedge = held[0].0 != "BOTH";
        let o = OrderRequest {
            client_order_id: key.to_string(),
            instrument: id,
            side: if amt > Decimal::ZERO { Side::Sell } else { Side::Buy },
            order_type: OrderType::Market,
            qty: Qty::new(amt.abs()),
            limit: None,
            stop: None,
            tif: TimeInForce::Gtc,
            parent: None,
            oca_group: None,
            account: None,
            opts: Default::default(),
        };
        trade::check_client_id(key).map_err(anyhow::Error::new)?;
        self.live_gate(s, &d, signer.as_ref(), Some(&o), true).await?;
        self.claim(s, std::slice::from_ref(&o))?;
        let r = self.place_one(&d, signer.as_ref(), &o, true, hedge, &self.scope(s)).await.map_err(|e| self.fail(s, e))?;
        Ok(Placed { key: key.to_string(), id: r.id(), tick: None })
    }

    /// The account's user data stream ([`super::binance_events`]).
    async fn events(&self, s: &HashMap<String, String>) -> Result<Option<futures::stream::BoxStream<'static, super::AccountEvent>>> {
        let (driver, signer) = self.key(s).await?;
        let venue = self.inner.caps().account.venue_label.clone().unwrap_or_else(|| self.label().to_string());
        let follow = super::binance_events::Follow { market: self.market(), testnet: self.inner.wants_sandbox(s), driver, signer, venue, row: open_order_of };
        super::binance_events::follow(follow, s).await.map(Some).map_err(|e| self.fail(s, e))
    }

    /// The account's figures as Binance states them, in USDT. USDⓈ-M: `/fapi/v3/account`.
    /// Spot: the USDT balance as cash (free as available) and, on live, the spot wallet's
    /// value in USDT as Binance computes it (`/sapi/v1/asset/wallet/balance`) as equity; Spot
    /// Demo Mode serves no `/sapi`, so its equity stays unstated rather than summed here.
    /// `symbol`'s base and quote assets come from the same balances.
    async fn account(&self, s: &HashMap<String, String>, symbol: Option<&str>) -> Result<Vec<AccountSummary>> {
        if self.market().kind == Kind::Spot {
            let (d, signer) = self.key(s).await?;
            let balances = trade::decode_spot_balances(&self.send(&d, signer.as_ref(), trade::spot_account(next_id())).await.map_err(|e| self.fail(s, e))?).map_err(anyhow::Error::new)?;
            let held = |asset: &str| balances.iter().find(|(a, _, _)| a == asset).map(|(_, free, locked)| (*free, *locked)).unwrap_or_default();
            let (free, locked) = held("USDT");
            let assets = match symbol {
                Some(sym) => self.pair(&d, signer.as_ref(), sym).await.map(|(base, quote)| {
                    [base, quote].into_iter().map(|asset| {
                        let (free, locked) = held(&asset);
                        super::AssetBalance { asset, free: f(free), locked: f(locked) }
                    }).collect()
                }),
                None => None,
            };
            let equity = if self.inner.wants_sandbox(s) {
                None
            } else {
                match self.send(&d, signer.as_ref(), trade::wallet_balance("USDT", next_id())).await.and_then(|b| trade::decode_spot_wallet(&b)) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("binance: spot wallet value not read, equity left unstated: {e}");
                        None
                    }
                }
            };
            return Ok(vec![AccountSummary {
                currency: "USDT".into(),
                equity: equity.map(f),
                cash: Some(f(free + locked)),
                available: Some(f(free)),
                assets: assets.unwrap_or_default(),
                ..Default::default()
            }]);
        }
        let (d, signer) = self.key(s).await?;
        let a = trade::decode_futures_account(&self.send(&d, signer.as_ref(), trade::futures_account(next_id())).await.map_err(|e| self.fail(s, e))?)
            .map_err(anyhow::Error::new)?;
        Ok(vec![AccountSummary {
            currency: "USDT".into(),
            equity: a.equity.map(f),
            cash: a.wallet.map(f),
            unrealized: a.unrealized.map(f),
            margin_used: a.initial_margin.map(f),
            maintenance_margin: a.maintenance_margin.map(f),
            available: a.available.map(f),
            ..Default::default()
        }])
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_model::{InstrumentId, Price};

    fn order(id: &str, side: Side, parent: Option<&str>) -> OrderRequest {
        OrderRequest {
            client_order_id: id.into(),
            instrument: InstrumentId::new("BINANCE_USDM", "BTCUSDT"),
            side,
            order_type: OrderType::Limit,
            qty: Qty::new(Decimal::ONE),
            limit: Some(Price::new(Decimal::from(80_000))),
            stop: None,
            tif: TimeInForce::Gtc,
            parent: parent.map(String::from),
            oca_group: parent.map(|_| "g".to_string()),
            account: None,
            opts: Default::default(),
        }
    }

    fn held(symbol: &str, side: &str, qty: f64) -> Position {
        Position {
            symbol: symbol.into(),
            side: side.into(),
            qty,
            avg_price: None,
            currency: "USDT".into(),
            asset_class: "future".into(),
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
    fn a_resting_entrys_bracket_needs_a_position_in_its_direction() {
        // T07
        let (buy, sell) = (order("e", Side::Buy, None), order("e", Side::Sell, None));
        let why = flat_bracket("BTCUSDT", &buy, &[]).unwrap();
        assert!(why.contains("holds no long BTCUSDT"), "{why}");
        assert!(flat_bracket("BTCUSDT", &buy, &[held("BTCUSDT", "short", 1.0)]).is_some(), "a short does not carry a long's exits");
        assert!(flat_bracket("BTCUSDT", &buy, &[held("ETHUSDT", "long", 1.0)]).is_some());
        assert!(flat_bracket("BTCUSDT", &buy, &[held("BTCUSDT", "long", 0.0)]).is_some());
        assert!(flat_bracket("BTCUSDT", &buy, &[held("btcusdt", "long", 0.01)]).is_none());
        assert!(flat_bracket("BTCUSDT", &sell, &[held("BTCUSDT", "short", 0.01)]).is_none());
        let market = OrderRequest { order_type: OrderType::Market, limit: None, ..buy };
        assert!(flat_bracket("BTCUSDT", &market, &[]).is_none(), "a market entry has filled before its exits leave");
    }

    #[test]
    fn exits_go_out_as_plain_orders_after_their_entry() {
        let out = unlink(&[order("e", Side::Buy, None), order("tp", Side::Sell, Some("e")), order("sl", Side::Sell, Some("e"))]).unwrap();
        assert_eq!(out.iter().map(|o| o.client_order_id.as_str()).collect::<Vec<_>>(), ["e", "tp", "sl"]);
        assert!(out.iter().all(|o| o.parent.is_none() && o.oca_group.is_none()));
    }

    fn leg(key: &str, id: Option<&str>, parent: Option<&str>, group: Option<&str>) -> StoredLeg {
        StoredLeg {
            key: key.into(),
            id: id.map(String::from),
            ticker: "BTCUSDT".into(),
            asset_type: "crypto".into(),
            parent: parent.map(String::from),
            group: group.map(String::from),
            status: "working".into(),
        }
    }

    fn ids(link: &(Option<OrderRef>, Vec<OrderRef>)) -> (Option<String>, Vec<String>) {
        (link.0.as_ref().map(OrderRef::id), link.1.iter().map(OrderRef::id).collect())
    }

    #[test]
    fn stored_legs_rebuild_brackets_and_oco_pairs() {
        let legs = [
            // A bracket still waiting on its entry.
            leg("e", Some("BTCUSDT:e"), None, Some("g1")),
            leg("tp", Some("BTCUSDT:tp"), Some("e"), Some("g1")),
            leg("sl", Some("BTCUSDT:algo:sl.r1"), Some("e"), Some("g1")),
            // An OCO pair.
            leg("a", Some("BTCUSDT:a"), None, Some("g2")),
            leg("b", Some("BTCUSDT:algo:b"), None, Some("g2")),
            // Exits whose entry filled: an OCO now.
            leg("x1", Some("ETHUSDT:x1"), Some("gone"), None),
            leg("x2", Some("ETHUSDT:algo:x2"), Some("gone"), None),
            // Alone, or never acknowledged: nothing to link.
            leg("solo", Some("BTCUSDT:solo"), None, Some("g3")),
            leg("plain", Some("BTCUSDT:plain"), None, None),
            leg("lost", None, None, Some("g2")),
        ];
        let links: Vec<_> = links_of(&legs).iter().map(ids).collect();
        assert_eq!(links.len(), 3, "{links:?}");
        assert!(links.contains(&(Some("BTCUSDT:e".into()), vec!["BTCUSDT:tp".into(), "BTCUSDT:algo:sl.r1".into()])));
        assert!(links.contains(&(None, vec!["BTCUSDT:a".into(), "BTCUSDT:algo:b".into()])));
        assert!(links.contains(&(None, vec!["ETHUSDT:x1".into(), "ETHUSDT:algo:x2".into()])));
        // A filled entry's lone exit links nothing.
        assert!(links_of(&[leg("x", Some("BTCUSDT:x"), Some("gone"), None)]).is_empty());
    }

    #[test]
    fn a_link_follows_a_stop_replaced_under_a_new_id() {
        let old = OrderRef::parse("BTCUSDT:algo:zz_follow").unwrap();
        lock(&RENAMED).insert(old.id(), OrderRef::parse("BTCUSDT:algo:zz_follow.r1").unwrap());
        lock(&RENAMED).insert("BTCUSDT:algo:zz_follow.r1".into(), OrderRef::parse("BTCUSDT:algo:zz_follow.r2").unwrap());
        assert_eq!(follow(old).id(), "BTCUSDT:algo:zz_follow.r2");
        assert_eq!(follow(OrderRef::parse("BTCUSDT:k").unwrap()).id(), "BTCUSDT:k");
    }

    #[test]
    fn an_exit_on_the_entry_side_or_another_entry_is_refused() {
        assert!(unlink(&[order("e", Side::Buy, None), order("tp", Side::Buy, Some("e"))]).is_err());
        assert!(unlink(&[order("e", Side::Buy, None), order("tp", Side::Sell, Some("x"))]).is_err());
        assert!(unlink(&[order("tp", Side::Sell, Some("e"))]).is_err());
    }
}
