//! Listings behind one product: the dated contracts of a future and the option chain of an
//! underlying, plus the per-contract prices the quant derivatives tools need.
//!
//! Only the two connectors that list them are served: Interactive Brokers (through the
//! gateway session, paced like every other IB request) and Massive (REST, where a free key
//! allows five requests a minute, so a 429 is waited out rather than failed on).
//!
//! Bars still come through [`super::Connector::fetch_chunk`], so a contract priced here is
//! priced exactly as a download of it would be.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use ibapi::contracts::{Contract, SecurityType};
use ibapi::market_data::historical::{BarSize, Duration as IbDuration, WhatToShow};
use ibapi::market_data::TradingHours;
use serde_json::Value;
use time::{Date, OffsetDateTime};

use super::ibkr::{contract, resolve, session};

/// The providers this module can list contracts for.
pub const PROVIDERS: &[&str] = &["ibkr", "massive"];

/// Progress hook: requests done, requests planned, what is being fetched.
pub type Progress<'a> = &'a (dyn Fn(usize, usize, &str) + Send + Sync);

fn check(provider: &str) -> Result<()> {
    if PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(anyhow!(
            "{provider} does not list futures contracts or option chains. Use an Interactive \
             Brokers or a Massive connector"
        ))
    }
}

/// The trading date of a daily bar stamped at the midnight of its date (IB, Massive stocks and
/// options), in whatever offset the provider wrote it: half a day forward lands on that date.
pub fn trading_date(ts: OffsetDateTime) -> Date {
    (ts + time::Duration::hours(12)).date()
}

fn limited(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<reqwest::Error>()
            .and_then(|r| r.status())
            .is_some_and(|s| s.as_u16() == 429)
    }) || format!("{e:#}").to_ascii_lowercase().contains("exceeded the maximum requests")
}

/// Run a provider request, waiting out rate-limit refusals. Massive's free key allows five
/// requests a minute and says so with a 429; a paid key never sees one and never waits.
async fn patient<T, F, Fut>(provider: &str, run: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    const TRIES: usize = 12;
    for attempt in 0..TRIES {
        match run().await {
            Ok(v) => return Ok(v),
            Err(e) if limited(&e) && attempt + 1 < TRIES => {
                let wait = crate::rate::retry_after(provider).unwrap_or(Duration::from_secs(13));
                tokio::time::sleep(wait.max(Duration::from_secs(2))).await;
            }
            Err(e) => return Err(e),
        }
    }
    Err(anyhow!("{provider} kept refusing requests over its rate limit"))
}

/// Drop the session still trading: its close is the last print so far, and a curve or a
/// realized volatility mixing it with finished sessions reads a move that has not happened.
fn complete(mut v: Vec<(Date, f64)>) -> Vec<(Date, f64)> {
    let today = OffsetDateTime::now_utc().date();
    v.retain(|p| p.0 < today);
    v
}

/// Daily closes of one instrument between two instants, keyed by trading date, finished
/// sessions only.
pub async fn daily_closes(
    provider: &str,
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    ticker: &str,
    asset_type: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Vec<(Date, f64)>> {
    if provider == "massive" && asset_type == "future" {
        return massive_settlements(http, secrets, ticker, from.date(), to.date()).await;
    }
    let connector = super::connector_for(provider)?;
    let step = time::Duration::days(connector.capability().max_bars_per_req.max(1) as i64);
    let mut out: Vec<(Date, f64)> = Vec::new();
    let mut cursor = from;
    while cursor < to {
        let end = (cursor + step).min(to);
        let chunk = patient(provider, || connector.fetch_chunk(http, secrets, ticker, asset_type, "1d", cursor, end))
            .await?;
        // IB widens a request to whole days or years, so a chunk can reach past its window.
        out.extend(
            chunk
                .bars
                .iter()
                .filter(|b| b.close > 0.0 && b.ts >= cursor && b.ts < end)
                .map(|b| (trading_date(b.ts), b.close)),
        );
        cursor = end;
    }
    out.sort_by_key(|p| p.0);
    out.dedup_by_key(|p| p.0);
    Ok(complete(out))
}

/// Daily settlements of one Massive futures contract, by the date each session settles on.
///
/// A curve wants settlements: every contract of the product gets one at the same moment,
/// where a last trade on a thin back month can be hours old. Massive names each session by
/// its `session_end_date`, so no date is inferred from a stamp (the stamp is the day the
/// session opened, the evening before). A session with no settlement keeps its close.
async fn massive_settlements(
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    ticker: &str,
    from: Date,
    to: Date,
) -> Result<Vec<(Date, f64)>> {
    let key = secrets
        .get("api_key")
        .filter(|k| !k.is_empty())
        .ok_or_else(|| anyhow!("this Massive connector has no API key"))?;
    let sym = ticker.trim().to_ascii_uppercase();
    // The session settling on `from` opened the day before.
    let start = from - time::Duration::days(1);
    let url = format!(
        "https://api.massive.com/futures/v1/aggs/{sym}?resolution=1session&window_start.gte={start}\
         &window_start.lt={to}&sort=window_start.asc&limit=50000&apiKey={key}"
    );
    let body = patient("massive", || super::generic::massive_get(http, &url)).await?;
    let mut out: Vec<(Date, f64)> = body
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|b| {
            let d = b.get("session_end_date").and_then(Value::as_str).and_then(iso_date)?;
            let px = b
                .get("settlement_price")
                .and_then(Value::as_f64)
                .filter(|p| *p > 0.0)
                .or_else(|| b.get("close").and_then(Value::as_f64).filter(|p| *p > 0.0))?;
            Some((d, px))
        })
        .filter(|(d, _)| *d >= from && *d <= to)
        .collect();
    out.sort_by_key(|p| p.0);
    out.dedup_by_key(|p| p.0);
    Ok(complete(out))
}

// ── Futures contracts ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FutContract {
    /// The ticker a download of this contract takes on this connector.
    pub ticker: String,
    /// What the exchange calls it (ESZ5).
    pub local: String,
    pub last_trade: Date,
}

fn ib_date(s: &str) -> Option<Date> {
    let fmt = time::macros::format_description!("[year][month][day]");
    Date::parse(s.get(..8)?, &fmt).ok()
}

fn iso_date(s: &str) -> Option<Date> {
    Date::parse(s.get(..10)?, &time::format_description::well_known::Iso8601::DEFAULT).ok()
}

/// Every contract of a futures product whose last trading day falls in `[from, to]`.
pub async fn futures_contracts(
    provider: &str,
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    root: &str,
    from: Date,
    to: Date,
) -> Result<Vec<FutContract>> {
    check(provider)?;
    let mut found = match provider {
        "ibkr" => ib_futures(secrets, root).await?,
        _ => massive_futures(http, secrets, root, from, to).await?,
    };
    found.retain(|c| c.last_trade >= from && c.last_trade <= to);
    found.sort_by_key(|c| c.last_trade);
    if found.is_empty() {
        return Err(anyhow!("{provider} lists no {root} contract expiring between {from} and {to}"));
    }
    Ok(found)
}

async fn ib_futures(secrets: &HashMap<String, String>, root: &str) -> Result<Vec<FutContract>> {
    let session = session::get(secrets).await?;
    let mut partial = contract::build(root, "future")?;
    if partial.security_type != SecurityType::ContinuousFuture {
        return Err(anyhow!("{root:?} names one contract. Give the product root, e.g. ES or ES@CME"));
    }
    partial.security_type = SecurityType::Future;
    // Expired contracts are only listed when the request asks for them.
    partial.include_expired = true;
    let details = session
        .call(&format!("{root} contracts"), move |client| {
            let partial = partial.clone();
            async move { client.contract_details(&partial).await }
        })
        .await?;
    let mut list: Vec<Contract> = details.into_iter().map(|d| d.contract).collect();
    list.sort_by_key(|c| c.contract_id);
    list.dedup_by_key(|c| c.contract_id);
    // One product only: the same root can list on several exchanges, in several currencies
    // or trading classes, and mixing them would splice two instruments into one curve.
    let mut groups: Vec<(String, String, String, String)> = list
        .iter()
        .map(|c| (c.exchange.0.clone(), c.currency.0.clone(), c.trading_class.clone(), c.multiplier.clone()))
        .collect();
    groups.sort();
    groups.dedup();
    if groups.len() > 1 {
        let named: Vec<String> = groups
            .iter()
            .map(|(ex, cur, tc, m)| format!("{tc}@{ex}:{cur} (×{m})"))
            .collect();
        return Err(anyhow!(
            "{root:?} lists several products at Interactive Brokers: {}. Add the exchange (ES@CME) \
             or the currency (FDAX@EUREX:EUR)",
            named.join(", ")
        ));
    }
    let mut out = Vec::new();
    for mut c in list {
        let Some(last_trade) = ib_date(&c.last_trade_date_or_contract_month) else { continue };
        c.include_expired = true;
        let ticker = contract::label(&c);
        // The download of each contract would look it up again; it is already resolved.
        resolve::remember(&session, &ticker, "future", c.clone()).await;
        out.push(FutContract { ticker, local: c.local_symbol.clone(), last_trade });
    }
    Ok(out)
}

async fn massive_futures(
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    root: &str,
    from: Date,
    to: Date,
) -> Result<Vec<FutContract>> {
    let key = secrets
        .get("api_key")
        .filter(|k| !k.is_empty())
        .ok_or_else(|| anyhow!("this Massive connector has no API key"))?;
    let product = root.split('@').next().unwrap_or(root).trim().to_ascii_uppercase();
    // The listing is point in time: a date returns the contracts that existed on it. Asking
    // every half year across the window catches a contract listed and expired in between.
    let today = OffsetDateTime::now_utc().date();
    let mut dates = Vec::new();
    let mut d = from;
    while d < today.min(to) {
        dates.push(d);
        d += time::Duration::days(180);
    }
    dates.push(today.min(to));
    let mut by_ticker: HashMap<String, FutContract> = HashMap::new();
    for d in dates {
        let url = format!(
            "https://api.massive.com/futures/v1/contracts?product_code={product}&date={d}\
             &type=single&limit=1000&apiKey={key}"
        );
        let body = patient("massive", || super::generic::massive_get(http, &url)).await?;
        for r in body.get("results").and_then(Value::as_array).into_iter().flatten() {
            let (Some(t), Some(lt)) = (
                r.get("ticker").and_then(Value::as_str),
                r.get("last_trade_date").and_then(Value::as_str).and_then(iso_date),
            ) else {
                continue;
            };
            // The listing carries calendar spreads (ESZ5-ESH6) next to the outright months,
            // and the type filter does not keep them out: a spread's price is a difference,
            // and one on the curve would read as a contract trading near zero.
            let combo = r.get("type").and_then(Value::as_str).is_some_and(|k| k != "single");
            if combo || t.contains('-') {
                continue;
            }
            by_ticker.entry(t.to_string()).or_insert(FutContract {
                ticker: t.to_string(),
                local: t.to_string(),
                last_trade: lt,
            });
        }
    }
    if by_ticker.is_empty() {
        return Err(anyhow!("Massive lists no futures contract for product code {product}"));
    }
    Ok(by_ticker.into_values().collect())
}

// ── Option chains ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OptContract {
    /// OCC symbol without a provider prefix (SPY261016P00760000).
    pub ticker: String,
    pub expiry: Date,
    pub strike: f64,
    pub call: bool,
}

/// What an underlying has listed: expiries and, per expiry, the strikes on offer.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub expiries: Vec<Date>,
    /// Strikes per expiry. IB publishes one strike list for the whole class, so every expiry
    /// shares it and a strike missing on one expiry is found out when it is priced.
    pub strikes: HashMap<Date, Vec<f64>>,
    /// IB: the trading class the chain was read from.
    pub trading_class: String,
}

pub fn occ(underlying: &str, expiry: Date, call: bool, strike: f64) -> String {
    format!(
        "{}{:02}{:02}{:02}{}{:08}",
        underlying,
        expiry.year() % 100,
        expiry.month() as u8,
        expiry.day(),
        if call { 'C' } else { 'P' },
        (strike * 1000.0).round() as i64
    )
}

pub async fn option_layout(
    provider: &str,
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    underlying: &str,
    asset_type: &str,
    from: Date,
    to: Date,
    strike_lo: f64,
    strike_hi: f64,
) -> Result<Layout> {
    check(provider)?;
    let mut layout = match provider {
        "ibkr" => ib_layout(secrets, underlying, asset_type).await?,
        _ => massive_layout(http, secrets, underlying, from, to, strike_lo, strike_hi).await?,
    };
    layout.expiries.retain(|e| *e >= from && *e <= to);
    layout.expiries.sort();
    layout.expiries.dedup();
    if layout.expiries.is_empty() {
        return Err(anyhow!("{provider} lists no {underlying} option expiring between {from} and {to}"));
    }
    Ok(layout)
}

async fn ib_layout(secrets: &HashMap<String, String>, underlying: &str, asset_type: &str) -> Result<Layout> {
    let session = session::get(secrets).await?;
    let base = contract::build(underlying, asset_type)?;
    let (symbol, sec_type) = (base.symbol.0.clone(), base.security_type.clone());
    let details = session
        .call(&format!("{underlying} contract"), move |client| {
            let base = base.clone();
            async move { client.contract_details(&base).await }
        })
        .await?;
    let con_id = details
        .first()
        .map(|d| d.contract.contract_id)
        .ok_or_else(|| anyhow!("Interactive Brokers has no contract for {underlying}"))?;
    let (sym, st) = (symbol.clone(), sec_type.clone());
    let rows = session
        .call(&format!("{underlying} option chain"), move |client| {
            let (sym, st) = (sym.clone(), st.clone());
            async move {
                let mut sub = client.option_chain(&sym, "", st, con_id).await?;
                let mut rows = Vec::new();
                while let Some(r) = sub.next().await {
                    rows.push(r?);
                }
                Ok(rows)
            }
        })
        .await?;
    // A chain comes back once per exchange and trading class. The class with the most
    // expiries is the one with the weeklies (SPXW next to SPX); the exchanges agree on it.
    let best = rows
        .iter()
        .filter(|r| !r.expirations.is_empty())
        .max_by_key(|r| (r.expirations.len(), r.trading_class == symbol))
        .ok_or_else(|| anyhow!("Interactive Brokers lists no options on {underlying}"))?;
    let class = best.trading_class.clone();
    let mut strikes: Vec<f64> = rows
        .iter()
        .filter(|r| r.trading_class == class)
        .flat_map(|r| r.strikes.iter().copied())
        .collect();
    strikes.sort_by(f64::total_cmp);
    strikes.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let expiries: Vec<Date> = best.expirations.iter().filter_map(|e| ib_date(e)).collect();
    Ok(Layout {
        strikes: expiries.iter().map(|e| (*e, strikes.clone())).collect(),
        expiries,
        trading_class: class,
    })
}

async fn massive_layout(
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    underlying: &str,
    from: Date,
    to: Date,
    strike_lo: f64,
    strike_hi: f64,
) -> Result<Layout> {
    let key = secrets
        .get("api_key")
        .filter(|k| !k.is_empty())
        .ok_or_else(|| anyhow!("this Massive connector has no API key"))?;
    let u = underlying.trim().to_ascii_uppercase();
    let mut url = format!(
        "https://api.massive.com/v3/reference/options/contracts?underlying_ticker={u}\
         &expiration_date.gte={from}&expiration_date.lte={to}&strike_price.gte={strike_lo}\
         &strike_price.lte={strike_hi}&expired=false&limit=1000&sort=expiration_date&order=asc&apiKey={key}"
    );
    let mut layout = Layout::default();
    // A wide window on a busy underlying runs to several pages; ten is thousands of contracts.
    for _ in 0..10 {
        let body = patient("massive", || super::generic::massive_get(http, &url)).await?;
        for r in body.get("results").and_then(Value::as_array).into_iter().flatten() {
            let (Some(e), Some(k)) = (
                r.get("expiration_date").and_then(Value::as_str).and_then(iso_date),
                r.get("strike_price").and_then(Value::as_f64),
            ) else {
                continue;
            };
            let ks = layout.strikes.entry(e).or_default();
            if !ks.iter().any(|x| (x - k).abs() < 1e-9) {
                ks.push(k);
            }
        }
        match body.get("next_url").and_then(Value::as_str) {
            Some(next) if !next.is_empty() => url = format!("{next}&apiKey={key}"),
            _ => break,
        }
    }
    for ks in layout.strikes.values_mut() {
        ks.sort_by(f64::total_cmp);
    }
    layout.expiries = layout.strikes.keys().copied().collect();
    Ok(layout)
}

/// The strikes IB really lists on one expiry. The chain's strike list is the union over every
/// expiry of the class, and a long-dated expiry lists only some of them.
pub async fn ib_strikes(
    secrets: &HashMap<String, String>,
    underlying: &str,
    trading_class: &str,
    expiry: Date,
) -> Result<Vec<f64>> {
    let session = session::get(secrets).await?;
    let mut c = Contract::option(
        underlying,
        &format!("{:04}{:02}{:02}", expiry.year(), expiry.month() as u8, expiry.day()),
        0.0,
        "C",
    );
    c.trading_class = trading_class.to_string();
    let details = session
        .call(&format!("{underlying} {expiry} strikes"), move |client| {
            let c = c.clone();
            async move { client.contract_details(&c).await }
        })
        .await?;
    let mut ks: Vec<f64> = details.iter().map(|d| d.contract.strike).filter(|k| *k > 0.0).collect();
    ks.sort_by(f64::total_cmp);
    ks.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    Ok(ks)
}

/// Recent prices of an option and when each was taken, empty when the contract does not exist
/// or printed nothing recently.
///
/// IB: hourly midpoints over the last days, since IB serves no daily bar on an option and the
/// bid/ask midpoint is the price a thin contract actually has; the caller picks the hour it
/// has the underlying for. Massive: the previous session's close.
pub async fn option_price(
    provider: &str,
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    o: &OptContract,
    underlying: &str,
    trading_class: &str,
) -> Result<Vec<(f64, OffsetDateTime)>> {
    check(provider)?;
    match provider {
        "ibkr" => {
            let session = session::get(secrets).await?;
            let mut c = Contract::option(
                underlying,
                &format!("{:04}{:02}{:02}", o.expiry.year(), o.expiry.month() as u8, o.expiry.day()),
                o.strike,
                if o.call { "C" } else { "P" },
            );
            c.trading_class = trading_class.to_string();
            let r = session
                .call(&o.ticker, move |client| {
                    let c = c.clone();
                    async move {
                        client
                            .historical_data(&c, None, IbDuration::days(3), BarSize::Hour, Some(WhatToShow::MidPoint), TradingHours::Regular)
                            .await
                    }
                })
                .await;
            match r {
                Ok(h) => Ok(h.bars.iter().filter(|b| b.close > 0.0).map(|b| (b.close, b.date)).collect()),
                // No such strike on this expiry, or nothing quoted in the window.
                Err(e) if session::unknown_contract(&e) || format!("{e:#}").contains("162") => Ok(Vec::new()),
                Err(e) => Err(e),
            }
        }
        _ => {
            let key = secrets
                .get("api_key")
                .filter(|k| !k.is_empty())
                .ok_or_else(|| anyhow!("this Massive connector has no API key"))?;
            // The last days of daily bars rather than `/prev`, whose "previous session" can lag
            // the one the range endpoint (and so the underlying's close) already has.
            let to = OffsetDateTime::now_utc();
            let from = to - time::Duration::days(10);
            let url = format!(
                "https://api.massive.com/v2/aggs/ticker/O:{}/range/1/day/{}/{}?adjusted=false&sort=asc&limit=50&apiKey={key}",
                o.ticker,
                from.unix_timestamp() * 1000,
                to.unix_timestamp() * 1000
            );
            let body = patient("massive", || super::generic::massive_get(http, &url)).await?;
            Ok(body
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|b| {
                    let c = b.get("c").and_then(Value::as_f64).filter(|c| *c > 0.0)?;
                    let t = b.get("t").and_then(Value::as_i64)?;
                    let ts = OffsetDateTime::from_unix_timestamp_nanos(t as i128 * 1_000_000).ok()?;
                    Some((c, ts))
                })
                .collect())
        }
    }
}

/// Hourly closes of the underlying over the last few days, to price it at the same instant
/// as each IB option midpoint.
pub async fn ib_hourly(secrets: &HashMap<String, String>, ticker: &str, asset_type: &str) -> Result<Vec<(OffsetDateTime, f64)>> {
    let session = session::get(secrets).await?;
    let c = resolve::contract_for(&session, ticker, asset_type).await?;
    let what = contract::what_to_show(asset_type);
    let h = session
        .call(&format!("{ticker} hourly"), move |client| {
            let c = c.clone();
            async move { client.historical_data(&c, None, IbDuration::days(5), BarSize::Hour, Some(what), TradingHours::Regular).await }
        })
        .await?;
    Ok(h.bars.iter().filter(|b| b.close > 0.0).map(|b| (b.date, b.close)).collect())
}

/// Interactive Brokers' own daily series on an underlying: its 30-day implied volatility,
/// its historical volatility and its closes, over `years`.
pub struct VolHistory {
    pub iv: Vec<(Date, f64)>,
    pub hv: Vec<(Date, f64)>,
    pub closes: Vec<(Date, f64)>,
}

pub async fn ib_vol_history(
    secrets: &HashMap<String, String>,
    ticker: &str,
    asset_type: &str,
    years: i32,
    progress: Progress<'_>,
) -> Result<VolHistory> {
    let session = session::get(secrets).await?;
    let c = resolve::contract_for(&session, ticker, asset_type).await?;
    let mut series = Vec::new();
    let kinds = [
        (WhatToShow::OptionImpliedVolatility, "implied volatility"),
        (WhatToShow::HistoricalVolatility, "historical volatility"),
        (contract::what_to_show(asset_type), "closes"),
    ];
    for (i, (what, name)) in kinds.into_iter().enumerate() {
        progress(i, 3, name);
        let c = c.clone();
        let h = session
            .call(&format!("{ticker} {name}"), move |client| {
                let c = c.clone();
                async move {
                    client
                        .historical_data(&c, None, IbDuration::years(years), BarSize::Day, Some(what), TradingHours::Regular)
                        .await
                }
            })
            .await
            .with_context(|| format!("{ticker} {name}"))?;
        series.push(
            h.bars
                .iter()
                .filter(|b| b.close.is_finite() && b.close > 0.0)
                .map(|b| (trading_date(b.date), b.close))
                .collect::<Vec<_>>(),
        );
        let last = series.pop().map(complete).unwrap_or_default();
        series.push(last);
    }
    progress(3, 3, "done");
    let closes = series.pop().unwrap_or_default();
    let hv = series.pop().unwrap_or_default();
    let iv = series.pop().unwrap_or_default();
    if iv.is_empty() {
        return Err(anyhow!(
            "Interactive Brokers returned no implied volatility for {ticker}. It is published for \
             shares, ETFs and indices with listed options"
        ));
    }
    Ok(VolHistory { iv, hv, closes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn occ_symbols_match_the_form_the_app_parses() {
        let s = occ("SPY", date!(2026 - 10 - 16), false, 760.0);
        assert_eq!(s, "SPY261016P00760000");
        let p = crate::histdata::parse_option_symbol(&s).unwrap();
        assert_eq!(p.occ(), s);
        assert_eq!(occ("SPX", date!(2026 - 12 - 18), true, 7512.5), "SPX261218C07512500");
    }

    #[test]
    fn a_daily_bar_is_dated_by_its_own_day() {
        // IB and Massive stocks and options stamp the midnight of the date, UTC or local.
        assert_eq!(trading_date(datetime!(2025-08-06 00:00 UTC)), date!(2025 - 08 - 06));
        assert_eq!(trading_date(datetime!(2025-08-06 04:00 UTC)), date!(2025 - 08 - 06));
        assert_eq!(trading_date(datetime!(2025-08-06 00:00 +02:00)), date!(2025 - 08 - 06));
    }
}
