//! Alpaca's option chain: the contract listing, then one options snapshot per expiry.
//!
//! The connector's `feed` setting picks the entitlement, and nothing else differs: `iex` (or
//! empty) reads the free `indicative` feed, `sip` reads `opra`. A chain tested on a free key
//! is therefore the same code a paid key runs. The free feed is not a market quote: trades are
//! 15 minutes late and quotes are modified, which its rows and a notice say.
//!
//! Open interest is only in the listing, which sits on the trading host of the key's account:
//! live or paper, and a data connector does not say which, so a refusal tries the other one.
//! Equity and ETF options only: Alpaca has no index data.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::{Date, OffsetDateTime};

use super::chain::{self, f, ExpiryList, Notice, RawQuote, Snapshot, Sources, Want};
use super::metrics;
use super::MULTIPLIER;
use crate::histdata::chains::{self, Progress};

const DATA: &str = "https://data.alpaca.markets";
const TRADING: &[&str] = &["https://api.alpaca.markets", "https://paper-api.alpaca.markets"];
/// Contracts per listing page (the most Alpaca answers), and pages followed.
const LISTING_PAGE: usize = 10_000;
const LISTING_PAGES: usize = 10;
/// Snapshots per page (the most Alpaca answers), and pages followed per expiry.
const SNAPSHOT_PAGE: usize = 1_000;
const SNAPSHOT_PAGES: usize = 5;
/// How far ahead the expiry listing looks, and its strike window around the spot.
const LISTING_DAYS: i64 = 730;
const LISTING_WINDOW: (f64, f64) = (0.5, 1.5);

/// Which feeds a key reads, from the connector's `feed` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    options: &'static str,
    stocks: &'static str,
    /// What a field read on this plan is tagged with.
    tag: &'static str,
}

const FREE: Plan = Plan { options: "indicative", stocks: "iex", tag: "alpaca:indicative" };
const PAID: Plan = Plan { options: "opra", stocks: "sip", tag: "alpaca" };

fn plan(secrets: &HashMap<String, String>) -> Plan {
    if secrets.get("feed").is_some_and(|f| f.trim().eq_ignore_ascii_case("sip")) {
        PAID
    } else {
        FREE
    }
}

struct Keys<'a> {
    key: &'a str,
    secret: &'a str,
}

fn keys(secrets: &HashMap<String, String>) -> Result<Keys<'_>> {
    let get = |n: &str| secrets.get(n).map(String::as_str).filter(|v| !v.is_empty());
    match (get("api_key"), get("api_secret")) {
        (Some(key), Some(secret)) => Ok(Keys { key, secret }),
        _ => Err(anyhow!("this Alpaca connector has no API key and secret")),
    }
}

fn refuse_index(asset_type: &str, underlying: &str) -> Result<()> {
    if asset_type == "index" {
        return Err(anyhow!(
            "Alpaca has no index options, so {underlying} cannot be read here. Use an Interactive Brokers \
             or a Massive connector"
        ));
    }
    Ok(())
}

#[cfg(not(test))]
fn host(base: &str) -> String {
    base.to_string()
}

#[cfg(test)]
fn host(base: &str) -> String {
    crate::brokers::at(base)
}

/// One Alpaca GET, waiting out its rate limit and saying so in `limited`. The keys ride in
/// headers, so no error here can print them.
async fn get(http: &reqwest::Client, k: &Keys<'_>, url: &str, page: Option<&str>, limited: &AtomicBool) -> Result<Value> {
    chains::patient("alpaca", || async {
        let mut req = http.get(url).header("APCA-API-KEY-ID", k.key).header("APCA-API-SECRET-KEY", k.secret);
        if let Some(token) = page {
            req = req.query(&[("page_token", token)]);
        }
        let r: Result<Value> = async {
            let body = crate::rate::send("alpaca", req).await.map_err(|e| e.without_url()).context("alpaca request")?;
            let body = body.error_for_status().map_err(|e| e.without_url()).context("alpaca status")?;
            body.json().await.context("alpaca decode")
        }
        .await;
        if r.as_ref().is_err_and(chain::over_rate_limit) {
            limited.store(true, Ordering::Relaxed);
        }
        r
    })
    .await
}

/// Alpaca turned the key down: a bad key, or a feed its plan does not have.
fn refused(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<reqwest::Error>()
            .and_then(|r| r.status())
            .is_some_and(|s| matches!(s.as_u16(), 401 | 403))
    })
}

fn missing_plan(p: Plan) -> Notice {
    let message = if p == PAID {
        "Alpaca refused the OPRA feed: this key has no paid market-data plan. Add one to the key, or \
         leave the connector's Market data feed empty to read the free indicative feed"
    } else {
        "Alpaca refused this key on the free feed. Check the API key and secret of the connector"
    };
    Notice { code: "missing_plan", message: message.into() }
}

fn indicative() -> Notice {
    Notice {
        code: "delayed",
        message: "Alpaca's free indicative feed: trades are 15 minutes late and bid and ask are modified \
                  quotes, not OPRA ones. Set the connector's Market data feed to sip on a paid plan for live data"
            .into(),
    }
}

fn next_page(body: &Value) -> Option<String> {
    body.get("next_page_token").and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string)
}

/// A figure Alpaca may write as a number or as a string (the listing writes strings).
fn num(v: &Value, key: &str) -> Option<f64> {
    let x = v.get(key)?;
    x.as_f64().or_else(|| x.as_str()?.parse().ok()).filter(|x: &f64| x.is_finite())
}

// ── Listing ──────────────────────────────────────────────────────────────────────────────

/// One listed contract.
#[derive(Debug, Clone, PartialEq)]
struct Listed {
    symbol: String,
    expiry: Date,
    strike: f64,
    call: bool,
    open_interest: Option<f64>,
}

/// A listing row. A contract of another size is an adjusted one: every figure here counts
/// [`MULTIPLIER`] shares, so it is left out rather than priced wrong.
fn listed(r: &Value) -> Option<Listed> {
    if num(r, "size").is_some_and(|s| (s - MULTIPLIER).abs() > 1e-9) {
        return None;
    }
    let call = match r.get("type")?.as_str()? {
        "call" => true,
        "put" => false,
        _ => return None,
    };
    Some(Listed {
        symbol: r.get("symbol")?.as_str()?.to_string(),
        expiry: r.get("expiration_date")?.as_str().and_then(chains::iso_date)?,
        strike: num(r, "strike_price")?,
        call,
        open_interest: num(r, "open_interest"),
    })
}

/// The listing as `base` holds it.
async fn contracts_at(
    http: &reqwest::Client,
    k: &Keys<'_>,
    base: &str,
    query: &str,
    limited: &AtomicBool,
) -> Result<Vec<Listed>> {
    let url = format!("{}/v2/options/contracts?{query}", host(base));
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..LISTING_PAGES {
        let body = get(http, k, &url, token.as_deref(), limited).await?;
        out.extend(body.get("option_contracts").and_then(Value::as_array).into_iter().flatten().filter_map(listed));
        token = next_page(&body);
        if token.is_none() {
            break;
        }
    }
    Ok(out)
}

/// An underlying's contracts expiring from `from` to `to`, strikes `lo` to `hi`.
async fn contracts(
    http: &reqwest::Client,
    k: &Keys<'_>,
    u: &str,
    (from, to): (Date, Date),
    (lo, hi): (f64, f64),
    limited: &AtomicBool,
) -> Result<Vec<Listed>> {
    let query = format!(
        "underlying_symbols={u}&status=active&expiration_date_gte={from}&expiration_date_lte={to}\
         &strike_price_gte={lo}&strike_price_lte={hi}&limit={LISTING_PAGE}"
    );
    for base in TRADING {
        match contracts_at(http, k, base, &query, limited).await {
            Err(e) if refused(&e) => continue,
            r => return r.with_context(|| format!("listing {u} options at Alpaca")),
        }
    }
    Err(anyhow!("Alpaca refused this key on its live and paper hosts. Check the API key and secret of the connector"))
}

// ── Spot ─────────────────────────────────────────────────────────────────────────────────

/// The underlying on a stock snapshot: the last trade, else the midpoint, else a close.
fn spot_of(v: &Value) -> Option<(f64, &'static str)> {
    let positive = |p: &[&str]| f(v, p).filter(|x| *x > 0.0);
    if let Some(p) = positive(&["latestTrade", "p"]) {
        return Some((p, "alpaca:last"));
    }
    if let (Some(b), Some(a)) = (positive(&["latestQuote", "bp"]), positive(&["latestQuote", "ap"])) {
        if a >= b {
            return Some(((a + b) / 2.0, "alpaca:mid"));
        }
    }
    positive(&["dailyBar", "c"]).or_else(|| positive(&["prevDailyBar", "c"])).map(|c| (c, "alpaca:close"))
}

/// The underlying's price on the plan's stock feed, or the notice of a plan without it.
async fn spot_now(
    http: &reqwest::Client,
    k: &Keys<'_>,
    u: &str,
    p: Plan,
    limited: &AtomicBool,
) -> Result<Result<Option<(f64, &'static str)>, Notice>> {
    let url = format!("{}/v2/stocks/{u}/snapshot?feed={}", host(DATA), p.stocks);
    match get(http, k, &url, None, limited).await {
        Ok(body) => Ok(Ok(spot_of(&body))),
        Err(e) if refused(&e) => Ok(Err(missing_plan(p))),
        Err(e) => Err(e.context(format!("reading the price of {u} at Alpaca"))),
    }
}

// ── Expiries ─────────────────────────────────────────────────────────────────────────────

pub(super) async fn expiries(
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    underlying: &str,
    asset_type: &str,
) -> Result<ExpiryList> {
    let k = keys(secrets)?;
    let u = underlying.trim().to_ascii_uppercase();
    refuse_index(asset_type, &u)?;
    let (now, limited) = (OffsetDateTime::now_utc(), AtomicBool::new(false));
    let today = metrics::ny_date(now);
    let (spot, source) = spot_now(http, &k, &u, plan(secrets), &limited)
        .await?
        .map_err(|n| anyhow!(n.message))?
        .ok_or_else(|| anyhow!("Alpaca returned no recent price for {u}"))?;
    let window = (spot * LISTING_WINDOW.0, spot * LISTING_WINDOW.1);
    let list = contracts(http, &k, &u, (today, today + time::Duration::days(LISTING_DAYS)), window, &limited).await?;
    if list.is_empty() {
        return Err(anyhow!("Alpaca lists no {u} option in the next two years"));
    }
    let mut by_expiry: HashMap<Date, Vec<f64>> = HashMap::new();
    for c in list {
        let ks = by_expiry.entry(c.expiry).or_default();
        if !ks.iter().any(|x| (x - c.strike).abs() < 1e-9) {
            ks.push(c.strike);
        }
    }
    let mut expiries: Vec<(Date, Option<Vec<f64>>)> = by_expiry
        .into_iter()
        .map(|(e, mut ks)| {
            ks.sort_by(f64::total_cmp);
            (e, Some(ks))
        })
        .collect();
    expiries.sort_by_key(|(e, _)| *e);
    Ok(ExpiryList {
        provider: "alpaca".into(),
        underlying: u,
        spot: Some(spot),
        spot_source: Some(source.to_string()),
        as_of: now,
        trading_class: String::new(),
        expiries,
        notices: Vec::new(),
    })
}

// ── Chain ────────────────────────────────────────────────────────────────────────────────

/// A listed contract with its snapshot, if it has one. A bid of 0 is a real quote (nobody
/// bids), an ask of 0 is none; a contract nobody quotes keeps its row with the quote empty.
fn row(c: &Listed, snap: Option<&Value>, tag: &'static str) -> RawQuote {
    let at = |p: &[&str]| snap.and_then(|v| f(v, p));
    let bid = at(&["latestQuote", "bp"]).filter(|b| *b >= 0.0);
    let ask = at(&["latestQuote", "ap"]).filter(|a| *a > 0.0);
    let last = at(&["latestTrade", "p"]).filter(|p| *p > 0.0);
    let volume = at(&["dailyBar", "v"]);
    let iv = at(&["impliedVolatility"]).filter(|v| *v > 0.0);
    let greeks = ["delta", "gamma", "vega", "theta"].map(|g| at(&["greeks", g]));
    RawQuote {
        ticker: c.symbol.clone(),
        expiry: Some(c.expiry),
        strike: c.strike,
        call: c.call,
        bid,
        ask,
        last,
        volume,
        open_interest: c.open_interest,
        provider_iv: iv,
        provider_delta: greeks[0],
        provider_gamma: greeks[1],
        provider_vega: greeks[2],
        provider_theta: greeks[3],
        provider_price: None,
        source: Sources {
            quote: (bid.is_some() || ask.is_some()).then_some(tag),
            last: last.map(|_| tag),
            volume: volume.map(|_| tag),
            open_interest: c.open_interest.map(|_| "alpaca"),
            greeks: (iv.is_some() || greeks.iter().any(Option::is_some)).then_some(tag),
        },
        error: None,
    }
}

/// The snapshots of one expiry inside the window, by contract symbol.
async fn snapshots(
    http: &reqwest::Client,
    k: &Keys<'_>,
    u: &str,
    e: Date,
    (lo, hi): (f64, f64),
    p: Plan,
    limited: &AtomicBool,
) -> Result<serde_json::Map<String, Value>> {
    let url = format!(
        "{}/v1beta1/options/snapshots/{u}?feed={}&expiration_date={e}&strike_price_gte={lo}\
         &strike_price_lte={hi}&limit={SNAPSHOT_PAGE}",
        host(DATA),
        p.options
    );
    let mut out = serde_json::Map::new();
    let mut token: Option<String> = None;
    for _ in 0..SNAPSHOT_PAGES {
        let mut body = get(http, k, &url, token.as_deref(), limited).await?;
        token = next_page(&body);
        if let Some(Value::Object(m)) = body.get_mut("snapshots").map(Value::take) {
            out.extend(m);
        }
        if token.is_none() {
            break;
        }
    }
    Ok(out)
}

fn refused_snapshot(u: &str, p: Plan) -> Snapshot {
    Snapshot {
        provider: "alpaca".into(),
        underlying: u.to_string(),
        as_of: OffsetDateTime::now_utc(),
        notices: vec![missing_plan(p)],
        ..Snapshot::default()
    }
}

pub(super) async fn snapshot(
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    want: &Want<'_>,
    progress: Progress<'_>,
) -> Result<Snapshot> {
    let k = keys(secrets)?;
    let p = plan(secrets);
    let u = want.underlying.trim().to_ascii_uppercase();
    refuse_index(want.asset_type, &u)?;
    let first = *want.expiries.iter().min().ok_or_else(|| anyhow!("no expiry asked for"))?;
    let last = *want.expiries.iter().max().unwrap_or(&first);
    let steps = want.expiries.len() + 1;
    progress(0, steps, "listing contracts");
    let spot = match spot_now(http, &k, &u, p, want.rate_limited).await? {
        Ok(s) => s,
        Err(_) => return Ok(refused_snapshot(&u, p)),
    };
    let (lo, hi) = chain::window(want, spot.map(|s| s.0))?;

    let listing = contracts(http, &k, &u, (first, last), (lo, hi), want.rate_limited).await?;
    let wanted: Vec<Listed> = listing.into_iter().filter(|c| want.expiries.contains(&c.expiry)).collect();
    // The listing is read inside the window, so an expiry missing from it is either not listed
    // at all or listed with no strike in the window: one unbounded read of it tells which.
    if let Some(e) = want.expiries.iter().find(|e| !wanted.iter().any(|c| c.expiry == **e)) {
        let wide = contracts(http, &k, &u, (*e, *e), (0.0, 1e9), want.rate_limited).await?;
        return Err(if wide.is_empty() {
            chain::not_listed("Alpaca", &u, *e)
        } else {
            chain::empty_window("Alpaca", &u, *e, lo, hi)
        });
    }
    chain::refuse_size(&u, wanted.len())?;

    let mut quoted = HashMap::new();
    for (i, e) in want.expiries.iter().enumerate() {
        progress(i + 1, steps, &format!("{u} {e}"));
        match snapshots(http, &k, &u, *e, (lo, hi), p, want.rate_limited).await {
            Ok(m) => quoted.extend(m),
            Err(e) if refused(&e) => return Ok(refused_snapshot(&u, p)),
            Err(e) => return Err(e),
        }
    }
    let quotes = wanted.iter().map(|c| row(c, quoted.get(&c.symbol), p.tag)).collect();
    let mut notices = Vec::new();
    if spot.is_none() {
        notices.push(chain::no_spot(&u));
    }
    if p == FREE {
        notices.push(indicative());
    }
    progress(steps, steps, "done");
    Ok(Snapshot {
        provider: "alpaca".into(),
        underlying: u,
        spot: spot.map(|s| s.0),
        spot_source: spot.map(|s| s.1.to_string()),
        as_of: OffsetDateTime::now_utc(),
        trading_class: String::new(),
        strike_lo: lo,
        strike_hi: hi,
        quotes,
        notices,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::macros::date;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const E1: Date = date!(2026 - 12 - 18);
    const E2: Date = date!(2027 - 01 - 15);

    fn creds(feed: Option<&str>) -> HashMap<String, String> {
        let mut m = HashMap::from([("api_key".to_string(), "kid".to_string()), ("api_secret".to_string(), "sec".to_string())]);
        if let Some(f) = feed {
            m.insert("feed".to_string(), f.to_string());
        }
        m
    }

    fn contract(symbol: &str, expiry: &str, kind: &str, strike: &str, oi: Option<&str>) -> Value {
        json!({"symbol": symbol, "expiration_date": expiry, "type": kind, "strike_price": strike, "size": "100", "open_interest": oi})
    }

    fn ok(v: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(v)
    }

    async fn stock(server: &MockServer, feed: &str) {
        let body = json!({"latestTrade": {"p": 700.5}, "latestQuote": {"bp": 700.0, "ap": 701.0}, "prevDailyBar": {"c": 690.0}});
        Mock::given(method("GET")).and(path("/v2/stocks/SPY/snapshot")).and(query_param("feed", feed)).respond_with(ok(body)).mount(server).await;
    }

    async fn serve() -> MockServer {
        let server = MockServer::start().await;
        crate::brokers::serve_from(&server.uri());
        server
    }

    async fn read(secrets: &HashMap<String, String>, expiries: &[Date], asset_type: &str) -> Result<Snapshot> {
        let limited = AtomicBool::new(false);
        let want = Want { underlying: "spy", asset_type, expiries, strike_lo: None, strike_hi: None, rate_limited: &limited };
        snapshot(&reqwest::Client::new(), secrets, &want, &|_, _, _| {}).await
    }

    #[test]
    fn the_feed_setting_picks_the_plan() {
        assert_eq!(plan(&creds(None)), FREE);
        assert_eq!(plan(&creds(Some(""))), FREE);
        assert_eq!(plan(&creds(Some("iex"))), FREE);
        assert_eq!(plan(&creds(Some(" SIP "))), PAID);
    }

    #[test]
    fn a_listing_row_reads_strings_and_leaves_adjusted_contracts_out() {
        let c = listed(&contract("SPY261218C00650000", "2026-12-18", "call", "650", Some("237"))).unwrap();
        assert_eq!(
            c,
            Listed { symbol: "SPY261218C00650000".into(), expiry: E1, strike: 650.0, call: true, open_interest: Some(237.0) }
        );
        let mut adjusted = contract("SPY1261218C00650000", "2026-12-18", "call", "650", None);
        adjusted["size"] = json!("10");
        assert_eq!(listed(&adjusted), None);
        assert_eq!(listed(&contract("X", "2026-12-18", "swap", "650", None)), None);
    }

    #[test]
    fn a_row_keeps_what_alpaca_said_and_where() {
        let c = Listed { symbol: "S".into(), expiry: E1, strike: 650.0, call: false, open_interest: Some(10.0) };
        let snap = json!({
            "latestQuote": {"bp": 0.0, "ap": 1.25}, "latestTrade": {"p": 1.1}, "dailyBar": {"v": 120.0},
            "impliedVolatility": 0.21, "greeks": {"delta": -0.12, "gamma": 0.004, "theta": -0.08, "vega": 0.35}
        });
        let q = row(&c, Some(&snap), FREE.tag);
        assert_eq!((q.bid, q.ask, q.last, q.volume), (Some(0.0), Some(1.25), Some(1.1), Some(120.0)));
        assert_eq!((q.provider_iv, q.provider_delta, q.provider_vega, q.provider_theta), (Some(0.21), Some(-0.12), Some(0.35), Some(-0.08)));
        assert_eq!((q.source.quote, q.source.open_interest), (Some("alpaca:indicative"), Some("alpaca")));
        // No ask, no greeks, or no snapshot at all: nothing is filled in.
        let q = row(&c, Some(&json!({"latestQuote": {"bp": 0.0, "ap": 0.0}})), PAID.tag);
        assert_eq!((q.ask, q.provider_iv, q.source.greeks, q.source.quote), (None, None, None, Some("alpaca")));
        let q = row(&c, None, PAID.tag);
        assert_eq!((q.bid, q.ask, q.source.quote, q.open_interest), (None, None, None, Some(10.0)));
    }

    #[test]
    fn the_spot_is_the_last_trade_then_the_mid_then_a_close() {
        assert_eq!(spot_of(&json!({"latestTrade": {"p": 10.0}, "latestQuote": {"bp": 9.0, "ap": 9.5}})), Some((10.0, "alpaca:last")));
        assert_eq!(spot_of(&json!({"latestTrade": {"p": 0.0}, "latestQuote": {"bp": 9.0, "ap": 10.0}})), Some((9.5, "alpaca:mid")));
        assert_eq!(spot_of(&json!({"latestQuote": {"bp": 0.0, "ap": 10.0}, "prevDailyBar": {"c": 8.0}})), Some((8.0, "alpaca:close")));
        assert_eq!(spot_of(&json!({"dailyBar": {"c": 7.0}, "prevDailyBar": {"c": 8.0}})), Some((7.0, "alpaca:close")));
        assert_eq!(spot_of(&json!({})), None);
    }

    #[tokio::test]
    async fn a_free_key_reads_the_indicative_feed_and_says_so() {
        let server = serve().await;
        stock(&server, "iex").await;
        // The listing pages on, in the window around the spot, authenticated by header.
        Mock::given(method("GET"))
            .and(path("/v2/options/contracts"))
            .and(query_param("underlying_symbols", "SPY"))
            .and(query_param("expiration_date_gte", "2026-12-18"))
            .and(query_param("expiration_date_lte", "2027-01-15"))
            .and(query_param("page_token", "p2"))
            .and(header("APCA-API-KEY-ID", "kid"))
            .and(header("APCA-API-SECRET-KEY", "sec"))
            .respond_with(ok(json!({"option_contracts": [contract("SPY270115P00650000", "2027-01-15", "put", "650", None)], "next_page_token": null})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/options/contracts"))
            .respond_with(ok(json!({
                "option_contracts": [
                    contract("SPY261218C00650000", "2026-12-18", "call", "650", Some("237")),
                    contract("SPY261218P00650000", "2026-12-18", "put", "650", Some("12")),
                    // Between the two asked for: not read.
                    contract("SPY261231C00650000", "2026-12-31", "call", "650", None),
                ],
                "next_page_token": "p2"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1beta1/options/snapshots/SPY"))
            .and(query_param("feed", "indicative"))
            .and(query_param("expiration_date", "2026-12-18"))
            .and(query_param("page_token", "s2"))
            .respond_with(ok(json!({"snapshots": {"SPY261218P00650000": {"latestQuote": {"bp": 2.0, "ap": 2.2}}}, "next_page_token": null})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1beta1/options/snapshots/SPY"))
            .and(query_param("feed", "indicative"))
            .and(query_param("expiration_date", "2026-12-18"))
            .respond_with(ok(json!({
                "snapshots": {"SPY261218C00650000": {"latestQuote": {"bp": 60.0, "ap": 61.0}, "impliedVolatility": 0.2, "greeks": {"delta": 0.9}}},
                "next_page_token": "s2"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1beta1/options/snapshots/SPY"))
            .and(query_param("expiration_date", "2027-01-15"))
            .respond_with(ok(json!({"snapshots": {}, "next_page_token": null})))
            .mount(&server)
            .await;

        let s = read(&creds(None), &[E1, E2], "equity").await.unwrap();
        assert_eq!((s.provider.as_str(), s.underlying.as_str()), ("alpaca", "SPY"));
        assert_eq!((s.spot, s.spot_source.as_deref()), (Some(700.5), Some("alpaca:last")));
        assert!((s.strike_lo - 420.3).abs() < 1e-6 && (s.strike_hi - 980.7).abs() < 1e-6);
        let by = |sym: &str| s.quotes.iter().find(|q| q.ticker == sym).unwrap();
        assert_eq!(s.quotes.len(), 3);
        assert_eq!((by("SPY261218C00650000").ask, by("SPY261218C00650000").open_interest), (Some(61.0), Some(237.0)));
        assert_eq!(by("SPY261218C00650000").source.quote, Some("alpaca:indicative"));
        assert_eq!(by("SPY261218P00650000").bid, Some(2.0));
        // Listed but unquoted keeps its row, with nothing filled in.
        assert_eq!((by("SPY270115P00650000").bid, by("SPY270115P00650000").strike), (None, 650.0));
        assert_eq!(s.notices.iter().map(|n| n.code).collect::<Vec<_>>(), ["delayed"]);
    }

    #[tokio::test]
    async fn a_paid_key_runs_the_same_read_on_opra() {
        let server = serve().await;
        stock(&server, "sip").await;
        Mock::given(method("GET"))
            .and(path("/v2/options/contracts"))
            .respond_with(ok(json!({"option_contracts": [contract("SPY261218C00650000", "2026-12-18", "call", "650", None)]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1beta1/options/snapshots/SPY"))
            .and(query_param("feed", "opra"))
            .respond_with(ok(json!({"snapshots": {"SPY261218C00650000": {"latestQuote": {"bp": 60.0, "ap": 61.0}}}})))
            .mount(&server)
            .await;
        let s = read(&creds(Some("sip")), &[E1], "etf").await.unwrap();
        assert_eq!(s.quotes[0].source.quote, Some("alpaca"));
        assert!(s.notices.is_empty(), "{:?}", s.notices);
    }

    #[tokio::test]
    async fn a_paper_key_finds_its_listing_on_the_second_host() {
        let server = serve().await;
        stock(&server, "iex").await;
        Mock::given(method("GET")).and(path("/v2/options/contracts")).respond_with(ResponseTemplate::new(403)).up_to_n_times(1).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/v2/options/contracts"))
            .respond_with(ok(json!({"option_contracts": [contract("SPY261218C00650000", "2026-12-18", "call", "650", None)]})))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/v1beta1/options/snapshots/SPY")).respond_with(ok(json!({"snapshots": {}}))).mount(&server).await;
        assert_eq!(read(&creds(None), &[E1], "equity").await.unwrap().quotes.len(), 1);
    }

    #[tokio::test]
    async fn a_key_refused_on_both_hosts_is_named() {
        let server = serve().await;
        stock(&server, "iex").await;
        Mock::given(method("GET")).and(path("/v2/options/contracts")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
        let e = read(&creds(None), &[E1], "equity").await.unwrap_err().to_string();
        assert!(e.contains("API key and secret"), "{e}");
    }

    #[tokio::test]
    async fn a_key_without_opra_gets_a_notice_and_no_rows() {
        let server = serve().await;
        stock(&server, "sip").await;
        Mock::given(method("GET"))
            .and(path("/v2/options/contracts"))
            .respond_with(ok(json!({"option_contracts": [contract("SPY261218C00650000", "2026-12-18", "call", "650", None)]})))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/v1beta1/options/snapshots/SPY")).respond_with(ResponseTemplate::new(403)).mount(&server).await;
        let s = read(&creds(Some("sip")), &[E1], "equity").await.unwrap();
        assert!(s.quotes.is_empty());
        assert_eq!(s.notices[0].code, "missing_plan");
        assert!(s.notices[0].message.contains("OPRA"), "{}", s.notices[0].message);
    }

    #[tokio::test]
    async fn an_expiry_outside_the_listing_says_which_way_it_is_missing() {
        let server = serve().await;
        stock(&server, "iex").await;
        Mock::given(method("GET"))
            .and(path("/v2/options/contracts"))
            .and(query_param("strike_price_gte", "0"))
            .respond_with(ok(json!({"option_contracts": [contract("SPY261218C00100000", "2026-12-18", "call", "100", None)]})))
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path("/v2/options/contracts")).respond_with(ok(json!({"option_contracts": []}))).mount(&server).await;
        let e = read(&creds(None), &[E1], "equity").await.unwrap_err().to_string();
        assert!(e.contains("Widen the strike window"), "{e}");
        server.reset().await;
        stock(&server, "iex").await;
        Mock::given(method("GET")).and(path("/v2/options/contracts")).respond_with(ok(json!({"option_contracts": []}))).mount(&server).await;
        let e = read(&creds(None), &[E1], "equity").await.unwrap_err().to_string();
        assert!(e.contains("not a listed"), "{e}");
    }

    #[tokio::test]
    async fn a_window_over_the_cap_is_refused_before_any_snapshot() {
        let server = serve().await;
        stock(&server, "iex").await;
        let many: Vec<Value> = (0..=super::super::MAX_CONTRACTS)
            .map(|i| contract(&format!("SPY261218C{i:08}"), "2026-12-18", "call", &format!("{}", 500 + i), None))
            .collect();
        Mock::given(method("GET")).and(path("/v2/options/contracts")).respond_with(ok(json!({"option_contracts": many}))).mount(&server).await;
        Mock::given(method("GET")).and(path("/v1beta1/options/snapshots/SPY")).respond_with(ok(json!({"snapshots": {}}))).expect(0).mount(&server).await;
        let e = read(&creds(None), &[E1], "equity").await.unwrap_err().to_string();
        assert!(e.contains("Narrow the strike window"), "{e}");
    }

    #[tokio::test]
    async fn an_index_and_a_connector_without_keys_are_refused_without_a_request() {
        let server = serve().await;
        let e = read(&creds(None), &[E1], "index").await.unwrap_err().to_string();
        assert!(e.contains("no index options"), "{e}");
        let e = read(&HashMap::new(), &[E1], "equity").await.err().unwrap().to_string();
        assert!(e.contains("no API key"), "{e}");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_expiry_list_groups_strikes_by_expiry() {
        let server = serve().await;
        stock(&server, "iex").await;
        Mock::given(method("GET"))
            .and(path("/v2/options/contracts"))
            .respond_with(ok(json!({"option_contracts": [
                contract("SPY270115C00700000", "2027-01-15", "call", "700", None),
                contract("SPY261218P00650000", "2026-12-18", "put", "650", None),
                contract("SPY261218C00650000", "2026-12-18", "call", "650", None),
                contract("SPY261218C00600000", "2026-12-18", "call", "600", None),
            ]})))
            .mount(&server)
            .await;
        let l = expiries(&reqwest::Client::new(), &creds(None), "spy", "equity").await.unwrap();
        assert_eq!((l.provider.as_str(), l.underlying.as_str(), l.spot), ("alpaca", "SPY", Some(700.5)));
        assert_eq!(l.expiries, vec![(E1, Some(vec![600.0, 650.0])), (E2, Some(vec![700.0]))]);
    }

    #[tokio::test]
    async fn a_paper_key_finds_symbols_on_the_paper_host() {
        use crate::histdata::Connector;
        let server = serve().await;
        Mock::given(method("GET")).and(path("/v2/assets")).respond_with(ResponseTemplate::new(401)).up_to_n_times(1).mount(&server).await;
        let asset = json!({"symbol": "SPY", "name": "SPDR S&P 500", "exchange": "ARCA", "class": "us_equity", "status": "active", "tradable": true});
        Mock::given(method("GET")).and(path("/v2/assets")).and(query_param("asset_class", "us_equity")).respond_with(ok(json!([asset]))).mount(&server).await;
        Mock::given(method("GET")).and(path("/v2/assets")).respond_with(ok(json!([]))).mount(&server).await;
        // Its own account id, so no other test's cached list answers for it.
        let mut secrets = creds(None);
        secrets.insert("account_id".to_string(), "paper-search-test".to_string());
        let hits = crate::histdata::native::alpaca().unwrap().search_symbols(&reqwest::Client::new(), &secrets, "SPY", "equity", 5).await.unwrap();
        assert_eq!(hits.iter().map(|h| h.symbol.as_str()).collect::<Vec<_>>(), ["SPY"]);
    }

    #[tokio::test]
    async fn a_provider_refusal_prints_no_url() {
        let server = serve().await;
        Mock::given(method("GET")).and(path("/v2/stocks/SPY/snapshot")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
        let e = format!("{:#}", read(&creds(None), &[E1], "equity").await.unwrap_err());
        assert!(e.contains("SPY") && !e.contains("http://"), "{e}");
    }

    #[test]
    fn alpaca_serves_the_chain_and_nothing_else_in_chains() {
        assert!(chains::CHAIN_PROVIDERS.contains(&"alpaca"));
        assert!(!chains::PROVIDERS.contains(&"alpaca"));
    }
}
