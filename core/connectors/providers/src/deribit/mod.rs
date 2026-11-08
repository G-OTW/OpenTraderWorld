//! Deribit (API v2, JSON-RPC over HTTP and a socket), one adapter for the whole venue:
//! perpetuals and dated futures, inverse (settled in the coin, `BTC-PERPETUAL`,
//! `BTC-27DEC26`) and linear (settled in USDC, `BTC_USDC-PERPETUAL`), options
//! (`BTC-27DEC26-100000-C`, `SOL_USDC-27DEC26-150-P`) and spot pairs (`BTC_USDT`). Candle
//! history, the instrument list, best bid/ask, the spot price board, the clock, live candles
//! and the account read only. Every kind answers on the same endpoints and its name says
//! which kind it is, so there is no book to choose. Endpoint-by-endpoint mapping in
//! PROVIDER.md.

mod account;
mod errors;
mod signer;
mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use otw_connector_model::{
    AssetClass, Candle, Capabilities, CapsError, CanonicalSymbol, Command, ConnectorError, ContractType, CorrId, DataType, Decimal, Envelope,
    ErrorKind, Event, HistoryItems, InstrumentId, InstrumentSpec, LastPrice, Money, OptionRight, Price, Qty, Quote, TfUnit, TimeRange,
    Timeframe, Timestamp,
};
use otw_connector_sdk::parse::{ts_millis, Dec, Int, OptDec};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, NativeRequest, RawFrame, Signer};
use serde::Deserialize;

const REST: &str = "rest";
const WS: &str = "ws";
const BASES: &[(&str, &str)] = &[(REST, "https://www.deribit.com"), (WS, "wss://www.deribit.com")];
pub(crate) const VENUE: &str = "DERIBIT";
const LABEL: &str = "Deribit";
/// Candles per history call. A wider window answers 5001 bars at most, its newest ones
/// (measured on 2026-10-19 over 1m, 1h and 1D): a page asks 5000.
const PAGE: i64 = 5000;
/// Daily bars open at 08:00 UTC, Deribit's settlement time (measured). Every shorter
/// resolution served here is aligned on the epoch.
const DAY_ANCHOR_MS: i64 = 8 * 3_600_000;
/// The archive flags no forming bar. Measured on 2026-10-19 by polling it every second
/// through ten minute boundaries over BTC-PERPETUAL, ETH-PERPETUAL, BTC_USDC-PERPETUAL,
/// SOL_USDC-PERPETUAL and BTC_USDT: a closed minute was rewritten up to 1.5 s after its
/// close (16 rewrites). A bar is final once its period plus this has passed, about six times
/// the worst one seen.
const SETTLE_MS: i64 = 10_000;
/// The spot currencies whose book summaries together cover every listed pair: a pair is
/// summarised under its base and under its quote (measured).
const BOARD_CURRENCIES: &[&str] = &["USDC", "USDT", "BTC", "ETH"];
/// What a spot pair matched on Coinbase Exchange carries as its note.
pub(crate) const ROUTED_NOTE: &str = "matched on Coinbase Exchange";

#[derive(Clone)]
pub struct Deribit {
    caps: Capabilities,
}

impl Deribit {
    pub fn new() -> Result<Self, CapsError> {
        Ok(Deribit { caps: Capabilities::from_toml(include_str!("capabilities.toml"))? })
    }
}

/// What an instrument is, read from its name. Deribit's names are unambiguous: a spot pair
/// is `BASE_QUOTE`, a perpetual `UNDER-PERPETUAL`, a dated future `UNDER-DDMMMYY`, an
/// option `UNDER-DDMMMYY-STRIKE-C|P`, where `UNDER` is the coin (inverse, settled in the
/// coin) or `COIN_USDC` (linear, settled in USDC). Anything else is a combo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Spot,
    Perp,
    Future,
    Option,
    Combo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Name<'a> {
    pub kind: Kind,
    /// The pair (spot), or the underlying (`BTC`, `BTC_USDC`).
    under: &'a str,
    /// `DDMMMYY`, dated contracts only.
    date: Option<&'a str>,
}

impl<'a> Name<'a> {
    pub fn of(native: &'a str) -> Name<'a> {
        let parts: Vec<&str> = native.split('-').collect();
        let combo = Name { kind: Kind::Combo, under: native, date: None };
        let named = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        match parts.as_slice() {
            [pair] => match pair.split_once('_') {
                Some((b, q)) if !b.is_empty() && !q.is_empty() && !q.contains('_') => Name { kind: Kind::Spot, under: pair, date: None },
                _ => combo,
            },
            [under, "PERPETUAL"] if named(under) => Name { kind: Kind::Perp, under, date: None },
            [under, date] if named(under) && expiry_ms(date).is_some() => Name { kind: Kind::Future, under, date: Some(date) },
            [under, date, strike, "C" | "P"] if named(under) && expiry_ms(date).is_some() && !strike.is_empty() => {
                Name { kind: Kind::Option, under, date: Some(date) }
            }
            _ => combo,
        }
    }

    /// Settled in USDC (`BTC_USDC-PERPETUAL`), not in the coin.
    pub fn linear(&self) -> bool {
        self.under.contains('_')
    }

    /// A coin-settled future or perpetual: one contract is a fixed number of USD, the
    /// profit is counted in the coin.
    pub fn inverse(&self) -> bool {
        matches!(self.kind, Kind::Perp | Kind::Future) && !self.linear()
    }

    /// The currency prices are written in: the pair's quote, USD on an inverse contract,
    /// the coin on a coin-settled option, USDC on a linear one.
    pub fn quote(&self) -> Option<String> {
        let (base, rest) = match self.under.split_once('_') {
            Some((b, r)) => (b, Some(r)),
            None => (self.under, None),
        };
        match (self.kind, rest) {
            (Kind::Combo, _) => None,
            (Kind::Spot | Kind::Perp | Kind::Future | Kind::Option, Some(q)) => Some(q.to_string()),
            (Kind::Perp | Kind::Future, None) => Some("USD".into()),
            (Kind::Option, None) => Some(base.to_string()),
            (Kind::Spot, None) => None,
        }
    }

    pub fn class(&self) -> Option<AssetClass> {
        match self.kind {
            Kind::Spot => Some(AssetClass::Spot),
            Kind::Perp => Some(AssetClass::Perp),
            Kind::Future => Some(AssetClass::Future),
            Kind::Option => Some(AssetClass::Option),
            Kind::Combo => None,
        }
    }

    /// When a dated contract stops trading (08:00 UTC on its date), in milliseconds.
    pub fn expiry_ms(&self) -> Option<i64> {
        self.date.and_then(expiry_ms)
    }
}

/// `27DEC26` at 08:00 UTC, in milliseconds.
fn expiry_ms(date: &str) -> Option<i64> {
    let split = date.find(|c: char| !c.is_ascii_digit())?;
    let (day, rest) = date.split_at(split);
    if rest.len() != 5 || day.is_empty() || day.len() > 2 {
        return None;
    }
    let (mon, year) = rest.split_at(3);
    let month = match mon {
        "JAN" => time::Month::January,
        "FEB" => time::Month::February,
        "MAR" => time::Month::March,
        "APR" => time::Month::April,
        "MAY" => time::Month::May,
        "JUN" => time::Month::June,
        "JUL" => time::Month::July,
        "AUG" => time::Month::August,
        "SEP" => time::Month::September,
        "OCT" => time::Month::October,
        "NOV" => time::Month::November,
        "DEC" => time::Month::December,
        _ => return None,
    };
    let year: i32 = year.parse().ok().filter(|_| year.chars().all(|c| c.is_ascii_digit()))?;
    let date = time::Date::from_calendar_date(2000 + year, month, day.parse().ok()?).ok()?;
    date.midnight().assume_utc().unix_timestamp().checked_mul(1000)?.checked_add(DAY_ANCHOR_MS)
}

/// A name as Deribit writes it: upper case, except the `d` that marks a fraction in an
/// option's strike (`XRP_USDC-25DEC26-1d75-C`).
pub(crate) fn normalize(raw: &str) -> String {
    let up = raw.trim().to_ascii_uppercase();
    let mut parts: Vec<String> = up.split('-').map(str::to_string).collect();
    if let [_, _, strike, _] = parts.as_mut_slice() {
        if strike.contains('D') && strike.chars().all(|c| c.is_ascii_digit() || c == 'D') {
            *strike = strike.replace('D', "d");
        }
    }
    parts.join("-")
}

#[derive(Default)]
pub struct Session {
    pending: HashMap<CorrId, Pending>,
    /// Whether a spot pair is matched on Coinbase Exchange, by name, as the list or a lookup
    /// said.
    routed: HashMap<String, bool>,
    /// The request an answer named as the next step, sent as its follow-up.
    next: Option<(CorrId, NativeRequest)>,
    /// Fill ids already handed out, per account query: the walks overlap.
    seen_fills: HashMap<CorrId, std::collections::HashSet<String>>,
    live: stream::LiveState,
}

enum Pending {
    /// `window`: the first bar open this page asked for (ms). `next`: where the following
    /// page ends (exclusive, ms), when one is due. `seen`: an earlier page held bars.
    Candles { instrument: InstrumentId, tf: Timeframe, range: TimeRange, window: i64, next: Option<i64>, seen: bool },
    /// A spot pair's terms, asked before its candles: one matched on Coinbase has none.
    Lookup { instrument: InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<String> },
    Instruments,
    Quote { instrument: InstrumentId },
    /// The board, one spot currency after another.
    Board { idx: usize, acc: Vec<LastPrice> },
    Time,
    Account(account::Pending),
}

/// The JSON-RPC answer envelope (`{"jsonrpc":"2.0","result":...,"usIn":...}`).
#[derive(Deserialize)]
pub(crate) struct Rpc<T> {
    pub result: T,
}

/// The resolution token, shared by the archive and the socket. 3h, 6h and 12h exist too,
/// but they open at 08:00 UTC like the day (measured), not on the epoch: the engine folds
/// them from hours instead.
pub(crate) fn resolution(tf: Timeframe) -> Option<&'static str> {
    Some(match (tf.n, tf.unit) {
        (1, TfUnit::Minute) => "1",
        (3, TfUnit::Minute) => "3",
        (5, TfUnit::Minute) => "5",
        (10, TfUnit::Minute) => "10",
        (15, TfUnit::Minute) => "15",
        (30, TfUnit::Minute) => "30",
        (1, TfUnit::Hour) => "60",
        (2, TfUnit::Hour) => "120",
        (1, TfUnit::Day) => "1D",
        _ => return None,
    })
}

/// Where a timeframe's bars open, as an offset from the epoch grid, in milliseconds.
fn anchor_ms(tf: Timeframe) -> i64 {
    if tf.unit == TfUnit::Day {
        DAY_ANCHOR_MS
    } else {
        0
    }
}

/// The first bar open at or after `t`.
fn open_at_or_after(t: i64, step: i64, anchor: i64) -> i64 {
    -(-(t - anchor)).div_euclid(step) * step + anchor
}

/// The last bar open strictly before `t`.
fn open_before(t: i64, step: i64, anchor: i64) -> i64 {
    (t - 1 - anchor).div_euclid(step) * step + anchor
}

#[derive(Deserialize)]
struct Chart {
    status: String,
    #[serde(default)]
    ticks: Vec<Int>,
    #[serde(default)]
    open: Vec<Dec>,
    #[serde(default)]
    high: Vec<Dec>,
    #[serde(default)]
    low: Vec<Dec>,
    #[serde(default)]
    close: Vec<Dec>,
    /// In the base coin, on every kind (an inverse contract included).
    #[serde(default)]
    volume: Vec<Dec>,
    /// In the quote: USD on an inverse contract, the coin on a coin-settled option.
    #[serde(default)]
    cost: Vec<Dec>,
}

pub(crate) struct Bar {
    pub open_ms: i64,
    pub o: Dec,
    pub h: Dec,
    pub l: Dec,
    pub c: Dec,
    pub volume: Dec,
    pub cost: Dec,
}

impl Bar {
    pub fn candle(&self, instrument: InstrumentId, tf: Timeframe, is_final: bool) -> Result<Candle, ConnectorError> {
        Ok(Candle {
            instrument,
            tf,
            open_ts: ts_millis(self.open_ms)?,
            open: self.o.price(),
            high: self.h.price(),
            low: self.l.price(),
            close: self.c.price(),
            volume: self.volume.qty(),
            quote_volume: Some(self.cost.money()),
            trades: None,
            is_final,
            adjusted: None,
            sides: None,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct InstrumentRow {
    instrument_name: String,
    #[serde(default)]
    kind: String,
    /// `reversed` (inverse) or `linear`.
    #[serde(default)]
    instrument_type: Option<String>,
    #[serde(default)]
    base_currency: String,
    #[serde(default)]
    quote_currency: String,
    #[serde(default)]
    settlement_currency: Option<String>,
    /// `perpetual`, `day`, `week`, `month`; absent on spot.
    #[serde(default)]
    settlement_period: Option<String>,
    #[serde(default)]
    tick_size: OptDec,
    /// The amount step: USD on an inverse contract, the base coin elsewhere.
    #[serde(default)]
    min_trade_amount: OptDec,
    #[serde(default)]
    contract_size: OptDec,
    #[serde(default)]
    expiration_timestamp: Option<Int>,
    #[serde(default)]
    strike: OptDec,
    #[serde(default)]
    option_type: Option<String>,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    is_cbe_routed: Option<bool>,
}

#[derive(Deserialize)]
struct Ticker {
    instrument_name: String,
    #[serde(default)]
    timestamp: Option<Int>,
    #[serde(default)]
    best_bid_price: OptDec,
    #[serde(default)]
    best_ask_price: OptDec,
    #[serde(default)]
    best_bid_amount: OptDec,
    #[serde(default)]
    best_ask_amount: OptDec,
}

#[derive(Deserialize)]
struct Summary {
    instrument_name: String,
    #[serde(default)]
    last: OptDec,
    #[serde(default)]
    mark_price: OptDec,
}

fn env(ctx: &MapCtx<'_>, corr: CorrId, event: Event) -> Envelope {
    Envelope::new(ctx.connector.clone(), Some(corr), ctx.received_ts, event)
}

fn decode_err(what: &str, e: serde_json::Error) -> ConnectorError {
    ConnectorError::decode(format!("deribit {what}: {e}"))
}

fn positive(d: Option<Decimal>) -> Option<Decimal> {
    d.filter(|d| d.is_sign_positive() && !d.is_zero())
}

/// The date a millisecond falls on, for a message.
fn day(ms: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)).map(|t| t.date().to_string()).unwrap_or_else(|_| ms.to_string())
}

/// A spot pair matched on Coinbase Exchange: Deribit sees its own users' share of the
/// trades only and publishes neither complete candles nor a socket topic for it.
fn routed_refusal(native: &str) -> ConnectorError {
    let coinbase = native.replace('_', "-");
    ConnectorError::unsupported(format!(
        "Deribit routes {native} to Coinbase Exchange and sees only its own users' share of the trades, so it publishes no complete \
         candles for it. Chart {coinbase} from Coinbase, or a pair Deribit matches itself (BTC_USDT, ETH_USDT, ETH_BTC)."
    ))
}

/// The terms of one listed row. Combos are left out: they are spreads of listed legs.
pub(crate) fn spec_of(r: &InstrumentRow) -> Option<InstrumentSpec> {
    let name = r.instrument_name.trim();
    if name.is_empty() || r.base_currency.is_empty() || r.quote_currency.is_empty() {
        return None;
    }
    let base = r.base_currency.to_ascii_uppercase();
    let quote = r.quote_currency.to_ascii_uppercase();
    let mut spec = InstrumentSpec::spot(InstrumentId::new(VENUE, name), &base, &quote);
    let routed = r.is_cbe_routed.unwrap_or(false);
    spec.active = r.is_active && r.state.as_deref().is_none_or(|s| s == "open") && !routed;
    spec.tick_size = positive(r.tick_size.0).map(Price);
    // Amounts step by the minimum: USD on an inverse contract, the base coin elsewhere.
    spec.min_qty = positive(r.min_trade_amount.0).map(Qty);
    spec.lot_size = spec.min_qty;
    spec.settlement_ccy = r.settlement_currency.as_ref().map(|s| s.to_ascii_uppercase()).filter(|s| !s.is_empty());
    let expiry = r.expiration_timestamp.map(|t| ts_millis(t.0)).transpose().ok().flatten();
    let reversed = match r.instrument_type.as_deref() {
        Some("reversed") => true,
        Some("linear") | None => false,
        Some(other) => {
            tracing::warn!("deribit instruments: unknown instrument_type `{other}` on {name}, left out");
            return None;
        }
    };
    match r.kind.as_str() {
        "spot" => {
            if routed {
                spec.note = Some(ROUTED_NOTE.to_string());
            }
        }
        "future" => {
            let perp = r.settlement_period.as_deref() == Some("perpetual");
            spec.symbol = CanonicalSymbol(name.to_string());
            spec.asset_class = if perp { AssetClass::Perp } else { AssetClass::Future };
            spec.contract = if perp { ContractType::Perp } else { ContractType::Future };
            if !perp {
                spec.expiry = expiry;
            }
            // One contract is a fixed number of USD; the amount is counted in USD.
            spec.inverse = reversed;
            if reversed {
                let size = r.contract_size.0.map(|d| d.normalize().to_string()).unwrap_or_default();
                spec.note = Some(format!("inverse: one contract is {size} USD, settled in {}", spec.settlement_ccy.clone().unwrap_or_else(|| base.clone())));
            }
        }
        "option" => {
            spec.symbol = CanonicalSymbol(name.to_string());
            spec.asset_class = AssetClass::Option;
            spec.contract = ContractType::Option;
            spec.expiry = expiry;
            spec.strike = positive(r.strike.0).map(Price);
            spec.right = match r.option_type.as_deref() {
                Some("call") => Some(OptionRight::Call),
                Some("put") => Some(OptionRight::Put),
                other => {
                    tracing::warn!("deribit instruments: unknown option_type `{other:?}` on {name}, left out");
                    return None;
                }
            };
        }
        "future_combo" | "option_combo" => return None,
        other => {
            tracing::warn!("deribit instruments: unknown kind `{other}` on {name}, left out");
            return None;
        }
    }
    Some(spec)
}

impl Deribit {
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

    /// Whether a spot pair is matched on Coinbase, as far as this session knows.
    fn routed(&self, instrument: &InstrumentId, s: &Session, ctx: &MapCtx<'_>) -> Option<bool> {
        if let Some(r) = s.routed.get(&instrument.native) {
            return Some(*r);
        }
        ctx.instruments.get(instrument).map(|spec| spec.note.as_deref() == Some(ROUTED_NOTE))
    }

    #[allow(clippy::too_many_arguments)]
    fn candles(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        if !self.caps.history_timeframes().contains(&tf) {
            return Err(ConnectorError::unsupported(format!("{LABEL} serves no {tf} history")));
        }
        if Name::of(&instrument.native).kind == Kind::Spot {
            match self.routed(instrument, s, ctx) {
                Some(true) => return Err(routed_refusal(&instrument.native)),
                Some(false) => {}
                None => {
                    // Whether the pair is matched on Coinbase decides, and an unknown pair is
                    // refused by name rather than answered with an empty page.
                    s.pending.insert(req_id, Pending::Lookup { instrument: instrument.clone(), tf, range, cursor: cursor.map(str::to_string) });
                    let h = HttpRequest::get("instrument", REST, "/api/v2/public/get_instrument", req_id).query("instrument_name", &instrument.native);
                    return Ok(vec![NativeRequest::Http(h)]);
                }
            }
        }
        Ok(vec![self.candle_request(req_id, instrument, tf, range, cursor, s)?])
    }

    /// Pages walk backwards from the range's end (or a dated contract's expiry), a page of
    /// bars at a time, and stop at the listing: the archive answers `no_data` before it.
    fn candle_request(&self, req_id: CorrId, instrument: &InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<&str>, s: &mut Session) -> Result<NativeRequest, ConnectorError> {
        let token = resolution(tf).ok_or_else(|| ConnectorError::unsupported(format!("{LABEL} has no {tf} candles")))?;
        let step = tf.fixed_secs().ok_or_else(|| ConnectorError::unsupported(format!("{LABEL} has no {tf} candles")))?.saturating_mul(1000);
        let anchor = anchor_ms(tf);
        let first = open_at_or_after(range.from.millis(), step, anchor);
        let (end, seen) = match cursor {
            Some(c) => {
                let bad = || ConnectorError::invalid(format!("bad cursor `{c}`"));
                let (end, seen) = c.split_once(':').ok_or_else(bad)?;
                (end.parse::<i64>().map_err(|_| bad())?, seen == "1")
            }
            None => {
                let mut end = range.to.millis();
                if let Some(exp) = Name::of(&instrument.native).expiry_ms() {
                    if exp <= range.from.millis() {
                        return Err(ConnectorError::not_found(format!(
                            "{} expired on {}: ask for a period before that.",
                            instrument.native,
                            day(exp)
                        )));
                    }
                    end = end.min(exp);
                }
                (end, false)
            }
        };
        let last = open_before(end, step, anchor);
        let start = first.max(last.saturating_sub((PAGE - 1).saturating_mul(step)));
        // Both ends are bar opens, both included (measured).
        let h = HttpRequest::get("chart", REST, "/api/v2/public/get_tradingview_chart_data", req_id)
            .query("instrument_name", &instrument.native)
            .query("start_timestamp", start)
            .query("end_timestamp", last.max(start))
            .query("resolution", token);
        s.pending.insert(req_id, Pending::Candles { instrument: instrument.clone(), tf, range, window: start, next: (start > first).then_some(start), seen });
        Ok(NativeRequest::Http(h))
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_candles(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, window: i64, next: Option<i64>, seen: bool, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let r: Rpc<Chart> = serde_json::from_slice(body).map_err(|e| decode_err("chart", e))?;
        let c = r.result;
        let n = c.ticks.len();
        if [c.open.len(), c.high.len(), c.low.len(), c.close.len(), c.volume.len(), c.cost.len()].iter().any(|l| *l != n) {
            return Err(ConnectorError::decode("deribit chart: columns of different lengths"));
        }
        let none = match c.status.as_str() {
            "ok" => n == 0,
            "no_data" => true,
            other => return Err(ConnectorError::decode(format!("deribit chart: status `{other}`"))),
        };
        let step = tf.fixed_secs().unwrap_or(86_400).saturating_mul(1000);
        let settled_before = ctx.received_ts.millis() - step - SETTLE_MS;
        let mut candles = Vec::with_capacity(n);
        for i in 0..n {
            let (Some(t), Some(o), Some(h), Some(l), Some(cl), Some(v), Some(q)) = (c.ticks.get(i), c.open.get(i), c.high.get(i), c.low.get(i), c.close.get(i), c.volume.get(i), c.cost.get(i)) else {
                continue;
            };
            let bar = Bar { open_ms: t.0, o: *o, h: *h, l: *l, c: *cl, volume: *v, cost: *q };
            // The archive answers the bar holding the window's start and, past the clock,
            // the forming one: only the asked bars inside the range are kept.
            if bar.open_ms < window || !range.contains(ts_millis(bar.open_ms)?) {
                continue;
            }
            candles.push(bar.candle(instrument.clone(), tf, bar.open_ms <= settled_before)?);
        }
        candles.sort_by_key(|c| c.open_ts);
        candles.dedup_by_key(|c| c.open_ts);
        // The listing is inside this window when its first bar opens after the window's
        // start; before the listing the archive answers `no_data`.
        let listed_inside = c.ticks.first().is_some_and(|t| t.0 > window);
        let next = match (none, seen) {
            (true, true) => None,
            (false, _) if listed_inside => None,
            _ => next,
        };
        let seen = seen || !none;
        let next = next.map(|n| format!("{n}:{}", u8::from(seen)));
        let done = next.is_none();
        Ok(vec![env(ctx, corr, Event::HistoryChunk { items: HistoryItems::Candles(candles), next_cursor: next, done })])
    }

    /// A spot pair's terms: refused when it is matched on Coinbase, its page queued
    /// otherwise.
    #[allow(clippy::too_many_arguments)]
    fn decode_lookup(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, tf: Timeframe, range: TimeRange, cursor: Option<String>, s: &mut Session) -> Result<Vec<Envelope>, ConnectorError> {
        let r: Rpc<InstrumentRow> = serde_json::from_slice(body).map_err(|e| decode_err("instrument", e))?;
        let routed = r.result.is_cbe_routed.unwrap_or(false);
        s.routed.insert(instrument.native.clone(), routed);
        if routed {
            return Err(routed_refusal(&instrument.native));
        }
        let req = self.candle_request(corr, &instrument, tf, range, cursor.as_deref(), s)?;
        s.next = Some((corr, req));
        Ok(vec![])
    }

    fn decode_instruments(&self, corr: CorrId, body: &[u8], s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let r: Rpc<Vec<InstrumentRow>> = serde_json::from_slice(body).map_err(|e| decode_err("instruments", e))?;
        let mut out = Vec::with_capacity(r.result.len() + 1);
        for row in &r.result {
            if row.kind == "spot" {
                s.routed.insert(row.instrument_name.clone(), row.is_cbe_routed.unwrap_or(false));
            }
            if let Some(spec) = spec_of(row) {
                out.push(env(ctx, corr, Event::Instrument { spec: Box::new(spec) }));
            }
        }
        out.push(env(ctx, corr, Event::InstrumentsEnd));
        Ok(out)
    }

    fn decode_quote(&self, corr: CorrId, body: &[u8], instrument: InstrumentId, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let r: Rpc<Ticker> = serde_json::from_slice(body).map_err(|e| decode_err("ticker", e))?;
        let t = r.result;
        if t.instrument_name != instrument.native {
            return Err(ConnectorError::decode(format!("deribit ticker answered {} for {}", t.instrument_name, instrument.native)));
        }
        let (Some(bid), Some(ask)) = (positive(t.best_bid_price.0), positive(t.best_ask_price.0)) else { return Ok(vec![]) };
        // An inverse contract's sizes are USD, not coins: they are not carried.
        let sized = !Name::of(&instrument.native).inverse();
        let size = |d: OptDec| d.0.filter(|_| sized).map(Qty);
        let e = env(ctx, corr, Event::Quote(Quote { instrument, bid: Price(bid), ask: Price(ask), bid_qty: size(t.best_bid_amount), ask_qty: size(t.best_ask_amount) }));
        Ok(vec![match t.timestamp {
            Some(ts) => e.at(ts_millis(ts.0)?),
            None => e,
        }])
    }

    fn board_request(req_id: CorrId, idx: usize) -> Option<NativeRequest> {
        let currency = BOARD_CURRENCIES.get(idx)?;
        Some(NativeRequest::Http(
            HttpRequest::get("bookSummary", REST, "/api/v2/public/get_book_summary_by_currency", req_id).query("currency", currency).query("kind", "spot"),
        ))
    }

    fn decode_board(&self, corr: CorrId, body: &[u8], idx: usize, mut acc: Vec<LastPrice>, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        let r: Rpc<Vec<Summary>> = serde_json::from_slice(body).map_err(|e| decode_err("book summary", e))?;
        for row in r.result {
            let Some(price) = positive(row.last.0.or(row.mark_price.0)) else { continue };
            if row.instrument_name.is_empty() || acc.iter().any(|p| p.instrument.native == row.instrument_name) {
                continue;
            }
            acc.push(LastPrice { instrument: InstrumentId::new(VENUE, &row.instrument_name), price: Price(price) });
        }
        match Self::board_request(corr, idx + 1) {
            Some(req) => {
                s.pending.insert(corr, Pending::Board { idx: idx + 1, acc });
                s.next = Some((corr, req));
                Ok(vec![])
            }
            None => Ok(vec![env(ctx, corr, Event::Board(acc))]),
        }
    }

    /// A market-data refusal about the instrument names it and how Deribit spells names.
    fn about(&self, e: ConnectorError, instrument: &InstrumentId) -> ConnectorError {
        if e.kind != ErrorKind::NotFound {
            return e;
        }
        let mut out = ConnectorError::not_found(format!(
            "{LABEL} does not list `{}`: {}. Deribit writes BTC-PERPETUAL, BTC_USDC-PERPETUAL, BTC-27DEC26, BTC-27DEC26-100000-C, BTC_USDT.",
            instrument.native, e.message
        ));
        out.provider_code = e.provider_code;
        out
    }
}

impl Mapper for Deribit {
    type Session = Session;

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        BASES
    }

    /// Exact, as Deribit writes it, case aside. The venue refuses a name it does not list.
    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        let s = normalize(symbol);
        if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(ConnectorError::not_found(format!(
                "Deribit: bad symbol `{symbol}`, write it as Deribit spells it (BTC-PERPETUAL, BTC_USDC-PERPETUAL, BTC-27DEC26, BTC-27DEC26-100000-C, BTC_USDT)"
            )));
        }
        Ok(InstrumentId::new(VENUE, &s))
    }

    fn own_option_symbols(&self) -> bool {
        true
    }

    fn encode(&self, cmd: &Command, s: &mut Session, ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        match cmd {
            Command::RequestHistory { req_id, instrument, data_type: DataType::Candles { tf }, range, cursor } => {
                self.candles(*req_id, instrument, *tf, *range, cursor.as_deref(), s, ctx)
            }
            Command::LoadInstruments { req_id } => {
                s.pending.insert(*req_id, Pending::Instruments);
                let h = HttpRequest::get("instruments", REST, "/api/v2/public/get_instruments", *req_id).query("currency", "any").query("expired", "false");
                Ok(vec![NativeRequest::Http(h)])
            }
            Command::RequestSnapshot { req_id, instrument, data_type: DataType::Quote } => {
                s.pending.insert(*req_id, Pending::Quote { instrument: instrument.clone() });
                Ok(vec![NativeRequest::Http(HttpRequest::get("ticker", REST, "/api/v2/public/ticker", *req_id).query("instrument_name", &instrument.native))])
            }
            Command::RequestBoard { req_id } if self.caps.history.board => {
                let req = Self::board_request(*req_id, 0).ok_or_else(|| ConnectorError::invalid("no board currency"))?;
                s.pending.insert(*req_id, Pending::Board { idx: 0, acc: Vec::new() });
                Ok(vec![req])
            }
            Command::SyncTime { req_id } => {
                s.pending.insert(*req_id, Pending::Time);
                Ok(vec![NativeRequest::Http(HttpRequest::get("time", REST, "/api/v2/public/get_time", *req_id))])
            }
            Command::Subscribe { .. } | Command::Unsubscribe { .. } => stream::encode(self, std::slice::from_ref(cmd), &mut s.live, ctx),
            Command::QueryAccount { kind, .. } if self.account_declared(*kind) => account::encode(cmd, &mut s.pending, &mut s.seen_fills),
            Command::Connect | Command::Disconnect => Ok(vec![]),
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
            RawFrame::Closed { .. } => {
                stream::closed(&mut s.live);
                return Ok(vec![]);
            }
        };
        let pending = s.pending.remove(&corr).ok_or_else(|| ConnectorError::decode(format!("answer for unknown request {corr}")))?;
        if let Some(e) = errors::from_frame(frame) {
            return match pending {
                Pending::Candles { instrument, .. } | Pending::Quote { instrument } | Pending::Lookup { instrument, .. } => Err(self.about(e, &instrument)),
                Pending::Account(p) => account::refused(p, corr, e, &mut s.seen_fills, ctx),
                _ => Err(e),
            };
        }
        match pending {
            Pending::Candles { instrument, tf, range, window, next, seen } => self.decode_candles(corr, body, instrument, tf, range, window, next, seen, ctx),
            Pending::Lookup { instrument, tf, range, cursor } => self.decode_lookup(corr, body, instrument, tf, range, cursor, s),
            Pending::Instruments => self.decode_instruments(corr, body, s, ctx),
            Pending::Quote { instrument } => self.decode_quote(corr, body, instrument, ctx),
            Pending::Board { idx, acc } => self.decode_board(corr, body, idx, acc, s, ctx),
            Pending::Time => {
                let r: Rpc<Int> = serde_json::from_slice(body).map_err(|e| decode_err("time", e))?;
                Ok(vec![env(ctx, corr, Event::ServerTime(ts_millis(r.result.0)?))])
            }
            Pending::Account(p) => account::decode(p, corr, body, &mut s.seen_fills, ctx),
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
        s.seen_fills.remove(&corr_id);
        if s.next.as_ref().is_some_and(|(c, _)| *c == corr_id) {
            s.next = None;
        }
    }

    fn keepalive(&self, s: &mut Session, ctx: &MapCtx<'_>) -> Vec<NativeRequest> {
        stream::keepalive(&mut s.live, ctx)
    }

    fn signer(&self, secrets: &HashMap<String, String>) -> Result<Option<Arc<dyn Signer>>, ConnectorError> {
        Ok(Some(Arc::new(signer::DeribitSigner::from_secrets(secrets)?)))
    }

    fn pair_symbol(&self, base: &str, quote: &str) -> Option<String> {
        Some(format!("{}_{}", base.trim().to_ascii_uppercase(), quote.trim().to_ascii_uppercase()))
    }
}

/// Money in a currency, for the account side.
pub(crate) fn money(d: Option<Decimal>) -> Money {
    Money(d.unwrap_or_default())
}

/// The moment a fill or an order was stamped, from milliseconds.
pub(crate) fn at_ms(t: Option<Int>) -> Result<Option<Timestamp>, ConnectorError> {
    t.map(|t| ts_millis(t.0)).transpose()
}

#[cfg(test)]
mod tests;
