//! The paper broker: a broker account run by OpenTraderWorld instead of a venue.
//!
//! It is connected like any broker (Settings, Brokers, or the chart's broker button) and
//! answers the same calls, so the chart, the order ticket, the positions and orders tabs, the
//! audit log and the journal's broker import treat it as one. The differences are where the
//! money and the matching live:
//!
//! - **Cash is the user's to move.** A deposit, a withdrawal or a reset (`paper_api`), in the
//!   account's one currency. Cash moves after that by what fills realize and cost.
//! - **Orders fill on the chart's live price.** A ticket names the chart's provider and
//!   connector; the order rests here and [`engine`] fills it on that feed with the rules of
//!   [`matching`]. No live feed for the instrument, no paper order: it is refused at placement.
//! - **Instrument facts are stated, never guessed.** The currency and the point value come from
//!   the instrument's facts (the chart's instrument info); one that has none is refused with
//!   the fix. A trade in another currency is converted at the journal's FX rate of the day.
//! - **A future ties up its margin.** The initial margin per contract is the user's to state
//!   in the instrument settings (no source publishes current ones for free), or the contract
//!   counts at its full value by their choice. Neither stated, an order that would open or
//!   grow the position is refused with `MarginMissing`, which the chart answers with both.
//!   Everything else ties up its value over the account's leverage.
//! - **No real-money guards.** No password to turn trading on, no trading lock: there is no
//!   money at risk. Every order call is still written to the audit log.
//!
//! A position is the fold of the account's fills on an instrument (`otw_store::paper_broker`),
//! so the book, the cash and the journal cannot disagree. A day order lasts until 00:00 UTC.
//!
//! Prices, quantities, fees, rates and money are [`Decimal`] from the ticket to the ledger and
//! back, as a venue books them. A float is read once where it comes in (a feed's print, the
//! instrument info, a journal FX rate) and written once where it goes out (the rows every
//! broker reports the same way), never decided on.

#![deny(clippy::float_arithmetic)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use otw_connector_model::{Decimal, OrderStatus, OrderType, Side, TifKind, TimeInForce};
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use tokio::sync::broadcast;
use uuid::Uuid;

use otw_store::brokers::{self as accounts, BrokerRow};
use otw_store::connectors as conn_store;
use otw_store::paper_broker::{self as store, Fill, Held, NewOrder, Order};

use super::{AccountEvent, Broker, BrokerCapability, ConfigField, ExecQuery, Execution, Lookup, OpenOrder, OrderTicket, Placed, Position};

pub mod engine;
pub mod matching;
#[cfg(test)]
mod scenario_tests;

/// The broker id on `broker_accounts.broker`.
pub const ID: &str = "paper";

/// The module a paper order borrows its price feed under: it is placed from a chart.
const FEED_MODULE: &str = "histviz";

static FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "currency",
        label: "Account currency",
        placeholder: "USD",
        kind: "text",
        required: true,
        help: "What the cash, the equity and the results are counted in. A trade in another \
               currency is converted at the journal's FX rate of the day.",
    },
    ConfigField {
        name: "start_balance",
        label: "Starting balance",
        placeholder: "100000",
        kind: "text",
        required: true,
        help: "Cash the account opens with, and what a reset puts back. Add or take out \
               cash at any time from the account.",
    },
    ConfigField {
        name: "commission_per_order",
        label: "Commission per order",
        placeholder: "0",
        kind: "text",
        required: false,
        help: "A flat amount on every fill, in the instrument's currency.",
    },
    ConfigField {
        name: "commission_per_unit",
        label: "Commission per unit",
        placeholder: "0",
        kind: "text",
        required: false,
        help: "Per share, contract or coin filled (0.005 for a typical US share rate).",
    },
    ConfigField {
        name: "commission_pct",
        label: "Commission (% of value)",
        placeholder: "0",
        kind: "text",
        required: false,
        help: "A percent of what the fill is worth (0.1 is 0.1%), the way crypto venues charge.",
    },
    ConfigField {
        name: "commission_min",
        label: "Minimum commission",
        placeholder: "0",
        kind: "text",
        required: false,
        help: "The least one fill costs.",
    },
    ConfigField {
        name: "slippage_bps",
        label: "Slippage (basis points)",
        placeholder: "0",
        kind: "text",
        required: false,
        help: "How much worse a market or stop order fills than the price it met: 5 is 0.05%. \
               A resting limit never slips.",
    },
    ConfigField {
        name: "leverage",
        label: "Leverage",
        placeholder: "1",
        kind: "text",
        required: false,
        help: "How many times the equity the open positions may be worth. 1 is a cash \
               account; an order that would go past it is refused. Futures are not leveraged \
               here: each contract ties up the margin set in its instrument settings.",
    },
    ConfigField {
        name: "shorts",
        label: "Short selling",
        placeholder: "yes",
        kind: "text",
        required: false,
        help: "yes or no. With no, a sell larger than what is held is refused.",
    },
    ConfigField {
        name: "fill_on",
        label: "Fill on",
        placeholder: "quote",
        kind: "text",
        required: false,
        help: "quote: buys on the ask and sells on the bid when the feed publishes them. \
               last: both on the last traded price.",
    },
];

pub(crate) static PAPER: BrokerCapability = BrokerCapability {
    broker: ID,
    label: "Paper trading",
    website: "",
    docs_url: "",
    rate_limit: "Runs inside OpenTraderWorld: no venue, no limit.",
    key_note: "No key and no venue: orders fill inside OpenTraderWorld on the live price of \
               the chart they are placed from. No real money is involved.",
    required_secrets: &[],
    config_fields: FIELDS,
    asset_classes: &["stock", "etf", "crypto", "forex", "future", "option", "other"],
    executions: true,
    needs_symbols: false,
    window_from_broker: false,
    history_days: 0,
    spot_only: false,
    positions: true,
    orders: true,
    holdings: false,
    quotes: false,
    testable: true,
};

// ── Settings ─────────────────────────────────────────────────────────────────

/// An account's settings, read from its config.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub currency: String,
    pub start_balance: Decimal,
    pub commission: matching::Commission,
    pub slippage_bps: Decimal,
    pub leverage: Decimal,
    pub shorts: bool,
    /// Fill on the bid and ask when the feed publishes them.
    pub quotes: bool,
}

impl Settings {
    pub fn parse(c: &HashMap<String, String>) -> Result<Settings> {
        let num = |name: &str, label: &str, default: Decimal| -> Result<Decimal> {
            match c.get(name).map(|v| v.trim()).filter(|v| !v.is_empty()) {
                None => Ok(default),
                Some(v) => match v.parse::<Decimal>() {
                    Ok(x) if x >= Decimal::ZERO => Ok(x),
                    _ => bail!("{label} must be a number, zero or more (got {v:?})"),
                },
            }
        };
        let currency = c.get("currency").map(|v| v.trim().to_uppercase()).unwrap_or_default();
        if currency.len() < 3 || currency.len() > 5 || !currency.chars().all(|ch| ch.is_ascii_alphabetic()) {
            bail!("Account currency must be a currency code such as USD or EUR");
        }
        let leverage = num("leverage", "Leverage", Decimal::ONE)?;
        if leverage < Decimal::ONE {
            bail!("Leverage is 1 for a cash account, or more for a margin one");
        }
        let flag = |name: &str, label: &str, yes: &str, no: &str, default: bool| -> Result<bool> {
            match c.get(name).map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty()) {
                None => Ok(default),
                Some(v) if v == yes => Ok(true),
                Some(v) if v == no => Ok(false),
                Some(v) => bail!("{label} is {yes} or {no} (got {v:?})"),
            }
        };
        Ok(Settings {
            currency,
            start_balance: num("start_balance", "Starting balance", Decimal::ZERO)?,
            commission: matching::Commission {
                per_order: num("commission_per_order", "Commission per order", Decimal::ZERO)?,
                per_unit: num("commission_per_unit", "Commission per unit", Decimal::ZERO)?,
                pct: num("commission_pct", "Commission (% of value)", Decimal::ZERO)?,
                min: num("commission_min", "Minimum commission", Decimal::ZERO)?,
            },
            slippage_bps: num("slippage_bps", "Slippage", Decimal::ZERO)?,
            leverage,
            shorts: flag("shorts", "Short selling", "yes", "no", true)?,
            quotes: flag("fill_on", "Fill on", "quote", "last", true)?,
        })
    }

    /// The settings of a stored account.
    pub fn of(account: &BrokerRow) -> Result<Settings> {
        Settings::parse(&config_map(&account.config))
    }
}

fn config_map(config: &serde_json::Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let serde_json::Value::Object(m) = config {
        for (k, v) in m {
            let text = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => continue,
                other => other.to_string(),
            };
            out.insert(k.clone(), text);
        }
    }
    out
}

// ── Instruments ──────────────────────────────────────────────────────────────

/// The id a paper position is reported (and closed) by: the chart's own name for the
/// instrument, `provider|asset_type|ticker`.
pub fn native_id(provider: &str, asset_type: &str, ticker: &str) -> String {
    format!("{provider}|{asset_type}|{ticker}")
}

fn parse_native(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.splitn(3, '|');
    let (p, a, t) = (parts.next()?, parts.next()?, parts.next()?);
    (!p.is_empty() && !a.is_empty() && !t.is_empty()).then_some((p, a, t))
}

/// Journal asset class of a chart asset type. An index has no market to trade.
fn asset_class(asset_type: &str) -> Option<&'static str> {
    Some(match asset_type {
        "equity" | "stock" => "stock",
        "etf" => "etf",
        "crypto" => "crypto",
        "fx" | "forex" => "forex",
        "future" | "futures" => "future",
        "option" | "options" => "option",
        "cfd" | "commodity" | "bond" => "other",
        _ => return None,
    })
}

/// What a position on an instrument ties up of the account.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Margin {
    /// A future's initial margin per contract, instrument currency.
    PerContract(Decimal),
    /// The full value of a future, by the user's choice.
    Notional,
    /// A future whose margin nobody stated: it cannot be opened or grown.
    Missing,
    /// Anything else: its value, over the account's leverage.
    Leveraged,
}

impl Margin {
    /// What `qty` at `px` ties up, instrument currency. A future with no stated margin that is
    /// already held counts at its full value.
    fn of(self, qty: Decimal, px: Decimal, multiplier: Decimal, leverage: Decimal) -> Decimal {
        let value = qty.abs() * px * multiplier;
        match self {
            Margin::PerContract(m) => qty.abs() * m,
            Margin::Notional | Margin::Missing => value,
            Margin::Leveraged => value / leverage.max(Decimal::ONE),
        }
    }
}

/// A future that would open or grow with no margin stated: the chart offers to set one, or to
/// count the contract at its full value (`brokers_api` answers it `margin_missing`).
#[derive(Debug, Clone)]
pub struct MarginMissing {
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
}

impl std::fmt::Display for MarginMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: no margin per contract is set. Set it in the instrument settings, or count the contract at its full value",
            self.ticker
        )
    }
}

impl std::error::Error for MarginMissing {}

/// What a fill on an instrument is booked with.
#[derive(Debug, Clone, PartialEq)]
struct Facts {
    currency: String,
    multiplier: Decimal,
    tick: Option<Decimal>,
    margin: Margin,
}

/// The facts the instrument info states. A forex pair names its quote currency in its
/// symbol (`EURUSD`, `EUR/USD`), which is read, not guessed; anything else without a stated
/// currency is refused with the fix, as is a contract without a point value.
fn facts_from(asset_type: &str, ticker: &str, meta: Option<&otw_store::instrument_meta::InstrumentMeta>) -> Result<Facts> {
    let stated = meta.and_then(|m| m.currency.clone()).map(|c| c.trim().to_uppercase()).filter(|c| !c.is_empty());
    let currency = match stated {
        Some(c) => c,
        None if matches!(asset_type, "fx" | "forex") => {
            let letters: String = ticker.chars().filter(|c| c.is_ascii_alphabetic()).collect();
            if letters.len() != 6 {
                bail!("{ticker}: no currency is set for it. Set it in the chart's instrument info before trading it on paper");
            }
            letters[3..].to_uppercase()
        }
        None => bail!("{ticker}: no currency is set for it. Set it in the chart's instrument info before trading it on paper"),
    };
    let stated = |v: Option<f64>| v.and_then(exact).filter(|v| *v > Decimal::ZERO);
    let pointvalue = stated(meta.and_then(|m| m.pointvalue));
    let multiplier = match (asset_type, pointvalue) {
        (_, Some(v)) => v,
        ("future" | "futures" | "option" | "options", None) => bail!(
            "{ticker}: no point value is set for this contract. Set it in the chart's instrument info before trading it on paper"
        ),
        _ => Decimal::ONE,
    };
    let margin = if matches!(asset_type, "future" | "futures") {
        match (meta.and_then(|m| m.margin_notional), stated(meta.and_then(|m| m.margin))) {
            (Some(true), _) => Margin::Notional,
            (_, Some(m)) => Margin::PerContract(m),
            _ => Margin::Missing,
        }
    } else {
        Margin::Leveraged
    };
    Ok(Facts { currency, multiplier, tick: tick_decimal(meta.and_then(|m| m.mintick)).filter(|t| *t > Decimal::ZERO), margin })
}

/// A held instrument's margin rule, read again (the user may have set it since). One whose
/// facts no longer read counts at its full value.
async fn margin_of(pool: &PgPool, h: &Holding) -> Margin {
    match facts(pool, &h.provider, &h.asset_type, &h.ticker).await {
        Ok(f) => f.margin,
        Err(_) => Margin::Notional,
    }
}

async fn facts(pool: &PgPool, provider: &str, asset_type: &str, ticker: &str) -> Result<Facts> {
    let meta = crate::instrument_meta_api::resolved(pool, provider, asset_type, ticker).await?;
    facts_from(asset_type, ticker, meta.as_ref())
}

/// One unit of `from` in `to`, at today's journal FX rate.
async fn fx(pool: &PgPool, from: &str, to: &str) -> Result<Decimal> {
    if from == to {
        return Ok(Decimal::ONE);
    }
    let today = OffsetDateTime::now_utc().date();
    otw_store::journal_fx::convert(pool, 1.0, from, to, today)
        .await?
        .and_then(exact)
        .filter(|r| *r > Decimal::ZERO)
        .ok_or_else(|| anyhow!("no {from} to {to} rate: add one in the journal's FX settings, or trade in {to}"))
}

// ── Live state shared with the engine ────────────────────────────────────────

/// The last price an instrument's feed printed, and its book when it publishes one.
#[derive(Debug, Clone, Copy)]
pub struct Mark {
    pub last: Decimal,
    pub at: Instant,
    pub book: Option<(Decimal, Decimal, Instant)>,
}

/// A book older than this is not a price to deal on.
pub const BOOK_FRESH: Duration = Duration::from_secs(15);

fn marks_slot() -> &'static Mutex<HashMap<String, Mark>> {
    static S: OnceLock<Mutex<HashMap<String, Mark>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn mark(native: &str) -> Option<Mark> {
    marks_slot().lock().ok()?.get(native).copied()
}

fn set_mark(native: &str, f: impl FnOnce(Option<Mark>) -> Mark) {
    if let Ok(mut m) = marks_slot().lock() {
        let next = f(m.get(native).copied());
        m.insert(native.to_string(), next);
    }
}

/// Each account's live changes, for whoever follows it (a chart's events stream).
fn bus() -> &'static Mutex<HashMap<Uuid, broadcast::Sender<AccountEvent>>> {
    static S: OnceLock<Mutex<HashMap<Uuid, broadcast::Sender<AccountEvent>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn sender(account: Uuid) -> Option<broadcast::Sender<AccountEvent>> {
    let mut b = bus().lock().ok()?;
    Some(b.entry(account).or_insert_with(|| broadcast::channel(256).0).clone())
}

fn publish(account: Uuid, ev: AccountEvent) {
    if let Some(tx) = sender(account) {
        let _ = tx.send(ev);
    }
}

/// Bumped on every change to the orders, so a watcher knows to read them again.
static VERSION: AtomicU64 = AtomicU64::new(0);

pub fn version() -> u64 {
    VERSION.load(Ordering::Acquire)
}

/// The orders changed: watchers read them again, and the engine looks at what to watch.
fn changed() {
    VERSION.fetch_add(1, Ordering::AcqRel);
    engine::poke();
}

// ── Rows as the broker trait reports them ────────────────────────────────────

/// A float a feed, the instrument info or a journal rate printed, as the decimal it reads as.
pub(crate) fn exact(v: f64) -> Option<Decimal> {
    store::decimal_of(v).ok()
}

/// A decimal in the rows every broker reports alike (positions, orders, fills, the account):
/// for display, never decided on again.
pub(crate) fn shown(v: Decimal) -> f64 {
    f64::try_from(v).unwrap_or_default()
}

fn open_order(o: &Order) -> OpenOrder {
    OpenOrder {
        id: o.id.to_string(),
        symbol: o.ticker.clone(),
        side: o.side.clone(),
        order_type: o.order_type.clone(),
        qty: shown(o.qty),
        filled_qty: 0.0,
        limit_price: o.limit_price.map(shown),
        stop_price: o.stop_price.map(shown),
        trail_start: None,
        trail_rate: None,
        currency: o.currency.clone(),
        asset_class: asset_class(&o.asset_type).unwrap_or("other").to_string(),
        placed_at: Some(o.placed_at),
        venue: o.provider.clone(),
        account: String::new(),
        parent_id: o.parent_id.map(|p| p.to_string()),
        reduce_only: o.reduce_only,
    }
}

/// One instrument the account holds, with what it was last seen at.
struct Holding {
    native: String,
    ticker: String,
    asset_type: String,
    provider: String,
    connector_id: Uuid,
    currency: String,
    multiplier: Decimal,
    held: Held,
}

impl Holding {
    fn mark(&self) -> Decimal {
        mark(&self.native).map(|m| m.last).unwrap_or(self.held.avg)
    }

    fn position(&self) -> Position {
        let px = mark(&self.native).map(|m| m.last);
        let long = self.held.qty > Decimal::ZERO;
        Position {
            symbol: self.ticker.clone(),
            side: if long { "long" } else { "short" }.into(),
            qty: shown(self.held.qty.abs()),
            avg_price: Some(shown(self.held.avg)),
            currency: self.currency.clone(),
            asset_class: asset_class(&self.asset_type).unwrap_or("other").into(),
            multiplier: shown(self.multiplier),
            venue: self.provider.clone(),
            account: String::new(),
            native_id: Some(self.native.clone()),
            market_price: px.map(shown),
            unrealized_pnl: px.map(|p| shown((p - self.held.avg) * self.held.qty * self.multiplier)),
            opened_at: self.held.opened_at,
            entry_fee: Some(self.held.entry_fee.round_dp(8).normalize()),
            fee_currency: self.currency.clone(),
            leverage: None,
            margin_mode: None,
        }
    }
}

/// What the account holds, one line per instrument not flat, in the order first traded.
fn holdings(fills: &[Fill]) -> Vec<Holding> {
    let mut out: Vec<Holding> = Vec::new();
    for f in fills {
        let native = native_id(&f.provider, &f.asset_type, &f.ticker);
        let i = match out.iter().position(|h| h.native == native) {
            Some(i) => i,
            None => {
                out.push(Holding {
                    native,
                    ticker: f.ticker.clone(),
                    asset_type: f.asset_type.clone(),
                    provider: f.provider.clone(),
                    connector_id: f.connector_id,
                    currency: f.currency.clone(),
                    multiplier: f.multiplier,
                    held: Held::default(),
                });
                out.len() - 1
            }
        };
        let h = &mut out[i];
        h.connector_id = f.connector_id;
        h.held.apply(f.side == "buy", f.qty, f.price, f.fee, f.multiplier, f.at);
    }
    out.retain(|h| !h.held.flat());
    out
}

// ── The account's money ──────────────────────────────────────────────────────

/// The account's money, in its currency, as the page shows it: [`Ledger`] in numbers.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub currency: String,
    /// Deposits less withdrawals.
    pub funded: f64,
    pub realized: f64,
    pub fees: f64,
    /// Funded, plus what fills realized, less what they cost.
    pub cash: f64,
    /// What the open positions would realize at their marks.
    pub unrealized: f64,
    pub equity: f64,
    /// What the open positions are worth, long and short alike.
    pub exposure: f64,
    pub leverage: f64,
    /// What the open positions tie up: a future's margin per contract, anything else its value
    /// over the leverage.
    pub margin_used: f64,
    /// Equity not tied up: what a new position may use.
    pub buying_power: f64,
    pub positions: usize,
    pub orders: usize,
    /// A position whose currency has no FX rate today, left out of the totals.
    pub unpriced: Vec<String>,
}

/// The account's money, in its currency, exact: what an order is checked against.
#[derive(Debug, Clone)]
struct Ledger {
    currency: String,
    funded: Decimal,
    realized: Decimal,
    fees: Decimal,
    cash: Decimal,
    unrealized: Decimal,
    equity: Decimal,
    exposure: Decimal,
    leverage: Decimal,
    margin_used: Decimal,
    buying_power: Decimal,
    positions: usize,
    orders: usize,
    unpriced: Vec<String>,
}

impl Ledger {
    fn shown(self) -> Summary {
        Summary {
            currency: self.currency,
            funded: shown(self.funded),
            realized: shown(self.realized),
            fees: shown(self.fees),
            cash: shown(self.cash),
            unrealized: shown(self.unrealized),
            equity: shown(self.equity),
            exposure: shown(self.exposure),
            leverage: shown(self.leverage),
            margin_used: shown(self.margin_used),
            buying_power: shown(self.buying_power),
            positions: self.positions,
            orders: self.orders,
            unpriced: self.unpriced,
        }
    }
}

/// One position's worth and result in the account's currency, at its mark or at `px`.
struct Worth {
    exposure: Decimal,
    unrealized: Decimal,
    /// Instrument currency to account currency.
    rate: Decimal,
}

async fn worth(pool: &PgPool, h: &Holding, qty: Decimal, avg: Decimal, px: Decimal, to: &str) -> Result<Worth> {
    let rate = fx(pool, &h.currency, to).await?;
    Ok(Worth {
        exposure: qty.abs() * px * h.multiplier * rate,
        unrealized: (px - avg) * qty * h.multiplier * rate,
        rate,
    })
}

pub async fn summary(pool: &PgPool, account: &BrokerRow) -> Result<Summary> {
    Ok(ledger(pool, account).await?.shown())
}

/// What may leave the account, in its currency: like a broker's excess liquidity, cash the open
/// positions' margin ties up stays.
pub async fn withdrawable(pool: &PgPool, account: &BrokerRow) -> Result<(Decimal, String)> {
    let l = ledger(pool, account).await?;
    Ok((l.cash.min(l.buying_power).max(Decimal::ZERO), l.currency))
}

async fn ledger(pool: &PgPool, account: &BrokerRow) -> Result<Ledger> {
    let s = Settings::of(account)?;
    let moves = store::cash_moves(pool, account.id).await?;
    let funded: Decimal = moves.iter().map(|m| if m.kind == "deposit" { m.amount } else { -m.amount }).sum();
    let fills = store::all_fills(pool, account.id).await?;
    let realized: Decimal = fills.iter().map(|f| f.realized * f.fx).sum();
    let fees: Decimal = fills.iter().map(|f| f.fee * f.fx).sum();
    let cash = funded + realized - fees;
    let (mut unrealized, mut exposure, mut margin_used, mut unpriced) = (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Vec::new());
    let held = holdings(&fills);
    for h in &held {
        match worth(pool, h, h.held.qty, h.held.avg, h.mark(), &s.currency).await {
            Ok(w) => {
                unrealized += w.unrealized;
                exposure += w.exposure;
                margin_used += margin_of(pool, h).await.of(h.held.qty, h.mark(), h.multiplier, s.leverage) * w.rate;
            }
            Err(_) => unpriced.push(h.ticker.clone()),
        }
    }
    let equity = cash + unrealized;
    let orders = store::open_orders(pool, account.id).await?.len();
    Ok(Ledger {
        currency: s.currency,
        funded,
        realized,
        fees,
        cash,
        unrealized,
        equity,
        exposure,
        leverage: s.leverage,
        margin_used,
        buying_power: (equity - margin_used).max(Decimal::ZERO),
        positions: held.len(),
        orders,
        unpriced,
    })
}

/// Whether going from `now` to `after` units only shrinks the position: flat counts as
/// shrinking from either side (a short covered to flat must not be read as a flip), and
/// crossing through zero to the other side does not.
pub(crate) fn takes_risk_off(now: Decimal, after: Decimal) -> bool {
    after.abs() <= now.abs() && (after.is_zero() || now.is_sign_positive() == after.is_sign_positive())
}

/// An amount as a refusal says it.
fn money(v: Decimal) -> String {
    format!("{:.2}", v.round_dp(2))
}

/// Whether the account may take a fill of `qty` at `price` on `o`'s instrument: shorts when
/// they are off, a future with no margin stated, and positions that would tie up more than
/// the equity (futures by their margin, the rest by their value over the leverage) are
/// refused. A fill that takes risk off is always allowed. The refusal is the sentence the
/// order is rejected with.
pub(crate) async fn allows(pool: &PgPool, account: &BrokerRow, o: &Order, price: Decimal, fee: Decimal) -> Result<std::result::Result<(), String>> {
    let s = Settings::of(account)?;
    let fills = store::all_fills(pool, account.id).await?;
    let native = native_id(&o.provider, &o.asset_type, &o.ticker);
    let held = holdings(&fills);
    let now = held.iter().find(|h| h.native == native).map(|h| h.held.qty).unwrap_or_default();
    let after = now + if o.buy() { o.qty } else { -o.qty };
    if takes_risk_off(now, after) {
        return Ok(Ok(()));
    }
    if after < Decimal::ZERO && !s.shorts {
        return Ok(Err("short selling is off for this paper account".into()));
    }
    let f = facts(pool, &o.provider, &o.asset_type, &o.ticker).await?;
    if f.margin == Margin::Missing {
        return Ok(Err(MarginMissing { provider: o.provider.clone(), asset_type: o.asset_type.clone(), ticker: o.ticker.clone() }.to_string()));
    }
    let sum = ledger(pool, account).await?;
    let rate = fx(pool, &o.currency, &s.currency).await?;
    let mut used = sum.margin_used;
    if let Some(h) = held.iter().find(|h| h.native == native) {
        used -= f.margin.of(h.held.qty, h.mark(), h.multiplier, s.leverage) * rate;
    }
    used += f.margin.of(after, price, o.multiplier, s.leverage) * rate;
    let equity = sum.equity - fee * rate;
    if used > equity {
        return Ok(Err(format!(
            "not enough funds: the positions would tie up {} {cur} and the account's equity is {} {cur}",
            money(used),
            money(equity.max(Decimal::ZERO)),
            cur = s.currency
        )));
    }
    Ok(Ok(()))
}

/// Open a new paper account with its starting balance.
pub async fn open(pool: &PgPool, account: &BrokerRow) -> Result<()> {
    let s = Settings::of(account)?;
    if s.start_balance > Decimal::ZERO {
        store::book_cash(pool, account.id, "deposit", s.start_balance).await?;
    }
    Ok(())
}

/// Start the account over with `amount` (its starting balance when `None`): every order,
/// fill and cash move goes. The journal keeps what it already imported.
pub async fn reset(pool: &PgPool, account: &BrokerRow, amount: Option<Decimal>) -> Result<()> {
    let s = Settings::of(account)?;
    let amount = amount.unwrap_or(s.start_balance);
    store::reset(pool, account.id, amount).await?;
    changed();
    publish(account.id, AccountEvent::Resync);
    Ok(())
}

// ── The broker ───────────────────────────────────────────────────────────────

/// The paper broker, bound to one account (`for_account`), or unbound in the registry, where
/// it only describes itself.
pub struct PaperBroker {
    bound: Option<(PgPool, Uuid)>,
}

pub fn paper() -> PaperBroker {
    PaperBroker { bound: None }
}

pub fn bound(pool: PgPool, account: Uuid) -> PaperBroker {
    PaperBroker { bound: Some((pool, account)) }
}

/// Whether `px` sits on the instrument's tick grid. A venue refuses an order priced between
/// two ticks (CME, IB error 110, Binance PRICE_FILTER); no stated tick accepts any price.
pub(crate) fn on_tick(px: Decimal, tick: Option<Decimal>) -> bool {
    match tick {
        Some(t) if t > Decimal::ZERO => (px % t).is_zero(),
        _ => true,
    }
}

/// The refusal for a price off the tick grid or a fraction of a contract, if any.
pub(crate) fn venue_refusal(asset_type: &str, tick: Option<Decimal>, qty: Option<Decimal>, prices: &[(Option<Decimal>, &str)]) -> Option<String> {
    for (px, what) in prices {
        if let Some(p) = px.filter(|p| !on_tick(*p, tick)) {
            return Some(format!("the {what} {p} is not on the instrument's {} tick", tick.unwrap_or_default()));
        }
    }
    let contracts = matches!(asset_type, "future" | "futures" | "option" | "options");
    if contracts && qty.is_some_and(|q| !q.fract().is_zero()) {
        return Some("a contract trades in whole units: the quantity cannot be a fraction".into());
    }
    None
}

fn positive(v: Option<Decimal>, what: &str) -> Result<Option<Decimal>> {
    match v {
        Some(d) if d <= Decimal::ZERO => bail!("the {what} must be more than zero"),
        v => Ok(v),
    }
}

fn order_type(t: OrderType) -> &'static str {
    match t {
        OrderType::Market => "market",
        OrderType::Limit => "limit",
        OrderType::Stop => "stop",
        OrderType::StopLimit => "stop_limit",
        OrderType::TrailingStop => "trailing_stop",
        OrderType::Mit => "mit",
        OrderType::Lit => "lit",
    }
}

/// What the paper broker takes: every type, time in force and option its matching rules
/// play out on the chart's live price. No iceberg, trigger choice, extended hours, close on
/// trigger nor algo: a live price with no depth, session or mark has nothing to play them on.
pub fn order_caps() -> otw_connector_model::OrderCaps {
    use otw_connector_model::TrailKind;
    otw_connector_model::OrderCaps {
        types: [OrderType::Market, OrderType::Limit, OrderType::Stop, OrderType::StopLimit, OrderType::TrailingStop, OrderType::Mit, OrderType::Lit].into(),
        tif: [TifKind::Day, TifKind::Gtc, TifKind::Gtd, TifKind::Ioc, TifKind::Fok].into(),
        amend: true,
        brackets: true,
        oca: true,
        reduce_only: true,
        post_only: true,
        trail: [TrailKind::Amount, TrailKind::Percent].into(),
        trail_start: true,
        client_id: true,
        ..Default::default()
    }
}

fn tif_name(t: TifKind) -> &'static str {
    match t {
        TifKind::Day => "day",
        TifKind::Gtc => "gtc",
        TifKind::Gtd => "gtd",
        TifKind::Ioc => "ioc",
        TifKind::Fok => "fok",
    }
}

/// When an order with this time in force stops working: a day order at the next 00:00 UTC.
fn expiry(tif: TifKind, expires: Option<i64>, now: OffsetDateTime) -> Result<Option<OffsetDateTime>> {
    match tif {
        TifKind::Day => {
            let midnight = (now.date() + time::Duration::days(1)).midnight().assume_utc();
            Ok(Some(midnight))
        }
        TifKind::Gtd => {
            let ms = expires.ok_or_else(|| anyhow!("a good-till-date order needs an expiry"))?;
            let at = OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
                .map_err(|_| anyhow!("the expiry is out of range"))?;
            if at <= now {
                bail!("the expiry is already past");
            }
            Ok(Some(at))
        }
        TifKind::Gtc | TifKind::Ioc | TifKind::Fok => Ok(None),
    }
}

/// The tick the instrument info states, as the decimal it reads as.
fn tick_decimal(t: Option<f64>) -> Option<Decimal> {
    t.and_then(exact)
}

impl PaperBroker {
    fn ctx(&self) -> Result<(&PgPool, Uuid)> {
        self.bound.as_ref().map(|(p, id)| (p, *id)).ok_or_else(|| anyhow!("the paper broker is not bound to an account"))
    }

    async fn account(&self) -> Result<BrokerRow> {
        let (pool, id) = self.ctx()?;
        accounts::get(pool, id).await?.ok_or_else(|| anyhow!("the paper account was deleted"))
    }

    /// The connector a chart's ticket prices on: the one it names, or the chart's default.
    async fn feed(&self, provider: &str, connector: Option<Uuid>) -> Result<conn_store::ConnectorRow> {
        let (pool, _) = self.ctx()?;
        let c = match connector {
            Some(id) => conn_store::get(pool, id).await?.ok_or_else(|| anyhow!("the chart's data connector was deleted"))?,
            None => conn_store::default_for_module(pool, provider, FEED_MODULE)
                .await?
                .ok_or_else(|| anyhow!("no {provider} connector is granted to the chart: paper orders fill on its live price"))?,
        };
        if c.provider != provider {
            bail!("connector {} is a {} connector, not {provider}", c.name, c.provider);
        }
        Ok(c)
    }

    /// Write a ticket's legs as orders on one instrument and tell the engine.
    async fn send(&self, ticker: &str, asset_type: &str, provider: &str, connector: Option<Uuid>, legs: &[super::OrderLeg]) -> Result<Vec<Placed>> {
        let (pool, account_id) = self.ctx()?;
        let account = self.account().await?;
        let s = Settings::of(&account)?;
        if legs.is_empty() {
            bail!("the ticket has no order");
        }
        if asset_class(asset_type).is_none() {
            bail!("{ticker} is an {asset_type}: it has no market to trade");
        }
        let conn = self.feed(provider, connector).await?;
        if let Some(why) = engine::live_refusal(provider, asset_type) {
            bail!("paper orders fill on the live price, and {why}");
        }
        let f = facts(pool, provider, asset_type, ticker).await?;
        fx(pool, &f.currency, &s.currency).await?;

        let now = OffsetDateTime::now_utc();
        // The ticket whole against what the paper broker takes, as a venue's would be.
        let instrument = otw_connector_model::InstrumentId::new("PAPER", ticker);
        let requests: Vec<otw_connector_model::OrderRequest> = legs.iter().map(|l| l.request(&instrument)).collect::<Result<_>>()?;
        let stamp = otw_connector_model::Timestamp::from_offset_datetime(now).ok_or_else(|| anyhow!("the clock is out of range"))?;
        otw_connector_model::check_orders_for(PAPER.label, &order_caps(), &requests, stamp).map_err(anyhow::Error::new)?;
        let native = native_id(provider, asset_type, ticker);
        let held_now = holdings(&store::all_fills(pool, account_id).await?).iter().find(|h| h.native == native).map(|h| h.held.qty).unwrap_or_default();
        let ids: HashMap<&str, Uuid> = legs.iter().map(|l| (l.key.as_str(), Uuid::new_v4())).collect();
        let mut orders = Vec::with_capacity(legs.len());
        for l in legs {
            if l.key.trim().is_empty() {
                bail!("an order has no key");
            }
            let qty = positive(Some(l.qty), "quantity")?.unwrap_or_default();
            let limit = positive(l.limit, "limit price")?;
            let stop = positive(l.stop, "stop price")?;
            if let Some(why) = venue_refusal(asset_type, f.tick, Some(qty), &[(limit, "limit price"), (stop, "stop price")]) {
                bail!(why);
            }
            let (needs_limit, needs_stop) = l.order_type.prices();
            if needs_limit != limit.is_some() {
                bail!("a {} order {} limit price", order_type(l.order_type), if needs_limit { "needs a" } else { "takes no" });
            }
            if needs_stop && stop.is_none() || stop.is_some() && !needs_stop && l.order_type != OrderType::TrailingStop {
                bail!("a {} order {} stop price", order_type(l.order_type), if needs_stop { "needs a" } else { "takes no" });
            }
            let trail_amount = positive(l.trail_amount, "trailing amount")?;
            let trail_pct = positive(l.trail_percent, "trailing percentage")?;
            let top_level = l.parent.as_deref().is_none_or(str::is_empty);
            // A post-only limit that would deal at once is refused, as a venue's matching
            // engine refuses a maker order that would take.
            if l.post_only {
                if let (Some(px), Some(m)) = (limit, mark(&native)) {
                    let (bid, ask) = m.book.map(|(b, a, _)| (b, a)).unwrap_or((m.last, m.last));
                    let takes = if l.side == Side::Buy { px >= ask } else { px <= bid };
                    if takes {
                        bail!("post-only: a {} limit at {px} would fill at once against {}", if l.side == Side::Buy { "buy" } else { "sell" }, if l.side == Side::Buy { ask } else { bid });
                    }
                }
            }
            // A reduce-only order on its own must face a position on the other side now; an
            // attached exit is checked when its entry has filled.
            if l.reduce_only && top_level {
                let against = if l.side == Side::Buy { -held_now } else { held_now };
                if against <= Decimal::ZERO {
                    bail!("reduce-only: the account holds no {} position on {ticker} to reduce", if l.side == Side::Buy { "short" } else { "long" });
                }
            }
            let parent = match l.parent.as_deref().filter(|p| !p.is_empty()) {
                None => None,
                Some(p) => {
                    let entry = legs.iter().find(|x| x.key == p).ok_or_else(|| anyhow!("an exit names an entry that is not on the ticket"))?;
                    if entry.parent.as_deref().is_some_and(|x| !x.is_empty()) {
                        bail!("an exit cannot hang on another exit");
                    }
                    if entry.side == l.side {
                        bail!("an exit is on the other side of its entry");
                    }
                    ids.get(p).copied()
                }
            };
            let tif = l.tif.unwrap_or(TifKind::Day);
            orders.push(NewOrder {
                id: ids[l.key.as_str()],
                key: l.key.clone(),
                connector_id: conn.id,
                provider: provider.to_string(),
                asset_type: asset_type.to_string(),
                ticker: ticker.to_string(),
                currency: f.currency.clone(),
                multiplier: f.multiplier,
                tick: f.tick,
                side: if l.side == Side::Buy { "buy" } else { "sell" }.into(),
                order_type: order_type(l.order_type).into(),
                qty,
                limit_price: limit,
                stop_price: stop,
                tif: tif_name(tif).into(),
                expires_at: expiry(tif, l.expires, now)?,
                parent_id: parent,
                oca_group: l.group.clone().filter(|g| !g.is_empty()),
                reduce_only: l.reduce_only,
                post_only: l.post_only,
                trail_amount,
                trail_pct,
            });
        }
        // A future with no margin stated cannot be opened or grown: said now, with the two ways
        // out, rather than as a rejection at the fill.
        if f.margin == Margin::Missing {
            let native = native_id(provider, asset_type, ticker);
            let now = holdings(&store::all_fills(pool, account_id).await?)
                .iter()
                .find(|h| h.native == native)
                .map(|h| h.held.qty)
                .unwrap_or_default();
            let grows = orders.iter().filter(|o| o.parent_id.is_none()).any(|o| {
                let after = now + if o.side == "buy" { o.qty } else { -o.qty };
                !takes_risk_off(now, after)
            });
            if grows {
                return Err(anyhow::Error::new(MarginMissing {
                    provider: provider.to_string(),
                    asset_type: asset_type.to_string(),
                    ticker: ticker.to_string(),
                }));
            }
        }
        // An entry the account plainly cannot afford is refused now rather than at its fill;
        // what it costs is checked again then, on the price it meets.
        for o in orders.iter().filter(|o| o.parent_id.is_none()) {
            let Some(px) = o.limit_price.or(o.stop_price).or_else(|| mark(&native).map(|m| m.last)) else { continue };
            let probe = Order {
                id: o.id,
                account_id,
                key: o.key.clone(),
                connector_id: o.connector_id,
                provider: o.provider.clone(),
                asset_type: o.asset_type.clone(),
                ticker: o.ticker.clone(),
                currency: o.currency.clone(),
                multiplier: o.multiplier,
                tick: o.tick,
                side: o.side.clone(),
                order_type: o.order_type.clone(),
                qty: o.qty,
                limit_price: o.limit_price,
                stop_price: o.stop_price,
                tif: o.tif.clone(),
                expires_at: o.expires_at,
                parent_id: None,
                oca_group: o.oca_group.clone(),
                status: "working".into(),
                triggered: false,
                reason: None,
                placed_at: now,
                reduce_only: o.reduce_only,
                post_only: o.post_only,
                trail_amount: o.trail_amount,
                trail_pct: o.trail_pct,
                trail_best: None,
            };
            let fee = s.commission.fee(o.qty, px, o.multiplier);
            if let Err(why) = allows(pool, &account, &probe, px, fee).await? {
                bail!("{why}");
            }
        }
        store::insert_orders(pool, account_id, &orders).await?;
        for o in store::open_orders(pool, account_id).await?.iter().filter(|o| ids.values().any(|id| *id == o.id)) {
            publish(account_id, AccountEvent::Order { order: open_order(o), key: Some(o.key.clone()) });
        }
        changed();
        let tick = f.tick.map(|t| t.normalize());
        Ok(legs.iter().map(|l| Placed { key: l.key.clone(), id: ids[l.key.as_str()].to_string(), tick }).collect())
    }
}

/// Where a stored order stands, in the venue-neutral words. An exit held for its entry is
/// accepted and waiting, as a venue's attached order is. Paper fills an order whole, so it is
/// never partly filled.
fn order_status(status: &str) -> Result<OrderStatus> {
    Ok(match status {
        "held" | "working" => OrderStatus::New,
        "filled" => OrderStatus::Filled,
        "cancelled" => OrderStatus::Cancelled,
        "rejected" => OrderStatus::Rejected,
        "expired" => OrderStatus::Expired,
        other => bail!("a paper order is {other:?}, which is no order status"),
    })
}

/// An order id as the chart sends it back.
fn order_id(id: &str) -> Result<Uuid> {
    Uuid::parse_str(id.trim()).map_err(|_| anyhow!("{id:?} is not a paper order id"))
}

/// Tell a following page that orders stopped working.
pub(crate) fn tell_done(account: Uuid, ids: &[Uuid], status: &str) {
    tell_done_why(account, ids, status, None);
}

/// [`tell_done`] with the reason, said on the first order (the one the reason is about).
pub(crate) fn tell_done_why(account: Uuid, ids: &[Uuid], status: &str, reason: Option<&str>) {
    for (i, id) in ids.iter().enumerate() {
        let reason = if i == 0 { reason.map(String::from) } else { None };
        publish(account, AccountEvent::Done { id: id.to_string(), status: status.to_string(), reason });
    }
}

#[async_trait::async_trait]
impl Broker for PaperBroker {
    fn capability(&self) -> &'static BrokerCapability {
        &PAPER
    }

    fn order_caps(&self) -> Option<otw_connector_model::OrderCaps> {
        Some(order_caps())
    }

    async fn is_live(&self, _settings: &HashMap<String, String>) -> Result<bool> {
        Ok(false)
    }

    async fn test(&self, _client: &reqwest::Client, _secrets: &HashMap<String, String>) -> Result<String> {
        let (pool, _) = self.ctx()?;
        let s = summary(pool, &self.account().await?).await?;
        Ok(format!(
            "Paper account: equity {:.2} {cur}, cash {:.2} {cur}, {} open position(s), {} working order(s)",
            s.equity,
            s.cash,
            s.positions,
            s.orders,
            cur = s.currency
        ))
    }

    async fn executions(&self, _client: &reqwest::Client, _secrets: &HashMap<String, String>, q: &ExecQuery) -> Result<Vec<Execution>> {
        let (pool, id) = self.ctx()?;
        Ok(store::fills(pool, id, q.from, q.to)
            .await?
            .into_iter()
            .map(|f| Execution {
                id: f.id.to_string(),
                order_id: f.order_id.to_string(),
                at: f.at,
                symbol: f.ticker.clone(),
                side: f.side.clone(),
                qty: shown(f.qty),
                price: shown(f.price),
                fee: shown(f.fee),
                fee_currency: f.currency.clone(),
                currency: f.currency.clone(),
                asset_class: asset_class(&f.asset_type).unwrap_or("other").into(),
                multiplier: shown(f.multiplier),
                venue: f.provider.clone(),
                account: String::new(),
            })
            .collect())
    }

    async fn symbols(&self, _client: &reqwest::Client, _secrets: &HashMap<String, String>) -> Result<Vec<String>> {
        let (pool, id) = self.ctx()?;
        let mut out: Vec<String> = Vec::new();
        for f in store::all_fills(pool, id).await? {
            if !out.contains(&f.ticker) {
                out.push(f.ticker);
            }
        }
        Ok(out)
    }

    async fn positions(&self, _client: &reqwest::Client, _secrets: &HashMap<String, String>) -> Result<Vec<Position>> {
        let (pool, id) = self.ctx()?;
        Ok(holdings(&store::all_fills(pool, id).await?).iter().map(Holding::position).collect())
    }

    async fn orders(&self, _client: &reqwest::Client, _secrets: &HashMap<String, String>) -> Result<Vec<OpenOrder>> {
        let (pool, id) = self.ctx()?;
        Ok(store::open_orders(pool, id).await?.iter().map(open_order).collect())
    }

    async fn marks(&self, _secrets: &HashMap<String, String>, native_ids: &[String]) -> Result<HashMap<String, f64>> {
        Ok(native_ids.iter().filter_map(|n| mark(n).map(|m| (n.clone(), shown(m.last)))).collect())
    }

    async fn account(&self, _settings: &HashMap<String, String>, _symbol: Option<&str>) -> Result<Vec<super::AccountSummary>> {
        let (pool, _) = self.ctx()?;
        let s = summary(pool, &self.account().await?).await?;
        Ok(vec![super::AccountSummary {
            account: String::new(),
            currency: s.currency,
            equity: Some(s.equity),
            cash: Some(s.cash),
            unrealized: Some(s.unrealized),
            realized: Some(s.realized),
            fees: Some(s.fees),
            margin_used: Some(s.margin_used),
            maintenance_margin: None,
            available: Some(s.buying_power),
            excess_liquidity: None,
            buying_power: None,
            gross_position_value: Some(s.exposure),
            assets: Vec::new(),
        }])
    }

    async fn events(&self, _settings: &HashMap<String, String>) -> Result<Option<futures::stream::BoxStream<'static, AccountEvent>>> {
        let (_, id) = self.ctx()?;
        let rx = sender(id).ok_or_else(|| anyhow!("the paper event bus is unavailable"))?.subscribe();
        let stream = futures::stream::unfold(rx, |mut rx| async move {
            let ev = match rx.recv().await {
                Ok(e) => e,
                Err(broadcast::error::RecvError::Lagged(_)) => AccountEvent::Resync,
                Err(broadcast::error::RecvError::Closed) => return None,
            };
            Some((ev, rx))
        });
        Ok(Some(Box::pin(stream)))
    }

    async fn place(&self, _settings: &HashMap<String, String>, ticket: &OrderTicket) -> Result<Vec<Placed>> {
        let provider = ticket
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| anyhow!("a paper order is placed from a chart: the ticket names no data provider"))?;
        self.send(ticket.ticker.trim(), ticket.asset_type.trim(), provider, ticket.connector, &ticket.legs).await
    }

    async fn modify(&self, _settings: &HashMap<String, String>, id: &str, change: &otw_connector_model::OrderAmend) -> Result<Option<crate::brokers::Placed>> {
        let (pool, account) = self.ctx()?;
        let id = order_id(id)?;
        let o = store::order(pool, account, id).await?.ok_or_else(|| anyhow!("no such paper order"))?;
        if !store::OPEN.contains(&o.status.as_str()) {
            bail!("the order is {} and can no longer be changed", o.status);
        }
        let limit = positive(change.limit.map(|p| p.value()), "limit price")?;
        let stop = positive(change.stop.map(|p| p.value()), "stop price")?;
        let qty = positive(change.qty.map(|q| q.value()), "quantity")?;
        if limit.is_some() && o.limit_price.is_none() {
            bail!("a {} order has no limit price to move", o.order_type);
        }
        if stop.is_some() && o.stop_price.is_none() {
            bail!("a {} order has no stop price to move", o.order_type);
        }
        if let Some(why) = venue_refusal(&o.asset_type, o.tick, qty, &[(limit, "limit price"), (stop, "stop price")]) {
            bail!(why);
        }
        let (tif, expires_at) = match change.tif {
            None => (None, None),
            Some(t) => {
                let kind = t.kind();
                let ms = match t {
                    TimeInForce::Gtd(ts) => Some(ts.millis()),
                    _ => None,
                };
                (Some(tif_name(kind).to_string()), Some(expiry(kind, ms, OffsetDateTime::now_utc())?))
            }
        };
        let a = store::Amend { limit, stop, qty, tif, expires_at };
        if !store::amend(pool, account, id, &a).await? {
            bail!("the order stopped working before it could be changed");
        }
        if let Some(o) = store::order(pool, account, id).await? {
            publish(account, AccountEvent::Order { order: open_order(&o), key: Some(o.key.clone()) });
        }
        changed();
        Ok(None)
    }

    async fn cancel(&self, _settings: &HashMap<String, String>, id: &str) -> Result<()> {
        let (pool, account) = self.ctx()?;
        let ids = store::stop(pool, account, order_id(id)?, "cancelled", None).await?;
        if ids.is_empty() {
            bail!("the order is no longer working");
        }
        tell_done(account, &ids, "cancelled");
        changed();
        Ok(())
    }

    async fn symbol_for(&self, _settings: &HashMap<String, String>, ticker: &str, _asset_type: &str) -> Result<Option<String>> {
        Ok(Some(ticker.to_string()))
    }

    /// The tick of the chart's instrument info, and contracts in whole units: what
    /// [`venue_refusal`] holds an order to. Any other size is taken.
    async fn grid(&self, _settings: &HashMap<String, String>, provider: Option<&str>, ticker: &str, asset_type: &str) -> Result<Option<super::Grid>> {
        let (pool, _) = self.ctx()?;
        let tick = match provider {
            Some(p) => tick_decimal(crate::instrument_meta_api::resolved(pool, p, asset_type, ticker).await?.and_then(|m| m.mintick)).filter(|t| *t > Decimal::ZERO),
            None => None,
        };
        let contracts = matches!(asset_type, "future" | "futures" | "option" | "options");
        Ok(Some(super::Grid { tick, lot_step: contracts.then_some(Decimal::ONE), min_qty: contracts.then_some(Decimal::ONE), ..Default::default() }))
    }

    async fn close_position(&self, _settings: &HashMap<String, String>, native: &str, _account: &str, key: &str) -> Result<Placed> {
        let (pool, id) = self.ctx()?;
        let (provider, asset_type, ticker) = parse_native(native).ok_or_else(|| anyhow!("{native:?} is not a paper position"))?;
        let held = holdings(&store::all_fills(pool, id).await?);
        let h = held.iter().find(|h| h.native == native).ok_or_else(|| anyhow!("the account holds no {ticker}"))?;
        let qty = h.held.qty.abs();
        let side = if h.held.qty > Decimal::ZERO { Side::Sell } else { Side::Buy };
        let leg = super::OrderLeg { tif: Some(TifKind::Gtc), ..super::OrderLeg::new(key, side, OrderType::Market, qty) };
        let mut placed = self.send(ticker, asset_type, provider, Some(h.connector_id), std::slice::from_ref(&leg)).await?;
        placed.pop().ok_or_else(|| anyhow!("the closing order was not placed"))
    }

    async fn lookup(&self, _settings: &HashMap<String, String>, _ticker: &str, _asset_type: &str, key: &str) -> Result<Lookup> {
        let (pool, account) = self.ctx()?;
        Ok(match store::by_key(pool, account, key).await? {
            Some((o, filled)) => Lookup::Found { id: o.id.to_string(), status: order_status(&o.status)?, filled_qty: filled, fills_as: None },
            None => Lookup::NotFound,
        })
    }

    async fn cancel_all(&self, _settings: &HashMap<String, String>) -> Result<usize> {
        let (pool, account) = self.ctx()?;
        let open = store::open_orders(pool, account).await?;
        let mut n = 0;
        for o in open.iter().filter(|o| o.parent_id.is_none() || !open.iter().any(|p| Some(p.id) == o.parent_id)) {
            let ids = store::stop(pool, account, o.id, "cancelled", None).await.context("cancelling a paper order")?;
            n += ids.len();
            tell_done(account, &ids, "cancelled");
        }
        changed();
        Ok(n)
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        Settings::parse(config).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(v: &str) -> Decimal {
        v.parse().unwrap()
    }

    fn cfg(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn settings_default_to_a_cash_account_that_fills_on_the_book() {
        let s = Settings::parse(&cfg(&[("currency", "eur"), ("start_balance", "25000")])).unwrap();
        assert_eq!((s.currency.as_str(), s.start_balance, s.leverage, s.shorts, s.quotes), ("EUR", d("25000"), Decimal::ONE, true, true));
        assert_eq!(s.commission, matching::Commission::default());
        let s = Settings::parse(&cfg(&[("currency", "USD"), ("commission_per_unit", "0.005"), ("shorts", "no"), ("fill_on", "last"), ("leverage", "4")])).unwrap();
        assert_eq!((s.commission.per_unit, s.shorts, s.quotes, s.leverage), (d("0.005"), false, false, d("4")));
    }

    #[test]
    fn a_setting_that_does_not_read_is_refused_with_its_name() {
        assert!(Settings::parse(&cfg(&[])).is_err());
        let e = Settings::parse(&cfg(&[("currency", "USD"), ("slippage_bps", "-1")])).unwrap_err().to_string();
        assert!(e.contains("Slippage"), "{e}");
        let e = Settings::parse(&cfg(&[("currency", "USD"), ("leverage", "0.5")])).unwrap_err().to_string();
        assert!(e.contains("Leverage"), "{e}");
        let e = Settings::parse(&cfg(&[("currency", "USD"), ("shorts", "maybe")])).unwrap_err().to_string();
        assert!(e.contains("yes or no"), "{e}");
    }

    fn meta(currency: Option<&str>, pointvalue: Option<f64>) -> otw_store::instrument_meta::InstrumentMeta {
        otw_store::instrument_meta::InstrumentMeta {
            provider: "p".into(),
            asset_type: "a".into(),
            ticker: "t".into(),
            mintick: Some(0.25),
            pointvalue,
            timezone: None,
            session: None,
            kind: None,
            currency: currency.map(String::from),
            basecurrency: None,
            description: None,
            margin: None,
            margin_notional: None,
        }
    }

    #[test]
    fn an_instrument_is_booked_with_its_stated_facts_and_refused_without_them() {
        let f = facts_from("future", "MNQ.202612", Some(&meta(Some("usd"), Some(2.0)))).unwrap();
        assert_eq!(f, Facts { currency: "USD".into(), multiplier: d("2"), tick: Some(d("0.25")), margin: Margin::Missing });
        // A contract with no point value would be booked a hundred times off.
        assert!(facts_from("future", "MNQ.202612", Some(&meta(Some("USD"), None))).is_err());
        // A pair names its quote currency; a stock does not.
        assert_eq!(facts_from("fx", "EUR/JPY", None).unwrap().currency, "JPY");
        assert!(facts_from("equity", "AAPL", None).unwrap_err().to_string().contains("instrument info"));
        assert_eq!(facts_from("equity", "AAPL", Some(&meta(Some("USD"), None))).unwrap().multiplier, Decimal::ONE);
    }

    #[test]
    fn a_future_ties_up_its_margin_and_the_rest_its_value_over_the_leverage() {
        let mut m = meta(Some("USD"), Some(2.0));
        m.margin = Some(4300.0);
        assert_eq!(facts_from("future", "MNQZ6", Some(&m)).unwrap().margin, Margin::PerContract(d("4300")));
        m.margin_notional = Some(true);
        assert_eq!(facts_from("future", "MNQZ6", Some(&m)).unwrap().margin, Margin::Notional);
        assert_eq!(facts_from("equity", "AAPL", Some(&meta(Some("USD"), None))).unwrap().margin, Margin::Leveraged);
        // 1 MNQ at 29 500, 2 USD a point: 4 300 tied up, not its 59 000 value.
        assert_eq!(Margin::PerContract(d("4300")).of(d("-1"), d("29500"), d("2"), d("1")), d("4300"));
        assert_eq!(Margin::Notional.of(d("1"), d("29500"), d("2"), d("4")), d("59000"));
        // 100 shares at 200 on a 4x account: 5 000 of 20 000.
        assert_eq!(Margin::Leveraged.of(d("100"), d("200"), d("1"), d("4")), d("5000"));
    }

    #[test]
    fn a_position_is_closed_by_the_chart_name_of_its_instrument() {
        let id = native_id("binance", "crypto", "BTC|USDT");
        assert_eq!(parse_native(&id), Some(("binance", "crypto", "BTC|USDT")));
        assert_eq!(parse_native("binance|crypto"), None);
    }

    #[test]
    fn a_day_order_lasts_until_midnight_utc_and_a_past_expiry_is_refused() {
        let now = time::macros::datetime!(2026-10-22 15:30 UTC);
        assert_eq!(expiry(TifKind::Day, None, now).unwrap(), Some(time::macros::datetime!(2026-10-23 00:00 UTC)));
        assert_eq!(expiry(TifKind::Gtc, None, now).unwrap(), None);
        assert!(expiry(TifKind::Gtd, Some(now.unix_timestamp() * 1000 - 1), now).is_err());
        assert!(expiry(TifKind::Gtd, None, now).is_err());
    }

    #[test]
    fn a_lookup_answers_where_the_order_stands_and_never_guesses_a_status() {
        assert_eq!(order_status("working").unwrap(), OrderStatus::New);
        // An exit waiting on its entry is accepted, as a venue's attached order is.
        assert_eq!(order_status("held").unwrap(), OrderStatus::New);
        assert_eq!(order_status("filled").unwrap(), OrderStatus::Filled);
        assert_eq!(order_status("cancelled").unwrap(), OrderStatus::Cancelled);
        assert_eq!(order_status("rejected").unwrap(), OrderStatus::Rejected);
        assert_eq!(order_status("expired").unwrap(), OrderStatus::Expired);
        assert!(order_status("lost").is_err());
    }

    #[test]
    fn a_price_or_size_crosses_the_display_boundary_once() {
        // A feed's float reads as the decimal it prints, not its binary neighbour.
        assert_eq!(exact(0.1), Some(d("0.1")));
        assert_eq!(exact(5000.25), Some(d("5000.25")));
        assert_eq!(exact(f64::NAN), None);
        assert_eq!(shown(d("0.3")), 0.3);
    }
}
