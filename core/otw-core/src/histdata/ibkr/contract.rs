//! Turning OpenTraderWorld coordinates into an IB request.
//!
//! Everywhere else a provider takes a ticker string. IB takes a **contract**: a symbol
//! plus a security type, an exchange and a currency, and it will happily serve a different
//! instrument if any of those is wrong. The rest of the app stores one string, so the
//! string has to be able to carry the other three when the defaults are not right:
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
//! stores, so a dataset downloaded from IB and one downloaded from Alpaca line up on the
//! same ticker.

use anyhow::{anyhow, Result};
use ibapi::contracts::{Contract, SecurityType};
use ibapi::market_data::historical::{BarSize, WhatToShow};
use ibapi::market_data::TradingHours;

use crate::timeframe::Timeframe;

/// The asset types this connector serves.
///
/// Options ride the OCC symbol (`histdata::parse_option_symbol`), the convention Alpaca and
/// Polygon already use here, so nothing new was invented for them. Futures carry their
/// contract month in the ticker, or arrive as IB's own local symbol, and are always looked
/// up against the gateway before use (see `resolve`): a future is not SMART routed and the
/// same root lists on several exchanges in several currencies, so guessing one would
/// quietly download the wrong instrument.
pub const ASSET_TYPES: &[&str] =
    &["equity", "etf", "crypto", "fx", "index", "future", "option"];

/// A ticker taken apart: symbol, optional exchange, optional currency.
struct Parts {
    symbol: String,
    exchange: Option<String>,
    currency: Option<String>,
}

/// Split `SYMBOL[@EXCHANGE][:CURRENCY]`. Neither separator occurs in an IB symbol (class
/// shares are `BRK B`, with a space), so this is unambiguous.
fn split(ticker: &str) -> Result<Parts> {
    let raw = ticker.trim();
    if raw.is_empty() {
        return Err(anyhow!("empty ticker"));
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
        return Err(anyhow!("ticker {ticker:?} has no symbol"));
    }
    if currency.as_ref().is_some_and(|c| c.len() != 3) {
        return Err(anyhow!("ticker {ticker:?}: a currency is three letters, e.g. AAPL:USD"));
    }
    Ok(Parts { symbol: symbol.to_uppercase(), exchange, currency })
}

/// Split a cash pair written as `EURUSD`, `EUR.USD` or `EUR/USD`.
fn forex_pair(symbol: &str) -> Result<(String, String)> {
    let s = symbol.replace(['/', '.', '-', '_'], "");
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(anyhow!(
            "{symbol:?} is not a currency pair. Write it as EURUSD, EUR.USD or EUR/USD"
        ));
    }
    Ok((s[..3].to_string(), s[3..].to_string()))
}

/// Split `SYMBOL[.YYYYMM|.YYYYMMDD]`. IB's own field is `last_trade_date_or_contract_month`
/// and takes exactly those two widths, so the ticker carries what IB expects rather than an
/// exchange month code (`Z25` is ambiguous a decade later, `202512` never is).
fn futures_expiry(symbol: &str) -> Result<(String, Option<String>)> {
    let Some((sym, date)) = symbol.split_once('.') else {
        return Ok((symbol.to_string(), None));
    };
    let ok = matches!(date.len(), 6 | 8) && date.chars().all(|c| c.is_ascii_digit());
    if sym.is_empty() || !ok {
        return Err(anyhow!(
            "{symbol:?} is not a futures contract. Write the month as ES.202512, or the last \
             trading day as ES.20251219"
        ));
    }
    Ok((sym.to_string(), Some(date.to_string())))
}

/// Whether a bare futures symbol is IB's **local symbol** (`MNQU6`, `ESZ5`): a root, a
/// month code letter, then a one or two digit year.
///
/// That is the string TWS puts on screen and therefore the one a user copies, so it is
/// accepted as it is rather than turned back into a month by hand. It is never guessed at:
/// the contract is looked up (`resolve`), and a root that merely looks like one resolves to
/// nothing and says so.
fn is_local_symbol(symbol: &str) -> bool {
    const MONTH_CODES: &str = "FGHJKMNQUVXZ";
    let b = symbol.as_bytes();
    let digits = b.iter().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 2 || b.len() < digits + 2 {
        return false;
    }
    let month = b.len() - digits - 1;
    MONTH_CODES.contains(b[month] as char)
        && symbol[..month].chars().all(|c| c.is_ascii_alphanumeric())
}

/// Build the IB contract an OTW `(ticker, asset_type)` pair names.
pub fn build(ticker: &str, asset_type: &str) -> Result<Contract> {
    // An OCC symbol carries no exchange or currency and may arrive with Polygon's `O:`
    // prefix, so it is parsed whole rather than run through the separator split.
    if asset_type == "option" {
        let occ = crate::histdata::parse_option_symbol(ticker)?;
        let right = if occ.cp == 'C' { "C" } else { "P" };
        let strike: f64 = occ
            .strike8
            .parse::<f64>()
            .map_err(|_| anyhow!("{ticker:?} has an unreadable strike"))?
            / 1000.0;
        // OCC writes a two-digit year and means this century, the same reading the rest of
        // the app takes when it builds the symbol from the download form.
        return Ok(Contract::option(
            &occ.underlying,
            &format!("20{}", occ.yymmdd),
            strike,
            right,
        ));
    }
    let p = split(ticker)?;
    match asset_type {
        // IB draws no line between a share and an ETF: both are STK, and the same
        // SMART-routed request serves them.
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
            Ok(Contract::forex(base, quote)
                .on_exchange(p.exchange.unwrap_or_else(|| "IDEALPRO".into()))
                .build())
        }
        // Index exchanges vary by index and the crate keeps the well-known ones; an
        // explicit `@EXCHANGE` still wins.
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
        // A futures contract is always **partial** here: the exchange, the currency and the
        // month may all be missing, and none of them can be defaulted. `resolve` asks the
        // gateway which listing this is before a single bar is requested.
        "future" => {
            let (symbol, expiry) = futures_expiry(&p.symbol)?;
            let local = expiry.is_none() && is_local_symbol(&symbol);
            let dated = local || expiry.is_some();
            let mut c = Contract {
                // No month and no local symbol means the continuous series, which is a
                // different security type to IB, not the same one with a blank field.
                security_type: if dated {
                    SecurityType::Future
                } else {
                    SecurityType::ContinuousFuture
                },
                last_trade_date_or_contract_month: expiry.unwrap_or_default(),
                // History of a contract that has already expired is refused unless the
                // request says so, and a backtest asks for expired contracts by nature.
                include_expired: dated,
                // `Contract::default()` is not neutral: it fills SMART and USD, which are
                // the two answers a futures lookup must not assume. SMART matches no
                // futures listing at all, and USD would hide the EUR one.
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
        other => Err(anyhow!(
            "Interactive Brokers connector does not serve asset type {other:?}"
        )),
    }
}

/// Whether this contract has to be looked up against the gateway before it is used.
///
/// Futures, always. IB does not SMART route them, the same root lists on several exchanges
/// in several currencies, and the month may have arrived as a local symbol, so every field
/// that would otherwise be defaulted is instead asked for. Everything else carries a
/// meaningful default (SMART for a share, IDEALPRO for a cash pair) and costs no request.
pub fn needs_resolution(c: &Contract) -> bool {
    matches!(c.security_type, SecurityType::Future | SecurityType::ContinuousFuture)
}

/// Render a contract IB resolved back into a ticker this module can parse again, so the
/// candidates in an ambiguity error are strings the user can paste into the form.
pub fn label(c: &Contract) -> String {
    let root = if c.symbol.0.is_empty() { c.local_symbol.clone() } else { c.symbol.0.clone() };
    let mut out = root;
    let month = &c.last_trade_date_or_contract_month;
    if !month.is_empty() {
        out.push('.');
        out.push_str(month);
    }
    if !c.exchange.0.is_empty() {
        out.push('@');
        out.push_str(&c.exchange.0);
    }
    // The currency is only worth spelling out when it is not the one the parser assumes.
    if !c.currency.0.is_empty() && !c.currency.0.eq_ignore_ascii_case("USD") {
        out.push(':');
        out.push_str(&c.currency.0);
    }
    out
}

/// Which price series to ask for.
///
/// Not cosmetic: TWS rejects `TRADES` on a crypto contract with error 10299, and cash forex
/// has no trade tape at all. `MIDPOINT` exists for both, and is what crypto uses here
/// because the `AGGTRADES` series only appears in the 3.x protocol level this connector
/// deliberately does not require (see the `ibapi` pin in Cargo.toml).
pub fn what_to_show(asset_type: &str) -> WhatToShow {
    match asset_type {
        "crypto" | "fx" => WhatToShow::MidPoint,
        _ => WhatToShow::Trades,
    }
}

/// Which live stream IB serves for an asset type.
///
/// **Tick-by-tick is the real feed.** `reqTickByTickData` publishes each print as it happens,
/// where `reqRealTimeBars` publishes one five-second bar and nothing in between: on a chart
/// that is the difference between a price that moves and a price that steps.
///
/// IB does not serve it everywhere, and says where: tick-by-tick for options is historical
/// only, and for indices it exists on CME contracts alone. Those two keep the five-second
/// bar rather than going quiet on half the contracts a user can type.
///
/// The tick type follows the same price basis as the history ([`what_to_show`]), or the live
/// candles would step onto a different basis at the seam: cash forex and IB crypto have no
/// usable tape, so they take the midpoint tick rather than the last print.
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

/// Regular hours for anything with a session, everything for the markets that have none.
/// Filtering a 24/7 instrument by "regular hours" would drop most of its bars.
pub fn trading_hours(asset_type: &str) -> TradingHours {
    match asset_type {
        // A future's real market is the electronic session, not the old pit hours that
        // "regular" still means for most contracts.
        "crypto" | "fx" | "future" => TradingHours::Extended,
        _ => TradingHours::Regular,
    }
}

/// Map an OTW timeframe onto an IB bar size. IB serves a fixed vocabulary, so a timeframe
/// it has no bar for is refused rather than approximated from a neighbouring one.
pub fn bar_size(timeframe: &str) -> Result<BarSize> {
    let tf = Timeframe::parse(timeframe)?;
    let secs = tf.secs().unwrap_or(0);
    Ok(match secs {
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
        _ => {
            return Err(anyhow!(
                "Interactive Brokers has no {timeframe} bar. Use 1m, 5m, 15m, 30m, 1h, 4h, 1d or 1w"
            ))
        }
    })
}

/// The widest window IB accepts in **one** request at this bar size, in seconds.
///
/// IB caps a historical request by duration, and the cap tightens as the bar gets finer:
/// one day of 1-minute bars, one week of 15-minute bars, one year of daily bars. The
/// download worker sizes its chunks in bars, not in IB's units, so `fetch_chunk` slices
/// its own window against this table instead of letting the request be rejected whole.
pub fn max_window_secs(timeframe: &str) -> Result<i64> {
    let secs = Timeframe::parse(timeframe)?.secs().unwrap_or(86400);
    const DAY: i64 = 86_400;
    Ok(match secs {
        s if s < 60 => 1_800,
        60 => DAY,
        120 => 2 * DAY,
        s if s <= 1_200 => 7 * DAY,
        s if s <= 28_800 => 30 * DAY,
        s if s <= DAY => 365 * DAY,
        _ => 730 * DAY,
    })
}

/// The asset type an IB security type belongs to, or `None` when we do not serve it.
pub fn asset_type_of(sec: &ibapi::contracts::SecurityType) -> Option<&'static str> {
    use ibapi::contracts::SecurityType as S;
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

/// Render a hit back into a ticker this module can parse again. A US instrument keeps its
/// bare symbol so it matches what the other providers store.
///
/// A symbol search returns underlyings, never a dated contract, so a futures hit comes back
/// as the continuous series: symbol plus the exchange it cannot do without.
pub fn format_ticker(kind: &str, symbol: &str, currency: &str, exchange: &str) -> String {
    if kind == "future" {
        return if exchange.is_empty() {
            symbol.to_string()
        } else {
            format!("{symbol}@{exchange}")
        };
    }
    if currency.is_empty() || currency.eq_ignore_ascii_case("USD") {
        symbol.to_string()
    } else {
        format!("{symbol}:{}", currency.to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticker_carries_exchange_and_currency() {
        let p = split("AAPL").unwrap();
        assert_eq!(p.symbol, "AAPL");
        assert!(p.exchange.is_none() && p.currency.is_none());

        let p = split("san:eur").unwrap();
        assert_eq!((p.symbol.as_str(), p.currency.as_deref()), ("SAN", Some("EUR")));

        let p = split("7203@tsej:jpy").unwrap();
        assert_eq!(p.exchange.as_deref(), Some("TSEJ"));
        assert_eq!(p.currency.as_deref(), Some("JPY"));

        assert!(split("AAPL:EURO").is_err());
        assert!(split("  ").is_err());
    }

    /// Which live stream each market gets, and why. Wrong here is either a pane that never
    /// speaks (asking IB for ticks it does not serve) or a live candle on a different price
    /// basis from the stored one it continues.
    #[test]
    fn the_live_source_follows_what_ib_actually_serves() {
        // A tape to read: the print itself.
        for a in ["equity", "etf", "future"] {
            assert_eq!(live_source(a), LiveSource::LastTick, "{a}");
            assert_eq!(what_to_show(a), WhatToShow::Trades, "{a}");
        }
        // No usable tape at IB, so history and stream are both the midpoint.
        for a in ["fx", "crypto"] {
            assert_eq!(live_source(a), LiveSource::MidPointTick, "{a}");
            assert_eq!(what_to_show(a), WhatToShow::MidPoint, "{a}");
        }
        // IB serves no real-time tick-by-tick for options, and for indices only on CME:
        // both keep the five-second bar rather than going quiet.
        for a in ["option", "index"] {
            assert_eq!(live_source(a), LiveSource::Bars5s, "{a}");
        }
    }

    #[test]
    fn forex_accepts_the_three_spellings() {
        for s in ["EURUSD", "EUR.USD", "EUR/USD"] {
            assert_eq!(forex_pair(s).unwrap(), ("EUR".into(), "USD".into()));
        }
        assert!(forex_pair("EUR").is_err());
        assert!(forex_pair("EURUS1").is_err());
    }

    #[test]
    fn a_future_is_built_partial_and_left_for_the_gateway_to_pin_down() {
        // No exchange is no longer an error: it is the question `resolve` asks IB, rather
        // than a listing picked here at random.
        let c = build("ES.202512", "future").unwrap();
        assert!(c.exchange.0.is_empty() && c.currency.0.is_empty());
        assert!(needs_resolution(&c));

        let c = build("ES.202512@CME", "future").unwrap();
        assert_eq!(c.security_type, SecurityType::Future);
        assert_eq!(c.last_trade_date_or_contract_month, "202512");
        assert_eq!(c.exchange.0, "CME");
        // History of a contract past its last trading day is refused unless asked for,
        // and a backtest asks for expired contracts by nature.
        assert!(c.include_expired);

        // A last trading day is as good as a month; no month at all is the continuous
        // series, which IB treats as its own security type.
        assert_eq!(
            build("ES.20251219@CME", "future").unwrap().last_trade_date_or_contract_month,
            "20251219"
        );
        let cont = build("ES@CME", "future").unwrap();
        assert_eq!(cont.security_type, SecurityType::ContinuousFuture);
        assert!(!cont.include_expired);
        assert!(build("ES.2025@CME", "future").is_err());
    }

    #[test]
    fn a_future_can_be_written_the_way_tws_writes_it() {
        // IB's own local symbol: a root, a month code, a year. It travels as `local_symbol`
        // and the gateway hands back the qualified contract.
        let c = build("MNQU6", "future").unwrap();
        assert_eq!(c.local_symbol, "MNQU6");
        assert!(c.symbol.0.is_empty());
        assert_eq!(c.security_type, SecurityType::Future);
        assert!(needs_resolution(&c));

        // A root that is not one stays a root, and stays the continuous series.
        assert_eq!(
            build("MNQ", "future").unwrap().security_type,
            SecurityType::ContinuousFuture
        );
        assert!(is_local_symbol("ESZ5") && is_local_symbol("MNQU6"));
        // No trailing year, an unknown month code, or nothing before the month: not one.
        assert!(!is_local_symbol("MNQ"));
        assert!(!is_local_symbol("6E"));
        assert!(!is_local_symbol("ESA5"));
        assert!(!is_local_symbol("Z5"));
    }

    #[test]
    fn only_futures_cost_a_lookup() {
        // Everything else carries a default worth having, so it never spends a request.
        for (ticker, kind) in
            [("AAPL", "equity"), ("BTC", "crypto"), ("EURUSD", "fx"), ("SPX", "index")]
        {
            assert!(!needs_resolution(&build(ticker, kind).unwrap()), "{ticker}");
        }
        assert!(needs_resolution(&build("ES@CME", "future").unwrap()));
    }

    #[test]
    fn a_candidate_is_written_the_way_the_form_accepts_it() {
        let mut c = build("ES.202512@CME", "future").unwrap();
        c.currency = "USD".into();
        // The parser already assumes USD, so spelling it out would only add noise.
        assert_eq!(label(&c), "ES.202512@CME");
        c.currency = "EUR".into();
        assert_eq!(label(&c), "ES.202512@CME:EUR");
    }

    #[test]
    fn an_option_is_read_from_the_occ_symbol_the_app_already_uses() {
        let c = build("SPY251219C00650000", "option").unwrap();
        assert_eq!(c.symbol.0, "SPY");
        assert_eq!(c.last_trade_date_or_contract_month, "20251219");
        assert_eq!(c.strike, 650.0);
        assert_eq!(c.right, "C");
        // Polygon's wire prefix travels on some stored tickers and must not be mistaken
        // for the currency separator.
        assert_eq!(build("O:SPY251219P00400500", "option").unwrap().strike, 400.5);
        assert!(build("NOTANOPTION", "option").is_err());
    }

    #[test]
    fn unsupported_asset_types_are_refused_not_guessed() {
        assert!(build("XAUUSD", "commodity").is_err());
        assert!(build("AAPL", "equity").is_ok());
    }

    #[test]
    fn windows_tighten_as_bars_get_finer() {
        assert_eq!(max_window_secs("1m").unwrap(), 86_400);
        assert_eq!(max_window_secs("15m").unwrap(), 7 * 86_400);
        assert_eq!(max_window_secs("1h").unwrap(), 30 * 86_400);
        assert_eq!(max_window_secs("1d").unwrap(), 365 * 86_400);
        // A timeframe with no IB bar is refused rather than rounded to a neighbour.
        assert!(bar_size("7m").is_err());
        assert!(bar_size("1d").is_ok());
    }
}
