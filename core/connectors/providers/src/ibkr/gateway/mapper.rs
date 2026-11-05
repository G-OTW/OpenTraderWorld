//! What the gateway answers, in canonical terms. Pure: `ibapi` values in, canonical values
//! out, no socket and no clock.
//!
//! `ibapi` has already read the TWS text into floats; a price crosses back to `Decimal`
//! through its shortest round-trip text, so the decimal is the one the gateway wrote and the
//! float OTW stores from it is bit-identical to the one `ibapi` read.

use std::str::FromStr;

use ibapi::contracts::ContractDescription;
use otw_connector_model::{AssetClass, CanonicalSymbol, Candle, ConnectorError, Decimal, ErrorKind, InstrumentId, InstrumentSpec, Price, Qty, Timeframe, Timestamp};

use super::contract;

/// `f64` to `Decimal` through its shortest text. A value no decimal can hold (NaN, infinity,
/// beyond 28 digits) is a decode error, never a silent zero.
pub fn dec(x: f64) -> Result<Decimal, ConnectorError> {
    Decimal::from_str(&x.to_string())
        .or_else(|_| Decimal::try_from(x))
        .map_err(|_| ConnectorError::decode(format!("ibkr: {x} is not a number a decimal holds")))
}

pub fn ts(t: time::OffsetDateTime) -> Result<Timestamp, ConnectorError> {
    Timestamp::from_offset_datetime(t).ok_or_else(|| ConnectorError::decode(format!("ibkr: time {t} out of range")))
}

/// One IB bar. The stamp is kept as IB sent it (a daily bar is midnight of its trading date,
/// an intraday bar its own instant): normalizing is the alignment layer's job on read. IB
/// writes -1 for a series with no volume (cash forex, midpoint bars): that is zero volume.
/// IB flags no bar as still forming: the period's end decides (`final_flag = false`).
pub fn candle(instrument: &InstrumentId, tf: Timeframe, b: &ibapi::market_data::historical::Bar) -> Result<Candle, ConnectorError> {
    Ok(Candle {
        instrument: instrument.clone(),
        tf,
        open_ts: ts(b.date)?,
        open: Price(dec(b.open)?),
        high: Price(dec(b.high)?),
        low: Price(dec(b.low)?),
        close: Price(dec(b.close)?),
        volume: Qty(dec(b.volume)?.max(Decimal::ZERO)),
        quote_volume: None,
        trades: u64::try_from(b.count).ok().filter(|c| *c > 0),
        is_final: false,
        adjusted: None,
        sides: None,
    })
}

/// The venue id each market rides on (the market cannot be read back from a ticker).
pub const VENUES: &[(&str, &str)] = &[
    ("equity", "IBKR"),
    ("crypto", "IBKR_CRYPTO"),
    ("fx", "IBKR_FX"),
    ("index", "IBKR_INDEX"),
    ("future", "IBKR_FUTURE"),
    ("option", "IBKR_OPTION"),
];

/// The OTW asset type an instrument id names.
pub fn asset_type(id: &InstrumentId) -> &'static str {
    VENUES.iter().find(|(_, v)| *v == id.venue.0).map(|(a, _)| *a).unwrap_or("equity")
}

pub fn venue_of(asset_type: &str) -> &'static str {
    let a = if asset_type == "etf" { "equity" } else { asset_type };
    VENUES.iter().find(|(k, _)| *k == a).map(|(_, v)| *v).unwrap_or("IBKR")
}

pub fn class_of(asset_type: &str) -> AssetClass {
    match asset_type {
        "crypto" => AssetClass::Spot,
        "fx" => AssetClass::Forex,
        "index" => AssetClass::Index,
        "future" => AssetClass::Future,
        "option" => AssetClass::Option,
        _ => AssetClass::Equity,
    }
}

/// A search line: the ticker written back the way this adapter parses it, IB's own local
/// symbol as the name, the primary exchange when IB names one.
pub fn search_hit(kind: &'static str, c: &ibapi::contracts::Contract) -> InstrumentSpec {
    let exchange = if c.primary_exchange.0.is_empty() { c.exchange.0.clone() } else { c.primary_exchange.0.clone() };
    let ticker = contract::format_ticker(kind, &c.symbol.0, &c.currency.0, &exchange);
    spec(kind, ticker, c.local_symbol.clone(), exchange, &c.currency.0)
}

/// A dated futures contract from the contract database, written the way it is typed back
/// (`MNQ.20261218@CME`), never as a bare root that would leave the exchange to a guess.
pub fn future_hit(c: &ibapi::contracts::Contract) -> InstrumentSpec {
    spec("future", contract::label(c), c.local_symbol.clone(), c.exchange.0.clone(), &c.currency.0)
}

fn spec(kind: &'static str, ticker: String, name: String, exchange: String, currency: &str) -> InstrumentSpec {
    let mut s = InstrumentSpec::spot(InstrumentId::new(venue_of(kind), &ticker), &ticker, currency);
    s.symbol = CanonicalSymbol(name);
    s.base = None;
    s.asset_class = class_of(kind);
    s.exchange = Some(exchange);
    s
}

/// The matching symbols IB answered, in its order, the kinds it serves only.
pub fn search_hits(found: &[ContractDescription]) -> Vec<(&'static str, InstrumentSpec)> {
    found.iter().filter_map(|d| Some((contract::asset_type_of(&d.contract.security_type)?, search_hit(contract::asset_type_of(&d.contract.security_type)?, &d.contract)))).collect()
}

/// The sentence code 200 is turned into, shared with [`unknown_contract`] so the two cannot
/// drift apart.
pub const UNKNOWN_CONTRACT: &str = "has no contract matching this symbol";

/// Whether a refusal means "IB knows no such contract" (error 200). A symbol search types a
/// character at a time, so most of its queries name nothing yet: an empty list, not a warning.
pub fn unknown_contract(e: &ConnectorError) -> bool {
    e.message.contains(UNKNOWN_CONTRACT)
}

/// A request refusal, named by IB's code: these are account settings rather than bugs.
pub fn request_error(what: &str, e: &ibapi::Error) -> ConnectorError {
    let ibapi::Error::Message(code, message) = e else {
        let kind = if super::session::dropped(e) { ErrorKind::Transient } else { ErrorKind::Unknown };
        return ConnectorError::new(kind, format!("{what}: {e}"));
    };
    let (kind, text) = match *code {
        162 => (
            ErrorKind::Unknown,
            format!("{what}: Interactive Brokers refused the request ({message}). Code 162 covers both a pacing violation and a symbol with no data in the window asked for."),
        ),
        200 => (
            ErrorKind::NotFound,
            format!("{what}: Interactive Brokers {UNKNOWN_CONTRACT} ({message}). Check the spelling, and add the currency for a non-US instrument, e.g. SAN:EUR."),
        ),
        354 | 10089 | 10090 => (
            ErrorKind::Permission,
            format!("{what}: this Interactive Brokers account has no market-data subscription for the instrument ({message}). Historical data needs the same subscription as live data."),
        ),
        10197 | 10167 => (ErrorKind::Permission, format!("{what}: no data permission or no data available ({message})")),
        other => (ErrorKind::Unknown, format!("{what}: Interactive Brokers error {other}: {message}")),
    };
    ConnectorError::new(kind, text).with_code(code.to_string())
}

/// The code a stream error carries when the socket itself went: the whole session is
/// affected, not one instrument.
pub const DROPPED: &str = "dropped";

/// A live refusal. IB answers with a numbered message and the number is the diagnosis: 354
/// is a subscription to buy, 10197 another IB session to close, 200 a ticker to fix.
/// Tick-by-tick refusals are read on their words: IB documents the limit but not its code.
/// Kinds: a ceiling or a seat held elsewhere is `RateLimited` (it clears), a missing
/// subscription `Permission`, a bad ticker `NotFound`, no tick stream `Unsupported`.
pub fn live_error(addr: &str, symbol: &str, connector: &str, e: &ibapi::Error) -> ConnectorError {
    let ibapi::Error::Message(code, message) = e else {
        let mut err = ConnectorError::new(ErrorKind::Transient, format!("{symbol} live: Interactive Brokers ({addr}): {e}"));
        if super::session::dropped(e) {
            err = err.with_code(DROPPED);
        }
        return err;
    };
    let low = message.to_ascii_lowercase();
    if low.contains("tick-by-tick") || low.contains("tick by tick") {
        let hit_ceiling = ["max", "maximum", "reached", "limit", "exceed"].iter().any(|w| low.contains(w));
        return if hit_ceiling {
            ConnectorError::new(
                ErrorKind::RateLimited,
                format!(
                    "Interactive Brokers has no live tick stream left for {symbol} ({message}). An account may hold only as many at once as it has market-depth lines, three on a plain account: stop live on another Interactive Brokers chart, or ask IB for more lines"
                ),
            )
        } else {
            ConnectorError::new(
                ErrorKind::Unsupported,
                format!(
                    "Interactive Brokers serves no tick-by-tick stream for {symbol} ({message}). Options and indices outside CME are the usual case; chart them from another provider, or download instead of streaming"
                ),
            )
        };
    }
    let (kind, text) = match *code {
        101 => (
            ErrorKind::RateLimited,
            format!("connector {connector}'s Interactive Brokers account is holding all the market-data lines it has ({message}). Stop live on another chart and this one starts"),
        ),
        200 => (
            ErrorKind::NotFound,
            format!("Interactive Brokers has no contract matching {symbol} ({message}). Check the spelling, and add the currency for a non-US instrument, e.g. SAN:EUR"),
        ),
        354 | 10089 | 10090 | 10091 => (
            ErrorKind::Permission,
            format!(
                "connector {connector}'s Interactive Brokers account has no live market-data subscription for {symbol} ({message}). Subscriptions are bought in Client Portal under Settings → Market Data Subscriptions; the delayed data IB offers instead is not a live feed"
            ),
        ),
        10197 => (
            ErrorKind::RateLimited,
            format!(
                "Interactive Brokers gives one live market-data session per account and another one holds it. Close the TWS, mobile or web session using this account and this reconnects on its own ({message})"
            ),
        ),
        322 | 420 => (
            ErrorKind::RateLimited,
            format!("Interactive Brokers is refusing further market-data requests on {addr} right now ({message}). Close a chart or wait for the current ones to expire"),
        ),
        _ => (ErrorKind::Transient, format!("{symbol} live: Interactive Brokers {code}: {message}")),
    };
    ConnectorError::new(kind, text).with_code(code.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn live(code: i32, text: &str) -> ConnectorError {
        live_error("127.0.0.1:4002", "AAPL", "ib", &ibapi::Error::Message(code, text.into()))
    }

    #[test]
    fn prices_cross_as_the_text_ibapi_read() {
        for x in [17000.25_f64, 0.1, 1.0e-7, 4500.0, 123456.789] {
            assert_eq!(dec(x).unwrap().to_string().parse::<f64>().unwrap().to_bits(), x.to_bits(), "{x}");
        }
        assert_eq!(dec(0.1).unwrap().to_string(), "0.1");
        assert!(dec(f64::NAN).is_err());
    }

    #[test]
    fn a_bar_keeps_its_stamp_and_never_a_negative_volume() {
        let bar = ibapi::market_data::historical::Bar { date: time::macros::datetime!(2024-03-15 0:00 UTC), open: 1.0, high: 2.0, low: 0.5, close: 1.5, volume: -1.0, wap: 0.0, count: 0 };
        let c = candle(&InstrumentId::new("IBKR_FX", "EURUSD"), Timeframe::D1, &bar).unwrap();
        assert_eq!((c.open_ts, c.volume.0, c.trades, c.is_final), (ts(time::macros::datetime!(2024-03-15 0:00 UTC)).unwrap(), Decimal::ZERO, None, false));
    }

    #[test]
    fn markets_ride_the_venue_id() {
        for a in ["equity", "crypto", "fx", "index", "future", "option"] {
            assert_eq!(asset_type(&InstrumentId::new(venue_of(a), "X")), a);
        }
        assert_eq!(venue_of("etf"), "IBKR");
    }

    #[test]
    fn refusals_carry_their_kind() {
        let r = |code| request_error("x", &ibapi::Error::Message(code, "m".into()));
        assert_eq!((r(200).kind, r(354).kind, r(162).kind, r(9999).kind), (ErrorKind::NotFound, ErrorKind::Permission, ErrorKind::Unknown, ErrorKind::Unknown));
        assert!(unknown_contract(&r(200)));
        assert_eq!(live(10190, "Max number (3) of tick-by-tick requests has been reached.").kind, ErrorKind::RateLimited);
        assert_eq!(live(10189, "maximum tick by tick subscriptions reached").kind, ErrorKind::RateLimited);
        let e = live(10189, "Failed to request tick-by-tick data. Not supported for this instrument.");
        assert!(e.kind == ErrorKind::Unsupported && e.message.contains("no tick-by-tick stream"));
        let e = live(101, "Max number of tickers has been reached.");
        assert!(e.kind == ErrorKind::RateLimited && e.message.contains("market-data lines"));
        assert_eq!((live(354, "x").kind, live(200, "x").kind, live(10197, "x").kind, live(1, "x").kind), (ErrorKind::Permission, ErrorKind::NotFound, ErrorKind::RateLimited, ErrorKind::Transient));
        let gone = live_error("a", "s", "c", &ibapi::Error::ConnectionReset);
        assert_eq!((gone.kind, gone.provider_code.as_deref()), (ErrorKind::Transient, Some(DROPPED)));
    }
}
