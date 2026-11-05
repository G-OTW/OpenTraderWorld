//! What an instrument is, as stored in the instrument master.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::id::{CanonicalSymbol, InstrumentId};
use crate::num::{Price, Qty, Timestamp};

/// A native enum value after mapping: either one of ours, or the raw value the venue sent
/// that we have no name for. Never a silent default.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mapped<T> {
    Known(T),
    Unknown(String),
}

impl<T> Mapped<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Mapped::Known(t) => Some(t),
            Mapped::Unknown(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    /// Crypto spot pair.
    Spot,
    /// Crypto perpetual swap.
    Perp,
    /// Dated future (crypto or listed).
    Future,
    Option,
    Equity,
    Etf,
    Index,
    Forex,
    Cfd,
    Bond,
    Commodity,
    /// None of the above, as the venue itself says (a calendar spread, a market internal):
    /// filed as what it is not would be a guess.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractType {
    Spot,
    Perp,
    Future,
    Option,
    Cfd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionRight {
    Call,
    Put,
}

/// A notional bound (price times size) a venue sets on an order (spec 4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotionalBound {
    pub value: Decimal,
    /// It holds for market orders too, valued at the reference price.
    pub market: bool,
    /// The minutes of the average price a market order is valued at when the venue has no
    /// reference price for the instrument.
    pub avg_mins: Option<u32>,
    /// The venue's name for the rule, as a refusal names it (`MIN_NOTIONAL`).
    pub filter: String,
}

/// A price band around the reference price, per side: `(floor, cap)` as multipliers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceBand {
    pub buy: (Option<Decimal>, Option<Decimal>),
    pub sell: (Option<Decimal>, Option<Decimal>),
    pub avg_mins: Option<u32>,
    pub filter: String,
}

/// The trading rules a venue states for an instrument beyond its tick and lot (spec 4.1),
/// loaded with the instrument list and checked before an order leaves. A rule the venue does
/// not state is `None`: nothing is checked against it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradingRules {
    pub min_price: Option<Decimal>,
    pub max_price: Option<Decimal>,
    pub max_qty: Option<Decimal>,
    /// A market order's own size grid, when the venue sets one apart.
    pub market_min_qty: Option<Decimal>,
    pub market_max_qty: Option<Decimal>,
    pub market_step: Option<Decimal>,
    pub min_notional: Option<NotionalBound>,
    pub max_notional: Option<NotionalBound>,
    pub price_band: Option<PriceBand>,
    /// Open orders per account on the instrument, conditional ones included.
    pub max_orders: Option<u32>,
    /// Open conditional orders per account on the instrument.
    pub max_algo_orders: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentSpec {
    pub id: InstrumentId,
    pub symbol: CanonicalSymbol,
    pub asset_class: AssetClass,
    /// Base asset for a pair, the underlying for a derivative, the ticker for an equity.
    pub base: Option<String>,
    /// Quote currency: the unit every price of this instrument is in.
    pub quote: String,
    pub tick_size: Option<Price>,
    pub lot_size: Option<Qty>,
    pub min_qty: Option<Qty>,
    /// Base units per contract. 1 for spot and equities.
    pub multiplier: Decimal,
    pub contract: ContractType,
    /// Inverse contract: quantity is quoted in the quote currency, settled in the base.
    pub inverse: bool,
    pub expiry: Option<Timestamp>,
    pub strike: Option<Price>,
    pub right: Option<OptionRight>,
    pub settlement_ccy: Option<String>,
    /// IANA zone the venue's sessions are delimited in (`America/New_York`). `None` = 24/7 UTC.
    pub session_tz: Option<String>,
    /// Whether the venue still lists it for trading.
    pub active: bool,
    /// The venue's own word for the listing (a contract type, a segment), when it has one.
    pub note: Option<String>,
    /// The exchange it is listed on, when the venue says (`NASDAQ`, `ARCA`).
    #[serde(default)]
    pub exchange: Option<String>,
    /// Trading rules beyond tick and lot, as the venue's list states them.
    #[serde(default)]
    pub rules: TradingRules,
}

impl InstrumentSpec {
    /// The minimal spot pair most crypto venues describe.
    pub fn spot(id: InstrumentId, base: &str, quote: &str) -> Self {
        InstrumentSpec {
            symbol: CanonicalSymbol(format!("{base}/{quote}")),
            id,
            asset_class: AssetClass::Spot,
            base: Some(base.to_string()),
            quote: quote.to_string(),
            tick_size: None,
            lot_size: None,
            min_qty: None,
            multiplier: Decimal::ONE,
            contract: ContractType::Spot,
            inverse: false,
            expiry: None,
            strike: None,
            right: None,
            settlement_ccy: None,
            session_tz: None,
            active: true,
            note: None,
            exchange: None,
            rules: TradingRules::default(),
        }
    }
}
