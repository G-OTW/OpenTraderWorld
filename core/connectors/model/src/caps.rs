//! What an adapter can do, declared as data (`capabilities.toml`). The planner reads only
//! this; anything not declared is `Unsupported`. Unknown at authoring time = not declared.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub use crate::instrument::AssetClass;
use crate::command::DataType;
use crate::timeframe::Timeframe;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Maturity {
    /// Compiles, capabilities declared, PROVIDER.md complete. Hidden.
    M0,
    /// Fixtures for every declared capability, unit + snapshot + property tests green.
    M1,
    /// Mock scenario catalog + conformance suite green. "experimental".
    M2,
    /// Real recordings replay green (kept local, never committed). "beta".
    M3,
    /// Confirmed against the live venue and by user reports. "stable".
    M4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Discovery {
    /// The venue publishes its whole instrument list.
    Full,
    SearchOnly,
    ByIdOnly,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    None,
    ApiKey,
    KeySecretHmac,
    Ed25519,
    OAuth2Refresh,
    Session,
    LocalGateway,
    /// On-chain: a wallet signs.
    Wallet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Paging {
    Cursor,
    Time,
    Offset,
}

/// How a data type is available live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LiveMode {
    Push,
    Poll { min_interval_ms: u64 },
    #[default]
    None,
}

impl LiveMode {
    pub fn is_none(&self) -> bool {
        matches!(self, LiveMode::None)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum LiveModeRepr {
    Name(String),
    Poll { poll_ms: u64 },
}

impl Serialize for LiveMode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            LiveMode::Push => LiveModeRepr::Name("push".into()).serialize(s),
            LiveMode::None => LiveModeRepr::Name("none".into()).serialize(s),
            LiveMode::Poll { min_interval_ms } => LiveModeRepr::Poll { poll_ms: *min_interval_ms }.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for LiveMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match LiveModeRepr::deserialize(d)? {
            LiveModeRepr::Name(n) => match n.as_str() {
                "push" => Ok(LiveMode::Push),
                "none" => Ok(LiveMode::None),
                other => Err(serde::de::Error::custom(format!("live mode `{other}`: push, none or {{ poll_ms = N }}"))),
            },
            LiveModeRepr::Poll { poll_ms } => Ok(LiveMode::Poll { min_interval_ms: poll_ms }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaps {
    #[serde(default)]
    pub trades: LiveMode,
    #[serde(default)]
    pub quote: LiveMode,
    /// Order book: snapshots, plus deltas when `book.delta`.
    #[serde(default)]
    pub book: LiveMode,
    #[serde(default)]
    pub candles: LiveMode,
    /// Timeframes streamed as candles, when narrower than `native_timeframes` (a venue that
    /// streams 1m bars only but serves every resolution as history).
    #[serde(default)]
    pub candle_timeframes: Option<Vec<Timeframe>>,
    /// The venue says when a streamed candle is final. When it does not, the adapter sends
    /// every candle as forming and the engine closes it when the next period starts.
    #[serde(default = "yes")]
    pub candles_final_flag: bool,
    /// Every streamed candle carries its bid and ask (`Candle.sides`), so the forming one's
    /// closes are the current quote with no subscription of its own.
    #[serde(default)]
    pub candle_sides: bool,
}

fn yes() -> bool {
    true
}

impl Default for LiveCaps {
    fn default() -> Self {
        LiveCaps {
            trades: LiveMode::None,
            quote: LiveMode::None,
            book: LiveMode::None,
            candles: LiveMode::None,
            candle_timeframes: None,
            candles_final_flag: true,
            candle_sides: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryCaps {
    pub max_items_per_call: u32,
    /// How far back the venue serves, in days. `None` = from listing.
    #[serde(default)]
    pub max_lookback_days: Option<u32>,
    pub paging: Paging,
    /// Candles only: the timeframes served as history, when narrower than `native_timeframes`.
    #[serde(default)]
    pub timeframes: Option<Vec<Timeframe>>,
    /// Candles only: the venue says which bar is still forming. When it does not, the
    /// adapter sends every bar as forming and the engine finalizes at the period's end.
    #[serde(default = "yes")]
    pub final_flag: bool,
    /// Candles only: how a timeframe folded from a smaller one is stamped. `bucket` = the
    /// bucket's start; `first_bar` = its first source bar's own stamp (a session that opens
    /// at the exchange's midnight, which is what the venue's own bars carry).
    #[serde(default)]
    pub aggregate_stamp: AggregateStamp,
    /// Candles only: every candle carries its bid and ask (`Candle.sides`).
    #[serde(default)]
    pub sides: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateStamp {
    #[default]
    Bucket,
    FirstBar,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryMap {
    #[serde(default)]
    pub candles: Option<HistoryCaps>,
    #[serde(default)]
    pub trades: Option<HistoryCaps>,
    /// REST order book snapshot (lets the planner poll a book).
    #[serde(default)]
    pub book_snapshot: bool,
    /// REST best bid/ask.
    #[serde(default)]
    pub quote: bool,
    /// Last price of every instrument in one call.
    #[serde(default)]
    pub board: bool,
    /// Markets whose past bid/ask can be read at a moment (`RequestQuoteAt`).
    #[serde(default)]
    pub quotes_at: Vec<AssetClass>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookCaps {
    #[serde(default)]
    pub snapshot_depths: Vec<u32>,
    #[serde(default)]
    pub delta: bool,
    #[serde(default)]
    pub has_seq: bool,
    /// Checksum algorithm the venue publishes (`crc32`), when any.
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateScope {
    Global,
    Ip,
    Account,
    Endpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimit {
    pub scope: RateScope,
    /// Endpoint id, for `scope = "endpoint"`.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Tokens (request weight) per window.
    pub capacity: u32,
    pub refill_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountCaps {
    #[serde(default)]
    pub balances: bool,
    #[serde(default)]
    pub positions: bool,
    #[serde(default)]
    pub fills_history: bool,
    #[serde(default)]
    pub open_orders: bool,
    /// A connection test that reports what the key can see.
    #[serde(default)]
    pub summary: bool,
    /// The fill history answers one instrument at a time: the caller names them.
    #[serde(default)]
    pub fills_need_symbols: bool,
    /// A cash account: a sell with nothing open is a sale, never a short.
    #[serde(default)]
    pub spot_only: bool,
    /// Quote assets a holding is valued against, most preferred first.
    #[serde(default)]
    pub quote_preference: Vec<String>,
    /// A holding may also be valued on the reversed pair (USDT on USDCUSDT, at 1/price).
    #[serde(default)]
    pub quote_reverse: bool,
    /// An asset may already be a listed instrument's own symbol (a contract), priced as is.
    #[serde(default)]
    pub quote_self: bool,
    /// A fill history for a pair the venue does not list is refused before any signed call.
    #[serde(default)]
    pub require_listed_pairs: bool,
    /// The account setting `env` value that selects the venue's test environment
    /// (`testnet`, `demo`). Absent = the venue has one environment.
    #[serde(default)]
    pub sandbox_env: Option<String>,
    /// Account rows, the price board and the pairs a holding is valued on name instruments
    /// by their display symbol (`XBT/USD`), not by the market-data id (`XBTUSD`): what OTW
    /// stored for this venue's accounts before the port.
    #[serde(default)]
    pub display_symbols: bool,
    /// What an account row's instrument is comes from asking the venue one id at a time
    /// (`ResolveInstruments`), not from a full list.
    #[serde(default)]
    pub resolve_by_id: bool,
    /// Holdings are the positions, with their cost basis (a securities account), not
    /// wallet balances.
    #[serde(default)]
    pub holdings_from_positions: bool,
    /// A holding is valued at the venue's own mark on its position.
    #[serde(default)]
    pub quotes_from_positions: bool,
    /// A holding is valued at the mid of the venue's own bid and ask, asked per instrument
    /// with the account's key (a dealing desk prices what it deals, no public board).
    #[serde(default)]
    pub quotes_from_snapshots: bool,
    /// With `quotes_from_snapshots`: the price is the last trade, not the mid.
    #[serde(default)]
    pub quotes_last_trade: bool,
    /// How the venue names itself on account rows, when not the label.
    #[serde(default)]
    pub venue_label: Option<String>,
    /// Every account row names its own class, exchange, currency and point value (a
    /// statement that describes each line): no instrument is looked up for it.
    #[serde(default)]
    pub rows_carry_terms: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Regions {
    /// ISO 3166 codes the venue refuses. Informational, shown in the UI.
    #[serde(default)]
    pub blocked: Vec<String>,
    #[serde(default)]
    pub sandbox_url: Option<String>,
    /// The base URL key the sandbox replaces (`rest` when absent): a venue whose account and
    /// market data live on different hosts moves only the account's.
    #[serde(default)]
    pub sandbox_base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// OTW provider id (`binance`): the key the connector list stores.
    pub provider: String,
    pub label: String,
    pub venue: String,
    pub api_version: String,
    pub maturity: Maturity,
    pub asset_classes: BTreeSet<AssetClass>,
    pub instrument_discovery: Discovery,
    pub auth: AuthKind,
    #[serde(default)]
    pub native_timeframes: Vec<Timeframe>,
    #[serde(default)]
    pub live: LiveCaps,
    #[serde(default)]
    pub history: HistoryMap,
    #[serde(default)]
    pub book: BookCaps,
    #[serde(default)]
    pub max_subs_per_connection: Option<u32>,
    #[serde(default)]
    pub max_connections: Option<u32>,
    #[serde(default)]
    pub rate_limits: Vec<RateLimit>,
    #[serde(default)]
    pub account: AccountCaps,
    #[serde(default)]
    pub regions: Regions,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapsError {
    #[error("capabilities.toml does not parse: {0}")]
    Parse(String),
    #[error("capabilities are inconsistent: {}", .0.join("; "))]
    Invalid(Vec<String>),
}

impl Capabilities {
    /// Parse and validate. Adapters call this on their embedded `capabilities.toml`; a test
    /// per adapter makes a bad file a build failure rather than a startup one.
    pub fn from_toml(src: &str) -> Result<Self, CapsError> {
        let caps: Capabilities = toml::from_str(src).map_err(|e| CapsError::Parse(e.to_string()))?;
        caps.validate()?;
        Ok(caps)
    }

    pub fn validate(&self) -> Result<(), CapsError> {
        let mut errs = Vec::new();
        if self.provider.trim().is_empty() || self.label.trim().is_empty() || self.venue.trim().is_empty() {
            errs.push("provider, label and venue are required".to_string());
        }
        if self.asset_classes.is_empty() {
            errs.push("asset_classes is empty".into());
        }
        let candles_declared = !self.live.candles.is_none() || self.history.candles.is_some();
        if candles_declared && self.native_timeframes.is_empty() {
            errs.push("candles are declared but native_timeframes is empty".into());
        }
        if let Some(h) = &self.history.candles {
            if let Some(tfs) = &h.timeframes {
                for tf in tfs {
                    if !self.native_timeframes.contains(tf) {
                        errs.push(format!("history.candles.timeframes has {tf}, not in native_timeframes"));
                    }
                }
            }
        }
        if let Some(tfs) = &self.live.candle_timeframes {
            for tf in tfs {
                if !self.native_timeframes.contains(tf) {
                    errs.push(format!("live.candle_timeframes has {tf}, not in native_timeframes"));
                }
            }
        }
        for (name, h) in [("candles", &self.history.candles), ("trades", &self.history.trades)] {
            if let Some(h) = h {
                if h.max_items_per_call == 0 {
                    errs.push(format!("history.{name}.max_items_per_call is 0"));
                }
            }
        }
        if (self.book.delta || !self.book.snapshot_depths.is_empty()) && self.live.book.is_none() && !self.history.book_snapshot {
            errs.push("book is described but neither live.book nor history.book_snapshot is declared".into());
        }
        if self.book.delta && !matches!(self.live.book, LiveMode::Push) {
            errs.push("book.delta needs live.book = \"push\"".into());
        }
        for (name, m) in [
            ("trades", self.live.trades),
            ("quote", self.live.quote),
            ("book", self.live.book),
            ("candles", self.live.candles),
        ] {
            if let LiveMode::Poll { min_interval_ms } = m {
                if min_interval_ms == 0 {
                    errs.push(format!("live.{name} polls every 0 ms"));
                }
            }
        }
        for r in &self.rate_limits {
            if r.capacity == 0 || r.refill_ms == 0 {
                errs.push("a rate limit has zero capacity or refill".into());
            }
            if r.scope == RateScope::Endpoint && r.endpoint.is_none() {
                errs.push("an endpoint-scoped rate limit names no endpoint".into());
            }
        }
        if errs.is_empty() {
            Ok(())
        } else {
            Err(CapsError::Invalid(errs))
        }
    }

    pub fn live_mode(&self, dt: &DataType) -> LiveMode {
        match dt {
            DataType::Trades => self.live.trades,
            DataType::Quote => self.live.quote,
            DataType::BookSnapshot { .. } => self.live.book,
            DataType::BookDelta => {
                if self.book.delta {
                    self.live.book
                } else {
                    LiveMode::None
                }
            }
            DataType::Candles { .. } => self.live.candles,
            _ => LiveMode::None,
        }
    }

    /// Timeframes streamed as live candles (empty when candles are not live).
    pub fn live_timeframes(&self) -> &[Timeframe] {
        if self.live.candles.is_none() {
            return &[];
        }
        match &self.live.candle_timeframes {
            Some(t) => t,
            None => &self.native_timeframes,
        }
    }

    /// Timeframes served as candle history.
    pub fn history_timeframes(&self) -> &[Timeframe] {
        match &self.history.candles {
            Some(HistoryCaps { timeframes: Some(t), .. }) => t,
            Some(_) => &self.native_timeframes,
            None => &[],
        }
    }

    /// Whether `self` only removes things from `base` (runtime narrowing is allowed, widening
    /// is not).
    pub fn narrows(&self, base: &Capabilities) -> bool {
        let live_ok = |a: LiveMode, b: LiveMode| match (a, b) {
            (LiveMode::None, _) => true,
            (LiveMode::Push, LiveMode::Push) => true,
            (LiveMode::Poll { min_interval_ms: x }, LiveMode::Poll { min_interval_ms: y }) => x >= y,
            (LiveMode::Poll { .. }, LiveMode::Push) => true,
            _ => false,
        };
        self.asset_classes.is_subset(&base.asset_classes)
            && self.native_timeframes.iter().all(|t| base.native_timeframes.contains(t))
            && self.live_timeframes().iter().all(|t| base.live_timeframes().contains(t))
            && live_ok(self.live.trades, base.live.trades)
            && live_ok(self.live.quote, base.live.quote)
            && live_ok(self.live.book, base.live.book)
            && live_ok(self.live.candles, base.live.candles)
            && (!self.live.candle_sides || base.live.candle_sides)
            && (self.history.candles.is_none() || base.history.candles.is_some())
            && (self.history.trades.is_none() || base.history.trades.is_some())
            && (!self.history.book_snapshot || base.history.book_snapshot)
            && (!self.history.quote || base.history.quote)
            && (!self.book.delta || base.book.delta)
            && (!self.account.balances || base.account.balances)
            && (!self.account.positions || base.account.positions)
            && (!self.account.fills_history || base.account.fills_history)
            && (!self.account.open_orders || base.account.open_orders)
            && (!self.history.board || base.history.board)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
provider = "demo"
label = "Demo"
venue = "DEMO"
api_version = "v1"
maturity = "M0"
asset_classes = ["spot"]
instrument_discovery = "full"
auth = "none"
native_timeframes = ["1m", "1h"]

[live]
trades = "push"
candles = { poll_ms = 2000 }

[history.candles]
max_items_per_call = 1000
paging = "time"

[[rate_limits]]
scope = "ip"
capacity = 1200
refill_ms = 60000
"#;

    #[test]
    fn parses_a_minimal_file() {
        let c = Capabilities::from_toml(MINIMAL).unwrap();
        assert_eq!(c.live.trades, LiveMode::Push);
        assert_eq!(c.live.candles, LiveMode::Poll { min_interval_ms: 2000 });
        assert_eq!(c.live.quote, LiveMode::None);
        assert_eq!(c.history_timeframes(), &[Timeframe::M1, Timeframe::H1]);
    }

    #[test]
    fn rejects_unknown_keys_and_inconsistencies() {
        assert!(matches!(Capabilities::from_toml(&format!("{MINIMAL}\nbogus = 1\n")), Err(CapsError::Parse(_))));
        let no_tf = MINIMAL.replace("native_timeframes = [\"1m\", \"1h\"]", "");
        assert!(matches!(Capabilities::from_toml(&no_tf), Err(CapsError::Invalid(_))));
        let delta = format!("{MINIMAL}\n[book]\ndelta = true\n");
        assert!(matches!(Capabilities::from_toml(&delta), Err(CapsError::Invalid(_))));
    }

    #[test]
    fn narrowing_never_widens() {
        let base = Capabilities::from_toml(MINIMAL).unwrap();
        let mut narrower = base.clone();
        narrower.live.trades = LiveMode::None;
        assert!(narrower.narrows(&base));
        let mut wider = base.clone();
        wider.live.quote = LiveMode::Push;
        assert!(!wider.narrows(&base));
        let mut faster_poll = base.clone();
        faster_poll.live.candles = LiveMode::Poll { min_interval_ms: 100 };
        assert!(!faster_poll.narrows(&base));
    }
}
