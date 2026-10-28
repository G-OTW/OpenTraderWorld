//! OANDA v20: mid candles with their bid and ask, the account's own prices, its instrument
//! list, and the account read side (fills, positions, pending orders). One bearer token,
//! the account's: OANDA has no market-data-only credential. The account id rides every path
//! and is written there by the signer, with the token. Endpoint-by-endpoint mapping in
//! PROVIDER.md.

mod account;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, CanonicalSymbol, Candle, Capabilities, CapsError, Command, ConnectorError, CorrId, DataType, Envelope, ErrorKind,
    Event, HistoryItems, InstrumentId, InstrumentSpec, Ohlc, Price, Qty, Quote, Sides, TfUnit, TimeRange, Timeframe, Timestamp,
};
use otw_connector_sdk::parse::{ts_rfc3339, OptDec};
use otw_connector_sdk::signer::SignableRequest;
use otw_connector_sdk::{http, HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

pub const VENUE: &str = "OANDA";
const REST: &str = "rest";
const BASES: &[(&str, &str)] = &[(REST, "https://api-fxtrade.oanda.com")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// Where the signer writes the account id.
const ACCOUNT: &str = "{account}";

#[derive(Clone)]
pub struct Oanda {
    caps: Capabilities,
}

impl Oanda {
    pub fn new() -> Result<Self, CapsError> {
        Ok(Oanda { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

/// An account id is four dash-separated groups of digits (`001-004-1234567-001`).
fn check_account(id: &str) -> Result<(), ConnectorError> {
    if id.split('-').count() != 4 || !id.chars().all(|c| c.is_ascii_digit() || c == '-') || id.split('-').any(str::is_empty) {
        return Err(ConnectorError::invalid(format!("an OANDA account id looks like 001-004-1234567-001, not \"{id}\"")));
    }
    Ok(())
}

/// The account's path, the id left for the signer.
fn acct(rest: &str) -> String {
    format!("/v3/accounts/{ACCOUNT}{rest}")
}

fn granularity(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "M1",
        (5, TfUnit::Minute) => "M5",
        (15, TfUnit::Minute) => "M15",
        (1, TfUnit::Hour) => "H1",
        (4, TfUnit::Hour) => "H4",
        (1, TfUnit::Day) => "D",
        (1, TfUnit::Week) => "W",
        _ => return None,
    })
}

/// OANDA's instrument type as a market. A type it may add later is a CFD until said
/// otherwise: OANDA deals CFDs and currencies, nothing that settles.
fn class_of(kind: &str) -> AssetClass {
    match kind {
        "CURRENCY" => AssetClass::Forex,
        "METAL" => AssetClass::Commodity,
        _ => AssetClass::Cfd,
    }
}

/// What an instrument settles in, read off OANDA's own `BASE_QUOTE` name: `EUR_USD` and
/// `SPX500_USD` both settle in USD. The venue's notation, not a suffix guess.
pub(crate) fn quote_ccy(instrument: &str) -> Option<String> {
    instrument.rsplit_once('_').map(|(_, q)| q.to_ascii_uppercase()).filter(|q| !q.is_empty())
}

struct CandleReq {
    instrument: InstrumentId,
    tf: Timeframe,
    range: TimeRange,
}

enum Pending {
    Candles(CandleReq),
    Quote { instrument: InstrumentId },
    Instruments,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<(CorrId, &'static str), Pending>,
}

#[derive(Deserialize)]
struct Sided {
    #[serde(default)]
    o: OptDec,
    #[serde(default)]
    h: OptDec,
    #[serde(default)]
    l: OptDec,
    #[serde(default)]
    c: OptDec,
}

impl Sided {
    fn ohlc(&self) -> Option<Ohlc> {
        Some(Ohlc { open: Price(self.o.0?), high: Price(self.h.0?), low: Price(self.l.0?), close: Price(self.c.0?) })
    }
}

#[derive(Deserialize)]
struct CandleRow {
    time: String,
    #[serde(default)]
    complete: bool,
    #[serde(default)]
    volume: OptDec,
    #[serde(default)]
    mid: Option<Sided>,
    #[serde(default)]
    bid: Option<Sided>,
    #[serde(default)]
    ask: Option<Sided>,
}

#[derive(Deserialize)]
struct CandlesAnswer {
    #[serde(default)]
    candles: Option<Vec<CandleRow>>,
}

#[derive(Deserialize)]
struct PriceRow {
    #[serde(default)]
    instrument: String,
    #[serde(default)]
    time: Option<String>,
    #[serde(rename = "closeoutBid", default)]
    closeout_bid: OptDec,
    #[serde(rename = "closeoutAsk", default)]
    closeout_ask: OptDec,
}

#[derive(Deserialize)]
struct PricingAnswer {
    #[serde(default)]
    prices: Vec<PriceRow>,
}

#[derive(Deserialize)]
struct InstrumentRow {
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(rename = "displayName", default)]
    display_name: String,
}

#[derive(Deserialize)]
struct InstrumentsAnswer {
    #[serde(default)]
    instruments: Vec<InstrumentRow>,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(rename = "errorMessage", default)]
    error_message: String,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

pub(crate) fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("oanda {what}: {e}"))
}

/// The fault a status means, in OANDA's own words (`errorMessage`). A 404, or a 400 naming
/// the instrument, is an instrument OANDA does not deal on this account.
pub(crate) fn refusal(frame: &RawFrame) -> Option<ConnectorError> {
    let mut e = http::timeout(frame, "oanda").or_else(|| http::classify(frame, "oanda"))?;
    let RawFrame::Http { status, body, endpoint_id, .. } = frame else { return Some(e) };
    let msg = serde_json::from_slice::<ErrorBody>(body).map(|b| b.error_message).unwrap_or_default();
    if msg.is_empty() {
        return Some(e);
    }
    e.message = match *status {
        401 => format!("OANDA refused the token ({msg})"),
        404 => format!("OANDA {endpoint_id}: bad symbol or account ({msg})"),
        400 if msg.to_ascii_lowercase().contains("instrument") => {
            e.kind = ErrorKind::NotFound;
            format!("OANDA {endpoint_id}: bad symbol ({msg}). Write it as OANDA does (EUR_USD, SPX500_USD)")
        }
        _ => format!("OANDA {endpoint_id}: {msg}"),
    };
    Some(e)
}

fn rfc3339(t: Timestamp) -> Result<String, ConnectorError> {
    t.to_offset_datetime()
        .and_then(|d| d.format(&time::format_description::well_known::Rfc3339).ok())
        .ok_or_else(|| ConnectorError::invalid(format!("time {t} out of range")))
}

/// The bearer token and the account it reads, both from the connector's settings.
pub struct OandaSigner {
    token: SecretString,
    account: String,
}

impl Signer for OandaSigner {
    fn sign(&self, req: &mut SignableRequest, _now: Timestamp) -> Result<(), ConnectorError> {
        req.path = req.path.replace(ACCOUNT, &self.account);
        req.headers.push(("Authorization".into(), format!("Bearer {}", self.token.expose_secret())));
        // Times come back as RFC 3339 rather than Unix seconds.
        req.headers.push(("Accept-Datetime-Format".into(), "RFC3339".into()));
        Ok(())
    }
}

impl Oanda {
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let g = granularity(tf).ok_or_else(|| ConnectorError::unsupported(format!("OANDA serves no {tf} candle")))?;
        let h = HttpRequest::get("candles", REST, acct(&format!("/instruments/{}/candles", instrument.native)), req_id)
            // Mid for the chart, bid and ask for fills priced on the side that deals.
            .query("price", "MBA")
            .query("granularity", g)
            .query("from", rfc3339(range.from)?)
            .query("to", rfc3339(range.to)?)
            // Midnight UTC, Monday weeks: OANDA's own default is the 17:00 New York roll.
            .query("dailyAlignment", "0")
            .query("alignmentTimezone", "UTC")
            .query("weeklyAlignment", "Monday")
            .signed();
        s.pending.insert((req_id, "candles"), Pending::Candles(CandleReq { instrument: instrument.clone(), tf, range }));
        Ok(vec![NativeRequest::Http(h)])
    }

    fn decode_candles(&self, corr: CorrId, body: &[u8], req: CandleReq, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let a: CandlesAnswer = serde_json::from_slice(body).map_err(|e| decode_err("candles", e))?;
        let rows = a.candles.ok_or_else(|| ConnectorError::decode(format!("OANDA answered no candle list for {}", req.instrument.native)))?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let open_ts = ts_rfc3339(&r.time)?;
            if !req.range.contains(open_ts) {
                continue;
            }
            let mid = r.mid.as_ref().and_then(Sided::ohlc).ok_or_else(|| ConnectorError::decode("OANDA candle carries no midpoint prices"))?;
            let sides = match (r.bid.as_ref().and_then(Sided::ohlc), r.ask.as_ref().and_then(Sided::ohlc)) {
                (Some(bid), Some(ask)) => Some(Sides { bid, ask }),
                _ => None,
            };
            out.push(Candle {
                instrument: req.instrument.clone(),
                tf: req.tf,
                open_ts,
                open: mid.open,
                high: mid.high,
                low: mid.low,
                close: mid.close,
                // The number of prices made in the period: the only volume a dealing desk has.
                volume: Qty(r.volume.0.unwrap_or_default()),
                quote_volume: None,
                trades: None,
                is_final: r.complete,
                adjusted: None,
                sides,
            });
        }
        out.sort_by_key(|c| c.open_ts);
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(out), next_cursor: None, done: true })])
    }

    fn instruments(&self, req_id: CorrId, ids: Option<&[InstrumentId]>, s: &mut Session) -> Vec<NativeRequest> {
        let mut h = HttpRequest::get("instruments", REST, acct("/instruments"), req_id).signed();
        if let Some(ids) = ids {
            h = h.query("instruments", ids.iter().map(|i| i.native.as_str()).collect::<Vec<_>>().join(","));
        }
        s.pending.insert((req_id, "instruments"), Pending::Instruments);
        vec![NativeRequest::Http(h)]
    }
}

impl Mapper for Oanda {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// OANDA's own spelling, `BASE_QUOTE` (`EUR_USD`, `SPX500_USD`, `XAU_USD`).
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = symbol.trim().to_ascii_uppercase();
        let ok = s.split_once('_').is_some_and(|(b, q)| !b.is_empty() && !q.is_empty()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            return Err(ConnectorError::not_found(format!("OANDA: bad symbol `{symbol}`, write it as OANDA does (EUR_USD, SPX500_USD)")));
        }
        Ok(InstrumentId::new(VENUE, &s))
    }

    fn instrument_for(&self, symbol: &str, _market: Option<AssetClass>) -> Result<InstrumentId, ConnectorError> {
        self.instrument_id(symbol)
    }

    fn encode(&self, cmd: &Command, s: &mut Session, _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, .. } => self.candles(*req_id, instrument, *tf, *range, s),
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert((*req_id, "pricing"), Pending::Quote { instrument: instrument.clone() });
                Ok(vec![NativeRequest::Http(HttpRequest::get("pricing", REST, acct("/pricing"), *req_id).query("instruments", &instrument.native).signed())])
            }
            Command::LoadInstruments { req_id } => Ok(self.instruments(*req_id, None, s)),
            Command::ResolveInstruments { req_id, ids } => Ok(self.instruments(*req_id, Some(ids), s)),
            Command::QueryAccount { .. } => account::encode(cmd, s),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            other => Err(ConnectorError::unsupported(format!("OANDA: {other:?} is not declared"))),
        }
    }

    fn decode(&self, frame: &RawFrame, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let (corr, endpoint, body) = match frame {
            RawFrame::Http { corr_id, endpoint_id, body, .. } => (*corr_id, *endpoint_id, body.clone()),
            RawFrame::Timeout { corr_id, endpoint_id } => {
                s.pending.remove(&(*corr_id, *endpoint_id));
                return Err(http::timeout(frame, "oanda").unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { .. } | RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s
            .pending
            .remove(&(corr, endpoint))
            .ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr} {endpoint}")))?;
        if let Some(e) = refusal(frame) {
            return Err(e);
        }
        match pending {
            Pending::Candles(req) => self.decode_candles(corr, &body, req, ctx),
            Pending::Quote { instrument } => {
                let a: PricingAnswer = serde_json::from_slice(&body).map_err(|e| decode_err("pricing", e))?;
                let Some(p) = a.prices.into_iter().find(|p| p.instrument.eq_ignore_ascii_case(&instrument.native)) else { return Ok(vec![]) };
                // The closeout prices: the two sides OANDA would actually fill at.
                let (Some(bid), Some(ask)) = (p.closeout_bid.0, p.closeout_ask.0) else { return Ok(vec![]) };
                let e = env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: None, ask_qty: None }));
                Ok(vec![match p.time.as_deref().and_then(|t| ts_rfc3339(t).ok()) {
                    Some(t) => e.at(t),
                    None => e,
                }])
            }
            Pending::Instruments => {
                let a: InstrumentsAnswer = serde_json::from_slice(&body).map_err(|e| decode_err("instruments", e))?;
                let mut out = Vec::with_capacity(a.instruments.len() + 1);
                for i in a.instruments.into_iter().filter(|i| !i.name.is_empty()) {
                    let (base, quote) = i.name.split_once('_').unwrap_or((i.name.as_str(), ""));
                    let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, &i.name), base, quote);
                    spec.symbol = CanonicalSymbol(i.display_name);
                    spec.asset_class = class_of(&i.kind);
                    // The instrument's kind as OANDA files it: CURRENCY, CFD or METAL.
                    spec.exchange = Some(i.kind).filter(|k| !k.is_empty());
                    out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
                }
                out.push(env(ctx, corr, Event::InstrumentsEnd));
                Ok(out)
            }
            Pending::Account(p) => account::decode(p, corr, &body, ctx),
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        let get = |k: &str| secrets.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let token = get("api_token").ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "OANDA requires an api_token credential: set it on the settings page"))?;
        let account = get("account_id").ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "OANDA requires the account_id setting: the v20 account number the token reads"))?;
        check_account(account)?;
        Ok(Some(Arc::new(OandaSigner { token: SecretString::from(token.to_string()), account: account.to_string() })))
    }

    fn check_settings(&self, settings: &HashMap<String, String>) -> Result<(), ConnectorError> {
        match settings.get("account_id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(id) => check_account(id),
            None => Ok(()),
        }
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        spec.exchange.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
