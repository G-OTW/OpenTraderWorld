//! Order sending, the venue side (onboarding spec 2, 4, 6, 8): what a venue writes so the
//! shared trader (`otw_connector_engine::trading`) can place, cancel and look up its orders.
//!
//! Pure translation, like [`Mapper`]: requests built, answers decoded, no I/O, no clock, no
//! secrets. Everything that is the same for every venue (the order of the calls, the venue's
//! rules checked before anything leaves, a lost answer looked up and never sent again, a ticket
//! never left half placed) is the trader's, written once. A venue adds orders by writing a
//! [`TradeMapper`] and its fixtures, nothing else.
//!
//! - **Exhaustive words.** A status word the mapper does not know is `Mapped::Unknown`, never a
//!   near match: the trader treats it as unmapped and alerts.
//! - **Exhaustive errors.** Every order error code the venue documents maps to one canonical
//!   kind ([`Canon`]); a code the table does not list is `Unknown` with `mapped = false`, which
//!   the trader treats as an order whose outcome is not known, and alerts.
//! - **Rules are the venue's.** [`VenueRules`] are read from the venue's own instrument list
//!   and checked before I/O ([`check_rules`]); a refusal names the rule and the nearest values
//!   that pass. Nothing is adjusted.

use std::collections::HashMap;

use otw_connector_model::{
    ConnectorError, CorrId, Decimal, ErrorKind, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, SizeUnit,
};

use crate::mapper::Mapper;
use crate::native::{HttpRequest, RawFrame};

/// An account's non-secret settings and its credentials' names, as OTW stores them.
pub type Settings = HashMap<String, String>;

/// How a venue spells the caller's order id (spec 2.1): its charset and its length.
#[derive(Debug, Clone, Copy)]
pub struct ClientIdRule {
    pub max_len: usize,
    /// The charset in words, for the refusal.
    pub charset: &'static str,
    pub allowed: fn(char) -> bool,
}

impl ClientIdRule {
    /// Letters, digits, `-` and `_`, the charset most venues take.
    pub fn alnum_dash_underscore(max_len: usize) -> Self {
        ClientIdRule { max_len, charset: "letters, digits, - and _", allowed: |c| c.is_ascii_alphanumeric() || c == '-' || c == '_' }
    }

    /// Refuse an id the venue would refuse, naming the rule.
    pub fn check(&self, venue: &str, id: &str) -> Result<(), ConnectorError> {
        let ok = !id.is_empty() && id.chars().count() <= self.max_len && id.chars().all(self.allowed);
        if ok {
            return Ok(());
        }
        Err(ConnectorError::invalid(format!("{venue} takes client order ids of 1 to {} {} ({id:?} is not one)", self.max_len, self.charset)))
    }
}

/// Which position an order acts on: the instrument's only one, or one side of a hedge pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositionSide {
    /// One position per instrument (one-way mode), or a book with no positions.
    Net,
    Long,
    Short,
}

/// The account's position mode on an instrument, as the venue states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionMode {
    OneWay,
    Hedge,
}

impl PositionMode {
    /// The position an order works on: a buy opens the long and a reduce-only sell closes it.
    pub fn side_for(self, side: Side, reduce_only: bool) -> PositionSide {
        match (self, side, reduce_only) {
            (PositionMode::OneWay, _, _) => PositionSide::Net,
            (PositionMode::Hedge, Side::Buy, false) | (PositionMode::Hedge, Side::Sell, true) => PositionSide::Long,
            (PositionMode::Hedge, Side::Sell, false) | (PositionMode::Hedge, Side::Buy, true) => PositionSide::Short,
        }
    }

    /// The mode the venue's position rows show for one instrument: every row net, or the rows
    /// sided. `None` when they say neither (no row, or both kinds at once): the caller refuses.
    pub fn of(rows: &[VenuePosition]) -> Option<PositionMode> {
        if rows.is_empty() {
            return None;
        }
        if rows.iter().all(|r| r.side == PositionSide::Net) {
            return Some(PositionMode::OneWay);
        }
        if rows.iter().all(|r| r.side != PositionSide::Net) {
            return Some(PositionMode::Hedge);
        }
        None
    }
}

/// Whether an instrument takes orders now, as the venue's list says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradingStatus {
    Trading,
    /// Only orders that reduce a position.
    CloseOnly,
    /// Listed, not trading (pre-launch, halted, settling, delisted): the venue's word.
    Closed(String),
    /// Only orders with a limit price (Coinbase `limit_only`, Kraken `limit_only`): the
    /// venue's word.
    LimitOnly(String),
    /// Only post-only limit orders (Coinbase `post_only`, Kraken `post_only`): the venue's word.
    PostOnly(String),
    /// Only buys (MEXC `tradeSideType` 2): the venue's word.
    BuyOnly(String),
    /// Only sells (MEXC `tradeSideType` 3): the venue's word.
    SellOnly(String),
}

impl TradingStatus {
    /// Whether rules in this state are held for the day: trading, or close-only. Any other
    /// state is read again on the next order, so the instrument takes orders the moment the
    /// venue lifts it.
    pub fn held(&self) -> bool {
        matches!(self, TradingStatus::Trading | TradingStatus::CloseOnly)
    }
}

/// An instrument's trading rules as the venue states them (spec 4.1). A field the venue does
/// not state is `None`, and the venue then checks alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenueRules {
    pub instrument: InstrumentId,
    pub tick: Option<Decimal>,
    pub min_price: Option<Decimal>,
    pub max_price: Option<Decimal>,
    pub lot_step: Option<Decimal>,
    pub min_qty: Option<Decimal>,
    /// Largest size of a limit order.
    pub max_qty: Option<Decimal>,
    /// Largest size of a market order, where the venue sets one apart.
    pub market_max_qty: Option<Decimal>,
    /// Smallest and largest value (price times size) of an order, in the quote currency.
    pub min_notional: Option<Decimal>,
    pub max_notional: Option<Decimal>,
    pub status: TradingStatus,
    /// How many base units one contract (or lot) is, for a book that counts its sizes in
    /// contracts (Gate futures, KuCoin Futures). The mapper states every size above in OTW's
    /// units (the base coin) and converts with this; `None`: sizes are in the base coin.
    pub contract_size: Option<Decimal>,
    /// The step of an amount in the quote coin (what a quote-sized market buy is rounded to).
    pub quote_step: Option<Decimal>,
    /// The unit the venue counts this book's sizes in ([`SizeUnit`]). `Quote` (Deribit
    /// inverse contracts, sized in USD): every size rule above (`lot_step`, `min_qty`,
    /// `max_qty`, `market_max_qty`) is in the quote currency, and an order's size is converted
    /// and rounded down to the step before it leaves ([`quote_size`]).
    pub size_unit: SizeUnit,
    /// The other names the venue gives this instrument in its answers (Kraken: the altname,
    /// the internal key, the socket name), for a mapper that reads them back
    /// ([`TradeMapper::uses_symbol_names`], [`DecodeCtx::names`]).
    pub aliases: Vec<String>,
}

impl VenueRules {
    /// A rule set stating nothing but the instrument: what a decoder starts from.
    pub fn new(instrument: InstrumentId) -> Self {
        VenueRules {
            instrument,
            tick: None,
            min_price: None,
            max_price: None,
            lot_step: None,
            min_qty: None,
            max_qty: None,
            market_max_qty: None,
            min_notional: None,
            max_notional: None,
            status: TradingStatus::Trading,
            contract_size: None,
            quote_step: None,
            size_unit: SizeUnit::Base,
            aliases: Vec::new(),
        }
    }

    /// Whether the rules are sound: steps and bounds above zero, a minimum under its maximum.
    /// The public conformance test asserts it of every instrument the venue lists.
    pub fn sound(&self) -> Result<(), String> {
        let pos = |v: Option<Decimal>, what: &str| match v {
            Some(d) if d <= Decimal::ZERO => Err(format!("{}: {what} {d} is not above zero", self.instrument.native)),
            _ => Ok(()),
        };
        pos(self.tick, "tick")?;
        pos(self.lot_step, "lot step")?;
        pos(self.max_qty, "max qty")?;
        pos(self.market_max_qty, "market max qty")?;
        pos(self.contract_size, "contract size")?;
        pos(self.quote_step, "quote step")?;
        if let (Some(lo), Some(hi)) = (self.min_qty, self.max_qty) {
            if lo > hi {
                return Err(format!("{}: min qty {lo} above max qty {hi}", self.instrument.native));
            }
        }
        if let (Some(lo), Some(hi)) = (self.min_price, self.max_price) {
            if lo > hi {
                return Err(format!("{}: min price {lo} above max price {hi}", self.instrument.native));
            }
        }
        if self.min_qty.is_some_and(|q| q < Decimal::ZERO) || self.min_notional.is_some_and(|q| q < Decimal::ZERO) {
            return Err(format!("{}: a minimum below zero", self.instrument.native));
        }
        Ok(())
    }
}

fn num(d: Decimal) -> String {
    d.normalize().to_string()
}

/// The grid values either side of `v`: what a refusal offers instead.
fn either_side(v: Decimal, step: Decimal) -> (Decimal, Decimal) {
    ((v / step).floor() * step, (v / step).ceil() * step)
}

fn off_grid(v: Decimal, step: Decimal) -> bool {
    step > Decimal::ZERO && !(v % step).is_zero()
}

/// An order against the instrument's rules, before anything leaves (spec 4.2). `reference` is
/// the price a market order's value is measured at, when the caller has one; without it, a
/// market order's value is left to the venue. A refusal names the rule and the nearest values
/// that pass. Nothing is adjusted.
pub fn check_rules(venue: &str, r: &VenueRules, o: &OrderRequest, reference: Option<Decimal>) -> Result<(), ConnectorError> {
    let sym = &r.instrument.native;
    let refuse = |m: String| Err(ConnectorError::invalid(m).with_code("venue_rule"));
    match &r.status {
        TradingStatus::Trading => {}
        TradingStatus::CloseOnly if o.opts.reduce_only => {}
        TradingStatus::CloseOnly => return refuse(format!("{venue} takes only orders that reduce a position on {sym} now: send it reduce-only, or wait")),
        TradingStatus::Closed(word) => return refuse(format!("{venue} does not trade {sym} now (its status is {word}): nothing was sent")),
        TradingStatus::LimitOnly(word) if o.limit.is_none() => {
            return refuse(format!("{venue} takes only orders with a limit price on {sym} now (its status is {word}): send a limit order, or wait"))
        }
        TradingStatus::PostOnly(word) if !(o.order_type == OrderType::Limit && o.opts.post_only) => {
            return refuse(format!("{venue} takes only post-only limit orders on {sym} now (its status is {word}): send it post-only, or wait"))
        }
        TradingStatus::BuyOnly(word) if o.side != Side::Buy => return refuse(format!("{venue} takes only buys on {sym} now (its status is {word}): nothing was sent")),
        TradingStatus::SellOnly(word) if o.side != Side::Sell => return refuse(format!("{venue} takes only sells on {sym} now (its status is {word}): nothing was sent")),
        TradingStatus::LimitOnly(_) | TradingStatus::PostOnly(_) | TradingStatus::BuyOnly(_) | TradingStatus::SellOnly(_) => {}
    }
    for (what, p) in [("limit", o.limit), ("stop", o.stop)] {
        let Some(p) = p.map(|p| p.value()) else { continue };
        if let Some(tick) = r.tick.filter(|t| off_grid(p, *t)) {
            let (lo, hi) = either_side(p, tick);
            return refuse(format!("{sym} moves in steps of {} on {venue}: the {what} price {} is off that step ({} or {})", num(tick), num(p), num(lo), num(hi)));
        }
        if let Some(min) = r.min_price.filter(|m| p < *m) {
            return refuse(format!("{venue} takes no {what} price under {} on {sym} ({} given)", num(min), num(p)));
        }
        if let Some(max) = r.max_price.filter(|m| p > *m) {
            return refuse(format!("{venue} takes no {what} price over {} on {sym} ({} given)", num(max), num(p)));
        }
    }
    match (r.size_unit, o.opts.size_unit) {
        // Sizes on a book sized in the quote currency are checked once converted ([`quote_size`]).
        (SizeUnit::Quote, _) => return Ok(()),
        (SizeUnit::Base, SizeUnit::Quote) => return refuse(format!("{venue} sizes {sym} in the base coin: give the size in it, not in the quote currency")),
        (SizeUnit::Base, SizeUnit::Base) => {}
    }
    let q = o.qty.value();
    if let Some(min) = r.min_qty.filter(|m| q < *m) {
        return refuse(format!("{venue} takes at least {} {sym} per order ({} given)", num(min), num(q)));
    }
    if let Some(step) = r.lot_step.filter(|s| off_grid(q, *s)) {
        let (lo, hi) = either_side(q, step);
        let lo_ok = r.min_qty.is_none_or(|m| lo >= m) && lo > Decimal::ZERO;
        let near = if lo_ok { format!("{} or {}", num(lo), num(hi)) } else { num(hi) };
        return refuse(format!("{sym} trades in steps of {} on {venue}: {} is off that step ({near})", num(step), num(q)));
    }
    let max = if o.order_type == OrderType::Market { r.market_max_qty.or(r.max_qty) } else { r.max_qty };
    if let Some(max) = max.filter(|m| q > *m) {
        let kind = if o.order_type == OrderType::Market { "a market order" } else { "an order" };
        return refuse(format!("{venue} takes at most {} {sym} in {kind} ({} given): split it", num(max), num(q)));
    }
    let price = o.limit.map(|p| p.value()).or(o.stop.map(|p| p.value())).or(reference);
    if let Some(px) = price {
        let value = px * q;
        if let Some(min) = r.min_notional.filter(|m| value < *m) {
            let need = min.checked_div(px).map(|n| match r.lot_step {
                Some(s) if s > Decimal::ZERO => (n / s).ceil() * s,
                _ => n,
            });
            let hint = need.map(|n| format!(": {} {sym} or more at that price", num(n))).unwrap_or_default();
            return refuse(format!("{venue} takes orders worth at least {} on {sym}; this one is worth {}{hint}", num(min), num(value)));
        }
        if let Some(max) = r.max_notional.filter(|m| value > *m) {
            return refuse(format!("{venue} takes orders worth at most {} on {sym}; this one is worth {}", num(max), num(value)));
        }
    }
    Ok(())
}

/// An order's size on a book sized in the quote currency ([`VenueRules::size_unit`]), as it
/// leaves: `raw` (in the quote currency: the size the user gave in it, or the coin size times
/// the price read just before sending) rounded DOWN to the venue's step, never up. A result
/// under the smallest size the venue takes is refused naming that size (and its coin worth at
/// `px`, when the size was given in the coin); over the largest, refused naming it.
pub fn quote_size(venue: &str, r: &VenueRules, raw: Decimal, market: bool, px: Option<Decimal>) -> Result<Decimal, ConnectorError> {
    let sym = &r.instrument.native;
    let refuse = |m: String| Err(ConnectorError::invalid(m).with_code("venue_rule"));
    let step = r.lot_step.filter(|s| *s > Decimal::ZERO);
    let amount = match step {
        Some(s) => (raw / s).floor() * s,
        None => raw,
    }
    .normalize();
    let smallest = match (step, r.min_qty.filter(|m| *m > Decimal::ZERO)) {
        (Some(s), Some(m)) => Some(if m > s { (m / s).ceil() * s } else { s }),
        (s, m) => s.or(m),
    };
    if amount <= Decimal::ZERO || smallest.is_some_and(|m| amount < m) {
        let least = smallest.map(num).unwrap_or_else(|| "more".into());
        let coin = match (px.filter(|p| *p > Decimal::ZERO), smallest) {
            (Some(p), Some(m)) => format!(" (about {} in the coin at {})", num((m / p).round_dp(8)), num(p)),
            _ => String::new(),
        };
        let steps = step.map(|s| format!(", in steps of {}", num(s))).unwrap_or_default();
        return refuse(format!("{venue} sizes {sym} in the quote currency{steps}: {} rounds down to {}, under the smallest size it takes, {least}{coin}", num(raw), num(amount)));
    }
    let max = if market { r.market_max_qty.or(r.max_qty) } else { r.max_qty };
    if let Some(m) = max.filter(|m| amount > *m) {
        return refuse(format!("{venue} takes at most {} (quote currency) of {sym} in one order ({} given): split it", num(m), num(amount)));
    }
    Ok(amount)
}

/// Why a venue refused an order, in OTW's words (spec 8, `Rejected(reason)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The instrument's rules (tick, lot, size, value): the rules are read again.
    VenueRule,
    /// Balance or margin.
    Funds,
    /// Price outside the venue's band, or a trigger already crossed.
    Price,
    /// A reduce-only order that would add to the position.
    ReduceOnly,
    /// A post-only order that would cross.
    PostOnly,
    /// The instrument takes no orders now (halted, close-only, not open).
    MarketClosed,
    /// The venue does not know the order (spec 6.3 "never seen").
    NotFound,
    /// The order is already final (filled, cancelled): a cancel of it is reconciled, not an error.
    Final,
    /// Any other refusal, in the venue's words.
    Other,
}

/// The canonical kinds every documented order error maps to (spec 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Canon {
    Rejected(Refusal),
    /// The venue may have acted: looked up, never sent again.
    Unknown,
    RateLimited,
    Auth,
    Permission,
    InvalidRequest,
}

/// A venue's error, mapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenueError {
    pub canon: Canon,
    /// The venue's code, verbatim.
    pub code: Option<String>,
    pub message: String,
    /// How long to wait, from the venue's answer.
    pub retry_after_ms: Option<u64>,
    /// When the venue's limit resets, epoch milliseconds, where it states a moment rather than
    /// a wait (the trader turns it into a wait on the venue clock).
    pub retry_at_ms: Option<i64>,
    /// The code is in the venue's table. `false`: an answer nobody mapped, which alerts.
    pub mapped: bool,
}

impl VenueError {
    pub fn new(canon: Canon, code: Option<String>, message: impl Into<String>) -> Self {
        VenueError { canon, code, message: message.into(), retry_after_ms: None, retry_at_ms: None, mapped: true }
    }

    /// A code the venue's table does not list: unknown, and it alerts.
    pub fn unmapped(code: Option<String>, message: impl Into<String>) -> Self {
        VenueError { mapped: false, ..Self::new(Canon::Unknown, code, message) }
    }

    /// The error in the connector model's terms.
    pub fn to_connector(&self) -> ConnectorError {
        let kind = match self.canon {
            Canon::Rejected(Refusal::NotFound) => ErrorKind::NotFound,
            Canon::Rejected(_) | Canon::InvalidRequest => ErrorKind::InvalidRequest,
            Canon::Unknown => ErrorKind::Unknown,
            Canon::RateLimited => ErrorKind::RateLimited,
            Canon::Auth => ErrorKind::Auth,
            Canon::Permission => ErrorKind::Permission,
        };
        let mut e = ConnectorError::new(kind, self.message.clone());
        e.provider_code = self.code.clone();
        e.retry_after_ms = self.retry_after_ms;
        e
    }
}

impl std::fmt::Display for VenueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// One row of a venue's error table: its code, the canonical kind, and its words.
pub type ErrorRow = (&'static str, Canon, &'static str);

/// A venue's answer to a placing call: the venue's id and the client id it echoes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    /// Empty when the venue accepted the call without naming the order (OKX close position,
    /// Bitget one-way reduce-only close): the trader then finds it by its client id, and never
    /// sends it again ([`Ack::without_id`]).
    pub venue_id: String,
    pub client_id: Option<String>,
}

impl Ack {
    /// An accepting answer that names no order.
    pub fn without_id(client_id: Option<String>) -> Self {
        Ack { venue_id: String::new(), client_id }
    }

    /// Whether the answer names the order.
    pub fn has_id(&self) -> bool {
        !self.venue_id.is_empty()
    }
}

/// Which of a venue's id spaces an order lives in: its plain orders, or the conditional orders
/// it keeps on endpoints of their own (OKX algo, Bitget plan, Gate price orders, KuCoin stops).
/// A conditional order's id carries its kind ([`OrderSpace::tag`]), so a cancel, a lookup or a
/// change by id goes to the right endpoint ([`OrderSpace::of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderSpace {
    Plain,
    Conditional,
}

impl OrderSpace {
    /// The prefix a conditional order's id carries.
    pub const CONDITIONAL: &'static str = "cond:";

    /// The id OTW follows the order by: the venue's id, tagged when it is conditional.
    pub fn tag(self, venue_id: &str) -> String {
        match self {
            OrderSpace::Plain => venue_id.to_string(),
            OrderSpace::Conditional => format!("{}{venue_id}", Self::CONDITIONAL),
        }
    }

    /// The space an id OTW follows names, and the venue's own id in it.
    pub fn of(id: &str) -> (OrderSpace, &str) {
        match id.strip_prefix(Self::CONDITIONAL) {
            Some(rest) => (OrderSpace::Conditional, rest),
            None => (OrderSpace::Plain, id),
        }
    }
}

/// How an account margins one instrument or position, as the venue states it (never changed
/// by OTW).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarginKind {
    Cross,
    Isolated,
}

/// The margin an order is sent under, read from the venue: its kind, and the coin it is held
/// in where the venue names one per order (Bitget `marginCoin`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarginMode {
    pub kind: MarginKind,
    pub coin: Option<String>,
}

impl MarginMode {
    pub fn cross() -> Self {
        MarginMode { kind: MarginKind::Cross, coin: None }
    }

    pub fn isolated() -> Self {
        MarginMode { kind: MarginKind::Isolated, coin: None }
    }

    /// `cross` or `isolated`, as the leverage read answers it.
    pub fn word(&self) -> &'static str {
        match self.kind {
            MarginKind::Cross => "cross",
            MarginKind::Isolated => "isolated",
        }
    }
}

/// Each instrument's contract size, for a book that counts its sizes in contracts
/// ([`TradeMapper::counts_contracts`]): read by the trader from the venue's instrument list,
/// handed to the decoders ([`DecodeCtx`]) so they answer sizes in the base coin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContractSizes(HashMap<String, Decimal>);

impl ContractSizes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, symbol: &str, size: Decimal) {
        self.0.insert(symbol.to_ascii_uppercase(), size);
    }

    pub fn get(&self, symbol: &str) -> Option<Decimal> {
        self.0.get(&symbol.to_ascii_uppercase()).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `contracts` of `symbol` in the base coin. An instrument whose size is not known is an
    /// error naming it, never a guess.
    pub fn to_base(&self, venue: &str, symbol: &str, contracts: Decimal) -> Result<Decimal, ConnectorError> {
        let size = self.get(symbol).ok_or_else(|| ConnectorError::decode(format!("{venue}: the contract size of {symbol} is not known, so its sizes cannot be read")))?;
        Ok(contracts * size)
    }
}

/// `qty` in the base coin as a whole number of contracts of `size`, refused (with the nearest
/// sizes that are) when it is not one. Nothing is rounded.
pub fn whole_contracts(venue: &str, symbol: &str, qty: Decimal, size: Decimal) -> Result<Decimal, ConnectorError> {
    if size <= Decimal::ZERO {
        return Err(ConnectorError::invalid(format!("{venue}: {symbol} has no contract size above zero")));
    }
    let n = qty / size;
    if !n.fract().is_zero() {
        let (lo, hi) = (n.floor() * size, n.ceil() * size);
        return Err(ConnectorError::invalid(format!(
            "{venue} trades {symbol} in contracts of {}: {} is not a whole number of them ({} or {})",
            num(size),
            num(qty),
            num(lo),
            num(hi)
        ))
        .with_code("venue_rule"));
    }
    Ok(n.normalize())
}

/// Every name the venue gives each instrument in its answers, to the one OTW follows it by
/// ([`VenueRules::instrument`]'s native symbol): built by the trader from the instrument list
/// ([`VenueRules::aliases`]) for a mapper that asks for it ([`TradeMapper::uses_symbol_names`]).
/// Names compare without case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolNames(HashMap<String, String>);

impl SymbolNames {
    pub fn new() -> Self {
        Self::default()
    }

    /// `alias` names `native` (the native symbol names itself too).
    pub fn insert(&mut self, alias: &str, native: &str) {
        self.0.insert(alias.to_ascii_uppercase(), native.to_string());
        self.0.entry(native.to_ascii_uppercase()).or_insert_with(|| native.to_string());
    }

    /// The native symbol `name` stands for; `None` for a name the list does not give.
    pub fn native(&self, name: &str) -> Option<&str> {
        self.0.get(&name.to_ascii_uppercase()).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The table of every alias the rules state.
    pub fn of(rules: &[VenueRules]) -> Self {
        let mut t = SymbolNames::new();
        for r in rules {
            t.insert(&r.instrument.native, &r.instrument.native);
            for a in &r.aliases {
                t.insert(a, &r.instrument.native);
            }
        }
        t
    }
}

/// What an answer is read with besides its body: the call it answers (a venue that keeps
/// conditional orders apart reads their list differently), each instrument's contract size
/// for a book that counts in contracts, and the venue's other names for its instruments.
#[derive(Debug, Clone, Copy)]
pub struct DecodeCtx<'a> {
    /// The endpoint id of the request answered ([`HttpRequest::endpoint_id`]), or the private
    /// socket's path for a stream frame.
    pub endpoint: &'a str,
    pub sizes: &'a ContractSizes,
    /// The name table, for a mapper that asks for it ([`TradeMapper::uses_symbol_names`]);
    /// `None` otherwise, or before the instrument list was read.
    pub names: Option<&'a SymbolNames>,
    /// The answer's headers (lower-cased names), for a decoder that reads one (Kraken Futures
    /// `Next-Continuation-Token`); empty for a stream frame.
    pub headers: &'a [(String, String)],
}

impl<'a> DecodeCtx<'a> {
    pub fn new(endpoint: &'a str, sizes: &'a ContractSizes) -> Self {
        DecodeCtx { endpoint, sizes, names: None, headers: &[] }
    }

    /// The same, with the venue's name table.
    pub fn with_names(self, names: &'a SymbolNames) -> Self {
        DecodeCtx { names: Some(names), ..self }
    }

    /// The same, with the answer's headers.
    pub fn with_headers(self, headers: &'a [(String, String)]) -> Self {
        DecodeCtx { headers, ..self }
    }

    /// One header of the answer, by name (any case); `None` when it does not carry it.
    pub fn header(&self, name: &str) -> Option<&'a str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The native symbol a name in an answer stands for. A name the table does not give, or no
    /// table, is an error naming it, never a guess: the book is read again.
    pub fn native(&self, venue: &str, name: &str) -> Result<&'a str, ConnectorError> {
        self.names
            .and_then(|n| n.native(name))
            .ok_or_else(|| ConnectorError::decode(format!("{venue}: {name} is not a name of any instrument in the list read, so the answer cannot be read")))
    }
}

/// One more fee line of a fill, beside its main fee (Bybit EU `extraFees`: a withholding tax).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeLine {
    /// Positive = a cost, in `currency`.
    pub fee: Decimal,
    pub currency: String,
    /// The venue's word for what the fee is (`WHT`), verbatim.
    pub kind: String,
}

/// How a venue's cancel-all reaches every working order (with `cancel_all_native`, except
/// [`CancelAllWay::ByIds`], which needs no native call).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelAllWay {
    /// One call (or a few) per symbol with working orders ([`TradeMapper::cancel_all`]).
    BySymbol,
    /// Calls that cover the whole book (per product family or margin coin): `cancel_all` is
    /// handed no symbol.
    Book,
    /// A batch cancel by ids, at most `max` per call ([`TradeMapper::cancel_ids`]).
    ByIds { max: usize },
}

/// One row of a batch cancel's answer: the order and the venue's per-row refusal, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelRow {
    pub venue_id: String,
    pub error: Option<VenueError>,
}

/// The client id a change made as a new order is sent under (cancel and replace): the old one
/// with a counter tail (`-r1`, `-r2`...), in a separator the venue's rule takes, cut to its
/// length. Minted before I/O.
pub fn next_client_id(rule: &ClientIdRule, id: &str) -> String {
    let sep = ['-', '_', '.'].into_iter().find(|c| (rule.allowed)(*c));
    let marker = match sep {
        Some(c) => format!("{c}r"),
        None => "r".to_string(),
    };
    let (base, n) = match id.rsplit_once(marker.as_str()) {
        Some((b, n)) if !b.is_empty() && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => (b, n.parse::<u64>().unwrap_or(0)),
        _ => (id, 0),
    };
    let tail = format!("{marker}{}", n + 1);
    let keep = rule.max_len.saturating_sub(tail.len()).min(base.chars().count());
    let head: String = base.chars().take(keep).collect();
    format!("{head}{tail}")
}

/// An order as the venue reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenueOrder {
    pub venue_id: String,
    /// The client id it was sent under, in the venue's spelling; `None` for an order placed
    /// outside OTW without one.
    pub client_id: Option<String>,
    pub symbol: String,
    pub side: Mapped<Side>,
    /// The venue's type word, verbatim.
    pub order_type: String,
    pub status: Mapped<OrderStatus>,
    pub qty: Decimal,
    pub filled_qty: Decimal,
    pub limit: Option<Decimal>,
    pub stop: Option<Decimal>,
    pub avg_price: Option<Decimal>,
    pub reduce_only: bool,
    pub position: PositionSide,
    pub created_ms: Option<i64>,
    /// A conditional order still waiting for its trigger (Bybit `Untriggered`). Its status is
    /// `New`: no canonical state is invented for it.
    pub trigger_pending: bool,
    /// The client id of the entry this order is an attached exit of, where the venue says.
    pub parent_client_id: Option<String>,
    /// Which attached exit it is, where the venue says.
    pub exit: Option<ExitKind>,
    /// The venue id of the order this one became (Deribit: a trigger order fires as a new
    /// order under a new id). The trader follows it for the order's state, its fills and its
    /// cancel. `None` for every other order.
    pub successor_id: Option<String>,
}

/// A change to the take profit or stop loss guarding a position ([`TradeMapper::change_exit`]).
#[derive(Debug, Clone, Copy)]
pub struct ExitChange<'a> {
    pub symbol: &'a str,
    /// The position as the venue keys it (from the account's position mode).
    pub position: PositionSide,
    /// The side of the trade that opened the position (a one-way account's position side).
    pub entry_side: Side,
    pub kind: ExitKind,
    /// The new trigger price (`Some`), or remove the exit (`None`).
    pub price: Option<Decimal>,
    /// The exit of this kind working on the position now, for a venue that lists them
    /// ([`TradeMapper::position_exits`]); `None` when it lists none or there is none.
    pub current: Option<&'a VenueOrder>,
}

/// An exit attached to an entry at the venue (a take profit or a stop loss the venue itself
/// creates when the entry fills).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExitKind {
    TakeProfit,
    StopLoss,
}

impl ExitKind {
    /// The exit an attached leg asks for: a stop is the stop loss; a limit or a market-if-touched
    /// is the take profit. `None`: a leg the venue's attached exits cannot carry.
    pub fn of(o: &OrderRequest) -> Option<ExitKind> {
        match o.order_type {
            OrderType::Stop => Some(ExitKind::StopLoss),
            OrderType::Limit | OrderType::Mit => Some(ExitKind::TakeProfit),
            _ => None,
        }
    }

    /// Its trigger price on a leg: the stop for a stop loss and a market-if-touched, the limit
    /// for a limit take profit.
    pub fn price(o: &OrderRequest) -> Option<Decimal> {
        o.stop.or(o.limit).map(|p| p.value())
    }

    pub fn word(self) -> &'static str {
        match self {
            ExitKind::TakeProfit => "tp",
            ExitKind::StopLoss => "sl",
        }
    }

    pub fn from_word(w: &str) -> Option<ExitKind> {
        match w {
            "tp" => Some(ExitKind::TakeProfit),
            "sl" => Some(ExitKind::StopLoss),
            _ => None,
        }
    }
}

/// One execution, keyed by the venue's execution id (spec 2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenueFill {
    pub exec_id: String,
    pub venue_order_id: String,
    pub client_id: Option<String>,
    pub symbol: String,
    pub side: Mapped<Side>,
    pub qty: Decimal,
    pub price: Decimal,
    /// Positive = a cost, in `fee_currency`.
    pub fee: Decimal,
    pub fee_currency: String,
    pub at_ms: i64,
    /// An execution that moved a position without an order of the account behind it (ADL, a
    /// takeover liquidation, a delivery): never dropped, it says the position changed.
    pub position_only: bool,
    /// More fee lines charged on the same execution, beside `fee`.
    pub extra_fees: Vec<FeeLine>,
    /// The frame carried no fee amount (a KuCoin stream fill): `fee` is not known yet and is
    /// read from the venue's fills ([`TradeMapper::fills_since`]) by the execution id.
    pub fee_pending: bool,
}

impl VenueFill {
    /// Every fee line of the execution, the main one first: `(amount, currency)`.
    pub fn fees(&self) -> Vec<(Decimal, &str)> {
        std::iter::once((self.fee, self.fee_currency.as_str())).chain(self.extra_fees.iter().map(|l| (l.fee, l.currency.as_str()))).collect()
    }
}

/// One position as the venue reports it: size always positive, `side` which one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenuePosition {
    pub symbol: String,
    pub side: PositionSide,
    /// Long or short for a net position; `None` when it is flat.
    pub direction: Option<Side>,
    pub qty: Decimal,
    /// The leverage the account set on it, where the venue states one per position.
    pub leverage: Option<Decimal>,
    /// How it is margined, where the venue states it per position.
    pub margin: Option<MarginMode>,
}

/// What a private stream frame says (spec 7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateEvent {
    Order(VenueOrder),
    Fill(VenueFill),
    Position(VenuePosition),
    /// The venue's answer to a frame OTW sent (`auth`, `subscribe`, `ping`).
    Answer { op: String, ok: bool, message: String },
    /// An order changed and the frame does not carry it whole (Kraken executions carry only
    /// the fields that changed): the order is read again by its venue id. `symbol` where the
    /// frame names it (the venue's id lookup may need it).
    OrderChanged { symbol: Option<String>, venue_id: String },
    /// The account's position mode, pushed by the venue (MEXC `push.personal.position.mode`):
    /// on `symbol`, or account wide (`None`). `mode` `None`: it changed, read it again.
    PositionMode { symbol: Option<String>, mode: Option<PositionMode> },
    /// [`PrivateEvent::OrderChanged`] for a frame that names the order by the client id it was
    /// sent under only (venue spelling; MEXC spot without its order id): the order is read
    /// again by that client id.
    OrderChangedByClient { symbol: Option<String>, client_id: String },
    /// The venue announced maintenance (`on`, Bitfinex `info` 20060) or its end (Bitfinex
    /// 20061): while it is announced, a new order is refused with `message`, and a cancel goes
    /// out as ever. A reconnect of the stream clears it.
    Maintenance { on: bool, message: String },
}

/// How a venue's private stream is opened and kept (spec 7.1): where, what to subscribe, the
/// heartbeat. The login frame is the signer's ([`crate::Signer::ws_login`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateSocket {
    /// The base URL key (resolved by the driver, so a test points it at a mock socket).
    pub base: &'static str,
    pub path: String,
    /// The subscribe frame. Empty: nothing is subscribed (a venue that pushes everything
    /// after the login, Bitfinex, MEXC futures); with `more` empty too, the login alone opens
    /// the stream.
    pub subscribe: Vec<u8>,
    /// The heartbeat frame. Empty: OTW sends none (Coinbase documents no client message
    /// besides subscribe), and the socket is still dropped after two silent intervals.
    pub ping: Vec<u8>,
    /// How often the heartbeat is sent, as the venue recommends.
    pub ping_every_ms: u64,
    /// The whole URL, where the venue hands one out before each connection (KuCoin's token
    /// bootstrap, [`TradeMapper::private_socket_bootstrap`]): used as is instead of `base` and `path`.
    pub url: Option<String>,
    /// Further subscribe frames, each sent after `subscribe` and answered in turn (a venue
    /// that takes one channel per frame). Every subscribe frame goes through the signer's
    /// [`crate::Signer::sign_frame`] first (Gate signs each one).
    pub more: Vec<Vec<u8>>,
}

impl PrivateSocket {
    /// A socket at `base` + `path`, one subscribe frame, no URL handed out.
    pub fn new(base: &'static str, path: impl Into<String>, subscribe: Vec<u8>, ping: Vec<u8>, ping_every_ms: u64) -> Self {
        PrivateSocket { base, path: path.into(), subscribe, ping, ping_every_ms, url: None, more: Vec::new() }
    }

    /// Every subscribe frame, in the order they are sent (an empty one is not sent).
    pub fn subscribes(&self) -> Vec<&[u8]> {
        std::iter::once(self.subscribe.as_slice()).chain(self.more.iter().map(Vec::as_slice)).filter(|f| !f.is_empty()).collect()
    }
}

/// How an order is to be sent, beyond the order itself: decided by the trader from the account
/// and the venue (the position it acts on, the venue's spelling of its client id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placing {
    /// The client id, in the venue's spelling; empty for a dry run (sent without one).
    pub client_id: String,
    pub position: PositionSide,
    pub corr: CorrId,
    /// How the instrument is margined on this account, as the venue says
    /// ([`TradeMapper::symbol_margin`] or a position row): never chosen by OTW. `None`: not
    /// read, or not stated.
    pub margin: Option<MarginMode>,
    /// The instrument's contract size, for a book that counts in contracts
    /// ([`VenueRules::contract_size`]): what `place` divides the size by ([`Placing::contracts`]).
    pub contract_size: Option<Decimal>,
    /// For a market buy the venue sizes in the quote coin ([`TradeMapper::market_buy_in_quote`]):
    /// the amount to spend, the size times the reference price read just before sending,
    /// rounded down to the venue's quote step. On a book sized in the quote currency
    /// ([`VenueRules::size_unit`]): every order's size in it, rounded down to the venue's step
    /// ([`quote_size`]); the mapper sends this, not the order's `qty`.
    pub quote_amount: Option<Decimal>,
    /// The leverage the venue holds for the instrument (and side), read from it: never chosen
    /// by OTW. `None`: the venue holds none (no position, no read), and the order goes without
    /// one, the venue applying its own default.
    pub leverage: Option<Decimal>,
    /// The venue's clock when the order is built, epoch milliseconds (a deadline or a
    /// `valid_until` the mapper derives from it). 0 when not known.
    pub now_ms: i64,
}

impl Placing {
    pub fn new(client_id: impl Into<String>, position: PositionSide, corr: CorrId) -> Self {
        Placing { client_id: client_id.into(), position, corr, margin: None, contract_size: None, quote_amount: None, leverage: None, now_ms: 0 }
    }

    /// `qty` (base coin) as the whole number of contracts the venue is sent.
    pub fn contracts(&self, venue: &str, symbol: &str, qty: Decimal) -> Result<Decimal, ConnectorError> {
        let size = self.contract_size.ok_or_else(|| ConnectorError::invalid(format!("{venue}: the contract size of {symbol} was not read, so nothing was sent")))?;
        whole_contracts(venue, symbol, qty, size)
    }
}

/// A page of a list answer: the rows and the cursor of the next page, when there is one
/// ([`TradeMapper::with_cursor`]).
pub type Page<T> = (Vec<T>, Option<String>);

/// The client id an order was sent under, in the venue's spelling, and when the trader made it
/// (epoch milliseconds, the venue's clock): what a lookup and a cancel are handed
/// ([`TradeMapper::query_by_client_id_at`], [`TradeMapper::cancel_order`]). For a minted id
/// ([`TradeMapper::mints_client_ids`]) it is stored with the leg and read back after a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRef {
    pub id: String,
    /// `None`: not known (an order sent before a restart, its id not minted).
    pub at_ms: Option<i64>,
}

/// When a lookup by client id runs: the order's creation time where known, and the venue's
/// clock now (both epoch milliseconds), for a venue whose lookup needs a time window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookupAt {
    pub created_ms: Option<i64>,
    pub now_ms: i64,
}

/// The trading half of a venue adapter. Requests are built for an account's `settings` (which
/// host its key belongs to, which environment), signed by the driver.
pub trait TradeMapper: Mapper {
    /// The venue's name in refusals.
    fn venue_name(&self) -> &str {
        &self.capabilities().label
    }

    /// The client id rule (spec 2.1).
    fn client_id_rule(&self) -> ClientIdRule;

    /// OTW's id in the venue's spelling, reversibly ([`TradeMapper::otw_id`]). The default
    /// keeps it as it is, once checked against [`TradeMapper::client_id_rule`].
    fn client_id(&self, otw_id: &str) -> Result<String, ConnectorError> {
        self.client_id_rule().check(self.venue_name(), otw_id)?;
        Ok(otw_id.to_string())
    }

    /// The inverse of [`TradeMapper::client_id`].
    fn otw_id(&self, venue_client_id: &str) -> String {
        venue_client_id.to_string()
    }

    /// Whether this account's orders go to the venue's test environment (no money).
    fn is_sandbox(&self, _settings: &Settings) -> bool {
        false
    }

    // ── Requests ──

    /// Place one order. Every option the venue's caps do not declare is refused before this
    /// is called; a mapper refuses here what its words cannot say.
    fn place(&self, settings: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError>;

    /// Place an entry with its exits attached at the venue (`brackets` in the caps): one call.
    /// Each exit is a take profit or a stop loss ([`ExitKind::of`]); a venue refuses here what
    /// its attached exits cannot carry.
    fn place_with_exits(&self, _settings: &Settings, _entry: &OrderRequest, _exits: &[(ExitKind, Decimal)], _p: &Placing) -> Result<HttpRequest, ConnectorError> {
        Err(ConnectorError::unsupported(format!("{} takes no attached exits", self.venue_name())))
    }

    /// Set (`Some(price)`) or remove (`None`) an attached exit on the position it guards.
    fn change_exit(&self, _settings: &Settings, _x: &ExitChange<'_>, _corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Err(ConnectorError::unsupported(format!("{} takes no change to an attached exit", self.venue_name())))
    }

    /// The take profits and stop losses working on `symbol`'s positions, for a venue whose
    /// change to one names it (Bitget): read before [`TradeMapper::change_exit`], decoded by
    /// [`TradeMapper::decode_orders_in`], each row with its `exit` and `position`. `None` (the
    /// default): the venue changes an exit by the position alone.
    fn position_exits(&self, _settings: &Settings, _symbol: &str, _corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    /// Recently ended orders on `symbol`: where an attached exit that is no longer working is
    /// found.
    fn recent_orders(&self, _settings: &Settings, _symbol: &str, _corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(Vec::new())
    }

    /// Change a working order in place, for a venue that declares `amend`. `p` carries the
    /// order's client id and position, the call's correlation id and, on a book that counts in
    /// contracts, the instrument's contract size (what a new size is divided by).
    fn amend(&self, _settings: &Settings, _cur: &VenueOrder, _change: &OrderAmend, _p: &Placing) -> Result<HttpRequest, ConnectorError> {
        Err(ConnectorError::unsupported(format!("{} orders cannot be changed in place", self.venue_name())))
    }

    /// Cancel one order, by the venue's id.
    fn cancel(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError>;

    /// The calls that cancel every working order at once, for a venue that declares
    /// `cancel_all_native`: one per symbol named, or for the whole book when `symbols` is empty.
    fn cancel_all(&self, _settings: &Settings, _symbols: &[String], _corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Err(ConnectorError::unsupported(format!("{} has no cancel-all call", self.venue_name())))
    }

    /// The calls that look an order up by its client id, asked in turn until one answers it
    /// (a venue that keeps recent orders apart from its history).
    fn query_by_client_id(&self, settings: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError>;

    /// The same, by the venue's own id: what a cancel the venue refused is reconciled with.
    fn query_by_venue_id(&self, settings: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError>;

    /// The working orders, first page of each part of the book the venue lists apart (one
    /// request where it lists the whole book at once): on `symbol`, or every symbol.
    fn open_orders(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError>;

    /// Executions on `symbol` over `[from_ms, to_ms]`, first page. The trader walks a longer
    /// period in windows of [`TradeMapper::fills_window_ms`].
    fn fills_since(&self, settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError>;

    /// The widest period one fills call answers, where the venue bounds it (Bybit: seven days).
    fn fills_window_ms(&self) -> Option<i64> {
        None
    }

    /// The instrument list with its trading rules, first page: `symbol` alone, or every one.
    fn instrument_rules(&self, settings: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError>;

    /// The calls that list `symbol` in the states the default list leaves out (a venue whose
    /// list shows trading instruments only unless asked by status): what tells an instrument
    /// that is not trading now from one the venue does not know.
    fn instrument_rules_any_status(&self, _settings: &Settings, _symbol: &str, _corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(Vec::new())
    }

    /// An order the venue's words cannot carry as asked, refused before I/O with the fix: a
    /// time in force the venue would silently replace on this order type, a combination its
    /// docs refuse. Checked after the declared capabilities.
    fn check_order(&self, _o: &OrderRequest) -> Result<(), ConnectorError> {
        Ok(())
    }

    /// Whether a cancel-all answer that carries no error code still says the call failed.
    fn decode_cancel_all(&self, _body: &[u8]) -> Result<(), ConnectorError> {
        Ok(())
    }

    /// The next page of a list request, by the cursor its answer handed back. The default
    /// sends the same request with `cursor` set.
    fn with_cursor(&self, req: &HttpRequest, cursor: &str) -> HttpRequest {
        let mut next = req.clone();
        next.query.retain(|(k, _)| k != "cursor");
        next.query.push(("cursor".into(), cursor.to_string()));
        next
    }

    /// The positions on `symbol` (both sides in hedge mode, the empty one included where the
    /// venue lists it). `None`: a book with no positions (spot).
    fn positions(&self, _settings: &Settings, _symbol: &str, _corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    /// [`TradeMapper::positions`] knowing the position mode held for the instrument or the
    /// account (`None`: none held yet), for a venue whose hedge-mode positions are read by a
    /// call of their own (Gate.io `dual_comp`). By default the mode changes nothing.
    fn positions_in_mode(&self, settings: &Settings, symbol: &str, _mode: Option<PositionMode>, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        self.positions(settings, symbol, corr)
    }

    /// Every held position of the account, first page of each part of the book: what the
    /// position mode of each instrument is read from once, at connect. Empty where the book
    /// holds none.
    fn all_positions(&self, _settings: &Settings, _corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(Vec::new())
    }

    /// Whether a refusal of an order may mean the position mode OTW holds for the instrument
    /// is out of date (the account switched it): the mode is read again before the next order.
    /// The refused order stays refused.
    fn invalidates_position_mode(&self, _e: &VenueError) -> bool {
        false
    }

    /// The account's margin mode ("cross", "isolated", ...), where the venue states it account
    /// wide: read only, once. `None`: not stated.
    fn margin_mode(&self, _settings: &Settings, _corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    fn decode_margin_mode(&self, _body: &[u8]) -> Result<Option<String>, ConnectorError> {
        Ok(None)
    }

    /// The venue's check of `o` without placing it (`dry_run` in the caps). `Ok(None)`: the
    /// venue's check does not cover this order (a conditional order on Bybit's pre-check). The
    /// trader hands it an empty client id: a check never carries the live order's id, so the
    /// venue never sees that id before the order itself (a duplicate-id refusal).
    fn dry_run(&self, _settings: &Settings, _o: &OrderRequest, _p: &Placing) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    // ── Answers ──

    /// The error a frame carries, mapped; `None` for a success. A timeout, a dropped socket
    /// and a 5xx are [`Canon::Unknown`]; a code the table does not list is unmapped.
    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError>;

    fn decode_ack(&self, body: &[u8]) -> Result<Ack, ConnectorError>;

    fn decode_orders(&self, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError>;

    fn decode_fills(&self, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError>;

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError>;

    fn decode_positions(&self, _body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        Ok(Vec::new())
    }

    /// What the dry run answered, in one line.
    fn decode_dry_run(&self, _body: &[u8]) -> Result<String, ConnectorError> {
        Ok("accepted".into())
    }

    /// A venue status word to the canonical state: exhaustive, an unknown word stays unknown.
    fn status_of(&self, word: &str) -> Mapped<OrderStatus>;

    /// The account's private stream, where the venue pushes orders and fills and this account's
    /// host has one. `None`: reconciled by polling only.
    fn private_socket(&self, _settings: &Settings) -> Option<PrivateSocket> {
        None
    }

    /// A private stream frame, where the venue pushes orders and fills. Empty by default: the
    /// account is then reconciled by polling only.
    fn decode_private(&self, _payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        Ok(Vec::new())
    }

    /// The frame's sequence number, where the venue numbers the private socket's frames one
    /// after the other on each connection (Coinbase `sequence_num`): a jump of more than one is
    /// a dropped frame, and the book is read again. `None` (the default): not numbered.
    fn private_sequence(&self, _payload: &[u8]) -> Option<u64> {
        None
    }

    /// Whether a fill whose row names no fee currency is charged in the instrument's quote
    /// currency, as the venue's doc says (Coinbase Advanced Trade): the trader then names it
    /// from the instrument list. `false` (the default): an unnamed currency stays unnamed.
    fn fees_in_quote(&self) -> bool {
        false
    }

    // ── Per-symbol margin and account-wide position mode ──

    /// The call that reads how `symbol` is margined on this account (cross or isolated, and the
    /// margin coin where orders name one), for a venue whose orders carry it. Read once per
    /// instrument and held, like the position mode; handed to `place` as [`Placing::margin`].
    /// A position row that states its margin ([`VenuePosition::margin`]) is held too. `None`:
    /// not read here.
    fn symbol_margin(&self, _settings: &Settings, _symbol: &str, _corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    fn decode_symbol_margin(&self, _symbol: &str, _body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        Ok(None)
    }

    /// Whether each side of a hedge-mode instrument is margined on its own (OKX: a long and a
    /// short each carry their `mgnMode`): the margin is then held per side and read with
    /// [`TradeMapper::decode_symbol_margin_on`]. By default one margin per instrument.
    fn margin_per_side(&self) -> bool {
        false
    }

    /// [`TradeMapper::decode_symbol_margin`] for the position side an order acts on, where
    /// [`TradeMapper::margin_per_side`]. By default the side changes nothing.
    fn decode_symbol_margin_on(&self, symbol: &str, _side: PositionSide, body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        self.decode_symbol_margin(symbol, body)
    }

    /// Whether a refusal of an order may mean the margin held for the instrument is out of
    /// date: it is read again before the next order. The refused order stays refused.
    fn invalidates_margin_mode(&self, _e: &VenueError) -> bool {
        false
    }

    /// Whether a refusal of an order may mean the leverage held for the instrument is out of
    /// date (MEXC 2021, 7004: the order's leverage is not the position's): the held value is
    /// dropped and read again before the next order. The refused order stays refused.
    fn invalidates_leverage(&self, _e: &VenueError) -> bool {
        false
    }

    /// The call that reads the account's position mode at account level (OKX `posMode`,
    /// Bitget, KuCoin): answered with no position open. Held per account, forgotten on a
    /// refusal [`TradeMapper::invalidates_position_mode`] names. `None`: the mode comes from
    /// the position rows, as on Bybit.
    fn account_position_mode(&self, _settings: &Settings, _corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    fn decode_account_position_mode(&self, _body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        Ok(None)
    }

    // ── Answers read with their context ──

    /// Whether this book counts sizes in contracts (Gate futures, KuCoin Futures): the trader
    /// then reads every instrument's [`VenueRules::contract_size`] and hands them to the
    /// decoders below ([`DecodeCtx::sizes`]) and to `place` ([`Placing::contract_size`]).
    fn counts_contracts(&self) -> bool {
        false
    }

    /// [`TradeMapper::decode_ack`], knowing the call answered.
    fn decode_ack_in(&self, _ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Ack, ConnectorError> {
        self.decode_ack(body)
    }

    /// [`TradeMapper::decode_orders`], knowing the call answered (a conditional order list
    /// tags its ids, [`OrderSpace`]) and the contract sizes.
    fn decode_orders_in(&self, _ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        self.decode_orders(body)
    }

    fn decode_fills_in(&self, _ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        self.decode_fills(body)
    }

    fn decode_positions_in(&self, _ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        self.decode_positions(body)
    }

    /// [`TradeMapper::decode_private`], knowing the socket (its path) and the contract sizes.
    fn decode_private_in(&self, _ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        self.decode_private(payload)
    }

    // ── Private socket variants ──

    /// Every private socket the account follows (OKX: `private` and `business`). The default
    /// is [`TradeMapper::private_socket`] alone.
    fn private_sockets(&self, settings: &Settings) -> Vec<PrivateSocket> {
        self.private_socket(settings).into_iter().collect()
    }

    /// A signed call made before each connection of `socket`, whose answer completes it (KuCoin:
    /// a token, a URL handed out and the ping interval). `None`: connected as declared.
    fn private_socket_bootstrap(&self, _settings: &Settings, _socket: &PrivateSocket, _corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    /// The socket completed by the bootstrap's answer ([`PrivateSocket::url`], its ping, its
    /// subscribe frames).
    fn decode_private_socket_bootstrap(&self, socket: PrivateSocket, _body: &[u8]) -> Result<PrivateSocket, ConnectorError> {
        Ok(socket)
    }

    // ── Change as cancel and replace ──

    /// Whether a change to a working order is made as a new order under a new client id, with a
    /// new venue id (Bitget, KuCoin spot), where the venue has no change in place. The trader
    /// then mints the new client id before I/O ([`next_client_id`]) and calls
    /// [`TradeMapper::amend_replace`] instead of [`TradeMapper::amend`].
    fn amend_replaces(&self) -> bool {
        false
    }

    /// The call that replaces `cur` with `change` applied, under `new_client_id` (venue
    /// spelling).
    fn amend_replace(&self, _settings: &Settings, _cur: &VenueOrder, _change: &OrderAmend, _new_client_id: &str, _corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Err(ConnectorError::unsupported(format!("{} orders cannot be changed", self.venue_name())))
    }

    /// The replacing order's answer: its new venue id (empty when the venue names none: it is
    /// then found by its new client id).
    fn decode_replace_ack(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Ack, ConnectorError> {
        self.decode_ack_in(ctx, body)
    }

    // ── Cancel-all variants ──

    /// How cancel-all reaches every working order ([`CancelAllWay`]).
    fn cancel_all_way(&self) -> CancelAllWay {
        CancelAllWay::BySymbol
    }

    /// One batch cancel of `orders` (at most the `max` of [`CancelAllWay::ByIds`]).
    fn cancel_ids(&self, _settings: &Settings, _orders: &[VenueOrder], _corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Err(ConnectorError::unsupported(format!("{} has no batch cancel", self.venue_name())))
    }

    /// A cancel-all or batch answer's rows: which order each row is and the venue's per-row
    /// refusal (OKX `sCode`, Gate `succeeded`). Empty when the answer carries none. What a
    /// failed row leaves working is cancelled one by one after.
    fn decode_cancel_rows(&self, _ctx: &DecodeCtx<'_>, _body: &[u8]) -> Result<Vec<CancelRow>, ConnectorError> {
        Ok(Vec::new())
    }

    // ── Quote-sized market buy ──

    /// Whether the venue takes `o` as an amount to spend in the quote coin rather than a size
    /// in the base coin (a spot market buy at Bitget and Gate). The trader then reads the
    /// reference price just before sending ([`TradeMapper::reference_price`]) and hands `place`
    /// the amount ([`Placing::quote_amount`]); the filled size may differ a little from the one
    /// asked, and the answer says so.
    fn market_buy_in_quote(&self, _o: &OrderRequest) -> bool {
        false
    }

    /// The read of the price a quote-sized buy is valued at, the one the venue's doc points to
    /// (best ask, last price). `None`: the ticket's reference (the chart's last) is used, and a
    /// ticket without one is refused.
    fn reference_price(&self, _settings: &Settings, _symbol: &str, _side: Side, _corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    fn decode_reference_price(&self, _body: &[u8], _side: Side) -> Result<Decimal, ConnectorError> {
        Err(ConnectorError::unsupported(format!("{} states no reference price", self.venue_name())))
    }

    // ── Minted client ids, lookups with a time anchor ──

    /// Whether the venue's client id is minted by the trader rather than spelled from OTW's
    /// key (Bitfinex: a 45-bit integer with its date). The trader then mints one per order
    /// before I/O ([`TradeMapper::mint_client_id`]), stores it with the leg, hands it (and its
    /// time) to the lookup and the cancel, and maps the stream's orders back to OTW's key
    /// through what it stored. [`TradeMapper::client_id`] and [`TradeMapper::otw_id`] are then
    /// not used.
    fn mints_client_ids(&self) -> bool {
        false
    }

    /// The venue's client id for an order the trader mints at `stamp_ms` (epoch milliseconds,
    /// the venue's clock, strictly increasing across the process's mints).
    fn mint_client_id(&self, _stamp_ms: i64) -> Result<String, ConnectorError> {
        Err(ConnectorError::unsupported(format!("{} spells its client ids from OpenTraderWorld's", self.venue_name())))
    }

    /// [`TradeMapper::query_by_client_id`] knowing when the order was made and the clock now
    /// (a lookup that needs a time window: Coinbase, Bitfinex history, Deribit). By default the
    /// time changes nothing.
    fn query_by_client_id_at(&self, settings: &Settings, symbol: &str, client_id: &str, _at: &LookupAt, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        self.query_by_client_id(settings, symbol, client_id, corr)
    }

    /// How many pages of each lookup call the trader reads, following the cursor
    /// ([`TradeMapper::with_cursor`]), before it says the order was not found there (a venue
    /// that can only list and match, newest first). 1 by default: the first page.
    fn lookup_pages(&self) -> usize {
        1
    }

    /// [`TradeMapper::cancel`] knowing the order's client id and its time, where the trader
    /// holds them (a venue that cancels by client id and date). By default the venue's id.
    fn cancel_order(&self, settings: &Settings, symbol: &str, venue_id: &str, _client: Option<&ClientRef>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        self.cancel(settings, symbol, venue_id, corr)
    }

    /// The fills calls for `[from_ms, to_ms]`, knowing the clock now (Deribit keeps recent and
    /// older fills in two stores, chosen by age): each one's pages are read and merged, a
    /// duplicate execution dropped. By default [`TradeMapper::fills_since`] alone.
    fn fills_requests(&self, settings: &Settings, symbol: &str, from_ms: i64, to_ms: i64, _now_ms: i64, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![self.fills_since(settings, symbol, from_ms, to_ms, corr)?])
    }

    // ── Leverage held by the venue ──

    /// The call that reads the leverage the venue holds for `symbol` on this account (MEXC
    /// `position/leverage`), read before each order that may need it and handed to `place` as
    /// [`Placing::leverage`]. `None` (the default): the leverage a position row or stream frame
    /// states is used, and none when nothing states one.
    fn symbol_leverage(&self, _settings: &Settings, _symbol: &str, _corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(None)
    }

    /// The leverage [`TradeMapper::symbol_leverage`] answers for the side an order acts on.
    /// `None`: the venue holds none (never a default invented here).
    fn decode_symbol_leverage(&self, _symbol: &str, _side: PositionSide, _body: &[u8]) -> Result<Option<Decimal>, ConnectorError> {
        Ok(None)
    }

    // ── Name tables, socket keepalive ──

    /// Whether the decoders read the venue's other names for its instruments (Kraken: altname,
    /// internal key, socket name): the trader then reads the whole instrument list once a day
    /// and hands its table ([`SymbolNames`], from [`VenueRules::aliases`]) in every
    /// [`DecodeCtx`].
    fn uses_symbol_names(&self) -> bool {
        false
    }

    /// A signed call that keeps `socket` (as connected, completed by its bootstrap) alive,
    /// and how often it is made, in milliseconds (MEXC spot: the listen key extended
    /// every 30 minutes). `None` (the default): nothing but the heartbeat.
    fn private_socket_keepalive(&self, _settings: &Settings, _socket: &PrivateSocket, _corr: CorrId) -> Result<Option<(HttpRequest, u64)>, ConnectorError> {
        Ok(None)
    }

    /// The venue's error table, for the docs check and the conformance runner.
    fn error_table(&self) -> &'static [ErrorRow];
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_model::{OrderOpts, Price, Qty, TimeInForce};

    fn rules() -> VenueRules {
        VenueRules {
            tick: Some(Decimal::new(1, 1)),
            lot_step: Some(Decimal::new(1, 3)),
            min_qty: Some(Decimal::new(1, 3)),
            max_qty: Some(Decimal::from(100)),
            market_max_qty: Some(Decimal::from(50)),
            min_notional: Some(Decimal::from(5)),
            ..VenueRules::new(InstrumentId::new("X", "BTCUSDT"))
        }
    }

    fn order(t: OrderType, qty: &str, limit: Option<&str>) -> OrderRequest {
        OrderRequest {
            client_order_id: "k".into(),
            instrument: InstrumentId::new("X", "BTCUSDT"),
            side: Side::Buy,
            order_type: t,
            qty: Qty::new(qty.parse().unwrap()),
            limit: limit.map(|l| Price::new(l.parse().unwrap())),
            stop: None,
            tif: TimeInForce::Gtc,
            parent: None,
            oca_group: None,
            account: None,
            opts: OrderOpts::default(),
        }
    }

    #[test]
    fn rules_name_the_rule_and_the_nearest_values_and_adjust_nothing() {
        // T11
        let r = rules();
        assert!(check_rules("V", &r, &order(OrderType::Limit, "0.01", Some("30000.1")), None).is_ok());
        let e = check_rules("V", &r, &order(OrderType::Limit, "0.01", Some("30000.15")), None).unwrap_err();
        assert!(e.message.contains("30000.1 or 30000.2"), "{}", e.message);
        assert_eq!(e.provider_code.as_deref(), Some("venue_rule"));
        let e = check_rules("V", &r, &order(OrderType::Limit, "0.0105", Some("30000")), None).unwrap_err();
        assert!(e.message.contains("0.01 or 0.011"), "{}", e.message);
        let e = check_rules("V", &r, &order(OrderType::Limit, "0.0005", Some("30000")), None).unwrap_err();
        assert!(e.message.contains("at least 0.001"), "{}", e.message);
        let e = check_rules("V", &r, &order(OrderType::Limit, "101", Some("30000")), None).unwrap_err();
        assert!(e.message.contains("at most 100"), "{}", e.message);
        let e = check_rules("V", &r, &order(OrderType::Market, "60", None), None).unwrap_err();
        assert!(e.message.contains("at most 50") && e.message.contains("market"), "{}", e.message);
        let e = check_rules("V", &r, &order(OrderType::Limit, "0.001", Some("1000")), None).unwrap_err();
        assert!(e.message.contains("at least 5") && e.message.contains("0.005 BTCUSDT or more"), "{}", e.message);
        // A market order is valued at the reference when there is one, else left to the venue.
        assert!(check_rules("V", &r, &order(OrderType::Market, "0.001", None), None).is_ok());
        assert!(check_rules("V", &r, &order(OrderType::Market, "0.001", None), Some(Decimal::from(1000))).is_err());
    }

    #[test]
    fn a_closed_or_close_only_instrument_refuses_new_risk() {
        // T18
        let mut r = rules();
        r.status = TradingStatus::Closed("PreLaunch".into());
        let e = check_rules("V", &r, &order(OrderType::Limit, "0.01", Some("30000")), None).unwrap_err();
        assert!(e.message.contains("PreLaunch"), "{}", e.message);
        r.status = TradingStatus::CloseOnly;
        assert!(check_rules("V", &r, &order(OrderType::Limit, "0.01", Some("30000")), None).is_err());
        let mut reduce = order(OrderType::Limit, "0.01", Some("30000"));
        reduce.opts.reduce_only = true;
        assert!(check_rules("V", &r, &reduce, None).is_ok());
    }

    #[test]
    fn a_client_id_outside_the_venues_rule_is_refused_with_it() {
        let rule = ClientIdRule::alnum_dash_underscore(36);
        assert!(rule.check("V", "otw-1_a").is_ok());
        assert!(rule.check("V", "a:b").unwrap_err().message.contains("letters, digits, - and _"));
        assert!(rule.check("V", &"a".repeat(37)).is_err());
        assert!(rule.check("V", "").is_err());
    }

    #[test]
    fn position_mode_comes_from_the_rows_and_picks_the_side() {
        let row = |side| VenuePosition { symbol: "S".into(), side, direction: None, qty: Decimal::ZERO, leverage: None, margin: None };
        assert_eq!(PositionMode::of(&[row(PositionSide::Net)]), Some(PositionMode::OneWay));
        assert_eq!(PositionMode::of(&[row(PositionSide::Long), row(PositionSide::Short)]), Some(PositionMode::Hedge));
        assert_eq!(PositionMode::of(&[]), None);
        assert_eq!(PositionMode::of(&[row(PositionSide::Net), row(PositionSide::Long)]), None);
        assert_eq!(PositionMode::Hedge.side_for(Side::Sell, true), PositionSide::Long);
        assert_eq!(PositionMode::Hedge.side_for(Side::Sell, false), PositionSide::Short);
        assert_eq!(PositionMode::OneWay.side_for(Side::Buy, false), PositionSide::Net);
    }

    #[test]
    fn a_conditional_id_carries_its_kind_both_ways() {
        assert_eq!(OrderSpace::Conditional.tag("77"), "cond:77");
        assert_eq!(OrderSpace::Plain.tag("77"), "77");
        assert_eq!(OrderSpace::of("cond:77"), (OrderSpace::Conditional, "77"));
        assert_eq!(OrderSpace::of("77"), (OrderSpace::Plain, "77"));
        assert!(!Ack::without_id(Some("k".into())).has_id());
    }

    #[test]
    fn a_replacing_client_id_counts_up_within_the_venues_rule() {
        let rule = ClientIdRule::alnum_dash_underscore(12);
        assert_eq!(next_client_id(&rule, "k1"), "k1-r1");
        assert_eq!(next_client_id(&rule, "k1-r1"), "k1-r2");
        assert_eq!(next_client_id(&rule, "k1-r9"), "k1-r10");
        let long = next_client_id(&rule, "abcdefghijklmnop");
        assert_eq!(long, "abcdefghi-r1");
        assert!(rule.check("V", &long).is_ok());
        let alnum = ClientIdRule { max_len: 10, charset: "letters and digits", allowed: |c| c.is_ascii_alphanumeric() };
        let n = next_client_id(&alnum, "k1");
        assert!(alnum.check("V", &n).is_ok(), "{n}");
    }

    #[test]
    fn contracts_are_whole_or_refused_with_the_nearest_and_read_back_in_the_base_coin() {
        let size = Decimal::new(1, 2);
        assert_eq!(whole_contracts("V", "BTC", Decimal::new(5, 2), size).unwrap(), Decimal::from(5));
        let e = whole_contracts("V", "BTC", Decimal::new(55, 3), size).unwrap_err();
        assert!(e.message.contains("0.05 or 0.06"), "{}", e.message);
        let mut sizes = ContractSizes::new();
        sizes.insert("btc", size);
        assert_eq!(sizes.to_base("V", "BTC", Decimal::from(7)).unwrap(), Decimal::new(7, 2));
        assert!(sizes.to_base("V", "ETH", Decimal::ONE).is_err(), "an unknown size is never guessed");
        let p = Placing { contract_size: Some(size), ..Placing::new("k", PositionSide::Net, CorrId(1)) };
        assert_eq!(p.contracts("V", "BTC", Decimal::new(3, 2)).unwrap(), Decimal::from(3));
        assert!(Placing::new("k", PositionSide::Net, CorrId(1)).contracts("V", "BTC", Decimal::ONE).is_err());
    }

    #[test]
    fn a_socket_sends_every_subscribe_frame_in_turn_and_a_fill_lists_every_fee() {
        let mut s = PrivateSocket::new("ws", "/p", b"a".to_vec(), b"ping".to_vec(), 1000);
        s.more.push(b"b".to_vec());
        assert_eq!(s.subscribes(), vec![b"a".as_slice(), b"b".as_slice()]);
        let f = VenueFill {
            exec_id: "x".into(),
            venue_order_id: "v".into(),
            client_id: None,
            symbol: "S".into(),
            side: Mapped::Known(Side::Buy),
            qty: Decimal::ONE,
            price: Decimal::ONE,
            fee: Decimal::new(3, 1),
            fee_currency: "USDT".into(),
            at_ms: 0,
            position_only: false,
            extra_fees: vec![FeeLine { fee: Decimal::new(5, 2), currency: "EUR".into(), kind: "WHT".into() }],
            fee_pending: false,
        };
        assert_eq!(f.fees(), vec![(Decimal::new(3, 1), "USDT"), (Decimal::new(5, 2), "EUR")]);
    }

    #[test]
    fn a_partial_window_refuses_what_it_does_not_take_in_the_venues_words() {
        let mut r = rules();
        let limit = order(OrderType::Limit, "0.01", Some("30000"));
        let market = order(OrderType::Market, "0.01", None);
        r.status = TradingStatus::LimitOnly("limit_only".into());
        assert!(check_rules("V", &r, &limit, None).is_ok());
        assert!(check_rules("V", &r, &market, None).unwrap_err().message.contains("limit_only"));
        r.status = TradingStatus::PostOnly("post_only".into());
        assert!(check_rules("V", &r, &limit, None).unwrap_err().message.contains("post_only"));
        let mut post = limit.clone();
        post.opts.post_only = true;
        assert!(check_rules("V", &r, &post, None).is_ok());
        r.status = TradingStatus::BuyOnly("2".into());
        assert!(check_rules("V", &r, &limit, None).is_ok());
        let sell = OrderRequest { side: Side::Sell, ..limit.clone() };
        assert!(check_rules("V", &r, &sell, None).unwrap_err().message.contains("only buys"));
        r.status = TradingStatus::SellOnly("3".into());
        assert!(check_rules("V", &r, &sell, None).is_ok());
        assert!(check_rules("V", &r, &limit, None).unwrap_err().message.contains("only sells"));
        assert!(!r.status.held() && TradingStatus::CloseOnly.held() && TradingStatus::Trading.held());
    }

    #[test]
    fn a_quote_sized_book_rounds_down_and_names_the_smallest_size() {
        let r = VenueRules { lot_step: Some(Decimal::TEN), min_qty: Some(Decimal::from(25)), max_qty: Some(Decimal::from(1000)), size_unit: SizeUnit::Quote, ..VenueRules::new(InstrumentId::new("X", "BTC-PERPETUAL")) };
        assert_eq!(quote_size("V", &r, Decimal::from(39), false, None).unwrap(), Decimal::from(30));
        let e = quote_size("V", &r, Decimal::from(29), false, Some(Decimal::from(50_000))).unwrap_err();
        assert!(e.message.contains("29 rounds down to 20") && e.message.contains("smallest size it takes, 30") && e.message.contains("0.0006 in the coin"), "{}", e.message);
        assert!(quote_size("V", &r, Decimal::from(1015), false, None).unwrap_err().message.contains("at most 1000"));
        // A coin size is never checked against the quote step before it is converted.
        let coin = order(OrderType::Limit, "0.00123", Some("30000"));
        assert!(check_rules("V", &r, &coin, None).is_ok());
        let mut base = rules();
        base.size_unit = SizeUnit::Base;
        let mut usd = coin;
        usd.opts.size_unit = SizeUnit::Quote;
        assert!(check_rules("V", &base, &usd, None).unwrap_err().message.contains("base coin"));
    }

    #[test]
    fn the_name_table_answers_every_alias_and_guesses_none() {
        let r = VenueRules { aliases: vec!["XBTUSD".into(), "XXBTZUSD".into(), "BTC/USD".into()], ..VenueRules::new(InstrumentId::new("X", "BTCUSD")) };
        let t = SymbolNames::of(&[r]);
        assert_eq!((t.native("xbtusd"), t.native("BTC/USD"), t.native("BTCUSD")), (Some("BTCUSD"), Some("BTCUSD"), Some("BTCUSD")));
        let sizes = ContractSizes::new();
        let ctx = DecodeCtx::new("x", &sizes).with_names(&t);
        assert_eq!(ctx.native("V", "XXBTZUSD").unwrap(), "BTCUSD");
        assert!(ctx.native("V", "DOGEUSD").is_err());
        assert!(DecodeCtx::new("x", &sizes).native("V", "XBTUSD").is_err(), "no table, no guess");
    }

    #[test]
    fn a_socket_with_nothing_to_subscribe_sends_no_frame() {
        let s = PrivateSocket::new("ws", "/p", Vec::new(), Vec::new(), 1000);
        assert!(s.subscribes().is_empty());
        let mut more = s.clone();
        more.more.push(b"b".to_vec());
        assert_eq!(more.subscribes(), vec![b"b".as_slice()]);
    }

    #[test]
    fn rules_soundness_catches_inverted_bounds() {
        let mut r = rules();
        assert!(r.sound().is_ok());
        r.min_qty = Some(Decimal::from(200));
        assert!(r.sound().is_err());
        let mut z = rules();
        z.tick = Some(Decimal::ZERO);
        assert!(z.sound().is_err());
    }
}
