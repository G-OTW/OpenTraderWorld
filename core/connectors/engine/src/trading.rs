//! The trader: order sending written once for every venue whose mapper implements
//! [`TradeMapper`] (onboarding spec 4, 6, 8). The venue's mapper says what the calls are; the
//! order they go in and what each answer means for the user is decided here.
//!
//! - **Refused before I/O.** Capabilities (`check_orders`), the client id rule, the venue's own
//!   instrument rules ([`check_rules`]) and the account's position mode are checked before
//!   anything leaves. A refusal names the fix; nothing is adjusted.
//! - **One turn per instrument.** Placing, changing and cancelling on one instrument of one
//!   account wait for each other, first come first served.
//! - **A lost answer is looked up, never resent.** A timeout, a dropped socket, a 5xx, an answer
//!   the mapper cannot read and an error code the venue's table does not list are not refusals:
//!   the order is looked up by its client id; found, it is placed; otherwise it is
//!   [`TradeError::Unknown`], for the reconciliation cycle to settle. Nothing is sent again.
//! - **A ticket is never left half placed.** A leg refused after others went out cancels them,
//!   and the answer names each leg's outcome; a leg whose cancel failed stays unknown, since it
//!   may be working.
//! - **Rules reloaded on a rule refusal.** The rules are read once a day, and again on the next
//!   order after the venue refused one on a rule (`Rejected(VenueRule)`).
//! - **Rate limits refuse, never queue.** A rate-limit answer holds new orders on the account
//!   until the venue's wait is over; the refusal says how long.
//! - **Announced maintenance refuses new orders.** While the venue's private stream says
//!   maintenance is announced ([`PrivateEvent::Maintenance`]), a new order is refused with the
//!   reason; cancels go out as ever. Nothing is queued or retried.
//! - **The venue's check first, when asked.** With `dry_run_first`, the first order of the
//!   account's session goes to the venue's own check (`dry_run` in the caps) before it is placed;
//!   an order that check does not cover is refused rather than sent unchecked.
//! - **Read from the venue, never chosen.** The position mode (per instrument from the rows, or
//!   account wide where the venue states it), the margin of each instrument and, for a book
//!   that counts in contracts, each contract's size are read once and held; an order carries
//!   them ([`Placing`]).
//! - **A quote-sized market buy** (a venue that takes a spot market buy as an amount to spend)
//!   is valued at the reference price read just before it leaves, rounded down to the venue's
//!   quote step; the answer says the size filled may differ a little.
//! - **A book sized in the quote currency** (Deribit inverse contracts, in USD): a size given in
//!   that currency goes out rounded down to the venue's step; a size given in the coin is
//!   converted at the price read just before it leaves, then rounded down. Never up: a size that
//!   rounds to nothing is refused, naming the smallest the venue takes.
//! - **Leverage is the venue's.** An order carries the leverage the venue holds for the
//!   instrument (read from it, or from the position rows and stream); none when it holds none,
//!   the venue then applying its own default. OTW never chooses one.
//! - **A minted client id** (a venue whose ids are not spelled from OTW's key, Bitfinex) is
//!   minted before I/O, stored with the leg by the caller, and handed with its time to every
//!   lookup and cancel; a stream order maps back to OTW's key through it.
//! - **A trigger that fires as a new order** (Deribit) is followed to that order for its state,
//!   its fills and its cancel ([`VenueOrder::successor_id`]).
//! - **Exact decimals only.** No float on this path (`float_arithmetic` is denied crate-wide).

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use otw_connector_model::{
    check_amend, check_orders, ConnectorError, CorrId, Decimal, DryRunCap, ErrorKind, Mapped, OrderAmend, OrderOpts, OrderRequest, OrderStatus,
    OrderType, Price, Qty, Side, SizeUnit, TimeInForce, TifKind,
};
use otw_connector_sdk::trade::{
    check_rules, next_client_id, quote_size, Ack, CancelAllWay, Canon, ClientRef, ContractSizes, DecodeCtx, ExitChange, ExitKind, LookupAt, MarginMode, Placing, PositionMode,
    PositionSide, Refusal, SymbolNames, TradeMapper, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
#[cfg(doc)]
use otw_connector_sdk::trade::PrivateEvent;
use otw_connector_sdk::{Driver, HttpRequest, Signer};

/// How long an instrument's rules are trusted (spec 4.2: refreshed daily).
pub const RULES_TTL: Duration = Duration::from_secs(86_400);
/// The most pages a list is read over, so a venue that keeps handing back a cursor cannot loop.
const MAX_PAGES: usize = 200;

static CORR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(2_000_000_000);

fn corr() -> CorrId {
    CorrId(CORR.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// The last stamp a minted client id was made at: each mint is strictly after the one before,
/// across every venue and account of the process.
static STAMP: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// A minting stamp at or after `now_ms`, strictly after every one handed out before.
fn stamp(now_ms: i64) -> i64 {
    let mut last = STAMP.load(std::sync::atomic::Ordering::Relaxed);
    loop {
        let next = now_ms.max(last.saturating_add(1));
        match STAMP.compare_exchange(last, next, std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Relaxed) {
            Ok(_) => return next,
            Err(seen) => last = seen,
        }
    }
}

/// How many walks a successor chain (a trigger that fired as a new order) is followed for.
const MAX_HOPS: usize = 4;
/// Client ids held past this many are pruned of those older than [`CLIENT_TTL_MS`].
const CLIENTS_KEPT: usize = 20_000;
const CLIENT_TTL_MS: i64 = 30 * 86_400_000;
/// Between a scope and an id in the client tables.
const SEP: char = '\u{1f}';

/// The tables held per host, and when they were read.
type HeldTables = (Arc<ContractSizes>, Arc<SymbolNames>, Instant);

/// What a book's answers are read with: each instrument's contract size and the venue's other
/// names for its instruments, read together from the instrument list.
#[derive(Default)]
struct Tables {
    sizes: Arc<ContractSizes>,
    names: Arc<SymbolNames>,
}

impl Tables {
    fn ctx<'a>(&'a self, endpoint: &'a str) -> DecodeCtx<'a> {
        DecodeCtx::new(endpoint, &self.sizes).with_names(&self.names)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The account an operation runs for.
pub struct Account<'a> {
    pub settings: &'a otw_connector_sdk::trade::Settings,
    pub signer: &'a dyn Signer,
    /// What this process keeps per account (holds, turns, the session's check): a name that is
    /// not a secret (a hash of the key) and that tells the venue's environments apart.
    pub scope: &'a str,
}

/// Why an operation did not do what was asked.
#[derive(Debug, Clone, PartialEq)]
pub enum TradeError {
    /// Nothing was placed or changed, for this reason (the refusal names the fix).
    Refused(ConnectorError),
    /// The venue's answer was lost or not understood: the legs named (OTW client ids) may be
    /// working. `alert`: the venue answered something its table does not map.
    Unknown { keys: Vec<String>, message: String, alert: bool },
}

impl TradeError {
    pub fn message(&self) -> &str {
        match self {
            TradeError::Refused(e) => &e.message,
            TradeError::Unknown { message, .. } => message,
        }
    }
}

impl std::fmt::Display for TradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for TradeError {}

/// The rules of `symbol` among `rows`: the row it names, else the one that gives it as another
/// name ([`VenueRules::aliases`], Kraken `XBTUSD` for `XBT/USD`).
fn pick_rules(rows: Vec<VenueRules>, symbol: &str) -> Option<VenueRules> {
    let named = rows.iter().position(|r| r.instrument.native.eq_ignore_ascii_case(symbol));
    let at = named.or_else(|| rows.iter().position(|r| r.aliases.iter().any(|n| n.eq_ignore_ascii_case(symbol))))?;
    rows.into_iter().nth(at)
}

fn refused(e: ConnectorError) -> TradeError {
    TradeError::Refused(e)
}

/// A leg the venue accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// OTW's client id.
    pub key: String,
    pub venue_id: String,
    pub symbol: String,
    /// What the user is told about how it went out (a market buy sent as an amount to spend).
    pub note: Option<String>,
}

/// What a cancel ended on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cancelled {
    /// The venue took the cancel.
    Sent,
    /// The venue no longer had it working: it had ended this way (spec 6.2, often `Filled`).
    AlreadyFinal(OrderStatus),
}

/// What a ticket is sent with, besides its orders.
#[derive(Debug, Clone, Default)]
pub struct TicketOptions {
    /// The price a market order's value is measured at (the chart's last), when known.
    pub reference: Option<Decimal>,
    /// Ask the venue's own check before the first order of this account's session.
    pub dry_run_first: bool,
}

/// The order path for one venue, shared by every account on it. Holds no driver: each call is
/// given the account's driver (live or test host), so a test can point it at a mock venue.
pub struct Trader<M: TradeMapper> {
    rules: Mutex<HashMap<String, (VenueRules, Instant)>>,
    /// Rules to read again before the next order (a venue refusal named one).
    stale: Mutex<HashSet<String>>,
    /// New orders held until then, per account (a rate-limit answer).
    holds: Mutex<HashMap<String, Instant>>,
    /// Maintenance the venue announced on the account's private stream (`scope`), in its
    /// words: new orders are refused with them until the stream says it ended or reconnects.
    maintenance: Mutex<HashMap<String, String>>,
    turns: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Accounts whose session check passed.
    checked: Mutex<HashSet<String>>,
    /// Position mode per account and instrument (`scope:symbol`): read once (at connect, or
    /// when the chart opens the instrument), updated from the private stream, forgotten on a
    /// refusal that may mean it changed. Never read again on the order path.
    modes: Mutex<HashMap<String, PositionMode>>,
    /// Leverage per account and instrument (`scope:symbol`), taken from every positions read
    /// and stream frame: read only, never on the order path.
    levers: Mutex<HashMap<String, Option<Decimal>>>,
    /// Margin mode per account (`scope`), read once.
    margins: Mutex<HashMap<String, Option<String>>>,
    /// Position mode per account (`scope`), where the venue states it account wide: read once,
    /// forgotten on a refusal that may mean it changed.
    account_modes: Mutex<HashMap<String, PositionMode>>,
    /// Margin per account and instrument (`scope:symbol`), as the venue states it: read once,
    /// updated from position rows, forgotten on a refusal that may mean it changed.
    symbol_margins: Mutex<HashMap<String, Option<MarginMode>>>,
    /// Contract sizes and name tables per host (a book that counts in contracts, a venue whose
    /// answers name instruments several ways), read with the rules.
    sizes: Mutex<HashMap<String, HeldTables>>,
    /// The client id each order was sent under and when (`scope:key`): minted ones (restored
    /// after a restart from the stored legs) and the others, for the lookup's time anchor.
    clients: Mutex<HashMap<String, ClientRef>>,
    /// OTW's key behind a venue client id (`scope:venue client id`), for minted ids.
    keys: Mutex<HashMap<String, String>>,
    /// OTW's key behind a venue order id (`scope:venue id`): what a cancel finds the client id by.
    by_venue: Mutex<HashMap<String, String>>,
    /// Attached exits by account and client id (`scope:key`): which entry they hang on.
    exits: Mutex<HashMap<String, AttachedExit>>,
    rules_ttl: Duration,
    _m: PhantomData<fn() -> M>,
}

impl<M: TradeMapper> Default for Trader<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// An error of a call that changes nothing (a read): the venue's refusal, or no answer.
fn read_error(e: VenueError) -> TradeError {
    match e.canon {
        Canon::Unknown => TradeError::Refused(ConnectorError::new(ErrorKind::Transient, e.message)),
        _ => TradeError::Refused(e.to_connector()),
    }
}

impl<M: TradeMapper> Trader<M> {
    pub fn new() -> Self {
        Trader {
            rules: Mutex::default(),
            stale: Mutex::default(),
            holds: Mutex::default(),
            maintenance: Mutex::default(),
            turns: Mutex::default(),
            checked: Mutex::default(),
            modes: Mutex::default(),
            levers: Mutex::default(),
            margins: Mutex::default(),
            account_modes: Mutex::default(),
            symbol_margins: Mutex::default(),
            sizes: Mutex::default(),
            clients: Mutex::default(),
            keys: Mutex::default(),
            by_venue: Mutex::default(),
            exits: Mutex::default(),
            rules_ttl: RULES_TTL,
            _m: PhantomData,
        }
    }

    /// Forget every hold and the session's checks (a test's mock venue reuses the account).
    pub fn reset(&self) {
        lock(&self.holds).clear();
        lock(&self.checked).clear();
    }

    /// Whether this account's session check passed.
    pub fn session_checked(&self, scope: &str) -> bool {
        lock(&self.checked).contains(scope)
    }

    // ── Plumbing ──

    /// One call, its answer or the venue's error, mapped. A transport failure is `Unknown`:
    /// the request may have reached the venue.
    async fn call(&self, d: &Driver<M>, a: &Account<'_>, req: HttpRequest) -> Result<Bytes, VenueError> {
        self.call_with_headers(d, a, req).await.map(|(body, _)| body)
    }

    /// [`Self::call`], with the answer's headers (what a list decoder may page by).
    async fn call_with_headers(&self, d: &Driver<M>, a: &Account<'_>, req: HttpRequest) -> Result<(Bytes, Vec<(String, String)>), VenueError> {
        let endpoint = req.endpoint_id;
        let frame = match d.send_as(req, Some(a.signer)).await {
            Ok(f) => f,
            Err(e) => {
                let canon = match e.kind {
                    ErrorKind::Auth => Canon::Auth,
                    ErrorKind::InvalidRequest => Canon::InvalidRequest,
                    _ => Canon::Unknown,
                };
                return Err(VenueError::new(canon, e.provider_code.clone(), format!("{}: {endpoint}: {}", d.mapper().venue_name(), e.message)));
            }
        };
        if let Some(e) = d.mapper().decode_error(&frame) {
            if e.canon == Canon::RateLimited {
                self.hold(d, a, &e);
            }
            return Err(e);
        }
        match frame {
            otw_connector_sdk::RawFrame::Http { body, headers, .. } => Ok((body, headers)),
            _ => Err(VenueError::new(Canon::Unknown, None, format!("{}: {endpoint}: no answer", d.mapper().venue_name()))),
        }
    }

    /// Every page of a list request.
    async fn pages<T>(
        &self,
        d: &Driver<M>,
        a: &Account<'_>,
        first: HttpRequest,
        t: &Tables,
        decode: impl Fn(&M, &DecodeCtx<'_>, &[u8]) -> Result<(Vec<T>, Option<String>), ConnectorError>,
    ) -> Result<Vec<T>, VenueError> {
        let mut out = Vec::new();
        let mut req = first;
        for _ in 0..MAX_PAGES {
            let (body, headers) = self.call_with_headers(d, a, req.clone()).await?;
            let ctx = t.ctx(req.endpoint_id).with_headers(&headers);
            let (rows, next) = decode(d.mapper(), &ctx, &body).map_err(|e| VenueError::unmapped(None, format!("{}: {} answer not read: {}", d.mapper().venue_name(), req.endpoint_id, e.message)))?;
            out.extend(rows);
            match next.filter(|c| !c.is_empty()) {
                Some(c) => req = d.mapper().with_cursor(&req, &c),
                None => return Ok(out),
            }
        }
        Err(VenueError::new(Canon::Unknown, None, format!("{}: more than {MAX_PAGES} pages, the list was not read to its end", d.mapper().venue_name())))
    }

    fn hold(&self, d: &Driver<M>, a: &Account<'_>, e: &VenueError) {
        let wait = e
            .retry_after_ms
            .or_else(|| e.retry_at_ms.map(|at| u64::try_from(at - d.venue_now().millis()).unwrap_or(0)))
            .filter(|ms| *ms > 0);
        if let Some(ms) = wait {
            lock(&self.holds).insert(a.scope.to_string(), Instant::now() + Duration::from_millis(ms));
        }
    }

    /// Refuse a new order while the venue's rate-limit wait runs (spec 8: refused, not queued).
    fn check_hold(&self, d: &Driver<M>, a: &Account<'_>) -> Result<(), TradeError> {
        let until = lock(&self.holds).get(a.scope).copied();
        if let Some(left) = until.and_then(|u| u.checked_duration_since(Instant::now())) {
            let secs = left.as_secs().max(1);
            return Err(refused(
                ConnectorError::new(ErrorKind::RateLimited, format!("{} holds new orders for {secs} s after its rate limit: try again then", d.mapper().venue_name()))
                    .with_retry_after_ms(u64::try_from(left.as_millis()).unwrap_or(u64::MAX)),
            ));
        }
        Ok(())
    }

    /// The maintenance the venue announced on the account's private stream (`Some`, in its
    /// words), or its end (`None`): what [`Self::check_maintenance`] refuses new orders with.
    pub fn set_maintenance(&self, a: &Account<'_>, message: Option<String>) {
        match message {
            Some(m) => lock(&self.maintenance).insert(a.scope.to_string(), m),
            None => lock(&self.maintenance).remove(a.scope),
        };
    }

    /// Whether the venue has maintenance announced on this account now.
    pub fn maintenance(&self, a: &Account<'_>) -> Option<String> {
        lock(&self.maintenance).get(a.scope).cloned()
    }

    /// Refuse a new order while the venue has maintenance announced: refused with the reason,
    /// never queued, held or sent again.
    fn check_maintenance(&self, d: &Driver<M>, a: &Account<'_>) -> Result<(), TradeError> {
        match self.maintenance(a) {
            Some(why) => Err(refused(ConnectorError::new(
                ErrorKind::ProviderDown,
                format!("{} announced maintenance ({why}): new orders are refused until it ends, nothing was sent: send it again once it is over", d.mapper().venue_name()),
            ))),
            None => Ok(()),
        }
    }

    async fn turn(&self, a: &Account<'_>, symbol: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let q = lock(&self.turns).entry(format!("{}:{symbol}", a.scope)).or_default().clone();
        q.lock_owned().await
    }

    /// Rules are the host's: the instrument list's resolved host and the symbol.
    fn rules_key(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str) -> String {
        let host = d.mapper().instrument_rules(a.settings, Some(symbol), CorrId(0)).ok().and_then(|r| d.base_url(r.base).ok()).unwrap_or_default();
        format!("{host}:{}:{symbol}", d.mapper().is_sandbox(a.settings))
    }

    /// The instrument's rules, read from the venue once a day and after a rule refusal. Unread,
    /// the order is refused: a venue onboarded from its documentation is never sent an order
    /// its own rules were not checked against.
    pub async fn rules(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str) -> Result<VenueRules, TradeError> {
        let key = self.rules_key(d, a, symbol);
        let stale = lock(&self.stale).remove(&key);
        if !stale {
            if let Some((r, at)) = lock(&self.rules).get(&key) {
                if at.elapsed() < self.rules_ttl {
                    return Ok(r.clone());
                }
            }
        }
        let m = d.mapper();
        let req = m.instrument_rules(a.settings, Some(symbol), corr()).map_err(refused)?;
        let rows = self.pages(d, a, req, &Tables::default(), |m, _, b| m.decode_rules(b)).await.map_err(|e| {
            refused(ConnectorError::new(ErrorKind::Transient, format!("{}: the rules of {symbol} could not be read, so nothing was sent ({})", m.venue_name(), e.message)))
        })?;
        let mut found = pick_rules(rows, symbol);
        if found.is_none() {
            // Not in the default list: maybe listed, not trading now (it then refuses orders).
            for req in m.instrument_rules_any_status(a.settings, symbol, corr()).map_err(refused)? {
                let rows = self.pages(d, a, req, &Tables::default(), |m, _, b| m.decode_rules(b)).await.map_err(read_error)?;
                found = pick_rules(rows, symbol);
                if found.is_some() {
                    break;
                }
            }
        }
        let r = found.ok_or_else(|| {
            refused(ConnectorError::not_found(format!("{} does not list {symbol} as trading: check its spelling, or that it trades on the venue now", m.venue_name())))
        })?;
        // Only trading rules are kept: an instrument that is not trading now (or only partly) is
        // read again on the next order, so it takes orders the moment it opens.
        if r.status.held() {
            lock(&self.rules).insert(key, (r.clone(), Instant::now()));
        }
        Ok(r)
    }

    /// Every instrument the venue lists, with its rules (the public conformance test).
    pub async fn all_rules(&self, d: &Driver<M>, a: &Account<'_>) -> Result<Vec<VenueRules>, TradeError> {
        let req = d.mapper().instrument_rules(a.settings, None, corr()).map_err(refused)?;
        self.pages(d, a, req, &Tables::default(), |m, _, b| m.decode_rules(b)).await.map_err(read_error)
    }

    /// Each instrument's contract size, for a book that counts in contracts: read from the
    /// whole instrument list once a day. Empty for any other book.
    pub async fn contract_sizes(&self, d: &Driver<M>, a: &Account<'_>) -> Result<Arc<ContractSizes>, TradeError> {
        Ok(self.tables(d, a).await?.sizes)
    }

    /// The venue's other names for its instruments, for a mapper that reads them
    /// ([`TradeMapper::uses_symbol_names`]): read from the whole instrument list once a day,
    /// with the contract sizes. Empty for any other venue.
    pub async fn symbol_names(&self, d: &Driver<M>, a: &Account<'_>) -> Result<Arc<SymbolNames>, TradeError> {
        Ok(self.tables(d, a).await?.names)
    }

    async fn tables(&self, d: &Driver<M>, a: &Account<'_>) -> Result<Tables, TradeError> {
        let m = d.mapper();
        if !m.counts_contracts() && !m.uses_symbol_names() {
            return Ok(Tables::default());
        }
        let key = self.sizes_key(d, a);
        if let Some((s, n, at)) = lock(&self.sizes).get(&key) {
            if at.elapsed() < self.rules_ttl {
                return Ok(Tables { sizes: s.clone(), names: n.clone() });
            }
        }
        let rules = self.all_rules(d, a).await?;
        let mut sizes = ContractSizes::default();
        if m.counts_contracts() {
            for r in &rules {
                if let Some(c) = r.contract_size {
                    sizes.insert(&r.instrument.native, c);
                }
            }
        }
        let names = if m.uses_symbol_names() { SymbolNames::of(&rules) } else { SymbolNames::default() };
        let (sizes, names) = (Arc::new(sizes), Arc::new(names));
        lock(&self.sizes).insert(key, (sizes.clone(), names.clone(), Instant::now()));
        Ok(Tables { sizes, names })
    }

    /// The contract sizes held, without a read (the private stream's frames).
    pub fn held_sizes(&self, d: &Driver<M>, a: &Account<'_>) -> Arc<ContractSizes> {
        lock(&self.sizes).get(&self.sizes_key(d, a)).map(|(s, _, _)| s.clone()).unwrap_or_default()
    }

    /// The name table held, without a read (the private stream's frames).
    pub fn held_names(&self, d: &Driver<M>, a: &Account<'_>) -> Arc<SymbolNames> {
        lock(&self.sizes).get(&self.sizes_key(d, a)).map(|(_, n, _)| n.clone()).unwrap_or_default()
    }

    /// The name OTW follows `symbol` by: the one the venue's answers are named with, through its
    /// name table ([`TradeMapper::uses_symbol_names`]: Kraken `XBT/USD` for `XBTUSD`), so a placed
    /// order carries the spelling its reads and stream frames carry. `symbol` itself on any other
    /// venue, or before the list was read.
    pub fn followed_name(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str) -> String {
        self.held_names(d, a).native(symbol).map(str::to_string).unwrap_or_else(|| symbol.to_string())
    }

    fn held_tables(&self, d: &Driver<M>, a: &Account<'_>) -> Tables {
        Tables { sizes: self.held_sizes(d, a), names: self.held_names(d, a) }
    }

    fn sizes_key(&self, d: &Driver<M>, a: &Account<'_>) -> String {
        let host = d.mapper().instrument_rules(a.settings, None, CorrId(0)).ok().and_then(|r| d.base_url(r.base).ok()).unwrap_or_default();
        format!("{host}:{}", d.mapper().is_sandbox(a.settings))
    }

    /// The account's positions on `symbol`, as the venue lists them. Empty for a cash book.
    pub async fn positions(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str) -> Result<Vec<VenuePosition>, TradeError> {
        // The mode held for the instrument, else for the account: a venue may read hedge-mode
        // positions by a call of their own.
        let held = self.held_mode(a, symbol).or_else(|| lock(&self.account_modes).get(a.scope).copied());
        let Some(req) = d.mapper().positions_in_mode(a.settings, symbol, held, corr()).map_err(refused)? else { return Ok(Vec::new()) };
        let t = self.tables(d, a).await?;
        let endpoint = req.endpoint_id;
        let body = self.call(d, a, req).await.map_err(read_error)?;
        let rows = d.mapper().decode_positions_in(&t.ctx(endpoint), &body).map_err(refused)?;
        self.remember(a, &rows, d.mapper().margin_per_side());
        // No row for the instrument: the venue holds no position there, so no leverage of one
        // closed since is kept (the next order reads it again).
        if !rows.iter().any(|r| r.symbol.eq_ignore_ascii_case(symbol)) {
            lock(&self.levers).remove(&Self::mode_key(a, symbol));
        }
        Ok(rows)
    }

    /// Keep the leverage the rows state, per instrument: a held side's first, else any row's.
    /// Where each side is margined on its own (`per_side`), a held side's margin is kept for
    /// that side too.
    fn remember(&self, a: &Account<'_>, rows: &[VenuePosition], per_side: bool) {
        let mut symbols: Vec<&str> = rows.iter().map(|r| r.symbol.as_str()).collect();
        symbols.sort_unstable();
        symbols.dedup();
        for sym in symbols {
            let of: Vec<&VenuePosition> = rows.iter().filter(|r| r.symbol == sym).collect();
            let lev = of.iter().filter(|r| r.qty > Decimal::ZERO).find_map(|r| r.leverage).or_else(|| of.iter().find_map(|r| r.leverage));
            lock(&self.levers).insert(Self::mode_key(a, sym), lev);
            let margin = of.iter().filter(|r| r.qty > Decimal::ZERO).find_map(|r| r.margin.clone()).or_else(|| of.iter().find_map(|r| r.margin.clone()));
            if margin.is_some() {
                lock(&self.symbol_margins).insert(Self::mode_key(a, sym), margin);
            }
            if per_side {
                for r in of.iter().filter(|r| r.qty > Decimal::ZERO && r.side != PositionSide::Net && r.margin.is_some()) {
                    lock(&self.symbol_margins).insert(Self::margin_key(a, sym, r.side, true), r.margin.clone());
                }
            }
        }
    }

    /// What one position frame of the private stream says: the instrument's position mode
    /// (never where the venue states the mode at account level: its rows may be sided in either
    /// mode, MEXC) and leverage. Where each side is margined on its own, a held side's margin is
    /// kept for that side too.
    pub fn set_position(&self, d: &Driver<M>, a: &Account<'_>, p: &VenuePosition) {
        let per_side = d.mapper().margin_per_side();
        if !Self::account_wide(d, a) {
            let mode = if p.side == PositionSide::Net { PositionMode::OneWay } else { PositionMode::Hedge };
            self.set_mode(a, &p.symbol, mode);
        }
        if p.qty.is_zero() {
            // Flat: the leverage held for the position closed is not kept (the next order reads
            // what the venue holds again).
            lock(&self.levers).remove(&Self::mode_key(a, &p.symbol));
        } else if p.leverage.is_some() {
            lock(&self.levers).insert(Self::mode_key(a, &p.symbol), p.leverage);
        }
        if p.margin.is_some() {
            lock(&self.symbol_margins).insert(Self::mode_key(a, &p.symbol), p.margin.clone());
        }
        if per_side && p.side != PositionSide::Net && p.qty > Decimal::ZERO && p.margin.is_some() {
            lock(&self.symbol_margins).insert(Self::margin_key(a, &p.symbol, p.side, true), p.margin.clone());
        }
    }

    /// How `symbol` is margined on this account, as the venue says: held from the last read or
    /// position row, read once ([`TradeMapper::symbol_margin`]) when nothing is held. `None`:
    /// the venue states none for it.
    pub async fn symbol_margin(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str) -> Result<Option<MarginMode>, TradeError> {
        self.symbol_margin_on(d, a, symbol, PositionSide::Net).await
    }

    /// [`Self::symbol_margin`] for the position side an order acts on: the same unless the
    /// venue margins each hedge-mode side on its own ([`TradeMapper::margin_per_side`]).
    pub async fn symbol_margin_on(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, side: PositionSide) -> Result<Option<MarginMode>, TradeError> {
        let m = d.mapper();
        let key = Self::margin_key(a, symbol, side, m.margin_per_side());
        if let Some(mm) = lock(&self.symbol_margins).get(&key).cloned() {
            return Ok(mm);
        }
        let Some(req) = m.symbol_margin(a.settings, symbol, corr()).map_err(refused)? else { return Ok(None) };
        let body = self.call(d, a, req).await.map_err(read_error)?;
        let mm = m.decode_symbol_margin_on(symbol, side, &body).map_err(refused)?;
        lock(&self.symbol_margins).insert(key, mm.clone());
        Ok(mm)
    }

    /// Where the margin of `symbol` is held: per instrument, or per side of a hedge-mode
    /// instrument where the venue margins each side on its own.
    fn margin_key(a: &Account<'_>, symbol: &str, side: PositionSide, per_side: bool) -> String {
        match side {
            PositionSide::Long if per_side => format!("{}:long", Self::mode_key(a, symbol)),
            PositionSide::Short if per_side => format!("{}:short", Self::mode_key(a, symbol)),
            _ => Self::mode_key(a, symbol),
        }
    }

    /// The leverage the account set on `symbol` and its margin mode, read only: held from the
    /// last read or stream frame, read once when nothing is held (from the positions, then the
    /// venue's own leverage read where it has one). `None` for a book with no positions (spot);
    /// `Some((None, _))`: the venue holds no leverage for it (no position), never a value made up.
    pub async fn leverage(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str) -> Result<Option<(Option<Decimal>, Option<String>)>, TradeError> {
        let m = d.mapper();
        if m.capabilities().orders.position_mode.is_none() {
            return Ok(None);
        }
        let held = lock(&self.levers).get(&Self::mode_key(a, symbol)).copied();
        let lev = match held {
            Some(l) => l,
            None => {
                self.positions(d, a, symbol).await?;
                lock(&self.levers).get(&Self::mode_key(a, symbol)).copied().flatten()
            }
        };
        let lev = match lev {
            Some(l) => Some(l),
            None => self.read_leverage(d, a, symbol, PositionSide::Net).await?,
        };
        if let Some(mm) = self.symbol_margin(d, a, symbol).await? {
            return Ok(Some((lev, Some(mm.word().to_string()))));
        }
        let held_margin = lock(&self.margins).get(a.scope).cloned();
        let margin = match held_margin {
            Some(mm) => mm,
            None => {
                let mm = match m.margin_mode(a.settings, corr()).map_err(refused)? {
                    Some(req) => {
                        let body = self.call(d, a, req).await.map_err(read_error)?;
                        m.decode_margin_mode(&body).map_err(refused)?
                    }
                    None => None,
                };
                lock(&self.margins).insert(a.scope.to_string(), mm.clone());
                mm
            }
        };
        Ok(Some((lev, margin)))
    }

    /// The leverage the venue's own read answers for `symbol` and `side`
    /// ([`TradeMapper::symbol_leverage`]), held for the chart; `None` where the venue has no
    /// such read, or holds none.
    async fn read_leverage(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, side: PositionSide) -> Result<Option<Decimal>, TradeError> {
        let m = d.mapper();
        let Some(req) = m.symbol_leverage(a.settings, symbol, corr()).map_err(refused)? else { return Ok(None) };
        let body = self.call(d, a, req).await.map_err(|e| {
            refused(ConnectorError::new(ErrorKind::Transient, format!("{}: the leverage held on {symbol} could not be read, so nothing was sent ({})", m.venue_name(), e.message)))
        })?;
        let lev = m.decode_symbol_leverage(symbol, side, &body).map_err(refused)?;
        lock(&self.levers).insert(Self::mode_key(a, symbol), lev);
        Ok(lev)
    }

    /// The leverage an order on `symbol` goes with: what is held (a position row, a stream
    /// frame or an earlier read), so no read sits in front of each order; else the venue's own
    /// read where it has one; else none (the venue then applies its own default). A reduce-only
    /// order whose read fails goes with none: closing never waits on a leverage read.
    async fn order_leverage(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, side: PositionSide, reduce_only: bool) -> Result<Option<Decimal>, TradeError> {
        let m = d.mapper();
        if m.capabilities().orders.position_mode.is_none() {
            return Ok(None);
        }
        let held = lock(&self.levers).get(&Self::mode_key(a, symbol)).copied().flatten();
        if held.is_some() {
            return Ok(held);
        }
        if m.symbol_leverage(a.settings, symbol, CorrId(0)).map_err(refused)?.is_some() {
            return match self.read_leverage(d, a, symbol, side).await {
                Err(_) if reduce_only => Ok(None),
                r => r,
            };
        }
        Ok(None)
    }

    fn mode_key(a: &Account<'_>, symbol: &str) -> String {
        format!("{}:{}", a.scope, symbol.to_ascii_uppercase())
    }

    /// Whether the venue states the position mode at account level: its rows then never say it.
    fn account_wide(d: &Driver<M>, a: &Account<'_>) -> bool {
        d.mapper().account_position_mode(a.settings, CorrId(0)).ok().flatten().is_some()
    }

    /// A position mode the venue pushed: on `symbol`, or account wide (`None`); `mode` `None`
    /// says it changed without saying to what, and it is read again before the next order.
    pub fn mode_pushed(&self, a: &Account<'_>, symbol: Option<&str>, mode: Option<PositionMode>) {
        match (symbol, mode) {
            (Some(sym), Some(m)) => self.set_mode(a, sym, m),
            (Some(sym), None) => {
                lock(&self.modes).remove(&Self::mode_key(a, sym));
            }
            (None, Some(m)) => {
                lock(&self.account_modes).insert(a.scope.to_string(), m);
            }
            (None, None) => {
                lock(&self.account_modes).remove(a.scope);
                let prefix = format!("{}:", a.scope);
                lock(&self.modes).retain(|k, _| !k.starts_with(&prefix));
            }
        }
    }

    /// The position mode the private stream or a read says `symbol` is in now.
    pub fn set_mode(&self, a: &Account<'_>, symbol: &str, mode: PositionMode) {
        lock(&self.modes).insert(Self::mode_key(a, symbol), mode);
    }

    /// The mode held for `symbol`, if any.
    pub fn held_mode(&self, a: &Account<'_>, symbol: &str) -> Option<PositionMode> {
        lock(&self.modes).get(&Self::mode_key(a, symbol)).copied()
    }

    /// Read the position mode of every instrument the account holds, once: at connect.
    pub async fn prefill_modes(&self, d: &Driver<M>, a: &Account<'_>) -> Result<usize, TradeError> {
        let m = d.mapper();
        if m.capabilities().orders.position_mode.is_none() {
            return Ok(0);
        }
        let account = self.account_mode(d, a).await?;
        let t = self.tables(d, a).await?;
        let mut rows: Vec<VenuePosition> = Vec::new();
        for req in m.all_positions(a.settings, corr()).map_err(refused)? {
            let endpoint = req.endpoint_id;
            let body = self.call(d, a, req).await.map_err(read_error)?;
            rows.extend(m.decode_positions_in(&t.ctx(endpoint), &body).map_err(refused)?);
        }
        self.remember(a, &rows, m.margin_per_side());
        // The venue states the mode account wide: its rows say nothing of it.
        if account.is_some() {
            return Ok(0);
        }
        let mut by_symbol: HashMap<String, Vec<VenuePosition>> = HashMap::new();
        for r in rows {
            by_symbol.entry(r.symbol.clone()).or_default().push(r);
        }
        let mut n = 0;
        for (symbol, rows) in by_symbol {
            if let Some(mode) = PositionMode::of(&rows) {
                self.set_mode(a, &symbol, mode);
                n += 1;
            }
        }
        Ok(n)
    }

    /// Read what an order on `symbol` needs before it can leave without another read: the rules
    /// and the position mode. Called when the chart opens the instrument on the account.
    pub async fn warm(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str) -> Result<(), TradeError> {
        self.rules(d, a, symbol).await?;
        self.mode(d, a, symbol).await?;
        self.symbol_margin(d, a, symbol).await.map(|_| ())
    }

    /// The account-wide position mode, where the venue states one ([`TradeMapper::account_position_mode`]):
    /// held once read. `None`: the venue reads it from the position rows.
    async fn account_mode(&self, d: &Driver<M>, a: &Account<'_>) -> Result<Option<PositionMode>, TradeError> {
        if let Some(mode) = lock(&self.account_modes).get(a.scope).copied() {
            return Ok(Some(mode));
        }
        let m = d.mapper();
        let Some(req) = m.account_position_mode(a.settings, corr()).map_err(refused)? else { return Ok(None) };
        let body = self.call(d, a, req).await.map_err(read_error)?;
        let mode = m.decode_account_position_mode(&body).map_err(refused)?;
        if let Some(mode) = mode {
            lock(&self.account_modes).insert(a.scope.to_string(), mode);
        }
        Ok(mode)
    }

    /// Forget the position mode held for `symbol` and for the account: read again next time.
    fn forget_mode(&self, a: &Account<'_>, symbol: &str) {
        lock(&self.modes).remove(&Self::mode_key(a, symbol));
        lock(&self.account_modes).remove(a.scope);
    }

    /// How the account holds positions on `symbol`, never changed by OTW. Held from the last
    /// read or stream frame; read from the venue only when nothing is held yet (the first order
    /// on an instrument nobody warmed). A book with no positions is net. A mode the rows do not
    /// tell is refused, naming the fix.
    async fn mode(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str) -> Result<PositionMode, TradeError> {
        let m = d.mapper();
        if m.capabilities().orders.position_mode.is_none() {
            return Ok(PositionMode::OneWay);
        }
        // Account wide where the venue states it there (answered with no position open): the
        // rows never override it.
        if let Some(mode) = self.account_mode(d, a).await? {
            return Ok(mode);
        }
        if let Some(mode) = self.held_mode(a, symbol) {
            return Ok(mode);
        }
        let rows = self.positions(d, a, symbol).await?;
        let mode = PositionMode::of(&rows).ok_or_else(|| {
            refused(ConnectorError::invalid(format!(
                "{} did not say whether {symbol} is in one-way or hedge mode on this account, so nothing was sent: check the position mode on {symbol} at the venue, then try again",
                m.venue_name()
            )))
        })?;
        self.set_mode(a, symbol, mode);
        Ok(mode)
    }

    // ── Client ids ──

    /// `scope` and an id, apart by a separator no scope or id holds.
    fn client_key(a: &Account<'_>, id: &str) -> String {
        format!("{}{SEP}{id}", a.scope)
    }

    /// Mint the venue client id an order under OTW key `key` will be sent with, before I/O, for
    /// a venue whose ids are minted ([`TradeMapper::mints_client_ids`]): the caller stores it
    /// with the leg. `None` for any other venue. Minted once: a key minted before answers the
    /// same id.
    pub fn mint(&self, d: &Driver<M>, a: &Account<'_>, key: &str) -> Result<Option<ClientRef>, TradeError> {
        let m = d.mapper();
        if !m.mints_client_ids() {
            return Ok(None);
        }
        if let Some(r) = lock(&self.clients).get(&Self::client_key(a, key)).cloned() {
            return Ok(Some(r));
        }
        let at = stamp(d.venue_now().millis());
        let r = ClientRef { id: m.mint_client_id(at).map_err(refused)?, at_ms: Some(at) };
        self.hold_client(a, key, r.clone(), None);
        Ok(Some(r))
    }

    /// Hold the client id OTW key `key` was sent under (and the venue's id, once known): after a
    /// restart, from what the caller stored with the leg.
    pub fn hold_client(&self, a: &Account<'_>, key: &str, r: ClientRef, venue_id: Option<&str>) {
        if let Some(v) = venue_id.filter(|v| !v.is_empty()) {
            lock(&self.by_venue).insert(Self::client_key(a, v), key.to_string());
        }
        lock(&self.keys).insert(Self::client_key(a, &r.id), key.to_string());
        self.keep_client(a, key, r);
    }

    /// Hold `r` for `key`, pruning what is older than [`CLIENT_TTL_MS`] once [`CLIENTS_KEPT`] are
    /// held (with the reverse tables that point to them).
    fn keep_client(&self, a: &Account<'_>, key: &str, r: ClientRef) {
        let mut held = lock(&self.clients);
        if held.len() >= CLIENTS_KEPT {
            if let Some(newest) = held.values().filter_map(|c| c.at_ms).max() {
                held.retain(|_, c| c.at_ms.is_some_and(|t| t >= newest - CLIENT_TTL_MS));
                let scope_of = |k: &str| k.split_once(SEP).map(|(s, _)| s.to_string()).unwrap_or_default();
                let ids: HashSet<String> = held.iter().map(|(k, c)| format!("{}{SEP}{}", scope_of(k), c.id)).collect();
                lock(&self.keys).retain(|k, _| ids.contains(k));
                lock(&self.by_venue).retain(|k, key| held.contains_key(&format!("{}{SEP}{key}", scope_of(k))));
            }
        }
        held.insert(Self::client_key(a, key), r);
    }

    /// OTW's key behind a client id the venue echoes (a stream order): through the minted ids
    /// held, else the mapper's inverse spelling. `None` for a minted id OTW does not hold (an
    /// order placed elsewhere): never guessed.
    pub fn otw_key(&self, d: &Driver<M>, a: &Account<'_>, venue_client_id: &str) -> Option<String> {
        if let Some(k) = lock(&self.keys).get(&Self::client_key(a, venue_client_id)) {
            return Some(k.clone());
        }
        let m = d.mapper();
        (!m.mints_client_ids()).then(|| m.otw_id(venue_client_id))
    }

    /// The client id an order under `key` is sent with: the one held (minted before, restored),
    /// else minted now, else spelled from the key. Held with the time it leaves.
    fn client_for_send(&self, d: &Driver<M>, a: &Account<'_>, key: &str) -> Result<String, ConnectorError> {
        if let Some(r) = lock(&self.clients).get(&Self::client_key(a, key)).cloned() {
            return Ok(r.id);
        }
        let m = d.mapper();
        if m.mints_client_ids() {
            return match self.mint(d, a, key) {
                Ok(Some(r)) => Ok(r.id),
                Ok(None) => Err(ConnectorError::invalid(format!("{}: no client id could be minted for {key}", m.venue_name()))),
                Err(e) => Err(ConnectorError::invalid(e.message().to_string())),
            };
        }
        let id = m.client_id(key)?;
        // Spelled from the key: held for the lookup's time anchor only, no reverse needed.
        self.keep_client(a, key, ClientRef { id: id.clone(), at_ms: Some(d.venue_now().millis()) });
        Ok(id)
    }

    /// The client id an order under `key` was sent with, to look it up: held, else spelled from
    /// the key. A minted id OTW does not hold cannot be told: refused, never guessed.
    fn client_known(&self, d: &Driver<M>, a: &Account<'_>, key: &str) -> Result<ClientRef, ConnectorError> {
        if let Some(r) = lock(&self.clients).get(&Self::client_key(a, key)).cloned() {
            return Ok(r);
        }
        let m = d.mapper();
        if m.mints_client_ids() {
            return Err(ConnectorError::invalid(format!(
                "{}: the client id order {key} was sent under is not held (it is stored with the order): it cannot be looked up until it is read back",
                m.venue_name()
            )));
        }
        Ok(ClientRef { id: m.client_id(key)?, at_ms: None })
    }

    /// The client id behind venue order `venue_id`, where OTW holds it (what a cancel by client
    /// id and date is handed).
    fn client_of_venue(&self, a: &Account<'_>, venue_id: &str) -> Option<ClientRef> {
        let key = lock(&self.by_venue).get(&Self::client_key(a, venue_id)).cloned()?;
        lock(&self.clients).get(&Self::client_key(a, &key)).cloned()
    }

    fn placed_as(&self, a: &Account<'_>, key: &str, venue_id: &str) {
        if !venue_id.is_empty() {
            lock(&self.by_venue).insert(Self::client_key(a, venue_id), key.to_string());
        }
    }

    // ── Reads ──

    /// The order sent under OTW client id `key`, as the venue reports it now (the order a
    /// trigger became, where it fired as a new one). `None` only when every call the venue
    /// offers answered that it does not know it.
    pub async fn lookup(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, key: &str) -> Result<Option<VenueOrder>, TradeError> {
        let m = d.mapper();
        let cref = self.client_known(d, a, key).map_err(refused)?;
        let cid = cref.id.clone();
        let t = self.tables(d, a).await?;
        let at = LookupAt { created_ms: cref.at_ms, now_ms: d.venue_now().millis() };
        let pages = m.lookup_pages().max(1);
        for first in m.query_by_client_id_at(a.settings, symbol, &cid, &at, corr()).map_err(refused)? {
            let mut req = first;
            for page in 1..=pages {
                let endpoint = req.endpoint_id;
                match self.call_with_headers(d, a, req.clone()).await {
                    Ok((body, headers)) => {
                        let (rows, next) = m.decode_orders_in(&t.ctx(endpoint).with_headers(&headers), &body).map_err(|e| unmapped_read(m, &e))?;
                        if let Some(o) = rows.into_iter().find(|o| o.client_id.as_deref() == Some(cid.as_str())) {
                            let o = self.follow(d, a, symbol, o).await?;
                            self.placed_as(a, key, &o.venue_id);
                            return Ok(Some(o));
                        }
                        match next.filter(|c| !c.is_empty()) {
                            Some(c) if page < pages => req = m.with_cursor(&req, &c),
                            _ => break,
                        }
                    }
                    Err(e) if e.canon == Canon::Rejected(Refusal::NotFound) => break,
                    Err(e) => return Err(read_error(e)),
                }
            }
        }
        Ok(None)
    }

    /// The order the venue knows under its own id `venue_id` (the order a trigger became,
    /// where it fired as a new one).
    pub async fn by_venue_id(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, venue_id: &str) -> Result<Option<VenueOrder>, TradeError> {
        match self.by_venue_id_once(d, a, symbol, venue_id).await? {
            Some(o) => self.follow(d, a, symbol, o).await.map(Some),
            None => Ok(None),
        }
    }

    /// One lookup by venue id: each call the venue offers, each read over at most
    /// [`TradeMapper::lookup_pages`] pages (following the cursor), as [`Self::lookup`] does.
    async fn by_venue_id_once(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, venue_id: &str) -> Result<Option<VenueOrder>, TradeError> {
        let m = d.mapper();
        let t = self.tables(d, a).await?;
        let pages = m.lookup_pages().max(1);
        for first in m.query_by_venue_id(a.settings, symbol, venue_id, corr()).map_err(refused)? {
            let mut req = first;
            for page in 1..=pages {
                let endpoint = req.endpoint_id;
                match self.call_with_headers(d, a, req.clone()).await {
                    Ok((body, headers)) => {
                        let (rows, next) = m.decode_orders_in(&t.ctx(endpoint).with_headers(&headers), &body).map_err(|e| unmapped_read(m, &e))?;
                        if let Some(o) = rows.into_iter().find(|o| o.venue_id == venue_id) {
                            return Ok(Some(o));
                        }
                        match next.filter(|c| !c.is_empty()) {
                            Some(c) if page < pages => req = m.with_cursor(&req, &c),
                            _ => break,
                        }
                    }
                    Err(e) if e.canon == Canon::Rejected(Refusal::NotFound) => break,
                    Err(e) => return Err(read_error(e)),
                }
            }
        }
        Ok(None)
    }

    /// An order that names the order it became ([`VenueOrder::successor_id`]) is followed to
    /// it; one the venue does not list (yet) leaves the order as read.
    async fn follow(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, mut o: VenueOrder) -> Result<VenueOrder, TradeError> {
        for _ in 0..MAX_HOPS {
            let Some(next) = o.successor_id.clone().filter(|n| !n.is_empty() && *n != o.venue_id) else { break };
            match self.by_venue_id_once(d, a, symbol, &next).await? {
                Some(n) => o = n,
                None => break,
            }
        }
        Ok(o)
    }

    /// Every working order: on `symbol`, or the whole book (every part of it).
    pub async fn open_orders(&self, d: &Driver<M>, a: &Account<'_>, symbol: Option<&str>) -> Result<Vec<VenueOrder>, TradeError> {
        let m = d.mapper();
        let mut out = Vec::new();
        let t = self.tables(d, a).await?;
        for (i, req) in m.open_orders(a.settings, symbol, corr()).map_err(refused)?.into_iter().enumerate() {
            let endpoint = req.endpoint_id;
            match self.pages(d, a, req, &t, |m, c, b| m.decode_orders_in(c, b)).await {
                Ok(rows) => out.extend(rows),
                // A part listed apart (the conditional orders) that the venue refuses leaves the
                // rest of the book read, so a cancel of everything still runs on it.
                Err(e) if i > 0 && matches!(e.canon, Canon::InvalidRequest | Canon::Permission | Canon::Rejected(_)) => {
                    tracing::warn!("{}: the {endpoint} list was refused, its orders are left out of this read: {}", m.venue_name(), e.message);
                }
                Err(e) => return Err(read_error(e)),
            }
        }
        // An order two requests (or two pages) both list is one order: kept once, as first read.
        let mut seen = std::collections::HashSet::new();
        out.retain(|o: &VenueOrder| o.venue_id.is_empty() || seen.insert(o.venue_id.clone()));
        Ok(out)
    }

    /// Executions on `symbol` since `since_ms`, each once: a duplicate execution id is dropped
    /// by its id, never by its content (spec 2.3, T24).
    pub async fn fills_since(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, since_ms: i64) -> Result<Vec<VenueFill>, TradeError> {
        let m = d.mapper();
        let now = d.venue_now().millis();
        let step = m.fills_window_ms().filter(|w| *w > 0).unwrap_or(i64::MAX);
        let mut rows = Vec::new();
        let mut from = since_ms;
        let t = self.tables(d, a).await?;
        loop {
            let to = from.saturating_add(step).min(now);
            for req in m.fills_requests(a.settings, symbol, from, to, now, corr()).map_err(refused)? {
                rows.extend(self.pages(d, a, req, &t, |m, c, b| m.decode_fills_in(c, b)).await.map_err(read_error)?);
            }
            if to >= now {
                break;
            }
            from = to;
        }
        let mut seen = HashSet::new();
        let mut rows: Vec<VenueFill> = rows.into_iter().filter(|f| seen.insert(f.exec_id.clone())).collect();
        self.name_fee_currency(d, a, &mut rows).await;
        Ok(rows)
    }

    /// Fills whose row names no fee currency, on a venue whose doc says fees are charged in the
    /// quote currency ([`TradeMapper::fees_in_quote`]): named from the instrument list (read
    /// once when it does not hold the instrument). One the list does not give stays unnamed.
    async fn name_fee_currency(&self, d: &Driver<M>, a: &Account<'_>, rows: &mut [VenueFill]) {
        let m = d.mapper();
        if !m.fees_in_quote() || rows.iter().all(|f| !f.fee_currency.is_empty()) {
            return;
        }
        let venue = &m.capabilities().venue;
        let quote_of = |sym: &str| d.instruments().ok().and_then(|c| c.get(&otw_connector_model::InstrumentId::new(venue, sym)).map(|s| s.quote.clone())).filter(|q| !q.is_empty());
        if rows.iter().any(|f| f.fee_currency.is_empty() && quote_of(&f.symbol).is_none()) {
            if let Err(e) = d.load_instruments_as(corr(), Some(a.signer)).await {
                tracing::warn!("{}: the instrument list was not read, fills keep no fee currency ({})", m.venue_name(), e.message);
            }
        }
        for f in rows.iter_mut().filter(|f| f.fee_currency.is_empty()) {
            if let Some(q) = quote_of(&f.symbol) {
                f.fee_currency = q;
            }
        }
    }

    /// Fills whose frame carried no fee ([`VenueFill::fee_pending`]) completed from the venue's
    /// fills since the oldest of them, by execution id. One the venue does not list yet stays
    /// pending.
    pub async fn complete_fees(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, fills: &mut [VenueFill]) -> Result<usize, TradeError> {
        let Some(since) = fills.iter().filter(|f| f.fee_pending).map(|f| f.at_ms).min() else { return Ok(0) };
        let read = self.fills_since(d, a, symbol, since).await?;
        let mut n = 0;
        for f in fills.iter_mut().filter(|f| f.fee_pending) {
            if let Some(r) = read.iter().find(|r| r.exec_id == f.exec_id && !r.fee_pending) {
                f.fee = r.fee;
                f.fee_currency = r.fee_currency.clone();
                f.extra_fees = r.extra_fees.clone();
                f.fee_pending = false;
                n += 1;
            }
        }
        Ok(n)
    }

    // ── Orders ──

    /// Everything checked before anything leaves: capabilities, client ids, the venue's rules.
    /// Answers the venue's spelling of each leg's client id and the rules read.
    async fn pre_trade(&self, d: &Driver<M>, a: &Account<'_>, orders: &[OrderRequest], reference: Option<Decimal>) -> Result<(Vec<String>, VenueRules), TradeError> {
        let m = d.mapper();
        check_orders(m.capabilities(), orders, d.venue_now()).map_err(refused)?;
        for o in orders {
            m.check_order(o).map_err(refused)?;
        }
        if orders.iter().any(|o| o.oca_group.is_some()) {
            return Err(refused(ConnectorError::unsupported(format!("{} takes no one-cancels-other group from OpenTraderWorld", m.venue_name()))));
        }
        attached_exits(m.venue_name(), orders).map_err(refused)?;
        let cids = orders.iter().map(|o| self.client_for_send(d, a, &o.client_order_id)).collect::<Result<Vec<_>, _>>().map_err(refused)?;
        let symbol = &orders[0].instrument.native;
        let rules = self.rules(d, a, symbol).await?;
        for o in orders {
            check_rules(m.venue_name(), &rules, o, reference).map_err(refused)?;
        }
        Ok((cids, rules))
    }

    /// The venue's own check of the first order of this account's session (spec 9, E2D).
    async fn session_check(&self, d: &Driver<M>, a: &Account<'_>, o: &OrderRequest, p: &Placing) -> Result<(), TradeError> {
        let m = d.mapper();
        if m.capabilities().orders.dry_run == DryRunCap::None || self.session_checked(a.scope) {
            return Ok(());
        }
        let check = Placing { client_id: String::new(), ..p.clone() };
        let Some(req) = m.dry_run(a.settings, o, &check).map_err(refused)? else {
            return Err(refused(ConnectorError::invalid(format!(
                "{}'s check does not cover this order, and the first order of a session on an account never tested against the venue goes through it: send a plain market or limit order first",
                m.venue_name()
            ))));
        };
        match self.call(d, a, req).await {
            Ok(_) => {
                lock(&self.checked).insert(a.scope.to_string());
                Ok(())
            }
            Err(e) => Err(refused(ConnectorError::invalid(format!("{}'s check refused this order, nothing was sent ({})", m.venue_name(), e.message)))),
        }
    }

    /// One order out. A lost or unread answer is looked up before anything is said: found, it
    /// was placed. `Ok(None)`: the venue refused it (the error, mapped).
    async fn send_leg(&self, d: &Driver<M>, a: &Account<'_>, o: &OrderRequest, p: &Placing) -> Result<Ack, LegFailure> {
        let req = d.mapper().place(a.settings, o, p).map_err(LegFailure::Refused)?;
        self.send_req(d, a, o, req).await
    }

    /// One placing call for `o`, already built.
    async fn send_req(&self, d: &Driver<M>, a: &Account<'_>, o: &OrderRequest, req: HttpRequest) -> Result<Ack, LegFailure> {
        let m = d.mapper();
        let endpoint = req.endpoint_id;
        let t = self.held_tables(d, a);
        let error = match self.call(d, a, req).await {
            Ok(body) => match m.decode_ack_in(&t.ctx(endpoint), &body) {
                Ok(ack) if ack.has_id() => {
                    self.placed_as(a, &o.client_order_id, &ack.venue_id);
                    return Ok(ack);
                }
                // Accepted without naming the order: found by its client id, never sent again.
                Ok(_) => {
                    return match self.lookup(d, a, &o.instrument.native, &o.client_order_id).await {
                        Ok(Some(v)) => Ok(Ack { venue_id: v.venue_id, client_id: v.client_id }),
                        _ => Err(LegFailure::Unknown {
                            message: format!(
                                "{}: order {} was accepted without its id and could not be found yet: it is looked up again until the venue answers, do not send it again",
                                m.venue_name(),
                                o.client_order_id
                            ),
                            alert: false,
                        }),
                    };
                }
                Err(e) => VenueError::unmapped(None, format!("{}: the answer to order {} was not read ({})", m.venue_name(), o.client_order_id, e.message)),
            },
            Err(e) => e,
        };
        // The leverage held for the instrument may be out of date: read again before the next order.
        if error.canon != Canon::Unknown && m.capabilities().orders.position_mode.is_some() && m.invalidates_leverage(&error) {
            lock(&self.levers).remove(&Self::mode_key(a, &o.instrument.native));
        }
        match error.canon {
            Canon::Unknown => {
                let found = self.lookup(d, a, &o.instrument.native, &o.client_order_id).await;
                match found {
                    Ok(Some(v)) => Ok(Ack { venue_id: v.venue_id, client_id: v.client_id }),
                    _ => Err(LegFailure::Unknown {
                        message: format!(
                            "{}: no answer to order {} that could be read, and it could not be found: it is looked up again until the venue answers, do not send it again ({})",
                            m.venue_name(),
                            o.client_order_id,
                            error.message
                        ),
                        alert: !error.mapped,
                    }),
                }
            }
            Canon::Rejected(Refusal::VenueRule) => {
                lock(&self.stale).insert(self.rules_key(d, a, &o.instrument.native));
                Err(LegFailure::Refused(error.to_connector()))
            }
            _ => {
                if m.capabilities().orders.position_mode.is_some() && m.invalidates_position_mode(&error) {
                    self.forget_mode(a, &o.instrument.native);
                }
                if m.invalidates_margin_mode(&error) {
                    let mut held = lock(&self.symbol_margins);
                    for side in [PositionSide::Net, PositionSide::Long, PositionSide::Short] {
                        held.remove(&Self::margin_key(a, &o.instrument.native, side, true));
                    }
                }
                Err(LegFailure::Refused(error.to_connector()))
            }
        }
    }

    /// An entry with its exits attached at the venue: one call. The exits answer under ids that
    /// name their entry and kind (`tpsl:ENTRY:tp:buy`); they are followed through the entry.
    async fn place_attached(&self, d: &Driver<M>, a: &Account<'_>, entry: &OrderRequest, legs: &[OrderRequest], exits: &[(ExitKind, Decimal)], p: &Placing) -> Result<Vec<Placed>, TradeError> {
        let m = d.mapper();
        let req = m.place_with_exits(a.settings, entry, exits, p).map_err(refused)?;
        let symbol = entry.instrument.native.clone();
        let keys: Vec<String> = std::iter::once(&entry.client_order_id).chain(legs.iter().map(|l| &l.client_order_id)).cloned().collect();
        let ack = match self.send_req(d, a, entry, req).await {
            Ok(ack) => ack,
            Err(LegFailure::Refused(e)) => return Err(TradeError::Refused(e)),
            Err(LegFailure::Unknown { message, alert }) => return Err(TradeError::Unknown { keys, message, alert }),
        };
        let mut out = vec![Placed { key: entry.client_order_id.clone(), venue_id: ack.venue_id, symbol: symbol.clone(), note: None }];
        for l in legs {
            let kind = ExitKind::of(l).unwrap_or(ExitKind::StopLoss);
            let x = AttachedExit { symbol: symbol.clone(), entry_key: entry.client_order_id.clone(), kind, entry_side: entry.side };
            out.push(Placed { key: l.client_order_id.clone(), venue_id: x.id(), symbol: symbol.clone(), note: None });
            self.register_exit(a, &l.client_order_id, x);
        }
        Ok(out)
    }

    /// Follow an attached exit under its OTW client id (after a restart, from its stored id).
    pub fn register_exit(&self, a: &Account<'_>, key: &str, x: AttachedExit) {
        lock(&self.exits).insert(format!("{}:{key}", a.scope), x);
    }

    /// The attached exit sent under `key`, if it is one.
    pub fn exit_of(&self, a: &Account<'_>, key: &str) -> Option<AttachedExit> {
        lock(&self.exits).get(&format!("{}:{key}", a.scope)).cloned()
    }

    /// Where an attached exit stands: working with its entry until the entry fills (the venue
    /// creates it then); then the venue's own order, found among the working and recent orders
    /// by its entry's client id and its kind. `None`: the entry was never seen.
    pub async fn exit_status(&self, d: &Driver<M>, a: &Account<'_>, x: &AttachedExit) -> Result<Option<VenueOrder>, TradeError> {
        let m = d.mapper();
        let Some(entry) = self.lookup(d, a, &x.symbol, &x.entry_key).await? else { return Ok(None) };
        let pending = |status: OrderStatus| VenueOrder {
            venue_id: x.id(),
            client_id: None,
            symbol: x.symbol.clone(),
            side: Mapped::Known(if x.entry_side == Side::Buy { Side::Sell } else { Side::Buy }),
            order_type: String::new(),
            status: Mapped::Known(status),
            qty: entry.qty,
            filled_qty: Decimal::ZERO,
            limit: None,
            stop: None,
            avg_price: None,
            reduce_only: true,
            position: entry.position,
            created_ms: entry.created_ms,
            trigger_pending: true,
            parent_client_id: entry.client_id.clone(),
            exit: Some(x.kind),
            successor_id: None,
        };
        if entry.filled_qty.is_zero() {
            return Ok(Some(match entry.status {
                Mapped::Known(st) if st.is_final() => pending(OrderStatus::Cancelled),
                _ => pending(OrderStatus::New),
            }));
        }
        let cid = self.client_known(d, a, &x.entry_key).map_err(refused)?.id;
        let mut rows = self.open_orders(d, a, Some(&x.symbol)).await?;
        let t = self.tables(d, a).await?;
        for req in m.recent_orders(a.settings, &x.symbol, corr()).map_err(refused)? {
            rows.extend(self.pages(d, a, req, &t, |m, c, b| m.decode_orders_in(c, b)).await.map_err(read_error)?);
        }
        Ok(rows.into_iter().find(|o| o.parent_client_id.as_deref() == Some(cid.as_str()) && o.exit == Some(x.kind)))
    }

    /// Set (`Some`) or remove (`None`) an attached exit's price on the position it guards.
    pub async fn change_exit(&self, d: &Driver<M>, a: &Account<'_>, x: &AttachedExit, price: Option<Decimal>) -> Result<(), TradeError> {
        let m = d.mapper();
        let _turn = self.turn(a, &x.symbol).await;
        if let Some(p) = price {
            let rules = self.rules(d, a, &x.symbol).await?;
            if let Some(tick) = rules.tick.filter(|t| *t > Decimal::ZERO && !(p % *t).is_zero()) {
                return Err(refused(ConnectorError::invalid(format!("{} moves in steps of {} on {}: {} is off that step", x.symbol, tick.normalize(), m.venue_name(), p.normalize())).with_code("venue_rule")));
            }
        }
        let mode = self.mode(d, a, &x.symbol).await?;
        let position = mode.side_for(x.entry_side, false);
        let mut working = Vec::new();
        if let Some(req) = m.position_exits(a.settings, &x.symbol, corr()).map_err(refused)? {
            let t = self.tables(d, a).await?;
            working = self.pages(d, a, req, &t, |m, c, b| m.decode_orders_in(c, b)).await.map_err(read_error)?;
        }
        let current = working.iter().find(|o| o.exit == Some(x.kind) && o.position == position && !matches!(o.status, Mapped::Known(st) if st.is_final()));
        let change = ExitChange { symbol: &x.symbol, position, entry_side: x.entry_side, kind: x.kind, price, current };
        let req = m.change_exit(a.settings, &change, corr()).map_err(refused)?;
        match self.call(d, a, req).await {
            Ok(_) => Ok(()),
            Err(e) if e.canon == Canon::Unknown => Err(TradeError::Unknown { keys: Vec::new(), message: format!("{}: the change to the exit on {} got no answer: read the position again ({})", m.venue_name(), x.symbol, e.message), alert: !e.mapped }),
            Err(e) => Err(refused(e.to_connector())),
        }
    }

    /// Cancel legs a refused ticket already placed: each one's outcome, in words.
    async fn undo(&self, d: &Driver<M>, a: &Account<'_>, placed: &[Placed]) -> Vec<(String, Result<(), String>)> {
        let mut out = Vec::new();
        for p in placed {
            let r = match self.cancel_inner(d, a, &p.symbol, &p.venue_id).await {
                Ok(_) => Ok(()),
                Err(e) => Err(e.message().to_string()),
            };
            out.push((p.key.clone(), r));
        }
        out
    }

    /// Send a ticket: one or more orders on one instrument, all or nothing (spec 6.1).
    pub async fn place(&self, d: &Driver<M>, a: &Account<'_>, orders: &[OrderRequest], opt: &TicketOptions) -> Result<Vec<Placed>, TradeError> {
        self.check_hold(d, a)?;
        self.check_maintenance(d, a)?;
        let (cids, rules) = self.pre_trade(d, a, orders, opt.reference).await?;
        let symbol = orders[0].instrument.native.clone();
        let _turn = self.turn(a, &symbol).await;
        let mode = self.mode(d, a, &symbol).await?;
        // The margin and leverage of the side each order acts on (one per instrument on most
        // venues), as the venue holds them.
        let mut margins = Vec::with_capacity(orders.len());
        let mut levers = Vec::with_capacity(orders.len());
        for o in orders {
            let side = mode.side_for(o.side, o.opts.reduce_only);
            margins.push(self.symbol_margin_on(d, a, &symbol, side).await?);
            levers.push(self.order_leverage(d, a, &symbol, side, o.opts.reduce_only).await?);
        }
        self.tables(d, a).await?;
        let named = self.followed_name(d, a, &symbol);
        let now_ms = d.venue_now().millis();
        let placing = |i: usize, o: &OrderRequest| Placing {
            margin: margins[i].clone(),
            contract_size: rules.contract_size,
            leverage: levers[i],
            now_ms,
            ..Placing::new(cids[i].clone(), mode.side_for(o.side, o.opts.reduce_only), corr())
        };
        if opt.dry_run_first {
            self.session_check(d, a, &orders[0], &placing(0, &orders[0])).await?;
        }
        let exits = attached_exits(d.mapper().venue_name(), orders).map_err(refused)?;
        if !exits.is_empty() {
            let mut p = placing(0, &orders[0]);
            let note = self.quote_buy(d, a, &orders[0], &rules, opt.reference, &mut p).await.map_err(refused)?;
            let mut out = self.place_attached(d, a, &orders[0], &orders[1..], &exits, &p).await?;
            out[0].note = note;
            for o in out.iter_mut() {
                o.symbol = named.clone();
            }
            return Ok(out);
        }
        let mut placed: Vec<Placed> = Vec::new();
        for (i, o) in orders.iter().enumerate() {
            let mut p = placing(i, o);
            let sent = match self.quote_buy(d, a, o, &rules, opt.reference, &mut p).await {
                Ok(note) => self.send_leg(d, a, o, &p).await.map(|ack| (ack, note)),
                Err(e) => Err(LegFailure::Refused(e)),
            };
            match sent {
                Ok((ack, note)) => placed.push(Placed { key: o.client_order_id.clone(), venue_id: ack.venue_id, symbol: named.clone(), note }),
                Err(fail) => return Err(self.ticket_failed(d, a, o, fail, &placed).await),
            }
        }
        Ok(placed)
    }

    /// The price an order is valued at, read now: the venue's read, else the ticket's
    /// reference. `what` names the order in the refusal.
    async fn price_now(&self, d: &Driver<M>, a: &Account<'_>, o: &OrderRequest, fallback: Option<Decimal>, what: &str) -> Result<Decimal, ConnectorError> {
        let m = d.mapper();
        let (venue, sym) = (m.venue_name(), o.instrument.native.as_str());
        let px = match m.reference_price(a.settings, sym, o.side, corr())? {
            Some(req) => {
                let body = self
                    .call(d, a, req)
                    .await
                    .map_err(|e| ConnectorError::new(ErrorKind::Transient, format!("{venue}: the price of {sym} could not be read, so nothing was sent ({})", e.message)))?;
                m.decode_reference_price(&body, o.side)?
            }
            None => fallback.ok_or_else(|| ConnectorError::invalid(format!("{venue} takes {what}, and no price was at hand to value it: nothing was sent, send it from the chart")))?,
        };
        if px <= Decimal::ZERO {
            return Err(ConnectorError::invalid(format!("{venue}: no price above zero to value the order on {sym}: nothing was sent")));
        }
        Ok(px)
    }

    /// An order on a book sized in the quote currency ([`VenueRules::size_unit`]): its size in
    /// that currency (as given, or the coin size at the price read now), rounded down to the
    /// venue's step ([`quote_size`]), set on `p`. Answers what the user is told when the size
    /// that leaves is not the one typed.
    async fn quote_sized(&self, d: &Driver<M>, a: &Account<'_>, o: &OrderRequest, rules: &VenueRules, fallback: Option<Decimal>, p: &mut Placing) -> Result<Option<String>, ConnectorError> {
        let m = d.mapper();
        let (venue, sym) = (m.venue_name(), o.instrument.native.as_str());
        let qty = o.qty.value();
        let (raw, px) = match o.opts.size_unit {
            SizeUnit::Quote => (qty, None),
            SizeUnit::Base => {
                let px = self.price_now(d, a, o, fallback, &format!("{sym} in the quote currency, and a size in the coin is converted at the price now")).await?;
                (qty * px, Some(px))
            }
        };
        let amount = quote_size(venue, rules, raw, o.order_type == OrderType::Market, px)?;
        p.quote_amount = Some(amount);
        Ok(match px {
            Some(px) => Some(format!("{venue} sizes {sym} in the quote currency: {} in the coin at {} went out as {amount}, rounded down to its step", qty.normalize(), px.normalize())),
            None if amount != qty.normalize() => Some(format!("{venue} sizes {sym} in steps: {} went out as {amount}, rounded down to its step", qty.normalize())),
            None => None,
        })
    }

    /// A market buy the venue takes as an amount to spend ([`TradeMapper::market_buy_in_quote`]):
    /// valued at the reference price read now (the venue's read, else the ticket's), rounded
    /// down to the venue's quote step, set on `p`. Answers what the user is told. On a book
    /// sized in the quote currency, every order is sized there ([`Self::quote_sized`]). Any
    /// other order: nothing.
    async fn quote_buy(&self, d: &Driver<M>, a: &Account<'_>, o: &OrderRequest, rules: &VenueRules, fallback: Option<Decimal>, p: &mut Placing) -> Result<Option<String>, ConnectorError> {
        let m = d.mapper();
        if rules.size_unit == SizeUnit::Quote {
            return self.quote_sized(d, a, o, rules, fallback, p).await;
        }
        if !m.market_buy_in_quote(o) {
            return Ok(None);
        }
        let (venue, sym) = (m.venue_name(), o.instrument.native.as_str());
        let px = self.price_now(d, a, o, fallback, &format!("a market buy of {sym} as an amount to spend")).await?;
        let qty = o.qty.value();
        let raw = qty * px;
        let amount = match rules.quote_step.filter(|s| *s > Decimal::ZERO) {
            Some(step) => (raw / step).floor() * step,
            None => raw,
        }
        .normalize();
        if amount <= Decimal::ZERO {
            return Err(ConnectorError::invalid(format!("{venue}: {} {sym} at {} is worth less than the smallest amount it takes: nothing was sent", qty.normalize(), px.normalize())).with_code("venue_rule"));
        }
        p.quote_amount = Some(amount);
        Ok(Some(format!(
            "{venue} takes a market buy as an amount to spend: {} {sym} at {} went out as {amount} in the quote coin, so the size filled may differ a little from {}",
            qty.normalize(),
            px.normalize(),
            qty.normalize()
        )))
    }

    /// A ticket stopped at `o`: what was placed before it is cancelled, and the answer says what
    /// became of every leg. A leg whose cancel failed may be working: it is unknown.
    async fn ticket_failed(&self, d: &Driver<M>, a: &Account<'_>, o: &OrderRequest, fail: LegFailure, placed: &[Placed]) -> TradeError {
        let undone = self.undo(d, a, placed).await;
        let mut unknown: Vec<String> = undone.iter().filter(|(_, r)| r.is_err()).map(|(k, _)| k.clone()).collect();
        let mut lines: Vec<String> = undone
            .iter()
            .map(|(k, r)| match r {
                Ok(()) => format!("{k}: placed, then cancelled"),
                Err(e) => format!("{k}: placed, and its cancel failed ({e}): it may be working, check it at the venue"),
            })
            .collect();
        let (head, alert) = match &fail {
            LegFailure::Refused(e) => (e.message.clone(), false),
            LegFailure::Unknown { message, alert } => {
                unknown.push(o.client_order_id.clone());
                (message.clone(), *alert)
            }
        };
        let message = if lines.is_empty() {
            head
        } else {
            lines.insert(0, format!("{}: {head}", o.client_order_id));
            lines.join("; ")
        };
        if unknown.is_empty() {
            match fail {
                LegFailure::Refused(e) => TradeError::Refused(ConnectorError { message, ..e }),
                LegFailure::Unknown { .. } => TradeError::Unknown { keys: unknown, message, alert },
            }
        } else {
            TradeError::Unknown { keys: unknown, message, alert }
        }
    }

    /// Cancel one order by the venue's id. One the venue no longer has working is looked up:
    /// already final is no error (spec 6.2), the state it ended in is answered.
    pub async fn cancel(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, venue_id: &str) -> Result<Cancelled, TradeError> {
        let _turn = self.turn(a, symbol).await;
        self.cancel_inner(d, a, symbol, venue_id).await
    }

    async fn cancel_inner(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, venue_id: &str) -> Result<Cancelled, TradeError> {
        let mut id = venue_id.to_string();
        for _ in 0..MAX_HOPS {
            match self.cancel_once(d, a, symbol, &id).await? {
                // The order fired as a new one, still working: that one is cancelled.
                Err(next) => id = next,
                Ok(c) => return Ok(c),
            }
        }
        Err(TradeError::Unknown { keys: Vec::new(), message: format!("{}: order {venue_id} kept moving to new orders: check it at the venue", d.mapper().venue_name()), alert: true })
    }

    /// One cancel. `Ok(Err(id))`: the order became order `id` (a trigger that fired), which
    /// still works and is to be cancelled in turn.
    async fn cancel_once(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, venue_id: &str) -> Result<Result<Cancelled, String>, TradeError> {
        let m = d.mapper();
        let client = self.client_of_venue(a, venue_id);
        let req = m.cancel_order(a.settings, symbol, venue_id, client.as_ref(), corr()).map_err(refused)?;
        let e = match self.call(d, a, req).await {
            Ok(_) => return Ok(Ok(Cancelled::Sent)),
            Err(e) => e,
        };
        let reconcile = matches!(e.canon, Canon::Rejected(Refusal::NotFound) | Canon::Rejected(Refusal::Final) | Canon::Unknown);
        if !reconcile {
            return Err(refused(e.to_connector()));
        }
        let found = self.by_venue_id(d, a, symbol, venue_id).await;
        if let Ok(Some(o)) = &found {
            if o.venue_id != venue_id && !matches!(o.status, Mapped::Known(st) if st.is_final()) {
                if let Some(key) = lock(&self.by_venue).get(&Self::client_key(a, venue_id)).cloned() {
                    self.placed_as(a, &key, &o.venue_id);
                }
                return Ok(Err(o.venue_id.clone()));
            }
        }
        Ok(Ok(match found {
            Ok(Some(o)) => match o.status {
                Mapped::Known(st) if st.is_final() => Ok(Cancelled::AlreadyFinal(st)),
                Mapped::Known(_) if e.canon == Canon::Unknown => Err(TradeError::Unknown {
                    keys: Vec::new(),
                    message: format!("{}: no answer to the cancel of {venue_id}, and it still works: cancel it again or check it at the venue ({})", m.venue_name(), e.message),
                    alert: !e.mapped,
                }),
                Mapped::Known(_) => Err(refused(e.to_connector())),
                Mapped::Unknown(w) => Err(TradeError::Unknown {
                    keys: Vec::new(),
                    message: format!("{}: order {venue_id} is in a state OpenTraderWorld does not know ({w}): check it at the venue", m.venue_name()),
                    alert: true,
                }),
            },
            _ if e.canon == Canon::Unknown => Err(TradeError::Unknown { keys: Vec::new(), message: format!("{}: the cancel of {venue_id} got no answer: check it at the venue ({})", m.venue_name(), e.message), alert: !e.mapped }),
            _ => Err(refused(e.to_connector())),
        }?))
    }

    /// The kill switch: every working order of the account, on every symbol, its own and those
    /// placed elsewhere. Answers how many were working.
    pub async fn cancel_all(&self, d: &Driver<M>, a: &Account<'_>) -> Result<usize, TradeError> {
        let m = d.mapper();
        let working = self.open_orders(d, a, None).await?;
        let mut symbols: Vec<String> = working.iter().map(|o| o.symbol.clone()).collect();
        symbols.sort();
        symbols.dedup();
        let way = m.cancel_all_way();
        let batch = matches!(way, CancelAllWay::ByIds { .. });
        if batch || m.capabilities().orders.cancel_all_native {
            if symbols.is_empty() {
                return Ok(0);
            }
            let reqs = match way {
                CancelAllWay::ByIds { max } => working.chunks(max.max(1)).map(|c| m.cancel_ids(a.settings, c, corr())).collect::<Result<Vec<_>, _>>(),
                CancelAllWay::Book => m.cancel_all(a.settings, &[], corr()),
                CancelAllWay::BySymbol => m.cancel_all(a.settings, &symbols, corr()),
            }
            .map_err(refused)?;
            for req in reqs {
                let endpoint = req.endpoint_id;
                match self.call(d, a, req).await {
                    // A call the answer says failed: what it left is cancelled one by one below.
                    Ok(body) => {
                        if let Err(e) = m.decode_cancel_all(&body) {
                            tracing::warn!("{}: cancel all did not go through ({}), cancelling one by one", m.venue_name(), e.message);
                        }
                        match m.decode_cancel_rows(&DecodeCtx::new(endpoint, &ContractSizes::default()), &body) {
                            Ok(rows) => {
                                for r in rows.iter().filter(|r| r.error.is_some()) {
                                    tracing::warn!("{}: cancel all left {} ({}), cancelling it on its own", m.venue_name(), r.venue_id, r.error.as_ref().map(|e| e.message.as_str()).unwrap_or_default());
                                }
                            }
                            Err(e) => tracing::warn!("{}: cancel all answer not read ({}), cancelling what is left one by one", m.venue_name(), e.message),
                        }
                    }
                    Err(e) if matches!(e.canon, Canon::Rejected(Refusal::NotFound | Refusal::Final)) => {}
                    Err(e) => return Err(refused(e.to_connector())),
                }
            }
            // A venue that cancels at most so many at once leaves the rest: walked one by one.
            let left = self.open_orders(d, a, None).await?;
            for o in &left {
                self.cancel_inner(d, a, &o.symbol, &o.venue_id).await?;
            }
            return Ok(working.len());
        }
        for o in &working {
            self.cancel_inner(d, a, &o.symbol, &o.venue_id).await?;
        }
        Ok(working.len())
    }

    /// Change a working order in place, for a venue that declares `amend`. The change is checked
    /// against the venue's rules as the order it leaves.
    pub async fn modify(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, venue_id: &str, change: &OrderAmend) -> Result<Option<Placed>, TradeError> {
        let m = d.mapper();
        check_amend(m.capabilities(), change, d.venue_now()).map_err(refused)?;
        self.check_hold(d, a)?;
        let _turn = self.turn(a, symbol).await;
        let cur = self
            .by_venue_id(d, a, symbol, venue_id)
            .await?
            .ok_or_else(|| refused(ConnectorError::not_found(format!("{} does not know order {venue_id}", m.venue_name()))))?;
        if matches!(cur.status, Mapped::Known(st) if st.is_final()) {
            return Err(refused(ConnectorError::invalid(format!("order {venue_id} is not working any more"))));
        }
        let rules = self.rules(d, a, symbol).await?;
        let probe = OrderRequest {
            client_order_id: cur.client_id.clone().unwrap_or_default(),
            instrument: rules.instrument.clone(),
            side: cur.side.known().copied().unwrap_or(Side::Buy),
            order_type: if change.limit.is_some() || cur.limit.is_some() { OrderType::Limit } else { OrderType::Market },
            qty: change.qty.unwrap_or(Qty::new(cur.qty)),
            limit: change.limit.or(cur.limit.map(Price::new)),
            stop: change.stop.or(cur.stop.map(Price::new)),
            tif: TimeInForce::Gtc,
            parent: None,
            oca_group: None,
            account: None,
            opts: OrderOpts { reduce_only: cur.reduce_only, ..Default::default() },
        };
        check_rules(m.venue_name(), &rules, &probe, None).map_err(refused)?;
        if m.amend_replaces() {
            return self.replace(d, a, symbol, &cur, change).await.map(Some);
        }
        let leverage = self.order_leverage(d, a, symbol, cur.position, cur.reduce_only).await?;
        let p = Placing {
            contract_size: rules.contract_size,
            leverage,
            now_ms: d.venue_now().millis(),
            ..Placing::new(cur.client_id.clone().unwrap_or_default(), cur.position, corr())
        };
        let req = m.amend(a.settings, &cur, change, &p).map_err(refused)?;
        match self.call(d, a, req).await {
            Ok(_) => Ok(None),
            Err(e) if e.canon == Canon::Unknown => self.amend_unknown(d, a, symbol, venue_id, change, e).await,
            Err(e) => {
                if e.canon == Canon::Rejected(Refusal::VenueRule) {
                    lock(&self.stale).insert(self.rules_key(d, a, symbol));
                }
                Err(refused(e.to_connector()))
            }
        }
    }

    /// A change whose answer does not say what became of the order (lost, or a refusal that may
    /// have ended it): the order is read again once by its venue id, and its state answered. It
    /// ended: the change is refused, naming the state it ended in. It works with every value
    /// asked: the change went through. It works without them: the change is refused, the order
    /// unchanged. Not read: unknown, as before.
    async fn amend_unknown(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, venue_id: &str, change: &OrderAmend, e: VenueError) -> Result<Option<Placed>, TradeError> {
        let venue = d.mapper().venue_name();
        let unknown = |e: &VenueError| TradeError::Unknown {
            keys: Vec::new(),
            message: format!("{venue}: the change to {venue_id} got no answer: read the order again before changing it ({})", e.message),
            alert: !e.mapped,
        };
        let o = match self.by_venue_id(d, a, symbol, venue_id).await {
            Ok(Some(o)) => o,
            _ => return Err(unknown(&e)),
        };
        let changed = change.limit.is_none_or(|p| o.limit == Some(p.value()))
            && change.stop.is_none_or(|p| o.stop == Some(p.value()))
            && change.qty.is_none_or(|q| o.qty == q.value());
        let said = |message: String| refused(ConnectorError { message, kind: ErrorKind::InvalidRequest, ..e.to_connector() });
        match o.status {
            Mapped::Known(st) if st.is_final() => Err(said(format!("{venue}: the change to {venue_id} was not made, and the order has ended ({st:?}, read again just now): {}", e.message))),
            Mapped::Known(_) if changed => Ok(None),
            Mapped::Known(_) => Err(said(format!("{venue}: the change to {venue_id} was not made: the order, read again just now, still works unchanged ({})", e.message))),
            Mapped::Unknown(w) => Err(TradeError::Unknown {
                keys: Vec::new(),
                message: format!("{venue}: after the change to {venue_id}, the order is in a state OpenTraderWorld does not know ({w}): check it at the venue ({})", e.message),
                alert: true,
            }),
        }
    }

    /// A change made as a new order (cancel and replace, [`TradeMapper::amend_replaces`]): under a
    /// new client id minted before I/O, answered with the new venue id. A lost or unread answer
    /// is looked up by the new client id, never sent again.
    async fn replace(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, cur: &VenueOrder, change: &OrderAmend) -> Result<Placed, TradeError> {
        let m = d.mapper();
        let venue = m.venue_name();
        let old = cur.client_id.as_deref().and_then(|c| self.otw_key(d, a, c)).filter(|k| !k.is_empty()).ok_or_else(|| {
            refused(ConnectorError::invalid(format!(
                "{venue} changes an order by replacing it under a new client id, and {} has none (placed outside OpenTraderWorld): cancel it and place a new one",
                cur.venue_id
            )))
        })?;
        let key = next_client_id(&m.client_id_rule(), &old);
        let cid = self.client_for_send(d, a, &key).map_err(refused)?;
        let req = m.amend_replace(a.settings, cur, change, &cid, corr()).map_err(refused)?;
        let endpoint = req.endpoint_id;
        let t = self.tables(d, a).await?;
        let (why, alert) = match self.call(d, a, req).await {
            Ok(body) => match m.decode_replace_ack(&t.ctx(endpoint), &body) {
                Ok(ack) if ack.has_id() => {
                    self.placed_as(a, &key, &ack.venue_id);
                    return Ok(Placed { key, venue_id: ack.venue_id, symbol: self.followed_name(d, a, symbol), note: None });
                }
                Ok(_) => ("accepted without the new order's id".to_string(), false),
                Err(e) => (format!("the answer was not read: {}", e.message), true),
            },
            Err(e) if e.canon == Canon::Unknown => (e.message.clone(), !e.mapped),
            Err(e) => {
                if e.canon == Canon::Rejected(Refusal::VenueRule) {
                    lock(&self.stale).insert(self.rules_key(d, a, symbol));
                }
                return Err(refused(e.to_connector()));
            }
        };
        match self.lookup(d, a, symbol, &key).await {
            Ok(Some(o)) => Ok(Placed { key, venue_id: o.venue_id, symbol: symbol.to_string(), note: None }),
            _ => Err(TradeError::Unknown {
                keys: Vec::new(),
                message: format!("{venue}: the change to {} went out as new order {key}, which could not be found yet: read the order again before changing it ({why})", cur.venue_id),
                alert,
            }),
        }
    }

    /// Close a position at the market with a reduce-only order: the whole of it, or `qty` of
    /// it. `direction` picks the long (`Buy`) or the short (`Sell`) one where both are held
    /// (hedge mode). The size is the venue's, read now.
    pub async fn close_position(&self, d: &Driver<M>, a: &Account<'_>, symbol: &str, direction: Option<Side>, qty: Option<Decimal>, key: &str) -> Result<Placed, TradeError> {
        let m = d.mapper();
        if !m.capabilities().orders.reduce_only {
            return Err(refused(ConnectorError::unsupported(format!("{} holds balances here, not positions: sell it with an order", m.venue_name()))));
        }
        // Closing sends a new order: refused too while the venue has maintenance announced.
        self.check_maintenance(d, a)?;
        let rows = self.positions(d, a, symbol).await?;
        if let Some(mode) = PositionMode::of(&rows).filter(|_| m.account_position_mode(a.settings, CorrId(0)).ok().flatten().is_none()) {
            self.set_mode(a, symbol, mode);
        }
        // Which way each held row goes: its side in hedge mode, its direction in one-way mode.
        let way = |p: &VenuePosition| match p.side {
            PositionSide::Long => Some(Side::Buy),
            PositionSide::Short => Some(Side::Sell),
            PositionSide::Net => p.direction,
        };
        let held: Vec<&VenuePosition> = rows.iter().filter(|p| p.qty > Decimal::ZERO && direction.is_none_or(|w| way(p) == Some(w))).collect();
        let row = match held.as_slice() {
            [] => return Err(refused(ConnectorError::not_found(format!("no position on {symbol} to close")))),
            [one] => *one,
            _ => return Err(refused(ConnectorError::invalid(format!("{symbol} is held both long and short (hedge mode): close each side from its own row")))),
        };
        let long = match way(row) {
            Some(Side::Buy) => true,
            Some(Side::Sell) => false,
            None => return Err(refused(ConnectorError::invalid(format!("{} did not say which way the position on {symbol} goes: nothing was sent", m.venue_name())))),
        };
        let size = qty.unwrap_or(row.qty);
        if size <= Decimal::ZERO || size > row.qty {
            return Err(refused(ConnectorError::invalid(format!("{symbol}: {} is held, so between 0 and that can be closed ({} asked)", row.qty.normalize(), size.normalize()))));
        }
        let tifs = &m.capabilities().orders.tif;
        let tif = if tifs.contains(&TifKind::Ioc) { TimeInForce::Ioc } else { TimeInForce::Gtc };
        let rules = self.rules(d, a, symbol).await?;
        let o = OrderRequest {
            client_order_id: key.to_string(),
            instrument: rules.instrument.clone(),
            side: if long { Side::Sell } else { Side::Buy },
            order_type: OrderType::Market,
            qty: Qty::new(size),
            limit: None,
            stop: None,
            tif,
            parent: None,
            oca_group: None,
            account: None,
            // The size as the venue holds it: in its own unit on a book sized in the quote currency.
            opts: OrderOpts { reduce_only: true, size_unit: rules.size_unit, ..Default::default() },
        };
        check_orders(m.capabilities(), std::slice::from_ref(&o), d.venue_now()).map_err(refused)?;
        m.check_order(&o).map_err(refused)?;
        check_rules(m.venue_name(), &rules, &o, None).map_err(refused)?;
        let cid = self.client_for_send(d, a, key).map_err(refused)?;
        let _turn = self.turn(a, symbol).await;
        let margin = match row.margin.clone() {
            Some(mm) => Some(mm),
            None => self.symbol_margin_on(d, a, symbol, row.side).await?,
        };
        let leverage = match row.leverage {
            Some(l) => Some(l),
            None => self.order_leverage(d, a, symbol, row.side, true).await?,
        };
        // A sided row on an account the venue states is one-way (MEXC: the mode is the account's,
        // its rows are always sided) closes in the one-way form; any other row as it was read.
        let one_way = m.capabilities().orders.position_mode.is_some() && row.side != PositionSide::Net && matches!(self.mode(d, a, symbol).await, Ok(PositionMode::OneWay));
        let position = if one_way { PositionSide::Net } else { row.side };
        let mut p = Placing { margin, contract_size: rules.contract_size, leverage, now_ms: d.venue_now().millis(), ..Placing::new(cid, position, corr()) };
        if rules.size_unit == SizeUnit::Quote {
            self.quote_sized(d, a, &o, &rules, None, &mut p).await.map_err(refused)?;
        }
        match self.send_leg(d, a, &o, &p).await {
            Ok(ack) => Ok(Placed { key: key.to_string(), venue_id: ack.venue_id, symbol: self.followed_name(d, a, symbol), note: None }),
            Err(LegFailure::Refused(e)) => Err(TradeError::Refused(e)),
            Err(LegFailure::Unknown { message, alert }) => Err(TradeError::Unknown { keys: vec![key.to_string()], message, alert }),
        }
    }

    /// Check a ticket the way the venue would, and place nothing: every check a place runs, then
    /// the venue's own check of each order it covers. Answers what was checked.
    pub async fn dry_run(&self, d: &Driver<M>, a: &Account<'_>, orders: &[OrderRequest], reference: Option<Decimal>) -> Result<String, TradeError> {
        let m = d.mapper();
        if m.capabilities().orders.dry_run == DryRunCap::None {
            return Err(refused(ConnectorError::unsupported(format!("{} has no dry run", m.venue_name()))));
        }
        let (_, rules) = self.pre_trade(d, a, orders, reference).await?;
        let mode = self.mode(d, a, &orders[0].instrument.native).await?;
        let (mut said, mut unchecked) = (Vec::new(), Vec::new());
        for o in orders {
            let side = mode.side_for(o.side, o.opts.reduce_only);
            let margin = self.symbol_margin_on(d, a, &o.instrument.native, side).await?;
            let leverage = self.order_leverage(d, a, &o.instrument.native, side, o.opts.reduce_only).await?;
            // Never the live order's client id: the venue must not see it before the order.
            let mut p = Placing { margin, contract_size: rules.contract_size, leverage, now_ms: d.venue_now().millis(), ..Placing::new(String::new(), side, corr()) };
            self.quote_buy(d, a, o, &rules, reference, &mut p).await.map_err(refused)?;
            match m.dry_run(a.settings, o, &p).map_err(refused)? {
                Some(req) => {
                    let body = self.call(d, a, req).await.map_err(|e| refused(ConnectorError::invalid(format!("{}'s check refused {} ({})", m.venue_name(), o.client_order_id, e.message))))?;
                    said.push(format!("{}: {}", o.client_order_id, m.decode_dry_run(&body).map_err(refused)?));
                }
                None => unchecked.push(o.client_order_id.clone()),
            }
        }
        let rest = if unchecked.is_empty() { String::new() } else { format!("; not covered by the venue's check: {}", unchecked.join(", ")) };
        Ok(format!("{}: {}, nothing placed{rest}", m.venue_name(), if said.is_empty() { "no order checked".to_string() } else { said.join("; ") }))
    }
}

/// An exit attached at the venue to an entry OTW sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedExit {
    pub symbol: String,
    /// The entry's OTW client id.
    pub entry_key: String,
    pub kind: ExitKind,
    pub entry_side: Side,
}

impl AttachedExit {
    /// The id it is followed by (after the book and symbol): `tpsl:ENTRY:tp:buy`.
    pub fn id(&self) -> String {
        format!("tpsl:{}:{}:{}", self.entry_key, self.kind.word(), if self.entry_side == Side::Buy { "buy" } else { "sell" })
    }

    /// Read back [`AttachedExit::id`] on `symbol`.
    pub fn parse(symbol: &str, id: &str) -> Option<AttachedExit> {
        let rest = id.strip_prefix("tpsl:")?;
        let mut parts = rest.rsplitn(3, ':');
        let side = match parts.next()? {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return None,
        };
        let kind = ExitKind::from_word(parts.next()?)?;
        let entry_key = parts.next()?.to_string();
        Some(AttachedExit { symbol: symbol.to_string(), entry_key, kind, entry_side: side })
    }
}

/// A ticket's exits attached to its entry, as the venue takes them: one entry first, then at
/// most one take profit and one stop loss on it, each on the other side and of the entry's size.
/// Empty for a ticket with no exit. The declared capabilities (`brackets`) are checked before.
fn attached_exits(venue: &str, orders: &[OrderRequest]) -> Result<Vec<(ExitKind, Decimal)>, ConnectorError> {
    let Some(entry) = orders.first() else { return Ok(Vec::new()) };
    let legs = &orders[1..];
    if legs.iter().all(|o| o.parent.is_none()) {
        return Ok(Vec::new());
    }
    let mut out: Vec<(ExitKind, Decimal)> = Vec::new();
    for o in legs {
        if o.parent.as_deref() != Some(entry.client_order_id.as_str()) {
            return Err(ConnectorError::unsupported(format!("{venue} attaches exits to the ticket's entry only, and sends no other order with them")));
        }
        let kind = ExitKind::of(o).ok_or_else(|| ConnectorError::unsupported(format!("{venue} attaches a take profit (limit or if-touched) or a stop loss (stop), not a {:?}", o.order_type)))?;
        if out.iter().any(|(k, _)| *k == kind) {
            return Err(ConnectorError::invalid(format!("{venue} attaches one take profit and one stop loss at most")));
        }
        if o.side == entry.side || o.qty != entry.qty {
            return Err(ConnectorError::invalid(format!("{venue} attaches exits that close the entry: on its other side and of its size")));
        }
        let price = ExitKind::price(o).ok_or_else(|| ConnectorError::invalid("an exit needs its price"))?;
        out.push((kind, price));
    }
    Ok(out)
}

/// Why one leg did not go out.
enum LegFailure {
    Refused(ConnectorError),
    Unknown { message: String, alert: bool },
}

fn unmapped_read<M: TradeMapper>(m: &M, e: &ConnectorError) -> TradeError {
    TradeError::Unknown { keys: Vec::new(), message: format!("{}: an order answer OpenTraderWorld could not read ({})", m.venue_name(), e.message), alert: true }
}
