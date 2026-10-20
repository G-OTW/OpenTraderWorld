//! OKX v5: candle history, the instrument lists, best bid/ask, the spot price board, live
//! candles on the business socket, and the account read only. Keyless market data.
//! Endpoint-by-endpoint mapping in PROVIDER.md.

mod account;
mod errors;
mod signer;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, CanonicalSymbol, Capabilities, CapsError, Candle, Command, ConnectorError, ContractType, CorrId, DataType, Decimal,
    Envelope, ErrorKind, Event, HistoryItems, InstrumentId, InstrumentSpec, LastPrice, Price, Qty, Quote, TfUnit, TimeRange,
    Timeframe,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::value::RawValue;

pub const VENUE: &str = "OKX";
const REST: &str = "rest";
const WS: &str = "ws";
const BASES: &[(&str, &str)] = &[(REST, "https://www.okx.com"), (WS, "wss://ws.okx.com:8443")];
const CAPS_TOML: &str = include_str!("capabilities.toml");
/// Candles per history page.
const PAGE: u32 = 300;
/// Instrument types the picker offers, in the order they are listed.
const LIST_TYPES: &[(&str, &str)] = &[("SPOT", "instrumentsSpot"), ("SWAP", "instrumentsSwap"), ("FUTURES", "instrumentsFutures")];

#[derive(Clone)]
pub struct Okx {
    caps: Capabilities,
}

impl Okx {
    pub fn new() -> Result<Self, CapsError> {
        Ok(Okx { caps: Capabilities::from_toml(CAPS_TOML)? })
    }
}

enum Pending {
    Candles { instrument: InstrumentId, tf: Timeframe, range: TimeRange },
    Instruments { inst_type: &'static str },
    Quote { instrument: InstrumentId },
    Board,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    /// Keyed by request and endpoint: one instrument load is three requests.
    pending: HashMap<(CorrId, &'static str), Pending>,
    live: stream::LiveState,
}

/// OKX's bar token. Daily and weekly are the UTC-anchored variants on purpose.
fn bar(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1m",
        (3, TfUnit::Minute) => "3m",
        (5, TfUnit::Minute) => "5m",
        (15, TfUnit::Minute) => "15m",
        (30, TfUnit::Minute) => "30m",
        (1, TfUnit::Hour) => "1H",
        (2, TfUnit::Hour) => "2H",
        (4, TfUnit::Hour) => "4H",
        (1, TfUnit::Day) => "1Dutc",
        (1, TfUnit::Week) => "1Wutc",
        (1, TfUnit::Month) => "1Mutc",
        _ => return None,
    })
}

/// Every answer is `{"code":"0","msg":"","data":[...]}`.
#[derive(Deserialize)]
struct Data<T> {
    data: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstrumentRow {
    inst_id: String,
    #[serde(default)]
    base_ccy: String,
    #[serde(default)]
    quote_ccy: String,
    #[serde(default)]
    settle_ccy: String,
    #[serde(default)]
    ct_val: OptDec,
    #[serde(default)]
    ct_mult: OptDec,
    #[serde(default)]
    ct_val_ccy: String,
    #[serde(default)]
    ct_type: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    tick_sz: OptDec,
    #[serde(default)]
    lot_sz: OptDec,
    #[serde(default)]
    min_sz: OptDec,
    #[serde(default)]
    exp_time: OptDec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TickerRow {
    inst_id: String,
    #[serde(default)]
    bid_px: OptDec,
    #[serde(default)]
    bid_sz: OptDec,
    #[serde(default)]
    ask_px: OptDec,
    #[serde(default)]
    ask_sz: OptDec,
    #[serde(default)]
    last: OptDec,
}

/// `[ts, o, h, l, c, vol, volCcy, volCcyQuote, confirm]`, all strings.
pub(crate) struct CandleRow {
    pub open_ms: i64,
    pub o: Dec,
    pub h: Dec,
    pub l: Dec,
    pub c: Dec,
    pub vol: Dec,
    pub vol_quote: Option<Dec>,
    /// `"0"` while the bar is forming, `"1"` once final; absent on some old rows.
    pub confirm: Option<String>,
}

impl CandleRow {
    pub fn read(row: &[Box<RawValue>]) -> Option<CandleRow> {
        let at = |i: usize| row.get(i).map(|v| v.get());
        let dec = |i: usize| at(i).and_then(|t| serde_json::from_str::<Dec>(t).ok());
        Some(CandleRow {
            open_ms: at(0).and_then(|t| serde_json::from_str::<Int>(t).ok())?.0,
            o: dec(1)?,
            h: dec(2)?,
            l: dec(3)?,
            c: dec(4)?,
            vol: dec(5)?,
            vol_quote: dec(7),
            confirm: at(8).and_then(|t| serde_json::from_str::<String>(t).ok()),
        })
    }

    pub fn candle(&self, instrument: InstrumentId, tf: Timeframe, is_final: bool) -> Result<Candle, ConnectorError> {
        Ok(Candle {
            instrument,
            tf,
            open_ts: ts_millis(self.open_ms)?,
            open: self.o.price(),
            high: self.h.price(),
            low: self.l.price(),
            close: self.c.price(),
            // In the venue's unit: coins on spot, contracts on a derivative (what OTW stores).
            volume: self.vol.qty(),
            quote_volume: self.vol_quote.map(|q| q.money()),
            trades: None,
            is_final,
            adjusted: None,
            sides: None,
        })
    }
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("okx {what}: {e}"))
}

impl Okx {
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

    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        let token = bar(tf)
            .filter(|_| self.caps.history_timeframes().contains(&tf))
            .ok_or_else(|| ConnectorError::unsupported(format!("OKX serves no {tf} history here")))?;
        // History reads backwards: `after` is exclusive and the window's end opens the walk.
        let after = match cursor {
            Some(c) => c.parse::<i64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?,
            None => range.to.millis(),
        };
        s.pending.insert((req_id, "historyCandles"), Pending::Candles { instrument: instrument.clone(), tf, range });
        Ok(vec![NativeRequest::Http(
            HttpRequest::get("historyCandles", REST, "/api/v5/market/history-candles", req_id)
                .query("instId", &instrument.native)
                .query("bar", token)
                .query("after", after)
                .query("limit", PAGE),
        )])
    }

    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Data<Vec<Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err("history-candles", e))?;
        let full = page.data.len() >= PAGE as usize;
        let mut candles = Vec::with_capacity(page.data.len());
        let mut oldest = None;
        for raw in &page.data {
            let r = CandleRow::read(raw).ok_or_else(|| ConnectorError::decode("okx history-candles: row shape"))?;
            oldest = Some(oldest.map_or(r.open_ms, |o: i64| o.min(r.open_ms)));
            if !range.contains(ts_millis(r.open_ms)?) {
                continue;
            }
            let is_final = r.confirm.as_deref() != Some("0");
            candles.push(r.candle(instrument.clone(), tf, is_final)?);
        }
        candles.sort_by_key(|c| c.open_ts);
        // A full page whose oldest bar is still inside the window: older bars remain.
        let next = match (full, oldest) {
            (true, Some(o)) if o > range.from.millis() => Some(o.to_string()),
            _ => None,
        };
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], inst_type: &'static str, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let page: Data<InstrumentRow> = serde_json::from_slice(body).map_err(|e| decode_err("instruments", e))?;
        let mut out = Vec::with_capacity(page.data.len() + 1);
        for i in page.data {
            let id = i.inst_id.to_ascii_uppercase();
            if id.is_empty() {
                continue;
            }
            // A derivative names its underlying and settlement where spot names base and quote.
            let base = if i.base_ccy.is_empty() { i.ct_val_ccy.clone() } else { i.base_ccy.clone() };
            let quote = if i.quote_ccy.is_empty() { i.settle_ccy.clone() } else { i.quote_ccy.clone() };
            let (asset_class, contract) = match inst_type {
                "SWAP" => (AssetClass::Perp, ContractType::Perp),
                "FUTURES" => (AssetClass::Future, ContractType::Future),
                _ => (AssetClass::Spot, ContractType::Spot),
            };
            // What one contract is worth: ctVal x ctMult; spot rows carry neither.
            let multiplier = match i.ct_val.0.filter(|v| v.is_sign_positive() && !v.is_zero()) {
                Some(v) => v * i.ct_mult.0.filter(|m| m.is_sign_positive() && !m.is_zero()).unwrap_or(Decimal::ONE),
                None => Decimal::ONE,
            };
            let spec = InstrumentSpec {
                id: InstrumentId::new(VENUE, &id),
                symbol: CanonicalSymbol(format!("{base}/{quote}")),
                asset_class,
                base: Some(base).filter(|b| !b.is_empty()),
                quote,
                tick_size: i.tick_sz.0.map(Price),
                lot_size: i.lot_sz.0.map(Qty),
                min_qty: i.min_sz.0.map(Qty),
                multiplier,
                contract,
                inverse: i.ct_type == "inverse",
                expiry: i.exp_time.0.and_then(|e| i64::try_from(e.trunc()).ok()).map(ts_millis).transpose()?,
                strike: None,
                right: None,
                settlement_ccy: Some(i.settle_ccy.to_ascii_uppercase()).filter(|s| !s.is_empty()),
                session_tz: None,
                active: i.state == "live",
                note: Some(inst_type.to_string()),
                exchange: None,
            };
            out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }
}

impl Mapper for Okx {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        // OKX's own spelling says which market: BTC-USDT spot, BTC-USDT-SWAP perpetual,
        // BTC-USD-241227 dated.
        let s = symbol.trim();
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ConnectorError::not_found(format!(
                "OKX: bad symbol `{symbol}`, write it as OKX spells it (BTC-USDT for spot, BTC-USDT-SWAP for a perpetual)"
            )));
        }
        Ok(InstrumentId::new(VENUE, &s.to_ascii_uppercase()))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } => {
                self.candles(*req_id, instrument, *tf, *range, cursor.as_deref(), s)
            }
            Command::LoadInstruments { req_id } => Ok(LIST_TYPES
                .iter()
                .map(|(t, ep)| {
                    s.pending.insert((*req_id, *ep), Pending::Instruments { inst_type: t });
                    NativeRequest::Http(HttpRequest::get(ep, REST, "/api/v5/public/instruments", *req_id).query("instType", t))
                })
                .collect()),
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert((*req_id, "ticker"), Pending::Quote { instrument: instrument.clone() });
                Ok(vec![NativeRequest::Http(HttpRequest::get("ticker", REST, "/api/v5/market/ticker", *req_id).query("instId", &instrument.native))])
            }
            Command::RequestBoard { req_id } if self.caps.history.board => {
                s.pending.insert((*req_id, "tickers"), Pending::Board);
                Ok(vec![NativeRequest::Http(HttpRequest::get("tickers", REST, "/api/v5/market/tickers", *req_id).query("instType", "SPOT"))])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live, ctx),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(cmd, &mut s.pending),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            // SyncTime is not declared: OKX accepts the host clock (as the legacy reader did).
            other => Err(ConnectorError::unsupported(format!("OKX: {other:?} is not declared"))),
        }
    }

    fn encode_batch(&self, cmds: &[Command], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if cmds.iter().all(|c| matches!(c, Command::Subscribe { .. } | Command::Unsubscribe { .. })) {
            return stream::encode(self, cmds, &mut s.live, ctx);
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
            return Err(e);
        }
        match pending {
            Pending::Candles { instrument, tf, range } => self.decode_candles(corr, body, instrument, tf, range, ctx),
            Pending::Instruments { inst_type } => self.decode_instruments(corr, body, inst_type, ctx),
            Pending::Quote { instrument } => {
                let page: Data<TickerRow> = serde_json::from_slice(body).map_err(|e| decode_err("ticker", e))?;
                let Some(t) = page.data.into_iter().next() else { return Ok(vec![]) };
                if !t.inst_id.eq_ignore_ascii_case(&instrument.native) {
                    return Err(ConnectorError::decode(format!("okx ticker answered {} for {}", t.inst_id, instrument.native)));
                }
                // A one-sided book is no quote: nothing is emitted rather than half a price.
                let (Some(bid), Some(ask)) = (t.bid_px.0, t.ask_px.0) else { return Ok(vec![]) };
                Ok(vec![env(
                    ctx,
                    corr,
                    Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: t.bid_sz.0.map(Qty), ask_qty: t.ask_sz.0.map(Qty) }),
                )])
            }
            Pending::Board => {
                let page: Data<TickerRow> = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
                let board = page
                    .data
                    .into_iter()
                    .filter_map(|t| t.last.0.map(|p| LastPrice { instrument: InstrumentId::new(VENUE, &t.inst_id.to_ascii_uppercase()), price: Price(p) }))
                    .collect();
                Ok(vec![env(ctx, corr, Event::Board(board))])
            }
            Pending::Account(p) => account::decode(p, corr, body, ctx),
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.retain(|(c, _), _| *c != corr_id);
    }

    /// Candles live on the business socket and the best bid/ask on the public one: two
    /// sockets, so two classes.
    fn socket_class(&self, _instrument: &InstrumentId, data_type: &DataType) -> String {
        match data_type {
            DataType::Quote => "public".into(),
            _ => String::new(),
        }
    }

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(&mut s.live, ctx)
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
        let demo = secrets.get("env").is_some_and(|v| v.trim().eq_ignore_ascii_case("demo"));
        Ok(Some(Arc::new(signer::OkxSigner { key: get("api_key")?, secret: get("api_secret")?, passphrase: get("api_passphrase")?, demo })))
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        Some(format!("{}-{}", base.to_ascii_uppercase(), quote.to_ascii_uppercase()))
    }

    fn search_exchange(&self, spec: &InstrumentSpec) -> String {
        format!("OKX {}", spec.note.as_deref().unwrap_or(""))
    }
}

#[cfg(test)]
mod tests;
