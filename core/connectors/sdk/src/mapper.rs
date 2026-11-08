//! The provider side of the contract: pure translation.

use otw_connector_model::{Capabilities, Command, ConnectorError, ConnectorId, CorrId, Envelope, InstrumentId, Timestamp};

use std::collections::HashMap;
use std::sync::Arc;

use crate::native::{NativeRequest, RawFrame};
use crate::signer::Signer;
use crate::symbols::InstrumentCache;

/// What a mapper may read besides its own session: the receive time (injected, never read
/// from the clock) and a read-only snapshot of the instruments.
pub struct MapCtx<'a> {
    pub received_ts: Timestamp,
    pub instruments: &'a InstrumentCache,
    pub connector: &'a ConnectorId,
}

/// Pure translation between canonical commands/events and the venue's wire format.
///
/// No I/O, no clock reads, no randomness, no secrets. Paging is expressed through the
/// cursor: a `HistoryChunk` carrying `next_cursor` is answered by the driver re-encoding the
/// same `RequestHistory` with that cursor, so a mapper never loops and a download can resume.
pub trait Mapper: Send + Sync + 'static {
    /// Explicit mutable state: pending requests, channel ids, last sequence numbers.
    type Session: Default + Send;

    fn capabilities(&self) -> &Capabilities;

    /// Base URLs by key, as the venue publishes them. The driver can override any of them.
    fn base_urls(&self) -> &'static [(&'static str, &'static str)];

    fn encode(&self, cmd: &Command, s: &mut Self::Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError>;

    fn decode(&self, frame: &RawFrame, s: &mut Self::Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError>;

    /// Several commands at once (panes joining together). The default encodes them one by
    /// one; a venue that takes a list in one frame coalesces them, which matters where the
    /// venue limits frames per second.
    fn encode_batch(&self, cmds: &[Command], s: &mut Self::Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        let mut out = Vec::new();
        for c in cmds {
            out.extend(self.encode(c, s, ctx)?);
        }
        Ok(out)
    }

    /// A symbol as OTW stores it or a user typed it, to the venue's id. Exact or an error
    /// naming the fix: never a best-effort match.
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError>;

    /// [`Mapper::instrument_id`] when the caller also says which market it means (a venue
    /// that serves stocks, crypto and options on different endpoints). The default ignores
    /// the market.
    fn instrument_for(&self, symbol: &str, _market: Option<otw_connector_model::AssetClass>) -> Result<InstrumentId, ConnectorError> {
        self.instrument_id(symbol)
    }

    /// The venue writes options its own way and reads that spelling (`AAPL 251219C150`): an
    /// option symbol that is not OCC is handed over as typed rather than refused upstream.
    fn own_option_symbols(&self) -> bool {
        false
    }

    /// The connector setting naming which account of a login to read, for a venue whose
    /// login may hold several (`QueryAccount.account`).
    fn account_setting(&self) -> Option<&'static str> {
        None
    }

    /// The `QueryAccount.account` value for an account's settings: the
    /// [`Mapper::account_setting`] one by default. A venue whose read is chosen by more than
    /// one setting (a saved query and the zone its statement is stamped in) writes them
    /// together here and reads them back in `encode`.
    fn account_selector(&self, settings: &HashMap<String, String>) -> Option<String> {
        let key = self.account_setting()?;
        settings.get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    }

    /// Requests that follow from answers already decoded for `corr_id` (a lookup whose
    /// answer names what to ask next), sent by the driver within the same command.
    fn follow_ups(&self, _corr_id: CorrId, _s: &mut Self::Session) -> Vec<NativeRequest> {
        Vec::new()
    }

    /// A symbol a connection test may look up to prove the key reaches market data, for a
    /// venue with no instrument list to read.
    fn probe_symbol(&self) -> Option<&'static str> {
        None
    }

    /// The most instruments one `ResolveInstruments` asks for.
    fn resolve_batch(&self) -> usize {
        100
    }

    /// The signer for an account's credentials (secret names as OTW stores them). Lives on
    /// the transport side: the mapper builds it and never sees what it signs with again.
    fn signer(&self, _secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        Ok(None)
    }

    /// An account's settings (the non-secret fields OTW stores next to the key), checked
    /// when they are saved: a malformed one is refused with the fix, before any call.
    fn check_settings(&self, _settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        Ok(())
    }

    /// The venue's symbol for a base/quote pair (`BTC` + `USDT` -> `BTCUSDT`), for venues
    /// whose symbols are built that way. `None` = not derivable, look it up instead.
    fn pair_symbol(&self, _base: &str, _quote: &str) -> Option<String> {
        None
    }

    /// Frames the socket owes the venue now (an application-level keepalive), asked after
    /// every message and on every idle check. `ctx.received_ts` is the current time.
    fn keepalive(&self, _s: &mut Self::Session, _ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        Vec::new()
    }

    /// Which socket a subscription rides. Subscriptions of one class share a connection;
    /// the default is one socket for everything. A venue that cannot multiplex some
    /// subscriptions on one socket separates them here.
    fn socket_class(&self, _instrument: &InstrumentId, _data_type: &otw_connector_model::DataType) -> String {
        String::new()
    }

    /// The socket opened at this path speaks MessagePack, not JSON. The live transport then
    /// asks for it and transcodes both ways, so the adapter still reads and writes JSON.
    fn ws_msgpack(&self, _path: &str) -> bool {
        false
    }

    /// This instrument streams no candles, only trades (Alpaca's option socket has no `bars`
    /// channel): its live candles are folded from the prints.
    fn live_trades_only(&self, _instrument: &InstrumentId) -> bool {
        false
    }

    /// A venue that hands out its socket's address per connection (KuCoin's bullet token):
    /// the request the live adapter sends before the socket opens. Its answer goes to
    /// `decode` on the feed's own session, which then knows where to open the socket.
    fn socket_bootstrap(&self, _corr_id: CorrId, _s: &mut Self::Session) -> Option<crate::native::HttpRequest> {
        None
    }

    /// Whether a live session needs the venue's instrument catalog before subscribing.
    fn live_requires_instruments(&self) -> bool {
        false
    }

    /// How an instrument's venue reads in a symbol search line.
    fn search_exchange(&self, _spec: &otw_connector_model::InstrumentSpec) -> String {
        self.capabilities().label.clone()
    }

    /// The request for `corr_id` never reached `decode` (transport failure): drop whatever
    /// the session kept for it.
    fn abandon(&self, _corr_id: CorrId, _s: &mut Self::Session) {}
}
