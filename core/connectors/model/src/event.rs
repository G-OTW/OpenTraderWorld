//! Events: adapter to engine, then engine to modules. Every one travels in an [`Envelope`].

use serde::{Deserialize, Serialize};

use crate::caps::Capabilities;
use crate::error::ConnectorError;
use crate::id::{ConnectorId, CorrId, InstrumentId};
use crate::instrument::{InstrumentSpec, Mapped};
use crate::num::{Money, Price, Qty, Timestamp};
use crate::timeframe::Timeframe;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnState {
    Connecting,
    Connected,
    Degraded,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggressor {
    Buy,
    Sell,
}

/// Set by an engine stage when the data is not what the venue sent as such.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedFrom {
    /// Candles folded from trades.
    CandlesFromTrades,
    /// Candles folded from a smaller native timeframe.
    CandlesFromCandles,
    /// Candles folded from the quote mid (no tape).
    Mid,
    /// Top of a maintained book.
    QuoteFromBook,
    /// Live emulated by polling a REST endpoint.
    Polled,
    /// Positions rebuilt from the fill history.
    FromFills,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candle {
    pub instrument: InstrumentId,
    pub tf: Timeframe,
    /// Open time: the bar covers `[open_ts, open_ts + tf)`.
    pub open_ts: Timestamp,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Qty,
    pub quote_volume: Option<Money>,
    pub trades: Option<u64>,
    /// The period is over and the values will not move again.
    pub is_final: bool,
    /// Prices adjusted for splits and dividends, when the venue publishes them next to the
    /// traded ones.
    #[serde(default)]
    pub adjusted: Option<Adjusted>,
    /// Bid and ask prices over the period, when the venue publishes both sides next to the
    /// mid (a dealing desk's candles).
    #[serde(default)]
    pub sides: Option<Sides>,
}

/// Split- and dividend-adjusted prices of one candle; a venue fills what it publishes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adjusted {
    pub open: Option<Price>,
    pub high: Option<Price>,
    pub low: Option<Price>,
    pub close: Option<Price>,
}

/// One side's prices over a candle's period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ohlc {
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
}

/// The bid and the ask of one candle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sides {
    pub bid: Ohlc,
    pub ask: Ohlc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trade {
    pub instrument: InstrumentId,
    pub id: Option<String>,
    pub price: Price,
    pub qty: Qty,
    pub aggressor: Option<Mapped<Aggressor>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    pub instrument: InstrumentId,
    pub bid: Price,
    pub ask: Price,
    pub bid_qty: Option<Qty>,
    pub ask_qty: Option<Qty>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: Price,
    /// Absolute quantity at this level; zero in a delta deletes it.
    pub qty: Qty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

/// One execution of the account, as the venue reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fill {
    /// The venue's execution id: what a re-sync deduplicates on.
    pub id: String,
    pub order_id: Option<String>,
    pub instrument: InstrumentId,
    pub ts: Timestamp,
    pub side: Mapped<Side>,
    pub qty: Qty,
    pub price: Price,
    /// Positive = cost.
    pub fee: Money,
    pub fee_asset: Option<String>,
    /// The instrument's quote currency, when known.
    pub quote_ccy: Option<String>,
    /// The sub-account it belongs to, when the venue names one (a Coinbase portfolio).
    #[serde(default)]
    pub account: Option<String>,
    /// The exchange the fill printed on, when the venue says.
    #[serde(default)]
    pub venue: Option<String>,
    /// What the instrument is, when the row itself says (a venue whose only clue is the
    /// row: a market name written as a currency pair).
    #[serde(default)]
    pub class: Option<crate::AssetClass>,
    /// What one unit of quantity is worth, when the venue writes it next to the fill (a
    /// futures point value the venue answers per contract, with no instrument list).
    #[serde(default)]
    pub multiplier: Option<rust_decimal::Decimal>,
}

/// One working order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub instrument: InstrumentId,
    pub side: Mapped<Side>,
    /// The venue's own order type, verbatim.
    pub order_type: String,
    pub qty: Qty,
    pub filled_qty: Qty,
    pub limit_price: Option<Price>,
    pub stop_price: Option<Price>,
    pub placed_at: Option<Timestamp>,
    pub quote_ccy: Option<String>,
    /// The sub-account it belongs to, when the venue names one.
    #[serde(default)]
    pub account: Option<String>,
    /// Where it works, when the venue names it.
    #[serde(default)]
    pub venue: Option<String>,
    /// What the instrument is, when the row itself says (a venue whose only clue is the
    /// row: a market name written as a currency pair).
    #[serde(default)]
    pub class: Option<crate::AssetClass>,
}

/// What a key can see of its account, for a connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSummary {
    pub account_type: Option<String>,
    pub permissions: Vec<String>,
    pub assets_held: u32,
    pub can_trade: Option<bool>,
    /// Margin accounts: positions open right now.
    pub open_positions: Option<u32>,
    /// Margin accounts: the wallet balance, as the venue writes it.
    pub wallet_balance: Option<String>,
    /// The venue's own sentence about the account, when it has more to say than the fields.
    pub detail: Option<String>,
}

/// Last traded price of one instrument, from a venue's whole board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastPrice {
    pub instrument: InstrumentId,
    pub price: Price,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String,
    pub total: Money,
    pub available: Money,
    /// The venue's own name for the wallet, when it gives one.
    #[serde(default)]
    pub name: Option<String>,
    /// A fiat currency wallet rather than a crypto asset.
    #[serde(default)]
    pub fiat: bool,
    /// The sub-account it belongs to, when the venue names one.
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub instrument: InstrumentId,
    /// Signed: negative is short. In the venue's own unit (contracts on a derivative).
    pub qty: Qty,
    pub avg_price: Option<Price>,
    pub unrealized_pnl: Option<Money>,
    /// The currency the venue reports the position in, when it says.
    pub currency: Option<String>,
    /// The venue's own mark for the instrument, when it prices its positions.
    #[serde(default)]
    pub mark_price: Option<Price>,
    /// The exchange the instrument trades on, when the venue says.
    #[serde(default)]
    pub venue: Option<String>,
    /// The sub-account it belongs to, when the venue names one.
    #[serde(default)]
    pub account: Option<String>,
    /// What the instrument is, when the row itself says (a venue whose only clue is the
    /// row: a market name written as a currency pair).
    #[serde(default)]
    pub class: Option<crate::AssetClass>,
    /// What one unit of quantity is worth, when the venue writes it on the position itself
    /// (a CFD's contract size).
    #[serde(default)]
    pub multiplier: Option<rust_decimal::Decimal>,    /// The venue's own name for the instrument, when the row says (a statement line's
    /// description).
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "items")]
pub enum HistoryItems {
    Candles(Vec<Candle>),
    Trades(Vec<Trade>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "items")]
pub enum AccountItems {
    Fills(Vec<Fill>),
    Orders(Vec<Order>),
    Balances(Vec<Balance>),
    Positions(Vec<Position>),
}

impl HistoryItems {
    pub fn len(&self) -> usize {
        match self {
            HistoryItems::Candles(v) => v.len(),
            HistoryItems::Trades(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Event {
    ConnectionState { state: ConnState, reason: Option<String> },
    /// Runtime narrowing of the static capabilities (account tier, data plan). Never widens.
    CapabilitiesUpdated { caps: Box<Capabilities> },
    Instrument { spec: Box<InstrumentSpec> },
    /// End of an instrument list or search answer.
    InstrumentsEnd,
    Trade(Trade),
    Quote(Quote),
    BookSnapshot { instrument: InstrumentId, seq: Option<u64>, bids: Vec<BookLevel>, asks: Vec<BookLevel>, checksum: Option<u32> },
    BookDelta { instrument: InstrumentId, seq: Option<u64>, prev_seq: Option<u64>, bids: Vec<BookLevel>, asks: Vec<BookLevel> },
    Candle(Candle),
    Balance(Balance),
    Position(Position),
    SubscriptionAck,
    SubscriptionFail { error: ConnectorError },
    SubscriptionEnd,
    HistoryChunk { items: HistoryItems, next_cursor: Option<String>, done: bool },
    /// A page of an account query; `next_cursor` continues it, like a history.
    AccountChunk { items: AccountItems, next_cursor: Option<String>, done: bool },
    AccountSummary(AccountSummary),
    /// The venue's own clock, for signatures that must carry it.
    ServerTime(Timestamp),
    /// Last prices of every listed instrument.
    Board(Vec<LastPrice>),
    Error(ConnectorError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub connector: ConnectorId,
    /// The `sub_id` or `req_id` this event answers; `None` for connection-wide events.
    pub corr_id: Option<CorrId>,
    pub exchange_ts: Option<Timestamp>,
    pub received_ts: Timestamp,
    pub seq: Option<u64>,
    pub derived: Option<DerivedFrom>,
    pub raw_ref: Option<String>,
    pub event: Event,
}

impl Envelope {
    pub fn new(connector: ConnectorId, corr_id: Option<CorrId>, received_ts: Timestamp, event: Event) -> Self {
        Envelope { connector, corr_id, exchange_ts: None, received_ts, seq: None, derived: None, raw_ref: None, event }
    }
    pub fn at(mut self, exchange_ts: Timestamp) -> Self {
        self.exchange_ts = Some(exchange_ts);
        self
    }
    pub fn derived(mut self, d: DerivedFrom) -> Self {
        self.derived = Some(d);
        self
    }
}
