//! Commands: engine to adapter.

use serde::{Deserialize, Serialize};

use crate::id::{CorrId, InstrumentId};
use crate::instrument::AssetClass;
use crate::num::Timestamp;
use crate::timeframe::Timeframe;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum DataType {
    Trades,
    /// Level 1: best bid and ask.
    Quote,
    BookSnapshot { depth: u32 },
    BookDelta,
    Candles { tf: Timeframe },
    FundingRate,
    OpenInterest,
    News,
    Custom { name: String },
}

/// Half-open `[from, to)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeRange {
    pub from: Timestamp,
    pub to: Timestamp,
}

impl TimeRange {
    pub fn new(from: Timestamp, to: Timestamp) -> Self {
        TimeRange { from, to }
    }
    pub fn contains(&self, t: Timestamp) -> bool {
        t >= self.from && t < self.to
    }
    pub fn is_empty(&self) -> bool {
        self.to <= self.from
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    Balances,
    Positions,
    Fills,
    OpenOrders,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Command {
    Connect,
    Disconnect,
    SearchInstruments { req_id: CorrId, query: String, asset_class: Option<AssetClass> },
    LoadInstruments { req_id: CorrId },
    Subscribe { sub_id: CorrId, instrument: InstrumentId, data_type: DataType },
    Unsubscribe { sub_id: CorrId },
    /// `cursor` resumes a paged history where an earlier chunk left off.
    RequestHistory {
        req_id: CorrId,
        instrument: InstrumentId,
        data_type: DataType,
        range: TimeRange,
        cursor: Option<String>,
    },
    /// One REST snapshot of a live data type (best bid/ask, a book): what a poller asks for.
    RequestSnapshot { req_id: CorrId, instrument: InstrumentId, data_type: DataType },
    /// `symbols` for venues that answer one instrument at a time; `range` for fills;
    /// `cursor` continues a paged answer.
    QueryAccount {
        req_id: CorrId,
        kind: AccountKind,
        symbols: Vec<InstrumentId>,
        range: Option<TimeRange>,
        cursor: Option<String>,
        /// Which account of a login that holds several, as the connector's setting names it.
        /// `None`: the login's only one, or the venue's own default.
        #[serde(default)]
        account: Option<String>,
    },
    /// The venue's clock.
    SyncTime { req_id: CorrId },
    /// Last price of every listed instrument, in one call.
    RequestBoard { req_id: CorrId },
    /// The best bid/ask in force at `at`: the first quote at or after it, or the last one
    /// before it when `before`. Answered by a `Quote` event stamped with its own time, or
    /// nothing when the venue kept none.
    RequestQuoteAt { req_id: CorrId, instrument: InstrumentId, at: Timestamp, before: bool },
    /// What these instruments are, asked one by one (a venue whose full list is too large
    /// or not published). Answered by `Instrument` events; an unknown id is left out.
    ResolveInstruments { req_id: CorrId, ids: Vec<InstrumentId> },
}

impl Command {
    pub fn corr_id(&self) -> Option<CorrId> {
        match self {
            Command::Connect | Command::Disconnect => None,
            Command::SearchInstruments { req_id, .. }
            | Command::LoadInstruments { req_id }
            | Command::RequestHistory { req_id, .. }
            | Command::RequestSnapshot { req_id, .. }
            | Command::QueryAccount { req_id, .. }
            | Command::SyncTime { req_id }
            | Command::RequestBoard { req_id }
            | Command::RequestQuoteAt { req_id, .. }
            | Command::ResolveInstruments { req_id, .. } => Some(*req_id),
            Command::Subscribe { sub_id, .. } | Command::Unsubscribe { sub_id } => Some(*sub_id),
        }
    }
}
