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
        }
    }
}
