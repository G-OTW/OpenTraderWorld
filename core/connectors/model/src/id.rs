//! Identity. An instrument is a venue plus the venue's own symbol, never a string parsed at
//! runtime; the display form is for people and search only.

use std::fmt;

use serde::{Deserialize, Serialize};

/// One configured connector (a row of OTW's connector list).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnectorId(pub String);

/// A venue: the ISO 10383 MIC when one exists (`XNAS`, `XCME`), otherwise the provider's
/// own upper-case name (`BINANCE`, `OANDA`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VenueId(pub String);

/// Primary key of an instrument: venue + native symbol, exactly as the venue spells it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstrumentId {
    pub venue: VenueId,
    pub native: String,
}

impl InstrumentId {
    pub fn new(venue: &str, native: &str) -> Self {
        InstrumentId { venue: VenueId(venue.to_string()), native: native.to_string() }
    }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.venue.0, self.native)
    }
}

/// Display form (`BTC/USDT`, `AAPL`, `ESZ6`). Never used as a key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanonicalSymbol(pub String);

/// Correlates an event with the command that caused it: a subscription id or a request id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CorrId(pub u64);

impl fmt::Display for CorrId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}
