//! Turning an OTW ticker into an IB request. Pure: no gateway, no clock.
//!
//! Everywhere else a provider takes a ticker string. IB takes a **contract**: a symbol plus a
//! security type, an exchange and a currency, and it will happily serve a different
//! instrument if any of those is wrong. OTW stores one string, so the string carries the
//! other three when the defaults are not right:
//!
//! ```text
//!   AAPL              stock, SMART routing, USD
//!   SAN:EUR           stock, SMART routing, EUR
//!   7203@TSEJ:JPY     stock, one exchange, one currency
//!   BTC               crypto on PAXOS, USD
//!   EURUSD            the EUR.USD cash pair (also EUR/USD, EUR.USD)
//!   ES.202512@CME     the December 2025 future (.YYYYMM, or .YYYYMMDD for a last day)
//!   MNQU6             the same thing in IB's own local symbol, as TWS writes it
//!   ES@CME            the continuous front-month future
//!   SPY251219C00650000  an option, in the OCC form the whole app already uses
//! ```
//!
//! A bare symbol is the common case and stays bit-identical to what every other provider
//! stores, so a dataset downloaded from IB and one downloaded from Alpaca line up.

use ibapi::contracts::{Contract, SecurityType};
use ibapi::market_data::historical::{BarSize, Duration as IbDuration, WhatToShow};
use ibapi::market_data::TradingHours;
use otw_connector_model::{ConnectorError, Timeframe};

/// The markets this adapter serves, as OTW spells them. Equity and ETF are one contract to
/// IB (both STK).
pub const ASSET_TYPES: &[&str] = &["equity", "etf", "crypto", "fx", "index", "future", "option"];

/// A ticker taken apart: symbol, optional exchange, optional currency.
struct Parts {
    symbol: String,
    exchange: Option<String>,
    currency: Option<String>,
}

/// Split `SYMBOL[@EXCHANGE][:CURRENCY]`. Neither separator occurs in an IB symbol (class
/// shares are `BRK B`, with a space), so this is unambiguous.
fn split(ticker: &str) -> Result<Parts, ConnectorError> {
    let raw = ticker.trim();
    if raw.is_empty() {
        return Err(ConnectorError::not_found("empty ticker"));
    }
    let (head, currency) = match raw.split_once(':') {
        Some((h, c)) => (h, Some(c.trim().to_uppercase())),
        None => (raw, None),
    };
    let (symbol, exchange) = match head.split_once('@') {
        Some((s, e)) => (s.trim(), Some(e.trim().to_uppercase())),
        None => (head.trim(), None),
    };
    if symbol.is_empty() {
        return Err(ConnectorError::not_found(format!("ticker {ticker:?} has no symbol")));
    }
    if currency.as_ref().is_some_and(|c| c.len() != 3) {
        return Err(ConnectorError::not_found(format!("ticker {ticker:?}: a currency is three letters, e.g. AAPL:USD")));
    }
    Ok(Parts { symbol: symbol.to_uppercase(), exchange, currency })
}

/// Split a cash pair written as `EURUSD`, `EUR.USD` or `EUR/USD`.
fn forex_pair(symbol: &str) -> Result<(String, String), ConnectorError> {
    let s = symbol.replace(['/', '.', '-', '_'], "");
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(ConnectorError::not_found(format!("{symbol:?} is not a currency pair. Write it as EURUSD, EUR.USD or EUR/USD")));
    }
    Ok((s[..3].to_string(), s[3..].to_string()))
}

/// Split `SYMBOL[.YYYYMM|.YYYYMMDD]`. IB's own field takes exactly those two widths, so the
/// ticker carries what IB expects rather than an exchange month code (`Z25` is ambiguous a
/// decade later, `202512` never is).
fn futures_expiry(symbol: &str) -> Result<(String, Option<String>), ConnectorError> {
    let Some((sym, date)) = symbol.split_once('.') else {
        return Ok((symbol.to_string(), None));
    };
    let ok = matches!(date.len(), 6 | 8) && date.chars().all(|c| c.is_ascii_digit());
    if sym.is_empty() || !ok {
        return Err(ConnectorError::not_found(format!(
            "{symbol:?} is not a futures contract. Write the month as ES.202512, or the last trading day as ES.20251219"
        )));
    }
    Ok((sym.to_string(), Some(date.to_string())))
}

/// Whether a bare futures symbol is IB's **local symbol** (`MNQU6`, `ESZ5`): a root, a month
/// code letter, then a one or two digit year. Never guessed at: the contract is looked up,
/// and a root that merely looks like one resolves to nothing and says so.
pub(crate) fn is_local_symbol(symbol: &str) -> bool {
    const MONTH_CODES: &str = "FGHJKMNQUVXZ";
    let b = symbol.as_bytes();
    let digits = b.iter().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 2 || b.len() < digits + 2 {
        return false;
    }
    let month = b.len() - digits - 1;
    b.get(month).is_some_and(|c| MONTH_CODES.contains(*c as char)) && symbol.get(..month).is_some_and(|r| r.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// An OCC option symbol (`SPY251219C00650000`, Polygon's `O:` prefix allowed): underlying,
/// expiry `yyyymmdd`, right, strike.
fn occ(sym: &str) -> Result<(String, String, &'static str, f64), ConnectorError> {
    let bad = || ConnectorError::not_found(format!("not a valid option symbol: {sym}"));
    let s = sym.trim();
    let s = s.strip_prefix("O:").or_else(|| s.strip_prefix("o:")).unwrap_or(s);
    if s.len() < 16 || !s.is_ascii() {
        return Err(bad());
    }
    let (underlying, tail) = s.split_at(s.len() - 15);
    let (yymmdd, rest) = tail.split_at(6);
    let right = match rest.as_bytes().first().map(u8::to_ascii_uppercase) {
        Some(b'C') => "C",
        Some(b'P') => "P",
        _ => return Err(bad()),
    };
    let strike8 = rest.get(1..).ok_or_else(bad)?;
    if underlying.is_empty()
        || !underlying.chars().all(|c| c.is_ascii_alphanumeric())
        || !yymmdd.chars().all(|c| c.is_ascii_digit())
        || strike8.len() != 8
        || !strike8.chars().all(|c| c.is_ascii_digit())
    {
        return Err(bad());
    }
    // Thousandths of the strike, read from their text: no float arithmetic.
    let thousandths = strike8.parse::<i64>().map_err(|_| bad())?;
    let strike = otw_connector_model::Decimal::new(thousandths, 3).to_string().parse::<f64>().map_err(|_| bad())?;
    // OCC writes a two-digit year and means this century, as the rest of the app reads it.
    Ok((underlying.to_ascii_uppercase(), format!("20{yymmdd}"), right, strike))
}

/// Build the IB contract an OTW `(ticker, asset_type)` pair names.
pub fn build(ticker: &str, asset_type: &str) -> Result<Contract, ConnectorError> {
    if asset_type == "option" {
        let (underlying, expiry, right, strike) = occ(ticker)?;
        return Ok(Contract::option(&underlying, &expiry, strike, right));
    }
    let p = split(ticker)?;
    match asset_type {
        // IB draws no line between a share and an ETF: both are STK.
        "equity" | "etf" => Ok(Contract::stock(p.symbol)
            .on_exchange(p.exchange.unwrap_or_else(|| "SMART".into()))
            .in_currency(p.currency.unwrap_or_else(|| "USD".into()))
            .build()),
        "crypto" => Ok(Contract::crypto(p.symbol)
            .on_exchange(p.exchange.unwrap_or_else(|| "PAXOS".into()))
            .in_currency(p.currency.unwrap_or_else(|| "USD".into()))
            .build()),
        "fx" => {
            let (base, quote) = forex_pair(&p.symbol)?;
            let quote = p.currency.unwrap_or(quote);
            Ok(Contract::forex(base, quote).on_exchange(p.exchange.unwrap_or_else(|| "IDEALPRO".into())).build())
        }
        // Index exchanges vary by index and the crate keeps the well-known ones; an explicit
        // `@EXCHANGE` still wins.
        "index" => {
            let mut c = Contract::index(&p.symbol);
            if let Some(ex) = p.exchange {
                c.exchange = ex.into();
            }
            if let Some(cur) = p.currency {
                c.currency = cur.into();
            }
            Ok(c)
        }
        // A futures contract is always **partial** here: exchange, currency and month may all
        // be missing, and none can be defaulted. The gateway is asked which listing it is.
        "future" => {
            let (symbol, expiry) = futures_expiry(&p.symbol)?;
            let local = expiry.is_none() && is_local_symbol(&symbol);
            let dated = local || expiry.is_some();
            let mut c = Contract {
                // No month and no local symbol is the continuous series, a different security
                // type to IB.
                security_type: if dated { SecurityType::Future } else { SecurityType::ContinuousFuture },
                last_trade_date_or_contract_month: expiry.unwrap_or_default(),
                // History of an expired contract is refused unless the request says so, and a
                // backtest asks for expired contracts by nature.
                include_expired: dated,
                // `Contract::default()` fills SMART and USD, the two answers a futures lookup
                // must not assume.
                exchange: "".into(),
                currency: "".into(),
                ..Default::default()
            };
            if local {
                c.local_symbol = symbol;
            } else {
                c.symbol = symbol.into();
            }
            if let Some(ex) = p.exchange {
                c.exchange = ex.into();
            }
            if let Some(cur) = p.currency {
                c.currency = cur.into();
            }
            Ok(c)
        }
        other => Err(ConnectorError::unsupported(format!("Interactive Brokers connector does not serve asset type {other:?}"))),
    }
}

/// Futures are always looked up before use: not SMART routed, the same root on several
/// exchanges in several currencies, maybe a local symbol. Everything else carries a default.
pub fn needs_resolution(c: &Contract) -> bool {
    matches!(c.security_type, SecurityType::Future | SecurityType::ContinuousFuture)
}

/// A contract IB resolved, written back as a ticker this module parses again, so the
/// candidates in an ambiguity error are strings the user can paste into the form.
pub fn label(c: &Contract) -> String {
    let mut out = if c.symbol.0.is_empty() { c.local_symbol.clone() } else { c.symbol.0.clone() };
    let month = &c.last_trade_date_or_contract_month;
    if !month.is_empty() {
        out.push('.');
        out.push_str(month);
    }
    if !c.exchange.0.is_empty() {
        out.push('@');
        out.push_str(&c.exchange.0);
    }
    // Worth spelling out only when it is not the one the parser assumes.
    if !c.currency.0.is_empty() && !c.currency.0.eq_ignore_ascii_case("USD") {
        out.push(':');
        out.push_str(&c.currency.0);
    }
    out
}

/// Which price series to ask for. TWS rejects `TRADES` on a crypto contract (10299) and cash
/// forex has no tape at all; `MIDPOINT` exists for both.
pub fn what_to_show(asset_type: &str) -> WhatToShow {
    match asset_type {
        "crypto" | "fx" => WhatToShow::MidPoint,
        _ => WhatToShow::Trades,
    }
}

/// The same choice in the streaming vocabulary, so live candles continue the stored ones.
pub fn realtime_what_to_show(asset_type: &str) -> ibapi::market_data::realtime::WhatToShow {
    match asset_type {
        "crypto" | "fx" => ibapi::market_data::realtime::WhatToShow::MidPoint,
        _ => ibapi::market_data::realtime::WhatToShow::Trades,
    }
}

/// Which live stream IB serves for an asset type. Tick-by-tick is the real feed; IB serves
/// none for options, and for indices only on CME, so those keep the five-second bar. Forex
/// and crypto take the midpoint tick, the same price basis as their history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveSource {
    /// `reqTickByTickData(Last)`: one event per print, with its size.
    LastTick,
    /// `reqTickByTickData(MidPoint)`: one event per midpoint move, no size.
    MidPointTick,
    /// `reqRealTimeBars`: five-second OHLCV, the only bar IB streams.
    Bars5s,
}

pub fn live_source(asset_type: &str) -> LiveSource {
    match asset_type {
        "crypto" | "fx" => LiveSource::MidPointTick,
        "option" | "index" => LiveSource::Bars5s,
        _ => LiveSource::LastTick,
    }
}

/// Regular hours for anything with a session, everything for the markets that have none. A
/// future's real market is the electronic session.
pub fn trading_hours(asset_type: &str) -> TradingHours {
    match asset_type {
        "crypto" | "fx" | "future" => TradingHours::Extended,
        _ => TradingHours::Regular,
    }
}

/// An OTW timeframe as an IB bar size. A timeframe IB has no bar for is refused, never
/// approximated from a neighbour.
pub fn bar_size(tf: Timeframe) -> Result<BarSize, ConnectorError> {
    Ok(match tf.fixed_secs().unwrap_or(0) {
        60 => BarSize::Min,
        120 => BarSize::Min2,
        180 => BarSize::Min3,
        300 => BarSize::Min5,
        600 => BarSize::Min10,
        900 => BarSize::Min15,
        1200 => BarSize::Min20,
        1800 => BarSize::Min30,
        3600 => BarSize::Hour,
        7200 => BarSize::Hour2,
        10800 => BarSize::Hour3,
        14400 => BarSize::Hour4,
        28800 => BarSize::Hour8,
        86400 => BarSize::Day,
        604800 => BarSize::Week,
        _ => return Err(ConnectorError::unsupported(format!("Interactive Brokers has no {tf} bar. Use 1m, 5m, 15m, 30m, 1h, 4h, 1d or 1w"))),
    })
}

/// The widest window IB accepts in **one** request at this bar size, in seconds: one day of
/// 1-minute bars, a week of 15-minute bars, a year of daily bars.
pub fn max_window_secs(tf: Timeframe) -> i64 {
    const DAY: i64 = 86_400;
    match tf.fixed_secs().unwrap_or(DAY) {
        s if s < 60 => 1_800,
        60 => DAY,
        120 => 2 * DAY,
        s if s <= 1_200 => 7 * DAY,
        s if s <= 28_800 => 30 * DAY,
        s if s <= DAY => 365 * DAY,
        _ => 730 * DAY,
    }
}

/// The fewest bars a request may ask for: IB refuses a window too short for the bar size
/// (error 321), so a top-up of the last candles is widened backwards. The extra bars are
/// ones already held, deduped and upserted.
const MIN_BARS_PER_REQ: i64 = 30;

/// How far back to ask, in the unit IB accepts for that span and bar size. Seconds are
/// refused past 86 400 whatever the bar, and for a bar coarser than half an hour; days are
/// rounded up (an overlapping bar is deduped, asking short would leave a hole), and capped at
/// a year, past which IB counts in years.
pub fn duration_back(secs: i64, bar_secs: i64) -> IbDuration {
    const DAY: i64 = 86_400;
    const SECONDS_MAX_BAR: i64 = 1_800;
    let bar_secs = bar_secs.max(1);
    let secs = secs.max(bar_secs.saturating_mul(MIN_BARS_PER_REQ)).max(60);
    if secs <= DAY && bar_secs <= SECONDS_MAX_BAR {
        return IbDuration::seconds(i32::try_from(secs).unwrap_or(i32::MAX));
    }
    let days = (secs + DAY - 1) / DAY;
    if days <= 365 {
        IbDuration::days(i32::try_from(days).unwrap_or(i32::MAX))
    } else {
        IbDuration::years(i32::try_from((days + 364) / 365).unwrap_or(i32::MAX))
    }
}

/// The asset type an IB security type belongs to, or `None` when it is not served.
pub fn asset_type_of(sec: &SecurityType) -> Option<&'static str> {
    use SecurityType as S;
    match sec {
        // IB reports an ETF as STK too; "equity" is the honest label for both.
        S::Stock => Some("equity"),
        S::Crypto => Some("crypto"),
        S::ForexPair => Some("fx"),
        S::Index => Some("index"),
        S::Future | S::ContinuousFuture => Some("future"),
        S::Option | S::FuturesOption => Some("option"),
        _ => None,
    }
}

/// A search hit written back as a ticker this module parses again. A US instrument keeps its
/// bare symbol; a futures hit is the continuous series with the exchange it cannot do without.
pub fn format_ticker(kind: &str, symbol: &str, currency: &str, exchange: &str) -> String {
    if kind == "future" {
        return if exchange.is_empty() { symbol.to_string() } else { format!("{symbol}@{exchange}") };
    }
    if currency.is_empty() || currency.eq_ignore_ascii_case("USD") {
        symbol.to_string()
    } else {
        format!("{symbol}:{}", currency.to_uppercase())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_arithmetic)]
mod tests {
    use super::*;

    #[test]
    fn ticker_carries_exchange_and_currency() {
        let p = split("AAPL").unwrap();
        assert!(p.symbol == "AAPL" && p.exchange.is_none() && p.currency.is_none());
        let p = split("san:eur").unwrap();
        assert_eq!((p.symbol.as_str(), p.currency.as_deref()), ("SAN", Some("EUR")));
        let p = split("7203@tsej:jpy").unwrap();
        assert_eq!((p.exchange.as_deref(), p.currency.as_deref()), (Some("TSEJ"), Some("JPY")));
        assert!(split("AAPL:EURO").is_err() && split("  ").is_err());
    }

    #[test]
    fn the_live_source_follows_what_ib_actually_serves() {
        for a in ["equity", "etf", "future"] {
            assert_eq!((live_source(a), what_to_show(a)), (LiveSource::LastTick, WhatToShow::Trades), "{a}");
        }
        for a in ["fx", "crypto"] {
            assert_eq!((live_source(a), what_to_show(a)), (LiveSource::MidPointTick, WhatToShow::MidPoint), "{a}");
        }
        for a in ["option", "index"] {
            assert_eq!(live_source(a), LiveSource::Bars5s, "{a}");
        }
    }

    #[test]
    fn forex_accepts_the_three_spellings() {
        for s in ["EURUSD", "EUR.USD", "EUR/USD"] {
            assert_eq!(forex_pair(s).unwrap(), ("EUR".into(), "USD".into()));
        }
        assert!(forex_pair("EUR").is_err() && forex_pair("EURUS1").is_err());
    }

    #[test]
    fn a_future_is_built_partial_and_left_for_the_gateway_to_pin_down() {
        let c = build("ES.202512", "future").unwrap();
        assert!(c.exchange.0.is_empty() && c.currency.0.is_empty() && needs_resolution(&c));
        let c = build("ES.202512@CME", "future").unwrap();
        assert_eq!((c.security_type.clone(), c.last_trade_date_or_contract_month.as_str(), c.exchange.0.as_str(), c.include_expired), (SecurityType::Future, "202512", "CME", true));
        assert_eq!(build("ES.20251219@CME", "future").unwrap().last_trade_date_or_contract_month, "20251219");
        let cont = build("ES@CME", "future").unwrap();
        assert!(cont.security_type == SecurityType::ContinuousFuture && !cont.include_expired);
        assert!(build("ES.2025@CME", "future").is_err());
    }

    #[test]
    fn a_future_can_be_written_the_way_tws_writes_it() {
        let c = build("MNQU6", "future").unwrap();
        assert!(c.local_symbol == "MNQU6" && c.symbol.0.is_empty() && c.security_type == SecurityType::Future);
        assert_eq!(build("MNQ", "future").unwrap().security_type, SecurityType::ContinuousFuture);
        assert!(is_local_symbol("ESZ5") && is_local_symbol("MNQU6"));
        assert!(!is_local_symbol("MNQ") && !is_local_symbol("6E") && !is_local_symbol("ESA5") && !is_local_symbol("Z5"));
    }

    #[test]
    fn only_futures_cost_a_lookup() {
        for (ticker, kind) in [("AAPL", "equity"), ("BTC", "crypto"), ("EURUSD", "fx"), ("SPX", "index")] {
            assert!(!needs_resolution(&build(ticker, kind).unwrap()), "{ticker}");
        }
        assert!(needs_resolution(&build("ES@CME", "future").unwrap()));
    }

    #[test]
    fn a_candidate_is_written_the_way_the_form_accepts_it() {
        let mut c = build("ES.202512@CME", "future").unwrap();
        c.currency = "USD".into();
        assert_eq!(label(&c), "ES.202512@CME");
        c.currency = "EUR".into();
        assert_eq!(label(&c), "ES.202512@CME:EUR");
    }

    #[test]
    fn an_option_is_read_from_the_occ_symbol() {
        let c = build("SPY251219C00650000", "option").unwrap();
        assert_eq!((c.symbol.0.as_str(), c.last_trade_date_or_contract_month.as_str(), c.strike, c.right.as_str()), ("SPY", "20251219", 650.0, "C"));
        assert_eq!(build("O:SPY251219P00400500", "option").unwrap().strike, 400.5);
        assert!(build("NOTANOPTION", "option").is_err());
        assert!(build("XAUUSD", "commodity").is_err());
    }

    #[test]
    fn windows_and_durations_are_ones_ib_accepts() {
        assert_eq!((max_window_secs(Timeframe::M1), max_window_secs(Timeframe::M15), max_window_secs(Timeframe::H1), max_window_secs(Timeframe::D1)), (86_400, 7 * 86_400, 30 * 86_400, 365 * 86_400));
        assert!(bar_size("7m".parse().unwrap()).is_err() && bar_size(Timeframe::D1).is_ok());
        for (secs, bar, want) in [
            (3_600, 60, "3600 S"),
            (86_400, 60, "86400 S"),
            (86_401, 60, "2 D"),
            (30 * 86_400, 60, "30 D"),
            (365 * 86_400, 60, "365 D"),
            (730 * 86_400, 60, "2 Y"),
            (2 * 3_600, 3_600, "2 D"),
            (86_400, 86_400, "30 D"),
            (300, 300, "9000 S"),
            (60, 60, "1800 S"),
            (0, 300, "9000 S"),
            (1, 1, "60 S"),
        ] {
            assert_eq!(duration_back(secs, bar).to_string(), want, "{secs} s of {bar} s bars");
        }
    }
}
