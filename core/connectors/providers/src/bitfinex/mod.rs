//! Bitfinex (API v2), spot pairs and the perpetual derivatives, one adapter per book: candle
//! history, the instrument lists, best bid/ask, the spot price board, live candles, and the
//! account read only (on the spot adapter, for the whole account). A pair is written
//! `tBTCUSD` (`tAVAX:USD` when a code is longer than three letters), a perpetual
//! `tBTCF0:USTF0`; the leading `t` may be left out when typing one. Both books ride the same
//! endpoints, so which one a connector reads is its setting. Endpoint-by-endpoint mapping in
//! PROVIDER.md.

mod account;
mod errors;
mod signer;
mod stream;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, Candle, Capabilities, CapsError, Command, ConnectorError, ContractType, CorrId, DataType, Decimal, Envelope, ErrorKind, Event,
    HistoryItems, InstrumentId, InstrumentSpec, LastPrice, Price, Qty, Quote, TfUnit, TimeRange, Timeframe,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use serde_json::value::RawValue;

const REST: &str = "rest";
const AUTH: &str = "auth";
const WS: &str = "ws";
/// Public data on `api-pub`, the account on `api` (Bitfinex's own split).
const BASES: &[(&str, &str)] = &[(REST, "https://api-pub.bitfinex.com"), (AUTH, "https://api.bitfinex.com"), (WS, "wss://api-pub.bitfinex.com")];
/// Candles per history call: `limit` takes 10000 at most, more is refused with `limit:
/// invalid` (measured on 2026-10-19).
const PAGE: i64 = 10_000;
/// The archive flags no forming bar. Measured on 2026-10-19 by polling BTCUSD, ETHUSD and
/// BTCF0:USTF0 through six minute boundaries: a closed minute was rewritten up to 20 s after
/// its close (11 rewrites, both books). A bar is final once its period plus this has passed,
/// about three times the worst one seen.
const SETTLE_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Spot,
    Derivatives,
}

impl Book {
    pub fn venue(self) -> &'static str {
        match self {
            Book::Spot => "BITFINEX",
            Book::Derivatives => "BITFINEX_PERP",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Book::Spot => "Bitfinex",
            Book::Derivatives => "Bitfinex derivatives",
        }
    }

    /// Which book a name belongs to: every perpetual is `<BASE>F0:<QUOTE>F0`.
    pub(crate) fn of(native: &str) -> Book {
        if native.contains("F0:") {
            Book::Derivatives
        } else {
            Book::Spot
        }
    }

    pub(crate) fn owns(self, native: &str) -> bool {
        Book::of(native) == self
    }
}

#[derive(Clone)]
pub struct Bitfinex {
    caps: Capabilities,
    book: Book,
}

impl Bitfinex {
    pub fn spot() -> Result<Self, CapsError> {
        Ok(Bitfinex { caps: Capabilities::from_toml(include_str!("capabilities_spot.toml"))?, book: Book::Spot })
    }

    pub fn derivatives() -> Result<Self, CapsError> {
        Ok(Bitfinex { caps: Capabilities::from_toml(include_str!("capabilities_derivatives.toml"))?, book: Book::Derivatives })
    }

    pub fn book(&self) -> Book {
        self.book
    }
}

enum Pending {
    /// `next`: where the following page starts (ms), when one is due.
    Candles { instrument: InstrumentId, tf: Timeframe, range: TimeRange, next: Option<i64> },
    /// An empty page for a name not known yet: whether Bitfinex ever traded it decides
    /// between a quiet period and an unknown symbol, so the page waits for the answer.
    Check { instrument: InstrumentId, next: Option<i64> },
    Instruments,
    Quote { instrument: InstrumentId },
    Board,
    Account(account::Pending),
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<CorrId, Pending>,
    /// Names Bitfinex answered bars for.
    known: HashSet<String>,
    /// The request an answer named as the next step, sent as its follow-up.
    next: Option<(CorrId, NativeRequest)>,
    live: stream::LiveState,
}

/// The candle key's timeframe token, the same on REST and the socket. `4h` answers though
/// the docs leave it out; `2h`, `8h` and `1d` answer nothing, `7D` opens on Thursday and
/// `14D` is not a calendar unit, so neither is used (all measured on 2026-10-19).
pub(crate) fn interval(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1m",
        (5, TfUnit::Minute) => "5m",
        (15, TfUnit::Minute) => "15m",
        (30, TfUnit::Minute) => "30m",
        (1, TfUnit::Hour) => "1h",
        (3, TfUnit::Hour) => "3h",
        (4, TfUnit::Hour) => "4h",
        (6, TfUnit::Hour) => "6h",
        (12, TfUnit::Hour) => "12h",
        (1, TfUnit::Day) => "1D",
        (1, TfUnit::Week) => "1W",
        (1, TfUnit::Month) => "1M",
        _ => return None,
    })
}

fn align_up(t: i64, step: i64) -> i64 {
    -(-t).div_euclid(step) * step
}

/// A currency as the account and the lists write it, the derivatives suffix dropped: the
/// collateral `USTF0` is USDt like `UST`, `BTCF0` is BTC (Bitfinex's `pub:map:currency:sym`).
pub(crate) fn currency(code: &str) -> String {
    let c = code.trim().to_ascii_uppercase();
    match c.strip_suffix("F0") {
        Some(base) if !base.is_empty() => base.to_string(),
        _ => c,
    }
}

/// Base and quote of a pair name (`tBTCUSD`, `tAVAX:USD`, `tBTCF0:USTF0`), as currencies.
pub(crate) fn split_pair(native: &str) -> Option<(String, String)> {
    let body = native.strip_prefix('t').unwrap_or(native);
    let (b, q) = match body.split_once(':') {
        Some((b, q)) => (b, q),
        None if body.len() == 6 && body.is_ascii() => body.split_at(3),
        None => return None,
    };
    (!b.is_empty() && !q.is_empty()).then(|| (currency(b), currency(q)))
}

/// A candle row: `[MTS, OPEN, CLOSE, HIGH, LOW, VOLUME]` (close before high and low).
pub(crate) struct CandleRow {
    pub open_ms: i64,
    pub o: Dec,
    pub c: Dec,
    pub h: Dec,
    pub l: Dec,
    /// In the base currency, on both books.
    pub vol: Dec,
}

impl CandleRow {
    pub fn parse(row: &[Box<RawValue>]) -> Option<CandleRow> {
        let dec = |i: usize| row.get(i).and_then(|t| serde_json::from_str::<Dec>(t.get()).ok());
        let open_ms = row.first().and_then(|t| serde_json::from_str::<Int>(t.get()).ok())?.0;
        Some(CandleRow { open_ms, o: dec(1)?, c: dec(2)?, h: dec(3)?, l: dec(4)?, vol: dec(5)? })
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
            volume: self.vol.qty(),
            quote_volume: None,
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
    ConnectorError::decode(format!("bitfinex {what}: {e}"))
}

fn positive(d: Decimal) -> Option<Decimal> {
    (d.is_sign_positive() && !d.is_zero()).then_some(d)
}

/// The first millisecond of the calendar month after the one `ms` falls in.
fn month_after(ms: i64) -> Option<i64> {
    let d = time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)).ok()?.date();
    let (y, m) = if d.month() == time::Month::December { (d.year() + 1, time::Month::January) } else { (d.year(), d.month().next()) };
    Some(time::Date::from_calendar_date(y, m, 1).ok()?.midnight().assume_utc().unix_timestamp() * 1000)
}

fn candles_path(token: &str, native: &str) -> String {
    format!("/v2/candles/trade:{token}:{native}/hist")
}

impl Bitfinex {
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

    /// A market-data call names an instrument of this connector's book: both books ride the
    /// same endpoints, so a name of the other book would be answered under the wrong venue.
    fn mine(&self, instrument: &InstrumentId) -> Result<(), ConnectorError> {
        if self.book.owns(&instrument.native) {
            return Ok(());
        }
        Err(ConnectorError::not_found(match self.book {
            Book::Spot => format!("`{}` is a Bitfinex derivatives contract: read it on a connector whose market is derivatives", instrument.native),
            Book::Derivatives => {
                format!("`{}` is a Bitfinex spot pair: derivatives are written <BASE>F0:<QUOTE>F0 (BTCF0:USTF0)", instrument.native)
            }
        }))
    }

    /// Pages walk forwards from the range's start, `PAGE` bars of the timeframe at a time;
    /// both ends of a window are inclusive on the open time (measured).
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !self.caps.history_timeframes().contains(&tf) {
            return Err(ConnectorError::unsupported(format!("{} serves no {tf} history", self.caps.label)));
        }
        self.mine(instrument)?;
        let token = interval(tf).ok_or_else(|| ConnectorError::unsupported(format!("{} has no {tf} candles", self.book.label())))?;
        let to_ms = range.to.millis();
        let (start, end, next) = match tf.fixed_secs() {
            Some(step) => {
                let step = step * 1000;
                let start = match cursor {
                    Some(c) => c.parse::<i64>().map_err(|_| ConnectorError::invalid(format!("bad cursor `{c}`")))?,
                    None => align_up(range.from.millis(), step),
                };
                let stop = start.saturating_add(PAGE * step).min(to_ms).max(start);
                (start, (stop - 1).max(start), (stop < to_ms).then_some(stop))
            }
            // A calendar month: a few hundred bars at most, one call.
            None => (range.from.millis(), (to_ms - 1).max(range.from.millis()), None),
        };
        let h = HttpRequest::get("candles", REST, candles_path(token, &instrument.native), req_id)
            .query("start", start)
            .query("end", end)
            .query("limit", PAGE)
            .query("sort", 1);
        s.pending.insert(req_id, Pending::Candles { instrument: instrument.clone(), tf, range, next });
        Ok(vec![NativeRequest::Http(h)])
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, next: Option<i64>, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let raw: Vec<Vec<Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err("candles", e))?;
        let rows = raw.iter().map(|r| CandleRow::parse(r).ok_or_else(|| ConnectorError::decode("bitfinex candles: row shape"))).collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() && !s.known.contains(&instrument.native) && ctx.instruments.get(&instrument).is_none() {
            // An unknown name answers an empty list too (measured): ask whether it ever
            // traded before calling this period quiet.
            let h = HttpRequest::get("candles", REST, candles_path("1M", &instrument.native), corr).query("limit", 1);
            s.pending.insert(corr, Pending::Check { instrument, next });
            s.next = Some((corr, NativeRequest::Http(h)));
            return Ok(vec![]);
        }
        if !rows.is_empty() {
            s.known.insert(instrument.native.clone());
        }
        let settled_before = ctx.received_ts.millis() - SETTLE_MS;
        let mut candles = Vec::with_capacity(rows.len());
        for r in &rows {
            if !range.contains(ts_millis(r.open_ms)?) {
                continue;
            }
            let is_final = match tf.fixed_secs() {
                Some(step) => r.open_ms.saturating_add(step * 1000) <= settled_before,
                // A month is over when the next one has opened plus the settle window.
                None => month_after(r.open_ms).is_some_and(|n| n <= settled_before),
            };
            candles.push(r.candle(instrument.clone(), tf, is_final)?);
        }
        candles.sort_by_key(|c| c.open_ts);
        Ok(vec![chunk(ctx, corr, candles, next)])
    }

    /// The answer to "did Bitfinex ever trade this name": a monthly bar or nothing.
    fn decode_check(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, next: Option<i64>, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let raw: Vec<Box<RawValue>> = serde_json::from_slice(body).map_err(|e| decode_err("candles", e))?;
        if raw.is_empty() {
            return Err(self.about(ConnectorError::not_found("no bar was ever traded under that name"), &instrument));
        }
        s.known.insert(instrument.native);
        Ok(vec![chunk(ctx, corr, Vec::new(), next)])
    }

    /// `pub:info:pair` (and its `:futures` twin): `[[[PAIR, [?, null, null, MIN_ORDER,
    /// MAX_ORDER, ...]], ...]]`. Every listed pair is there (checked against
    /// `pub:list:pair:*` on 2026-10-19). Paper-trading pairs (`TEST...`) are left out.
    fn decode_instruments(&self, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        type Row = (String, Vec<Box<RawValue>>);
        let outer: Vec<Vec<Row>> = serde_json::from_slice(body).map_err(|e| decode_err("pair info", e))?;
        let mut out = Vec::new();
        for (pair, info) in outer.into_iter().flatten() {
            let pair = pair.trim().to_ascii_uppercase();
            if pair.is_empty() || pair.starts_with("TEST") {
                continue;
            }
            let native = format!("t{pair}");
            if !self.book.owns(&native) {
                continue;
            }
            let Some((base, quote)) = split_pair(&native) else {
                tracing::warn!("bitfinex pairs: `{pair}` does not split into base and quote, left out");
                continue;
            };
            let mut spec = InstrumentSpec::spot(InstrumentId::new(self.book.venue(), &native), &base, &quote);
            spec.min_qty = info.get(3).and_then(|v| serde_json::from_str::<Dec>(v.get()).ok()).and_then(|d| positive(d.0)).map(Qty);
            if self.book == Book::Derivatives {
                spec.asset_class = AssetClass::Perp;
                spec.contract = ContractType::Perp;
                // Amounts count the base currency itself: one unit is one coin.
                spec.settlement_ccy = Some(quote.clone());
            }
            out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }

    /// `[BID, BID_SIZE, ASK, ASK_SIZE, DAILY_CHANGE, DAILY_CHANGE_RELATIVE, LAST_PRICE,
    /// VOLUME, HIGH, LOW, ...]`.
    fn decode_quote(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let row: Vec<Box<RawValue>> = serde_json::from_slice(body).map_err(|e| decode_err("ticker", e))?;
        let dec = |i: usize| row.get(i).and_then(|t| serde_json::from_str::<Dec>(t.get()).ok()).map(|d| d.0);
        let (Some(bid), Some(ask)) = (dec(0).and_then(positive), dec(2).and_then(positive)) else { return Ok(vec![]) };
        Ok(vec![env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: dec(1).map(Qty), ask_qty: dec(3).map(Qty) }))])
    }

    /// `[[SYMBOL, BID, BID_SIZE, ASK, ASK_SIZE, DAILY_CHANGE, DAILY_CHANGE_RELATIVE,
    /// LAST_PRICE, ...], ...]`; funding tickers (`fUSD`) are laid out otherwise and left out,
    /// as are the derivatives and the paper-trading pairs.
    fn decode_board(&self, corr: CorrId, body: &[u8], ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_slice(body).map_err(|e| decode_err("tickers", e))?;
        let board = rows
            .iter()
            .filter_map(|r| {
                let symbol: String = serde_json::from_str(r.first()?.get()).ok()?;
                if !symbol.starts_with('t') || symbol.starts_with("tTEST") || !self.book.owns(&symbol) {
                    return None;
                }
                let last = serde_json::from_str::<Dec>(r.get(7)?.get()).ok()?.0;
                Some(LastPrice { instrument: InstrumentId::new(self.book.venue(), &symbol), price: Price(positive(last)?) })
            })
            .collect();
        Ok(vec![env(ctx, corr, Event::Board(board))])
    }

    /// A market-data refusal that is about the symbol says which one, and how Bitfinex
    /// spells its names.
    fn about(&self, e: ConnectorError, instrument: &InstrumentId) -> ConnectorError {
        if e.kind != ErrorKind::NotFound {
            return e;
        }
        let mut out = ConnectorError::not_found(format!(
            "{} does not list `{}`: {}. Pairs are written BTCUSD (or tBTCUSD), with a colon when a code is longer than three letters (AVAX:USD); derivatives BTCF0:USTF0.",
            self.book.label(),
            instrument.native,
            e.message
        ));
        out.provider_code = e.provider_code;
        out
    }
}

fn chunk(ctx: &MapCtx<'_>, corr: CorrId, candles: Vec<Candle>, next: Option<i64>) -> Envelope {
    let next = next.map(|n| n.to_string());
    let done = next.is_none();
    env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })
}

/// A name as typed (`BTCUSD`, `tBTCUSD`, `avax:usd`, `BTCF0:USTF0`) to Bitfinex's own
/// (`tBTCUSD`, `tAVAX:USD`, `tBTCF0:USTF0`). A lowercase `t` followed by the upper-case pair
/// is the prefix; anything else is the pair itself, so `TRXUSD` stays TRX.
pub(crate) fn native_of(symbol: &str) -> Option<String> {
    let s = symbol.trim();
    let body = match s.strip_prefix('t') {
        Some(rest) if !rest.is_empty() && !rest.chars().any(|c| c.is_ascii_lowercase()) => rest.to_string(),
        _ => s.to_ascii_uppercase(),
    };
    let ok_part = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric());
    let valid = match body.split_once(':') {
        Some((b, q)) => ok_part(b) && ok_part(q),
        None => ok_part(&body) && body.len() == 6,
    };
    valid.then(|| format!("t{body}"))
}

impl Mapper for Bitfinex {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// Either book takes either spelling (an account names pairs and contracts in one list);
    /// the id carries the venue of the book the name belongs to.
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let native = native_of(symbol).ok_or_else(|| {
            ConnectorError::not_found(format!(
                "Bitfinex: bad symbol `{symbol}`. Write the pair as Bitfinex does: BTCUSD (or tBTCUSD), AVAX:USD when a code is longer than three letters, BTCF0:USTF0 for a perpetual"
            ))
        })?;
        Ok(InstrumentId::new(Book::of(&native).venue(), &native))
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } => {
                self.candles(*req_id, instrument, *tf, *range, cursor.as_deref(), s)
            }
            Command::LoadInstruments { req_id } => {
                s.pending.insert(*req_id, Pending::Instruments);
                let path = match self.book {
                    Book::Spot => "/v2/conf/pub:info:pair",
                    Book::Derivatives => "/v2/conf/pub:info:pair:futures",
                };
                Ok(vec![NativeRequest::Http(HttpRequest::get("conf", REST, path, *req_id))])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } if self.caps.history.quote => {
                self.mine(instrument)?;
                s.pending.insert(*req_id, Pending::Quote { instrument: instrument.clone() });
                Ok(vec![NativeRequest::Http(HttpRequest::get("ticker", REST, format!("/v2/ticker/{}", instrument.native), *req_id))])
            }
            Command::RequestBoard { req_id } if self.caps.history.board => {
                s.pending.insert(*req_id, Pending::Board);
                Ok(vec![NativeRequest::Http(HttpRequest::get("tickers", REST, "/v2/tickers", *req_id).query("symbols", "ALL"))])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live, ctx),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(cmd, &mut s.pending),
            Command::Connect | Command::Disconnect => Ok(vec![]),
            // SyncTime is not declared: the nonce is a counter, not a clock check.
            other => Err(ConnectorError::unsupported(format!("{}: {other:?} is not declared", self.caps.label))),
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
        let (corr, body) = match frame {
            RawFrame::Http { corr_id, body, .. } => (*corr_id, body),
            RawFrame::Timeout { corr_id, .. } => {
                s.pending.remove(corr_id);
                return Err(errors::from_frame(frame).unwrap_or_else(|| ConnectorError::decode("timeout")));
            }
            RawFrame::Ws { payload, .. } => return stream::decode(payload, &mut s.live, ctx),
            RawFrame::Closed { .. } => return Ok(vec![]),
        };
        let pending = s.pending.remove(&corr).ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr}")))?;
        if let Some(e) = errors::from_frame(frame) {
            return match pending {
                Pending::Candles { instrument, .. } | Pending::Quote { instrument } | Pending::Check { instrument, .. } => Err(self.about(e, &instrument)),
                Pending::Account(p) => Err(account::hint(&p, e)),
                _ => Err(e),
            };
        }
        match pending {
            Pending::Candles { instrument, tf, range, next } => self.decode_candles(corr, body, instrument, tf, range, next, s, ctx),
            Pending::Check { instrument, next } => self.decode_check(corr, body, instrument, next, s, ctx),
            Pending::Instruments => self.decode_instruments(corr, body, ctx),
            Pending::Quote { instrument } => self.decode_quote(corr, body, instrument, ctx),
            Pending::Board => self.decode_board(corr, body, ctx),
            Pending::Account(p) => account::decode(p, corr, body, s, ctx),
        }
    }

    fn follow_ups(&self, corr_id: CorrId, s: &mut Session) -> Vec<NativeRequest> {
        match s.next.take() {
            Some((corr, req)) if corr == corr_id => vec![req],
            other => {
                s.next = other;
                Vec::new()
            }
        }
    }

    fn abandon(&self, corr_id: CorrId, s: &mut Session) {
        s.pending.remove(&corr_id);
        if s.next.as_ref().is_some_and(|(c, _)| *c == corr_id) {
            s.next = None;
        }
    }

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(&mut s.live, ctx)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        Ok(Some(Arc::new(signer::BitfinexSigner::from_secrets(secrets)?)))
    }

    /// `tBTCUSD`, or `tAVAX:USD` when either code is longer than three letters (every listed
    /// pair follows that rule, checked on 2026-10-19).
    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        if self.book != Book::Spot {
            return None;
        }
        let (b, q) = (base.trim().to_ascii_uppercase(), quote.trim().to_ascii_uppercase());
        if b.is_empty() || q.is_empty() {
            return None;
        }
        Some(if b.len() == 3 && q.len() == 3 { format!("t{b}{q}") } else { format!("t{b}:{q}") })
    }
}

#[cfg(test)]
mod tests;
