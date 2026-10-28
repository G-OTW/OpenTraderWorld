//! Broker accounts: what *I* did, as opposed to what the market did.
//!
//! The data broker ([`crate::histdata`]) reads prices; this one reads an account:
//! executions, positions and open orders. The credentials the user is asked for are the
//! read-only kind, and a broker that cannot issue a read-only key says so in `key_note`.
//!
//! **Orders** go out only through [`Broker::place`], [`Broker::modify`] and [`Broker::cancel`],
//! only to an account carrying the `trading` grant (checked in `brokers_api`), and only for a
//! broker that implements them: every other one refuses by default. Today that is the IB
//! Gateway broker, whose socket can trade once the user unticks Read Only API, and the
//! [`paper`] broker, which OpenTraderWorld runs itself on the chart's live price.
//!
//! Each broker declares a [`BrokerCapability`] (which credentials it needs, whether its
//! execution history takes an arbitrary window, whether it must be asked one instrument at
//! a time, how far back it answers) and the consumers, journal import first, drive it
//! generically from that.
//!
//! **A cash account has no shorts.** On a spot venue a sell with nothing open is the sale of
//! a holding bought before the window, not the opening of a short. `spot_only` says so, and
//! the importer reports such a fill as an error naming the fix (widen the period) rather
//! than inventing a short position nobody took.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use otw_connector_model::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub use crate::histdata::ConfigField;

mod binance;
mod binance_events;
pub mod cache;
pub(crate) mod caps;
pub mod estimate;
mod ibkr;
pub mod live;
mod native;
pub mod paper;
pub mod reconcile;
pub mod risk;

#[cfg(test)]
mod mock_tests;
#[cfg(test)]
mod net_tests;
#[cfg(test)]
mod ibkr_paper_tests;
#[cfg(test)]
mod binance_testnet_tests;

/// The setting the core adds to the one account named by `OTW_LIVE_CANARY` (a maintainer's
/// live canary, onboarding spec 9): its live orders pass an order path still below E4. Not a
/// field any broker declares, so the account form can never store it.
pub const LIVE_CANARY: &str = "live_canary";

/// Static description of what a broker's API can answer. Drives the account form, the
/// import modal's controls, and is enforced again server-side before a pull is attempted.
#[derive(Debug, Clone, Serialize)]
pub struct BrokerCapability {
    pub broker: &'static str,
    pub label: &'static str,
    pub website: &'static str,
    /// Link to the broker's API documentation (shown as an info tooltip).
    pub docs_url: &'static str,
    /// Short human note on rate limits for this broker.
    pub rate_limit: &'static str,
    /// One line on what the key must be allowed to do, and what it must *not*.
    pub key_note: &'static str,
    /// Secret names the broker needs. Surfaced on the settings page, write-only.
    pub required_secrets: &'static [&'static str],
    /// Non-secret settings (a Flex query id, a sub-account label).
    pub config_fields: &'static [ConfigField],
    /// Asset classes this account can report, in journal vocabulary.
    pub asset_classes: &'static [&'static str],
    /// Executions (fills) can be pulled.
    pub executions: bool,
    /// The API answers one instrument at a time: the caller must name the symbols.
    pub needs_symbols: bool,
    /// The window is the broker's, not ours: the pull returns whatever its own saved query
    /// covers and the requested period only filters the answer (IBKR Flex).
    pub window_from_broker: bool,
    /// Furthest back the API answers, in days. 0 = no published limit.
    pub history_days: u32,
    /// A cash account: a sell with nothing open is a sale, never a short.
    pub spot_only: bool,
    pub positions: bool,
    pub orders: bool,
    /// What the account owns can be listed (a balance sheet, not a trade history).
    pub holdings: bool,
    /// The venue can be asked what an asset trades at right now.
    pub quotes: bool,
    pub testable: bool,
}

/// One fill, as the broker reports it. Prices and quantities are the broker's own; the
/// symbol is its own spelling and is never rewritten into a guess.
#[derive(Debug, Clone, Serialize)]
pub struct Execution {
    /// The broker's execution id: the identity a re-sync deduplicates on.
    pub id: String,
    /// The order this fill belongs to (empty when the broker reports none).
    pub order_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// The instrument, spelled as the broker spells it.
    pub symbol: String,
    /// "buy" | "sell".
    pub side: String,
    pub qty: f64,
    pub price: f64,
    /// Commission as a positive cost.
    pub fee: f64,
    /// Currency (or crypto asset) the fee was billed in.
    pub fee_currency: String,
    /// The instrument's quote/settlement currency.
    pub currency: String,
    /// Journal asset class: stock / etf / crypto / forex / future / option.
    pub asset_class: String,
    /// Point value, when the broker reports one (futures). 1 otherwise.
    pub multiplier: f64,
    /// Exchange/venue label, empty when the broker reports none.
    pub venue: String,
    /// Sub-account id, when the broker has several.
    pub account: String,
}

/// One open position as the broker reports it now. A broker that trades can close it
/// ([`Broker::close_position`], by `native_id`).
#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub symbol: String,
    /// "long" | "short".
    pub side: String,
    pub qty: f64,
    /// Weighted average cost, when the broker reports one.
    pub avg_price: Option<f64>,
    pub currency: String,
    pub asset_class: String,
    pub multiplier: f64,
    pub venue: String,
    pub account: String,
    /// The broker's own id for the instrument (IB's contract id): what a close is sent on.
    pub native_id: Option<String>,
    /// The broker's mark for the instrument, when it values its positions.
    pub market_price: Option<f64>,
    /// The broker's unrealized result, in `currency`.
    pub unrealized_pnl: Option<f64>,
    /// When the position opened, when the broker's fills still reach back to it.
    #[serde(with = "time::serde::rfc3339::option")]
    pub opened_at: Option<OffsetDateTime>,
    /// Commission paid on the fills that opened it, when the broker reported it.
    pub entry_fee: Option<Decimal>,
    pub fee_currency: String,
    /// The leverage the venue applies to this instrument, when it sets one per instrument
    /// (Binance USDⓈ-M).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leverage: Option<Decimal>,
    /// "cross" | "isolated", when the venue margins each instrument one way or the other.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_mode: Option<String>,
}

/// What the broker says of an order sent under a client id, asked when the answer to its
/// submission was lost, after a restart, or on a reconciliation cycle (spec 6.3).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Lookup {
    /// The broker has it: its id and where it stands now.
    Found {
        id: String,
        status: otw_connector_model::OrderStatus,
        filled_qty: Decimal,
    },
    /// The broker answered and has never seen that client id.
    NotFound,
}

/// A submission whose outcome is not known: no answer came back and the order could not be
/// looked up. The legs named stay `unknown` until a lookup settles them, and nothing is sent
/// again on their instrument meanwhile. An adapter returns it as the error of `place` or
/// `close_position` (`anyhow::Error::new(OrderUnknown { .. })`).
#[derive(Debug, Clone)]
pub struct OrderUnknown {
    /// Client ids of the legs whose fate is unknown.
    pub keys: Vec<String>,
    pub message: String,
    /// The broker answered in a way the adapter does not map (an unknown error code): the
    /// user is told at once, whatever the lookup then finds (spec 8).
    pub alert: bool,
}

impl std::fmt::Display for OrderUnknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for OrderUnknown {}

/// A leg OpenTraderWorld sent and still follows, as stored (`broker_orders`): what a broker
/// needs to take back up what it linked itself after a restart ([`Broker::resume`]).
#[derive(Debug, Clone, Serialize)]
pub struct StoredLeg {
    pub key: String,
    /// The broker's id, once it accepted the leg.
    pub id: Option<String>,
    /// The chart's spelling of the instrument, as the ticket named it.
    pub ticker: String,
    pub asset_type: String,
    /// The `key` of the entry this exit hangs on.
    pub parent: Option<String>,
    pub group: Option<String>,
    pub status: String,
}

/// One live working order.
#[derive(Debug, Clone, Serialize)]
pub struct OpenOrder {
    pub id: String,
    pub symbol: String,
    /// "buy" | "sell".
    pub side: String,
    /// The broker's own order type, verbatim ("limit", "stop-loss", "LMT"…).
    pub order_type: String,
    pub qty: f64,
    pub filled_qty: f64,
    pub limit_price: Option<f64>,
    pub stop_price: Option<f64>,
    /// A trailing stop's start price, when it was given one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trail_start: Option<f64>,
    /// How far a trailing stop trails, in percent: the chart works out where it trails now
    /// when the broker does not report it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trail_rate: Option<f64>,
    pub currency: String,
    pub asset_class: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub placed_at: Option<OffsetDateTime>,
    pub venue: String,
    pub account: String,
    /// The broker id of the entry this order is an exit of (a bracket's take-profit or
    /// stop-loss), when the broker links them.
    pub parent_id: Option<String>,
    /// It may only shrink the position (reduce-only, a close-position stop): an exit the
    /// broker does not link to an entry.
    #[serde(default)]
    pub reduce_only: bool,
}

/// What changed on an account, the moment the broker says so: what a page following the
/// account live applies to the book it last read. A row is sent whole as it now stands, never
/// as a difference, so an event replaces the row it names.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountEvent {
    /// A working order as it now stands: new, moved, or partly filled. `key` is the id the
    /// caller placed it under, when the broker echoes it back.
    Order { order: OpenOrder, key: Option<String> },
    /// How much of a working order has filled so far.
    Filled { id: String, filled_qty: f64 },
    /// An order that stopped working: `filled`, `cancelled`, `rejected` or `expired`. The book
    /// drops it. `reason` says why, when the broker says (a paper order's rejection).
    Done {
        id: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A position as it now stands, without the figures only a full read gives (mark, result,
    /// entry). A zero `qty` means flat: the book drops it.
    Position { position: Position },
    /// Events may have been missed: read the book again.
    Resync,
}

/// An account's money as the broker states it, in its base currency: the same figures for every
/// broker that answers them, so a chart shows a venue's account and a paper one alike. A
/// figure the broker does not state stays `None` rather than being worked out here.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AccountSummary {
    /// The broker's account id, when it has several (IB's `DU…`), else empty.
    pub account: String,
    pub currency: String,
    /// Net liquidation value: cash plus what the positions are worth.
    pub equity: Option<f64>,
    pub cash: Option<f64>,
    /// Result of the open positions at their marks.
    pub unrealized: Option<f64>,
    /// Result already realized (today's at IB, the account's whole history on paper).
    pub realized: Option<f64>,
    /// Commissions paid, when the broker totals them.
    pub fees: Option<f64>,
    /// What the open positions tie up: the initial margin.
    pub margin_used: Option<f64>,
    pub maintenance_margin: Option<f64>,
    /// Equity not tied up: what a new position may use.
    pub available: Option<f64>,
    /// Equity over the maintenance margin: how far the account is from a margin call.
    pub excess_liquidity: Option<f64>,
    pub buying_power: Option<f64>,
    /// What the positions are worth, long and short alike.
    pub gross_position_value: Option<f64>,
    /// The asked pair's own assets (base, then quote), when the broker holds assets by name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<AssetBalance>,
}

/// One asset the account holds, as the broker states it: free to trade, and locked in orders.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AssetBalance {
    pub asset: String,
    pub free: f64,
    pub locked: f64,
}

/// One line of what an account owns: an asset and how much of it.
///
/// Deliberately not a [`Position`]. A position has a side and a cost basis because it was
/// opened; a spot balance is a quantity of something the account holds, and the exchange
/// says nothing about how it was acquired. Anything the broker does not report stays
/// `None` rather than being filled in with a plausible number.
#[derive(Debug, Clone, Serialize)]
pub struct Holding {
    /// The asset, spelled as the broker spells it: "BTC", "AAPL", "ESZ4".
    pub symbol: String,
    /// The broker's own description, empty when it gives none.
    pub name: String,
    /// Units held. Always positive; a short is reported with `side` = "short".
    pub qty: f64,
    /// "long" | "short". A spot balance is always long: it is owned.
    pub side: String,
    /// Journal asset class: stock / etf / crypto / forex / future / option.
    pub asset_class: String,
    /// Weighted average cost, only when the broker reports one. A crypto exchange does
    /// not: a balance has no cost basis, and inventing one would misstate every gain.
    pub avg_price: Option<f64>,
    /// Currency `avg_price` is expressed in. Empty when there is no price.
    pub currency: String,
    pub venue: String,
    pub account: String,
}

/// What one asset trades at on this venue right now.
///
/// The price is the venue's, and so is the money it is quoted in: a crypto exchange
/// prices BTC in USDT, not in dollars, and `pair` says which market answered so the
/// number is never mistaken for a dollar price nobody asked for.
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    /// The asset, spelled as the holding spells it.
    pub symbol: String,
    /// Last traded price, per unit, in `currency`.
    pub price: f64,
    /// The market's quote currency ("USDT", "USD", "EUR").
    pub currency: String,
    /// The market that answered ("BTCUSDT", "XBT/USD"), for the line to show.
    pub pair: String,
}

/// One order of a ticket, as the chart builds it (`histviz/orders.js`). `key` is the chart's
/// own id for the order: the client order id the broker carries, echoed back with the
/// broker's id so the chart can follow it. Numbers arrive as decimal strings, never floats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderLeg {
    pub key: String,
    pub side: otw_connector_model::Side,
    #[serde(rename = "type")]
    pub order_type: otw_connector_model::OrderType,
    pub qty: Decimal,
    #[serde(default)]
    pub limit: Option<Decimal>,
    #[serde(default)]
    pub stop: Option<Decimal>,
    /// Day when absent.
    #[serde(default)]
    pub tif: Option<otw_connector_model::TifKind>,
    /// Epoch milliseconds, what a `gtd` order needs and only it.
    #[serde(default)]
    pub expires: Option<i64>,
    /// The `key` of the entry this exit is attached to: it works only once that one fills.
    #[serde(default)]
    pub parent: Option<String>,
    /// One-cancels-other group shared by the legs it links.
    #[serde(default)]
    pub group: Option<String>,
    /// Only shrink the position, never open or turn one: for a broker whose order
    /// capabilities say `reduce_only`, refused by the others.
    #[serde(default)]
    pub reduce_only: bool,
    /// Rest only: refused rather than filled at once (`post_only`).
    #[serde(default)]
    pub post_only: bool,
    /// A trailing stop's distance, one of the two: a price amount, or a percentage.
    #[serde(default)]
    pub trail_amount: Option<Decimal>,
    #[serde(default)]
    pub trail_percent: Option<Decimal>,
    /// Iceberg: the size shown on the book.
    #[serde(default)]
    pub display_qty: Option<Decimal>,
    #[serde(default)]
    pub outside_rth: bool,
    /// The price a conditional order triggers on; the broker's default when absent.
    #[serde(default)]
    pub trigger: Option<otw_connector_model::TriggerKind>,
    /// Close whatever is held on the trigger, whatever `qty` says.
    #[serde(default)]
    pub close_position: bool,
    /// Worked by the broker's adaptive algo at this urgency.
    #[serde(default)]
    pub adaptive: Option<otw_connector_model::Urgency>,
}

impl OrderLeg {
    /// A leg with nothing but its side, type and size: no prices, Day, no option.
    pub fn new(key: impl Into<String>, side: otw_connector_model::Side, order_type: otw_connector_model::OrderType, qty: Decimal) -> Self {
        OrderLeg {
            key: key.into(),
            side,
            order_type,
            qty,
            limit: None,
            stop: None,
            tif: None,
            expires: None,
            parent: None,
            group: None,
            reduce_only: false,
            post_only: false,
            trail_amount: None,
            trail_percent: None,
            display_qty: None,
            outside_rth: false,
            trigger: None,
            close_position: false,
            adaptive: None,
        }
    }

    /// The canonical order (connector model, v2) this leg asks for on `instrument`. Whether
    /// it is well formed and one the venue takes is the model's check, made by the adapter.
    pub fn request(&self, instrument: &otw_connector_model::InstrumentId) -> Result<otw_connector_model::OrderRequest> {
        use otw_connector_model::{OrderOpts, Price, Qty, TifKind, TimeInForce, Timestamp, Trail};
        let tif = match self.tif.unwrap_or(TifKind::Day) {
            TifKind::Day => TimeInForce::Day,
            TifKind::Gtc => TimeInForce::Gtc,
            TifKind::Ioc => TimeInForce::Ioc,
            TifKind::Fok => TimeInForce::Fok,
            TifKind::Gtd => TimeInForce::Gtd(
                self.expires
                    .and_then(Timestamp::from_millis)
                    .ok_or_else(|| anyhow!("a good-till-date order needs an expiry"))?,
            ),
        };
        Ok(otw_connector_model::OrderRequest {
            client_order_id: self.key.clone(),
            instrument: instrument.clone(),
            side: self.side,
            order_type: self.order_type,
            qty: Qty::new(self.qty),
            limit: self.limit.map(Price::new),
            stop: self.stop.map(Price::new),
            tif,
            parent: self.parent.clone().filter(|p| !p.is_empty()),
            oca_group: self.group.clone().filter(|g| !g.is_empty()),
            account: None,
            opts: OrderOpts {
                reduce_only: self.reduce_only,
                post_only: self.post_only,
                trail: match (self.trail_amount, self.trail_percent) {
                    (Some(_), Some(_)) => return Err(anyhow!("a trailing distance is an amount or a percentage, not both")),
                    (Some(a), None) => Some(Trail::Amount(Price::new(a))),
                    (None, Some(p)) => Some(Trail::Percent(p)),
                    (None, None) => None,
                },
                display_qty: self.display_qty.map(Qty::new),
                outside_rth: self.outside_rth,
                trigger: self.trigger,
                close_position: self.close_position,
                adaptive: self.adaptive,
            },
        })
    }
}

/// Orders sent together on one instrument, named the way the chart names it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderTicket {
    pub ticker: String,
    pub asset_type: String,
    pub legs: Vec<OrderLeg>,
    /// The chart's data provider and connector: where the paper broker reads the price its
    /// orders fill on. A venue that trades its own market ignores both.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub connector: Option<uuid::Uuid>,
    /// The chart's last price for the instrument: what a price collar (a user risk limit)
    /// measures limits and stops against.
    #[serde(default)]
    pub reference: Option<Decimal>,
    /// When the chart last saw that price, epoch milliseconds: a collar refuses a price too
    /// old to measure from.
    #[serde(default)]
    pub reference_at: Option<i64>,
    /// The user confirmed sending an order like one sent seconds ago (the duplicate limit).
    #[serde(default)]
    pub confirm_duplicate: bool,
}

/// A leg the broker accepted: the chart's key and the broker's order id.
#[derive(Debug, Clone, Serialize)]
pub struct Placed {
    pub key: String,
    pub id: String,
    /// The instrument's price increment as the broker states it, when it does: what a
    /// dragged order line has to snap to before it is moved.
    pub tick: Option<Decimal>,
}

/// What to pull, in broker-independent terms.
pub struct ExecQuery {
    pub from: OffsetDateTime,
    pub to: OffsetDateTime,
    /// Instruments to ask for, in the broker's spelling. Required when the capability says
    /// `needs_symbols`, ignored by brokers that answer the whole account at once.
    pub symbols: Vec<String>,
}

/// A broker account connector: capability metadata, account queries and, for the brokers that
/// trade, the order calls.
#[async_trait::async_trait]
pub trait Broker: Send + Sync {
    fn capability(&self) -> &'static BrokerCapability;

    /// Reach the API and report what answered, or fail with a message naming what to fix.
    async fn test(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<String> {
        Err(anyhow!("{} has no connection test", self.capability().label))
    }

    /// Whether [`Broker::executions`] answers a chart's fill markers. A broker can serve
    /// recent fills without being an import source (`executions` in its capability).
    fn chart_fills(&self) -> bool {
        self.capability().executions
    }

    /// Fills over `[from, to]`. Brokers page internally; the result is the whole window.
    async fn executions(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        _q: &ExecQuery,
    ) -> Result<Vec<Execution>> {
        Err(anyhow!(
            "{} does not publish an execution history",
            self.capability().label
        ))
    }

    /// Instruments worth offering in a symbol picker: the ones this account actually
    /// holds or has worked. A suggestion list, never a filter, so the user may type another.
    async fn symbols(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn positions(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<Vec<Position>> {
        Err(anyhow!("{} does not report positions", self.capability().label))
    }

    /// What the account holds of `ticker` where it holds assets by name rather than
    /// positions (a spot crypto account: the pair's base asset, free and locked). `None`
    /// where its positions say it.
    async fn held_asset(&self, _settings: &HashMap<String, String>, _ticker: &str) -> Result<Option<Decimal>> {
        Ok(None)
    }

    async fn orders(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<Vec<OpenOrder>> {
        Err(anyhow!("{} does not report open orders", self.capability().label))
    }

    /// Live marks of held instruments, by the broker's own id ([`Position::native_id`]), in
    /// each instrument's currency: what a position is valued at between two account reads.
    /// An instrument with no live mark is left out. A broker that streams none answers
    /// nothing, and its positions keep the figures of the last read.
    async fn marks(
        &self,
        _secrets: &HashMap<String, String>,
        _native_ids: &[String],
    ) -> Result<HashMap<String, f64>> {
        Ok(HashMap::new())
    }

    /// The account's money right now, one line per account the connection reaches. `symbol`,
    /// the pair a chart trades, asks for its assets too where the broker holds assets by name.
    async fn account(&self, _settings: &HashMap<String, String>, _symbol: Option<&str>) -> Result<Vec<AccountSummary>> {
        Err(anyhow!("{} does not report the account's balances", self.capability().label))
    }

    /// Follow the account live: each change to its orders and positions as the broker pushes
    /// it. `None` when the broker pushes nothing, and its book is read on a cycle instead.
    async fn events(&self, _settings: &HashMap<String, String>) -> Result<Option<futures::stream::BoxStream<'static, AccountEvent>>> {
        Ok(None)
    }

    /// Everything the account owns right now, one line per asset.
    async fn holdings(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
    ) -> Result<Vec<Holding>> {
        Err(anyhow!(
            "{} does not list what the account holds",
            self.capability().label
        ))
    }

    /// What these assets trade at right now, one line per asset that has a market. An
    /// asset the venue does not price is left out rather than priced from somewhere else:
    /// the point of asking the exchange is that the answer comes from the exchange.
    async fn quotes(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        _symbols: &[String],
    ) -> Result<Vec<Quote>> {
        Err(anyhow!(
            "{} does not publish a live price",
            self.capability().label
        ))
    }

    /// Send a ticket and report each leg's broker id once the
    /// broker has accepted it. A leg refused after others went out cancels those, so a
    /// ticket is never left half placed.
    async fn place(&self, _settings: &HashMap<String, String>, _ticket: &OrderTicket) -> Result<Vec<Placed>> {
        Err(anyhow!("{} does not take orders from OpenTraderWorld", self.capability().label))
    }

    /// Change a working order in place (prices, quantity, time in force). `None` leaves a field
    /// as it is. A broker that can only make the change as a new order (a Binance USDⓈ-M stop)
    /// answers it: its client id and broker id, which replace the old order's.
    async fn modify(&self, _settings: &HashMap<String, String>, _id: &str, _change: &otw_connector_model::OrderAmend) -> Result<Option<Placed>> {
        Err(anyhow!("{} does not take orders from OpenTraderWorld", self.capability().label))
    }

    /// Cancel a working order (and, at the broker, the exits attached to it).
    async fn cancel(&self, _settings: &HashMap<String, String>, _id: &str) -> Result<()> {
        Err(anyhow!("{} does not take orders from OpenTraderWorld", self.capability().label))
    }

    /// How this account spells the instrument a chart shows (`ticker`, `asset_type` in the
    /// chart's words), when the broker can say: a book row with that symbol is the chart's
    /// instrument. Asked of the broker, never guessed; `None` when it cannot tell, and the
    /// chart then matches on its own ticker.
    async fn symbol_for(&self, _settings: &HashMap<String, String>, _ticker: &str, _asset_type: &str) -> Result<Option<String>> {
        Ok(None)
    }

    /// The size step an order on this instrument must be a multiple of, as the broker states
    /// it (Binance's `LOT_SIZE`), so a size worked out from an amount is rounded down to it
    /// before it leaves. `None` when the broker does not say.
    async fn qty_step(&self, _settings: &HashMap<String, String>, _ticker: &str) -> Result<Option<Decimal>> {
        Ok(None)
    }

    /// The leverage and margin mode ("cross" | "isolated") the account set on this instrument,
    /// where the venue sets them per instrument (Binance USDⓈ-M). Read only: OTW changes
    /// neither. `None` when the broker has no such setting.
    async fn leverage(&self, _settings: &HashMap<String, String>, _ticker: &str) -> Result<Option<(Option<Decimal>, Option<String>)>> {
        Ok(None)
    }

    /// Close a position at the market: the other side, for what is held. `native_id` and
    /// `account` are the ones the position was reported with; `key` is the caller's order id.
    async fn close_position(&self, _settings: &HashMap<String, String>, _native_id: &str, _account: &str, _key: &str) -> Result<Placed> {
        Err(anyhow!("{} does not take orders from OpenTraderWorld", self.capability().label))
    }

    /// The kill switch: cancel every working order of the account. Answers how many were
    /// working.
    async fn cancel_all(&self, _settings: &HashMap<String, String>) -> Result<usize> {
        Err(anyhow!("{} does not take orders from OpenTraderWorld", self.capability().label))
    }

    /// What the broker says of the order sent under client id `key` on the chart's instrument
    /// (`ticker`, `asset_type`). An error means it could not be asked, and the order stays
    /// unknown; [`Lookup::NotFound`] only when the broker answered that it never saw the id.
    async fn lookup(&self, _settings: &HashMap<String, String>, _ticker: &str, _asset_type: &str, _key: &str) -> Result<Lookup> {
        Err(anyhow!("{} cannot look an order up by its client id", self.capability().label))
    }

    /// Take back up, after a restart, what this broker links itself on top of the venue (a
    /// Binance USDⓈ-M bracket): `legs` are the account's orders still working, as stored.
    /// Brokers whose venue links its own orders have nothing to do.
    async fn resume(&self, _settings: &HashMap<String, String>, _legs: &[StoredLeg]) -> Result<()> {
        Ok(())
    }

    /// Check a ticket the way the venue would and place nothing: Binance's test route, IB's
    /// what-if. Answers what was checked. The live dry run a maintainer runs before the live
    /// canary (spec 9), on any account; a broker without one refuses.
    async fn dry_run(&self, _settings: &HashMap<String, String>, _ticket: &OrderTicket) -> Result<String> {
        Err(anyhow!("{} has no dry run", self.capability().label))
    }

    /// Whether orders on this account move real money: `false` for a sandbox or paper account.
    /// Asked before each order that opens or changes exposure ([`live`]). A broker that cannot
    /// tell answers `true`, so an account it cannot place is treated as live.
    async fn is_live(&self, _settings: &HashMap<String, String>) -> Result<bool> {
        Ok(true)
    }

    /// What this broker takes in orders (types, time in force, changes, linked orders), when
    /// it states it: the ticket offers no more. `None`: the ticket offers everything and the
    /// broker refuses what it does not take.
    fn order_caps(&self) -> Option<otw_connector_model::OrderCaps> {
        None
    }

    /// Check an account's non-secret settings before they are stored.
    fn validate_config(&self, _config: &HashMap<String, String>) -> Result<()> {
        Ok(())
    }
}

fn all_brokers() -> Vec<Box<dyn Broker>> {
    // A venue whose capabilities do not parse is left out; its provider test makes that a
    // build failure.
    let all: Vec<Option<Box<dyn Broker>>> = vec![
        native::alpaca().map(|b| Box::new(b) as Box<dyn Broker>),
        binance::binance().map(|b| Box::new(b) as Box<dyn Broker>),
        binance::binance_futures().map(|b| Box::new(b) as Box<dyn Broker>),
        native::bitget().map(|b| Box::new(b) as Box<dyn Broker>),
        native::bybit().map(|b| Box::new(b) as Box<dyn Broker>),
        native::bybit_eu().map(|b| Box::new(b) as Box<dyn Broker>),
        native::coinbase().map(|b| Box::new(b) as Box<dyn Broker>),
        native::kraken().map(|b| Box::new(b) as Box<dyn Broker>),
        native::gateio().map(|b| Box::new(b) as Box<dyn Broker>),
        native::kucoin().map(|b| Box::new(b) as Box<dyn Broker>),
        native::kucoin_futures().map(|b| Box::new(b) as Box<dyn Broker>),
        native::mexc().map(|b| Box::new(b) as Box<dyn Broker>),
        native::bitfinex().map(|b| Box::new(b) as Box<dyn Broker>),
        native::deribit().map(|b| Box::new(b) as Box<dyn Broker>),
        native::ibkr_flex().map(|b| Box::new(b) as Box<dyn Broker>),
        ibkr::ibkr().map(|b| Box::new(b) as Box<dyn Broker>),
        native::oanda().map(|b| Box::new(b) as Box<dyn Broker>),
        native::okx().map(|b| Box::new(b) as Box<dyn Broker>),
        native::tradestation().map(|b| Box::new(b) as Box<dyn Broker>),
        native::forexcom().map(|b| Box::new(b) as Box<dyn Broker>),
        native::capitalcom().map(|b| Box::new(b) as Box<dyn Broker>),
        native::ninjatrader().map(|b| Box::new(b) as Box<dyn Broker>),
        Some(Box::new(paper::paper()) as Box<dyn Broker>),
    ];
    all.into_iter().flatten().collect()
}

/// Every broker's capability, for the settings page and the import modal.
pub fn capabilities() -> Vec<&'static BrokerCapability> {
    all_brokers().iter().map(|b| b.capability()).collect()
}

/// Look a broker up by id.
pub fn broker_for(broker: &str) -> Result<Box<dyn Broker>> {
    all_brokers()
        .into_iter()
        .find(|b| b.capability().broker == broker)
        .ok_or_else(|| anyhow!("unknown broker: {broker}"))
}

/// The broker that answers for a stored account. The paper broker keeps its state in the
/// database, so it is bound to the account; every other one is the registry's.
pub fn for_account(pool: &sqlx::PgPool, account: &otw_store::brokers::BrokerRow) -> Result<Box<dyn Broker>> {
    if account.broker == paper::ID {
        return Ok(Box::new(paper::bound(pool.clone(), account.id)));
    }
    broker_for(&account.broker)
}

// The host a connector's requests go to, in tests: a thread pointed at a mock server
// (`serve_from`) sends every venue's calls there (`histdata::native::routed`), so a paging
// loop, a cursor, a rate-limit answer and a refusal wrapped in a 200 are all reachable
// without an account.
#[cfg(test)]
thread_local! {
    static BASE_OVERRIDE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn at(base: &str) -> String {
    BASE_OVERRIDE
        .with(|b| b.borrow().clone())
        .unwrap_or_else(|| base.to_string())
}

/// Send every connector call on this thread to `url` instead of the real venue. One
/// `#[tokio::test]` is one thread, so tests do not leak into each other.
#[cfg(test)]
pub(crate) fn serve_from(url: &str) {
    BASE_OVERRIDE.with(|b| *b.borrow_mut() = Some(url.trim_end_matches('/').to_string()));
}

/// HTTP client for account calls. Longer than the market-data one: a Flex statement is
/// generated on demand and a crypto history call pages server-side.
pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .user_agent("OpenTraderWorld/brokers")
        .build()
        .map_err(|e| anyhow!("building broker http client: {e}"))
}

/// Refuse a period the broker's own archive does not reach.
///
/// `history_days` is not decoration: a venue that keeps three months of fills answers an
/// older window with an empty page and no error at all, which reads as "you traded nothing
/// then". The pull is stopped here instead, with the date it *can* answer from, so the
/// user narrows the period rather than filing a quiet hole in their book.
///
/// A broker whose window is its own ([`BrokerCapability::window_from_broker`]) is exempt:
/// the dates only filter a statement it has already built.
pub fn check_window(cap: &BrokerCapability, from: OffsetDateTime) -> Result<()> {
    if cap.history_days == 0 || cap.window_from_broker {
        return Ok(());
    }
    let earliest = OffsetDateTime::now_utc() - time::Duration::days(cap.history_days as i64);
    if from >= earliest {
        return Ok(());
    }
    Err(anyhow!(
        "{} serves {} days of execution history over its API, so it cannot answer for {}. \
         Start the period on {} or later, and take anything older from the broker's own \
         statement export.",
        cap.label,
        cap.history_days,
        from.date(),
        earliest.date(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A broker id is stored on `broker_accounts.broker`, so two connectors answering to one
    /// id would read each other's accounts. It is also never renamed after a release, which
    /// is why this test exists rather than a comment.
    #[test]
    fn every_broker_id_is_its_own_and_its_flags_agree_with_its_methods() {
        let mut seen: Vec<&str> = Vec::new();
        for cap in capabilities() {
            assert!(!cap.broker.is_empty(), "{}: a broker needs an id", cap.label);
            assert!(
                !seen.contains(&cap.broker),
                "two connectors answer to the broker id {}",
                cap.broker
            );
            assert!(!cap.key_note.is_empty(), "{}: owes a note on what its key may do", cap.broker);
            assert!(
                !cap.asset_classes.is_empty(),
                "{}: says nothing about what it can report",
                cap.broker
            );
            // A broker that must be asked one instrument at a time has to be able to
            // suggest some, or the import modal asks for a list nobody can produce.
            assert!(
                !cap.needs_symbols || cap.executions,
                "{}: needs symbols for a history it does not publish",
                cap.broker
            );
            seen.push(cap.broker);
        }
    }

    fn cap(history_days: u32, window_from_broker: bool) -> BrokerCapability {
        BrokerCapability {
            broker: "test",
            label: "Test",
            website: "",
            docs_url: "",
            rate_limit: "",
            key_note: "",
            required_secrets: &[],
            config_fields: &[],
            asset_classes: &[],
            executions: true,
            needs_symbols: false,
            window_from_broker,
            history_days,
            spot_only: false,
            positions: false,
            orders: false,
            holdings: false,
            quotes: false,
            testable: false,
        }
    }

    #[test]
    fn a_bounded_archive_refuses_a_period_it_cannot_answer() {
        let now = OffsetDateTime::now_utc();
        let c = cap(90, false);
        // Inside the window: nothing to say.
        assert!(check_window(&c, now - time::Duration::days(89)).is_ok());
        // Past it, the refusal names the date to start from rather than answering nothing.
        let e = check_window(&c, now - time::Duration::days(200)).unwrap_err();
        let msg = format!("{e}");
        assert!(msg.contains("90 days"), "{msg}");
        assert!(msg.contains(&(now - time::Duration::days(90)).date().to_string()), "{msg}");
    }

    #[test]
    fn a_chart_leg_becomes_the_canonical_order_it_asks_for() {
        use otw_connector_model::{InstrumentId, OrderType, Side, TimeInForce};
        let leg: OrderLeg = serde_json::from_str(
            r#"{"key":"tp","side":"sell","type":"limit","qty":"2","limit":"30405.25","stop":null,"tif":"gtd","expires":1792598400000,"parent":"e","group":null}"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let id = InstrumentId::new("CME", "MNQ.20261218@CME");
        let r = leg.request(&id).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((r.client_order_id.as_str(), r.side, r.order_type, r.parent.as_deref()), ("tp", Side::Sell, OrderType::Limit, Some("e")));
        assert_eq!(r.limit.map(|p| p.value().to_string()).as_deref(), Some("30405.25"));
        assert!(matches!(r.tif, TimeInForce::Gtd(t) if t.millis() == 1_792_598_400_000));
        // A gtd with no expiry is refused rather than defaulted.
        let bare: OrderLeg = serde_json::from_str(r#"{"key":"e","side":"buy","type":"market","qty":"1","tif":"gtd"}"#).unwrap_or_else(|e| panic!("{e}"));
        assert!(bare.request(&id).is_err());
    }

    #[test]
    fn a_broker_without_a_published_limit_is_left_alone() {
        let old = OffsetDateTime::now_utc() - time::Duration::days(4000);
        // No published limit at all.
        assert!(check_window(&cap(0, false), old).is_ok());
        // A window that is the broker's own: the dates only filter what it already built,
        // so refusing them would refuse a statement that does cover the period.
        assert!(check_window(&cap(365, true), old).is_ok());
    }
}
