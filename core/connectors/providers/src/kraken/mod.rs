//! Kraken spot: candle history (the last 720 bars only), the pair list, best bid/ask, the
//! price board, live candles on the v2 socket, and the account read only. Keyless market
//! data. Endpoint-by-endpoint mapping in PROVIDER.md.
//!
//! Kraken spells a pair three ways: a key (`XXBTZUSD`), an altname (`XBTUSD`, what OTW
//! stores for market data) and a wsname (`XBT/USD`, what OTW stores on account rows). The
//! instrument list is the only place one turns into another, so the session keeps it.

mod account;
mod errors;
mod signer;
mod stream;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use otw_connector_model::{
    Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Decimal, Envelope, ErrorKind, Event,
    HistoryItems, InstrumentId, InstrumentSpec, LastPrice, Price, Qty, Quote, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::{ts_secs, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::value::RawValue;

pub const VENUE: &str = "KRAKEN";
const REST: &str = "rest";
const WS: &str = "ws";
const BASES: &[(&str, &str)] = &[(REST, "https://api.kraken.com"), (WS, "wss://ws.kraken.com")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// How long the pair and asset names the session keeps are trusted before a reload.
const NAMES_TTL_NS: i64 = 3_600_000_000_000;

#[derive(Clone)]
pub struct Kraken {
    caps: Capabilities,
}

impl Kraken {
    pub fn new() -> Result<Self, CapsError> {
        Ok(Kraken { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// One pair as the account side names it.
#[derive(Debug, Clone)]
pub(crate) struct PairName {
    pub wsname: String,
    pub quote: String,
}

enum Pending {
    Candles { instrument: InstrumentId, tf: Timeframe, range: TimeRange },
    Instruments,
    /// The pair list, loaded only for its names (account rows, the board).
    PairNames,
    Quote { instrument: InstrumentId },
    Board,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
    /// Pair key and altname to its wsname and quote, with when it was loaded.
    pairs: Option<(Timestamp, Arc<HashMap<String, PairName>>)>,
    /// Asset code (`XXBT`) to the name it trades under (`XBT`), with when it was loaded.
    assets: Option<(Timestamp, Arc<HashMap<String, String>>)>,
    live: stream::LiveState,
}

impl Session {
    fn fresh<T>(slot: &Option<(Timestamp, T)>, now: Timestamp) -> bool {
        slot.as_ref().is_some_and(|(at, _)| now.nanos().saturating_sub(at.nanos()) < NAMES_TTL_NS)
    }

    pub(crate) fn pairs(&self) -> Arc<HashMap<String, PairName>> {
        self.pairs.as_ref().map(|(_, p)| p.clone()).unwrap_or_default()
    }

    pub(crate) fn assets(&self) -> Arc<HashMap<String, String>> {
        self.assets.as_ref().map(|(_, a)| a.clone()).unwrap_or_default()
    }

    /// The pair list request, when the names the session keeps are missing or stale.
    pub(crate) fn need_pairs(&mut self, req_id: CorrId, now: Timestamp) -> Option<NativeRequest> {
        if Self::fresh(&self.pairs, now) {
            return None;
        }
        self.pending.insert((req_id, "pairNames"), Pending::PairNames);
        Some(NativeRequest::Http(HttpRequest::get("pairNames", REST, "/0/public/AssetPairs", req_id)))
    }
}

/// Kraken's interval in minutes.
pub(crate) fn interval(tf: Timeframe) -> Option<u32> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => 1,
        (5, TfUnit::Minute) => 5,
        (15, TfUnit::Minute) => 15,
        (30, TfUnit::Minute) => 30,
        (1, TfUnit::Hour) => 60,
        (4, TfUnit::Hour) => 240,
        (1, TfUnit::Day) => 1440,
        (1, TfUnit::Week) => 10080,
        _ => return None,
    })
}

/// Every answer is `{"error":[],"result":...}`.
#[derive(Deserialize)]
struct Answer<T> {
    result: T,
}

#[derive(Deserialize)]
struct PairRow {
    altname: String,
    #[serde(default)]
    wsname: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    tick_size: OptDec,
    #[serde(default)]
    lot_decimals: Option<u32>,
    #[serde(default)]
    ordermin: OptDec,
}

impl PairRow {
    /// Kraken's own readable spelling; the altname when a pair has none.
    fn display(&self) -> String {
        self.wsname.clone().filter(|w| !w.is_empty()).unwrap_or_else(|| self.altname.clone())
    }
}

#[derive(Deserialize)]
pub(crate) struct TickerRow {
    #[serde(default)]
    a: Vec<OptDec>,
    #[serde(default)]
    b: Vec<OptDec>,
    #[serde(default)]
    c: Vec<OptDec>,
}

fn at(v: &[OptDec], i: usize) -> Option<Decimal> {
    v.get(i).and_then(|d| d.0)
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("kraken {what}: {e}"))
}

/// Pair key and altname to the names the account side uses.
fn pair_names(rows: &BTreeMap<String, PairRow>) -> HashMap<String, PairName> {
    let mut out = HashMap::with_capacity(rows.len() * 2);
    for (key, p) in rows {
        let wsname = p.display();
        let quote = wsname.split('/').nth(1).unwrap_or_default().to_string();
        let name = PairName { wsname, quote };
        out.insert(p.altname.clone(), name.clone());
        out.insert(key.clone(), name);
    }
    out
}

impl Kraken {
    fn account_declared(&self, kind: otw_connector_model::AccountKind) -> bool {
        use otw_connector_model::AccountKind as K;
        let a = &self.caps.account;
        match kind {
            K::Balances => a.balances,
            K::Positions => a.positions,
            K::Fills => a.fills_history,
            K::OpenOrders => a.open_orders,
            K::Summary => a.summary,
        }
    }

    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let minutes = interval(tf)
            .filter(|_| self.caps.history_timeframes().contains(&tf))
            .ok_or_else(|| ConnectorError::unsupported(format!("Kraken serves no {tf} history here")))?;
        // `since` keeps bars opening at or after it, and the venue answers its last 720 bars
        // whatever the window: one page, filtered to the window here.
        s.pending.insert((req_id, "ohlc"), Pending::Candles { instrument: instrument.clone(), tf, range });
        Ok(vec![NativeRequest::Http(
            HttpRequest::get("ohlc", REST, "/0/public/OHLC", req_id)
                .query("pair", &instrument.native)
                .query("interval", minutes)
                .query("since", range.from.secs()),
        )])
    }

    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Answer<BTreeMap<String, Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err("OHLC", e))?;
        // The series sits under whatever key Kraken normalized the pair to, next to `last`:
        // the open time of the newest committed bar. The row after it is still forming.
        let last = page.result.get("last").and_then(|v| serde_json::from_str::<Int>(v.get()).ok()).map(|i| i.0);
        let series = page
            .result
            .iter()
            .find(|(k, _)| k.as_str() != "last")
            .map(|(_, v)| v)
            .ok_or_else(|| ConnectorError::not_found(format!("kraken OHLC: empty result for {}, bad symbol?", instrument.native)))?;
        let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_str(series.get()).map_err(|e| decode_err("OHLC rows", e))?;
        let step = tf.fixed_secs().unwrap_or(0);
        let mut candles = Vec::with_capacity(rows.len());
        for row in &rows {
            let field = |i: usize| row.get(i).map(|v| v.get()).ok_or_else(|| ConnectorError::decode("kraken OHLC: row shape"));
            let num = |i: usize| -> Result<Decimal, ConnectorError> {
                let raw = field(i)?;
                serde_json::from_str::<otw_connector_sdk::Dec>(raw).map(|d| d.0).map_err(|e| decode_err("OHLC number", e))
            };
            let open_s = serde_json::from_str::<Int>(field(0)?).map_err(|e| decode_err("OHLC time", e))?.0;
            let open_ts = ts_secs(open_s)?;
            if !range.contains(open_ts) {
                continue;
            }
            let is_final = match last {
                Some(l) => open_s <= l,
                None => step > 0 && open_ts.plus_secs(step) <= ctx.received_ts,
            };
            candles.push(Candle {
                instrument: instrument.clone(),
                tf,
                open_ts,
                open: Price(num(1)?),
                high: Price(num(2)?),
                low: Price(num(3)?),
                close: Price(num(4)?),
                volume: Qty(num(6)?),
                quote_volume: None,
                trades: field(7).ok().and_then(|t| serde_json::from_str::<Int>(t).ok()).and_then(|t| u64::try_from(t.0).ok()),
                is_final,
                adjusted: None,
                sides: None,
            });
        }
        candles.sort_by_key(|c| c.open_ts);
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: None, done: true })])
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Answer<BTreeMap<String, PairRow>> = serde_json::from_slice(body).map_err(|e| decode_err("AssetPairs", e))?;
        s.pairs = Some((ctx.received_ts, Arc::new(pair_names(&page.result))));
        let mut out = Vec::with_capacity(page.result.len() + 1);
        for p in page.result.values() {
            let display = p.display();
            let (base, quote) = display.split_once('/').unwrap_or((display.as_str(), ""));
            let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &p.altname.to_ascii_uppercase()), base, quote);
            spec.symbol = otw_connector_model::CanonicalSymbol(display.clone());
            spec.tick_size = p.tick_size.0.map(Price);
            spec.lot_size = p.lot_decimals.and_then(|d| Decimal::try_from_i128_with_scale(1, d).ok()).map(Qty);
            spec.min_qty = p.ordermin.0.map(Qty);
            // A pair with no status is listed; any other status is not open for trading.
            spec.active = p.status.as_deref().is_none_or(|st| st == "online");
            out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }
}

impl Mapper for Kraken {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // As OTW stores it: the altname for market data (XBTUSD), the wsname on account
        // rows (XBT/USD). Kraken answers either; nothing is reshaped here.
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '.') {
            return Err(ConnectorError::not_found(format!(
                "Kraken: bad symbol `{symbol}`, write the pair as Kraken lists it (XBTUSD, or XBT/USD)"
            )));
        }
        Ok(InstrumentId::new(VENUE, &s.to_ascii_uppercase()))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, .. } => {
                self.candles(*req_id, instrument, *tf, *range, s)
            }
            Command::LoadInstruments { req_id } => {
                s.pending.insert((*req_id, "assetPairs"), Pending::Instruments);
                Ok(vec![NativeRequest::Http(HttpRequest::get("assetPairs", REST, "/0/public/AssetPairs", *req_id))])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert((*req_id, "ticker"), Pending::Quote { instrument: instrument.clone() });
                Ok(vec![NativeRequest::Http(HttpRequest::get("ticker", REST, "/0/public/Ticker", *req_id).query("pair", &instrument.native))])
            }
            Command::RequestBoard { req_id } if self.caps.history.board => {
                let mut out: Vec<NativeRequest> = s.need_pairs(*req_id, ctx.received_ts).into_iter().collect();
                s.pending.insert((*req_id, "tickers"), Pending::Board);
                out.push(NativeRequest::Http(HttpRequest::get("tickers", REST, "/0/public/Ticker", *req_id)));
                Ok(out)
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => {
                let pairs = s.pairs();
                stream::encode(self, std::slice::from_ref(cmd), &mut s.live, &pairs, ctx.instruments)
            }
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(cmd, s, ctx),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            // SyncTime is not declared: the nonce is a counter, not a clock check.
            other => Err(ConnectorError::unsupported(format!("Kraken: {other:?} is not declared"))),
        }
    }

    fn encode_batch(&self, cmds: &[Command], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if cmds.iter().all(|c| matches!(c, Command::Subscribe { .. } | Command::Unsubscribe { .. })) {
            let pairs = s.pairs();
            return stream::encode(self, cmds, &mut s.live, &pairs, ctx.instruments);
        }
        let mut out = Vec::new();
        for c in cmds {
            out.extend(self.encode(c, s, ctx)?);
        }
        Ok(out)
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(errors::from_frame(frame).unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { payload, .. } => return stream::decode(payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if let Some(e) = errors::from_frame(frame) {
            // Names are a convenience for balances: without them a code reads as itself.
            if matches!(pending, Pending::Account(account::Pending::AssetNames)) && e.kind != ErrorKind::RateLimited {
                return Ok(vec![]);
            }
            return Err(e);
        }
        match pending {
            Pending::Candles { instrument, tf, range } => self.decode_candles(corr, body, instrument, tf, range, ctx),
            Pending::Instruments => self.decode_instruments(corr, body, s, ctx),
            Pending::PairNames => {
                let page: Answer<BTreeMap<String, PairRow>> = serde_json::from_slice(body).map_err(|e| decode_err("AssetPairs", e))?;
                s.pairs = Some((ctx.received_ts, Arc::new(pair_names(&page.result))));
                Ok(vec![])
            }
            Pending::Quote { instrument } => {
                let page: Answer<BTreeMap<String, TickerRow>> = serde_json::from_slice(body).map_err(|e| decode_err("Ticker", e))?;
                // The pair key is whatever Kraken normalized the input to: one row is asked for.
                let Some(t) = page.result.into_values().next() else { return Ok(vec![]) };
                let (Some(bid), Some(ask)) = (at(&t.b, 0), at(&t.a, 0)) else { return Ok(vec![]) };
                Ok(vec![env(
                    ctx,
                    corr,
                    Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: at(&t.b, 2).map(Qty), ask_qty: at(&t.a, 2).map(Qty) }),
                )])
            }
            Pending::Board => {
                let page: Answer<BTreeMap<String, TickerRow>> = serde_json::from_slice(body).map_err(|e| decode_err("Ticker", e))?;
                let names = s.pairs();
                // Keyed by wsname, as the account side names pairs.
                let board = page
                    .result
                    .iter()
                    .filter_map(|(k, t)| {
                        let n = names.get(k)?;
                        at(&t.c, 0).map(|p| LastPrice { instrument: InstrumentId::new(VENUE, &n.wsname.to_ascii_uppercase()), price: Price(p) })
                    })
                    .collect();
                Ok(vec![env(ctx, corr, Event::Board(board))])
            }
            Pending::Account(p) => account::decode(p, corr, body, s, ctx),
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |name: &str| {
            secrets
                .get(name)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| SecretString::from(v.to_string()))
                .ok_or_else(|| ConnectorError::new(ErrorKind::Auth, format!("{name} is not set on this broker account")))
        };
        Ok(Some(Arc::new(signer::KrakenSigner::new(get("api_key")?, get("api_secret")?)?)))
    }

    /// The wsname a holding is valued on (`XBT/USD`); only a pair on the board is used.
    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        Some(format!("{}/{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }

    /// One socket per interval, as before the port: the subscription frame takes one
    /// interval, and a socket's rows are then routed on their symbol alone.
    fn socket_class(&self, _instrument: &InstrumentId, data_type: &DataType) -> String {
        match data_type {
            DataType::Candles { tf } => interval(*tf).map(|m| m.to_string()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn live_requires_instruments(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
