//! The canonical model: the only language spoken between the engine, the OTW modules and the
//! provider adapters. No I/O, no clock, no provider name in here.
//!
//! An adapter translates its venue's bytes into these types and nothing else; everything a
//! venue does differently (paging, symbols, units, error codes) stays on its side of the line.

pub mod caps;
pub mod command;
pub mod error;
pub mod event;
pub mod id;
pub mod instrument;
pub mod num;
pub mod timeframe;

pub use caps::{
    AggregateStamp,
    AccountCaps, AuthKind, BookCaps, Capabilities, CapsError, Discovery, HistoryCaps, HistoryMap,
    LiveCaps, LiveMode, Maturity, Paging, RateLimit, RateScope, Regions,
};
pub use command::{AccountKind, Command, DataType, TimeRange};
pub use error::{ConnectorError, ErrorKind};
pub use event::{
    Adjusted, Ohlc, Sides,
    AccountItems, AccountSummary, Aggressor, Balance, BookLevel, Candle, ConnState, DerivedFrom, Envelope, Event,
    Fill, HistoryItems, LastPrice, Order, Position, Quote, Side, Trade,
};
pub use id::{CanonicalSymbol, ConnectorId, CorrId, InstrumentId, VenueId};
pub use instrument::{AssetClass, ContractType, InstrumentSpec, Mapped, OptionRight};
pub use num::{Money, Price, Qty, Timestamp};
pub use rust_decimal::Decimal;
pub use timeframe::{TfUnit, Timeframe};
