//! One read of an underlying's option chain: the listed contracts of the expiries asked for,
//! inside a strike window, each with its quote, and the underlying's price at that moment.
//!
//! - **Massive**: the options snapshot, one listing per expiry, paged. A key whose plan has
//!   no options snapshot gets a notice naming the plan, never per-contract aggregates instead.
//! - **Alpaca**: the contract listing, then the options snapshot, in `super::alpaca`.
//! - **Interactive Brokers**: the layout and per-expiry strikes from `histdata::chains`, then
//!   one `reqMktData` line per contract (bid, ask, last, volume, open interest and IB's model
//!   computation), at most [`IBKR_CHAIN_PARALLEL`] open at once. The account's lines are
//!   shared with the live charts and the broker book, so a refused line fails its contract
//!   alone. Delayed ticks are not a quote.
//!
//! Every field says where it came from (`source`); nothing missing is filled in.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use futures::StreamExt;
use ibapi::contracts::tick_types::TickType;
use ibapi::contracts::Contract;
use ibapi::market_data::realtime::TickTypes;
use serde::Serialize;
use serde_json::Value;
use time::{Date, OffsetDateTime};

use otw_connector_providers::ibkr::gateway::{contract, resolve, session};

use super::metrics::{self, Derived, Market, Quote};
use super::pricing::Model as Pricing;
use super::MAX_CONTRACTS;
use crate::histdata::chains::{self, Progress};

/// Market-data lines a chain read holds open at once (IB's default allowance is 100).
const IBKR_CHAIN_PARALLEL: usize = 40;
/// How long a line is held for its bid, ask and model computation.
const LINE_WAIT: Duration = Duration::from_secs(4);
/// Option volume, option open interest, option implied volatility.
const GENERIC_TICKS: &[&str] = &["100", "101", "106"];
/// Expiries the IB listing reads strikes for; the rest answer `strikes: null`.
const IBKR_STRIKE_EXPIRIES: usize = 12;
/// Massive snapshot pages followed per expiry (250 contracts a page).
const MASSIVE_PAGES: usize = 20;

const USE_A_CHAIN_CONNECTOR: &str = "Use an Interactive Brokers, a Massive or an Alpaca connector";

#[derive(Debug, Clone, Serialize)]
pub struct Notice {
    pub code: &'static str,
    pub message: String,
}

/// Where each raw field of a row came from: `massive`, `massive:delayed`, `ibkr`, `alpaca`
/// (OPRA) or `alpaca:indicative` (the free feed).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Sources {
    pub quote: Option<&'static str>,
    pub last: Option<&'static str>,
    pub volume: Option<&'static str>,
    pub open_interest: Option<&'static str>,
    pub greeks: Option<&'static str>,
}

/// One contract as the provider quoted it.
#[derive(Debug, Clone, Default)]
pub struct RawQuote {
    pub ticker: String,
    pub expiry: Option<Date>,
    pub strike: f64,
    pub call: bool,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub last: Option<f64>,
    pub volume: Option<f64>,
    pub open_interest: Option<f64>,
    pub provider_iv: Option<f64>,
    pub provider_delta: Option<f64>,
    pub provider_gamma: Option<f64>,
    /// Per vol point, as the providers publish it.
    pub provider_vega: Option<f64>,
    /// Per calendar day, as the providers publish it.
    pub provider_theta: Option<f64>,
    /// IB's model price.
    pub provider_price: Option<f64>,
    pub source: Sources,
    /// This contract's own failure (IB refused its line).
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub provider: String,
    pub underlying: String,
    pub spot: Option<f64>,
    /// `last`, `mid`, `close` or `model` (IB's computation), with the provider in front.
    pub spot_source: Option<String>,
    pub as_of: OffsetDateTime,
    pub trading_class: String,
    /// The strike window read, as asked or centred on the spot.
    pub strike_lo: f64,
    pub strike_hi: f64,
    pub quotes: Vec<RawQuote>,
    pub notices: Vec<Notice>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            provider: String::new(),
            underlying: String::new(),
            spot: None,
            spot_source: None,
            as_of: OffsetDateTime::UNIX_EPOCH,
            trading_class: String::new(),
            strike_lo: 0.0,
            strike_hi: 0.0,
            quotes: Vec::new(),
            notices: Vec::new(),
        }
    }
}

/// What one chain call asks for.
pub struct Want<'a> {
    pub underlying: &'a str,
    pub asset_type: &'a str,
    pub expiries: &'a [Date],
    /// `None` = spot x 0.6 to spot x 1.4.
    pub strike_lo: Option<f64>,
    pub strike_hi: Option<f64>,
    /// Set once the provider answers a request with its rate limit (a 429). Owned by the
    /// caller, so it still reads true after a timeout dropped the read.
    pub rate_limited: &'a AtomicBool,
}

/// The window around the spot when none was given.
const AUTO_WINDOW: (f64, f64) = (0.6, 1.4);

/// The strike window to read: the one asked for, else centred on the spot.
pub(super) fn window(want: &Want<'_>, spot: Option<f64>) -> Result<(f64, f64)> {
    let s = spot.filter(|s| *s > 0.0);
    match (want.strike_lo, want.strike_hi) {
        (Some(lo), Some(hi)) => Ok((lo, hi)),
        (lo, hi) => {
            let s = s.ok_or_else(|| {
                anyhow!(
                    "no price came back for {}, so the strike window cannot be centred on it. Send strike_lo \
                     and strike_hi",
                    want.underlying
                )
            })?;
            Ok((lo.unwrap_or(s * AUTO_WINDOW.0), hi.unwrap_or(s * AUTO_WINDOW.1)))
        }
    }
}

// ── Rows ─────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Row {
    pub ticker: String,
    pub expiry: String,
    pub dte: i64,
    pub strike: f64,
    pub call: bool,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub last: Option<f64>,
    pub volume: Option<f64>,
    pub open_interest: Option<f64>,
    #[serde(flatten)]
    pub derived: Derived,
    pub provider_iv: Option<f64>,
    pub provider_delta: Option<f64>,
    pub provider_gamma: Option<f64>,
    pub provider_vega: Option<f64>,
    pub provider_theta: Option<f64>,
    pub provider_price: Option<f64>,
    pub source: Sources,
    pub error: Option<String>,
}

/// The rows of a snapshot read with `model`, by expiry, strike, calls first.
pub fn rows(s: &Snapshot, model: Pricing<'_>) -> Vec<Row> {
    let market = Market { spot: s.spot, today: metrics::ny_date(s.as_of), model };
    let mut out: Vec<Row> = s
        .quotes
        .iter()
        .filter_map(|q| {
            let expiry = q.expiry?;
            let dte = metrics::dte(s.as_of, expiry);
            let derived = metrics::derive(
                market,
                &Quote { strike: q.strike, call: q.call, dte, bid: q.bid, ask: q.ask, provider_iv: q.provider_iv },
            );
            Some(Row {
                ticker: q.ticker.clone(),
                expiry: expiry.to_string(),
                dte,
                strike: q.strike,
                call: q.call,
                bid: q.bid,
                ask: q.ask,
                last: q.last,
                volume: q.volume,
                open_interest: q.open_interest,
                derived,
                provider_iv: q.provider_iv,
                provider_delta: q.provider_delta,
                provider_gamma: q.provider_gamma,
                provider_vega: q.provider_vega,
                provider_theta: q.provider_theta,
                provider_price: q.provider_price,
                source: q.source.clone(),
                error: q.error.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| (&a.expiry, a.strike, !a.call).partial_cmp(&(&b.expiry, b.strike, !b.call)).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub(super) fn refuse_size(underlying: &str, count: usize) -> Result<()> {
    if count > MAX_CONTRACTS {
        bail!(
            "{underlying} lists {count} contracts on those expiries inside that strike window, over the \
             {MAX_CONTRACTS} a chain call reads. Narrow the strike window or pick fewer expiries"
        );
    }
    Ok(())
}

pub(super) fn not_listed(provider: &str, underlying: &str, e: Date) -> anyhow::Error {
    anyhow!("{e} is not a listed {underlying} expiry at {provider}. Pick one from GET /api/options/expiries")
}

pub(super) fn empty_window(provider: &str, underlying: &str, e: Date, lo: f64, hi: f64) -> anyhow::Error {
    anyhow!(
        "{underlying} lists {e} at {provider}, but no strike between {lo} and {hi}. Widen the strike window"
    )
}

/// A provider refusal over its rate limit: a 429, or the body Massive sends with one.
pub(super) fn over_rate_limit(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<reqwest::Error>()
            .and_then(|r| r.status())
            .is_some_and(|s| s.as_u16() == 429)
    }) || format!("{e:#}").to_ascii_lowercase().contains("exceeded the maximum requests")
}

/// One Massive GET, waiting out its rate limit, and saying so in `limited`.
async fn massive_get(http: &reqwest::Client, url: &str, limited: &AtomicBool) -> Result<Value> {
    chains::patient("massive", || async {
        let r = crate::histdata::massive_get(http, url).await;
        if r.as_ref().is_err_and(over_rate_limit) {
            limited.store(true, Ordering::Relaxed);
        }
        r
    })
    .await
}

pub async fn snapshot(
    provider: &str,
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    want: &Want<'_>,
    progress: Progress<'_>,
) -> Result<Snapshot> {
    match provider {
        "ibkr" => ibkr_snapshot(http, secrets, want, progress).await,
        "massive" => massive_snapshot(http, secrets, want, progress).await,
        "alpaca" => super::alpaca::snapshot(http, secrets, want, progress).await,
        other => Err(anyhow!("{other} does not list option chains. {USE_A_CHAIN_CONNECTOR}")),
    }
}

// ── Expiries ─────────────────────────────────────────────────────────────────────────────

pub struct ExpiryList {
    pub provider: String,
    pub underlying: String,
    pub spot: Option<f64>,
    pub spot_source: Option<String>,
    pub as_of: OffsetDateTime,
    pub trading_class: String,
    /// Each expiry with its strikes; `None` where they were not read.
    pub expiries: Vec<(Date, Option<Vec<f64>>)>,
    pub notices: Vec<Notice>,
}

pub async fn expiries(
    provider: &str,
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    underlying: &str,
    asset_type: &str,
) -> Result<ExpiryList> {
    let now = OffsetDateTime::now_utc();
    let today = metrics::ny_date(now);
    match provider {
        "ibkr" => {
            let session = session::get(secrets).await?;
            let symbol = contract::build(underlying, asset_type)?.symbol.0.clone();
            let (spot, spot_source, mut notices) = ibkr_spot(&session, underlying, asset_type).await?;
            // IB's layout ignores the strike window: one strike list for the whole class.
            let layout = chains::option_layout(
                "ibkr", http, secrets, underlying, asset_type, today, today + time::Duration::days(3 * 366), 0.0, f64::MAX,
            )
            .await?;
            let mut out = Vec::new();
            for (i, e) in layout.expiries.iter().enumerate() {
                let strikes = if i < IBKR_STRIKE_EXPIRIES {
                    Some(chains::ib_strikes(secrets, &symbol, &layout.trading_class, *e).await?)
                } else {
                    None
                };
                out.push((*e, strikes));
            }
            if spot.is_none() && notices.is_empty() {
                notices.push(no_spot(underlying));
            }
            Ok(ExpiryList {
                provider: provider.into(),
                underlying: underlying.into(),
                spot,
                spot_source,
                as_of: now,
                trading_class: layout.trading_class,
                expiries: out,
                notices,
            })
        }
        "massive" => {
            let key = massive_key(secrets)?;
            let u = underlying.trim().to_ascii_uppercase();
            let (spot, spot_source, notices) = massive_spot_now(http, secrets, &key, &u, asset_type, &AtomicBool::new(false)).await?;
            let s = spot.ok_or_else(|| anyhow!("Massive returned no recent price for {u}"))?;
            let layout = chains::option_layout(
                provider, http, secrets, &u, asset_type, today, today + time::Duration::days(730), s * 0.5, s * 1.5,
            )
            .await?;
            let expiries = layout.expiries.iter().map(|e| (*e, layout.strikes.get(e).cloned())).collect();
            Ok(ExpiryList {
                provider: provider.into(),
                underlying: u,
                spot,
                spot_source,
                as_of: now,
                trading_class: String::new(),
                expiries,
                notices,
            })
        }
        "alpaca" => super::alpaca::expiries(http, secrets, underlying, asset_type).await,
        other => Err(anyhow!("{other} does not list option chains. {USE_A_CHAIN_CONNECTOR}")),
    }
}

// ── Massive ──────────────────────────────────────────────────────────────────────────────

fn massive_key(secrets: &HashMap<String, String>) -> Result<String> {
    secrets
        .get("api_key")
        .filter(|k| !k.is_empty())
        .cloned()
        .ok_or_else(|| anyhow!("this Massive connector has no API key"))
}

/// A key whose plan does not include the options snapshot: a 403, or a NOT_AUTHORIZED body.
fn not_authorized(e: &anyhow::Error) -> bool {
    let status = e.chain().any(|c| {
        c.downcast_ref::<reqwest::Error>()
            .and_then(|r| r.status())
            .is_some_and(|s| s.as_u16() == 403)
    });
    let msg = format!("{e:#}").to_ascii_lowercase();
    status || msg.contains("not_authorized") || msg.contains("not entitled")
}

fn missing_plan() -> Notice {
    Notice {
        code: "missing_plan",
        message: "Massive refused the options snapshot: this key's plan does not include options \
                  data. Add a Massive options plan (Options Starter or above) to this key"
            .into(),
    }
}

pub(super) fn no_spot(underlying: &str) -> Notice {
    Notice {
        code: "missing_spot",
        message: format!("no price came back for {underlying}: the columns that need the spot stay empty"),
    }
}

pub(super) fn f(v: &Value, path: &[&str]) -> Option<f64> {
    path.iter().try_fold(v, |v, k| v.get(*k))?.as_f64().filter(|x| x.is_finite())
}

fn s<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(*k))?.as_str()
}

/// The underlying's price on a snapshot row and whether it is delayed.
fn massive_spot(r: &Value) -> Option<(f64, bool)> {
    let px = f(r, &["underlying_asset", "price"]).filter(|p| *p > 0.0)?;
    Some((px, s(r, &["underlying_asset", "timeframe"]).is_some_and(|t| t.eq_ignore_ascii_case("DELAYED"))))
}

/// One snapshot row as a quote. Massive writes 0 for a side with nothing on it; a bid of 0 is
/// a real quote (nobody bids), an ask of 0 is none.
fn massive_row(r: &Value) -> Option<RawQuote> {
    let call = match s(r, &["details", "contract_type"])? {
        "call" => true,
        "put" => false,
        _ => return None,
    };
    let expiry = s(r, &["details", "expiration_date"]).and_then(chains::iso_date)?;
    let strike = f(r, &["details", "strike_price"])?;
    let ticker = s(r, &["details", "ticker"]).unwrap_or("").trim_start_matches("O:").to_string();
    let delayed = |p: &[&str]| s(r, p).is_some_and(|t| t.eq_ignore_ascii_case("DELAYED"));
    let tag = |d: bool| if d { "massive:delayed" } else { "massive" };
    let bid = f(r, &["last_quote", "bid"]).filter(|b| *b >= 0.0);
    let ask = f(r, &["last_quote", "ask"]).filter(|a| *a > 0.0);
    let last = f(r, &["last_trade", "price"]).filter(|p| *p > 0.0);
    let volume = f(r, &["day", "volume"]);
    let open_interest = f(r, &["open_interest"]);
    let greeks = ["delta", "gamma", "vega", "theta"].map(|g| f(r, &["greeks", g]));
    let iv = f(r, &["implied_volatility"]).filter(|v| *v > 0.0);
    Some(RawQuote {
        ticker,
        expiry: Some(expiry),
        strike,
        call,
        bid,
        ask,
        last,
        volume,
        open_interest,
        provider_iv: iv,
        provider_delta: greeks[0],
        provider_gamma: greeks[1],
        provider_vega: greeks[2],
        provider_theta: greeks[3],
        provider_price: None,
        source: Sources {
            quote: (bid.is_some() || ask.is_some()).then(|| tag(delayed(&["last_quote", "timeframe"]))),
            last: last.map(|_| tag(delayed(&["last_trade", "timeframe"]))),
            volume: volume.map(|_| "massive"),
            open_interest: open_interest.map(|_| "massive"),
            greeks: (iv.is_some() || greeks.iter().any(Option::is_some)).then_some("massive"),
        },
        error: None,
    })
}

/// The underlying's price before any chain is read: the options snapshot's, else the last
/// daily close. A plan without options snapshots says so in a notice.
async fn massive_spot_now(
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    key: &str,
    u: &str,
    asset_type: &str,
    limited: &AtomicBool,
) -> Result<(Option<f64>, Option<String>, Vec<Notice>)> {
    let mut notices = Vec::new();
    let url = format!("https://api.massive.com/v3/snapshot/options/{u}?limit=1&apiKey={key}");
    match massive_get(http, &url, limited).await {
        Ok(body) => {
            let first = body.get("results").and_then(Value::as_array).and_then(|a| a.first());
            if let Some((px, delayed)) = first.and_then(massive_spot) {
                let tag = if delayed { "massive:delayed:last" } else { "massive:last" };
                return Ok((Some(px), Some(tag.to_string()), notices));
            }
        }
        Err(e) if not_authorized(&e) => notices.push(missing_plan()),
        Err(e) => return Err(e),
    }
    let now = OffsetDateTime::now_utc();
    let closes = chains::daily_closes("massive", http, secrets, u, asset_type, now - time::Duration::days(10), now).await?;
    let last = closes.last().map(|(_, c)| *c);
    Ok((last, last.map(|_| "massive:close".to_string()), notices))
}

async fn massive_snapshot(
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    want: &Want<'_>,
    p: Progress<'_>,
) -> Result<Snapshot> {
    let key = massive_key(secrets)?;
    let u = want.underlying.trim().to_ascii_uppercase();
    let first = *want.expiries.iter().min().ok_or_else(|| anyhow!("no expiry asked for"))?;
    let last = *want.expiries.iter().max().unwrap_or(&first);
    p(0, want.expiries.len() + 1, "listing contracts");
    // Without a window the spot has to come first, to centre one on it.
    let pre = if want.strike_lo.is_none() || want.strike_hi.is_none() {
        Some(massive_spot_now(http, secrets, &key, &u, want.asset_type, want.rate_limited).await?)
    } else {
        None
    };
    if let Some((_, _, n)) = &pre {
        if !n.is_empty() {
            let mut snap = Snapshot { provider: "massive".into(), underlying: u.clone(), ..Snapshot::default() };
            snap.notices = n.clone();
            snap.as_of = OffsetDateTime::now_utc();
            return Ok(snap);
        }
    }
    let (lo, hi) = window(want, pre.as_ref().and_then(|p| p.0))?;
    // The reference listing counts the window before a snapshot is spent on it.
    let layout = chains::option_layout("massive", http, secrets, &u, want.asset_type, first, last, lo, hi).await;
    let missing = match &layout {
        Ok(l) => want.expiries.iter().find(|e| !l.strikes.contains_key(e)).copied(),
        Err(_) => Some(first),
    };
    // The listing is read inside the window, so an expiry missing from it is either not
    // listed at all or listed with no strike in the window: one unbounded read of that
    // expiry tells which.
    if let Some(e) = missing {
        let wide = chains::option_layout("massive", http, secrets, &u, want.asset_type, e, e, 0.0, 1e9).await;
        return Err(match (wide, layout) {
            (Ok(w), _) if w.strikes.contains_key(&e) => empty_window("Massive", &u, e, lo, hi),
            (Err(_), Err(first_err)) => first_err,
            _ => not_listed("Massive", &u, e),
        });
    }
    let layout = layout?;
    let mut count = 0;
    for e in want.expiries {
        count += 2 * layout.strikes.get(e).map_or(0, Vec::len);
    }
    refuse_size(&u, count)?;

    let mut snap = Snapshot { provider: "massive".into(), underlying: u.clone(), strike_lo: lo, strike_hi: hi, ..Snapshot::default() };
    let mut delayed_spot = false;
    for (i, e) in want.expiries.iter().enumerate() {
        p(i + 1, want.expiries.len() + 1, &format!("{u} {e}"));
        let mut url = format!(
            "https://api.massive.com/v3/snapshot/options/{u}?expiration_date={e}&strike_price.gte={lo}\
             &strike_price.lte={hi}&limit=250&apiKey={key}"
        );
        for _ in 0..MASSIVE_PAGES {
            let body = match massive_get(http, &url, want.rate_limited).await {
                Ok(b) => b,
                Err(e) if not_authorized(&e) => {
                    snap.quotes.clear();
                    snap.notices = vec![missing_plan()];
                    snap.as_of = OffsetDateTime::now_utc();
                    return Ok(snap);
                }
                Err(e) => return Err(e),
            };
            for r in body.get("results").and_then(Value::as_array).into_iter().flatten() {
                if snap.spot.is_none() {
                    if let Some((px, d)) = massive_spot(r) {
                        snap.spot = Some(px);
                        delayed_spot = d;
                    }
                }
                if let Some(q) = massive_row(r) {
                    snap.quotes.push(q);
                }
            }
            match body.get("next_url").and_then(Value::as_str) {
                Some(next) if !next.is_empty() => url = format!("{next}&apiKey={key}"),
                _ => break,
            }
        }
    }
    snap.as_of = OffsetDateTime::now_utc();
    snap.spot_source = snap.spot.map(|_| if delayed_spot { "massive:delayed:last" } else { "massive:last" }.to_string());
    if snap.spot.is_none() {
        snap.notices.push(no_spot(&u));
    }
    if snap.quotes.iter().any(|q| q.source.quote == Some("massive:delayed")) {
        snap.notices.push(Notice {
            code: "delayed",
            message: "Massive serves this key's option quotes delayed: bid and ask are not live".into(),
        });
    }
    p(want.expiries.len() + 1, want.expiries.len() + 1, "done");
    Ok(snap)
}

// ── Interactive Brokers ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
struct Model {
    iv: Option<f64>,
    delta: Option<f64>,
    gamma: Option<f64>,
    vega: Option<f64>,
    theta: Option<f64>,
    price: Option<f64>,
    underlying: Option<f64>,
}

/// What a line answered before it was dropped.
#[derive(Debug, Default)]
struct Line {
    bid: Option<f64>,
    ask: Option<f64>,
    last: Option<f64>,
    close: Option<f64>,
    seen_bid: bool,
    seen_ask: bool,
    seen_last: bool,
    volume: Option<f64>,
    side_volume: Option<f64>,
    oi_call: Option<f64>,
    oi_put: Option<f64>,
    iv: Option<f64>,
    model: Option<Model>,
    /// 354: no market-data subscription for this contract.
    unsubscribed: bool,
    /// 200: no such contract.
    unknown: bool,
    /// Any other refusal (101: no line left), with IB's code and words.
    refused: Option<String>,
}

/// IB's 101, max number of tickers reached: the account has no market-data line left. Read on
/// a line's notice (`error 101:`) or on a failed request (`[101]`).
fn line_limit(msg: &str) -> bool {
    msg.contains("error 101:") || msg.contains("[101]")
}

/// IB writes a huge number or a negative sentinel for a figure it has not computed.
fn sane(v: Option<f64>, lo: f64, hi: f64) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x >= lo && *x <= hi)
}

impl Line {
    fn price(&mut self, kind: TickType, px: f64) {
        // IB sends -1 (or 0) for a side with nothing on it.
        let v = (px > 0.0 && px.is_finite()).then_some(px);
        match kind {
            TickType::Bid => (self.bid, self.seen_bid) = (v, true),
            TickType::Ask => (self.ask, self.seen_ask) = (v, true),
            TickType::Last => (self.last, self.seen_last) = (v.or(self.last), true),
            TickType::Close => self.close = v,
            _ => {}
        }
    }

    fn size(&mut self, kind: TickType, v: f64) {
        let v = (v >= 0.0 && v.is_finite()).then_some(v);
        match kind {
            TickType::Volume => self.volume = v,
            TickType::OptionCallVolume | TickType::OptionPutVolume => self.side_volume = v,
            TickType::OptionCallOpenInterest => self.oi_call = v,
            TickType::OptionPutOpenInterest => self.oi_put = v,
            _ => {}
        }
    }

    fn model(&mut self, o: &ibapi::contracts::OptionComputation) {
        let m = Model {
            iv: sane(o.implied_volatility, 1e-6, 10.0),
            delta: sane(o.delta, -1.0, 1.0),
            gamma: sane(o.gamma, -1e6, 1e6),
            vega: sane(o.vega, -1e6, 1e6),
            theta: sane(o.theta, -1e6, 1e6),
            price: sane(o.option_price, 0.0, 1e9),
            underlying: sane(o.underlying_price, 1e-9, 1e9),
        };
        // The first computation often comes before IB has the underlying: nothing in it.
        if m.iv.is_some() || m.delta.is_some() {
            self.model = Some(m);
        }
    }

    fn done(&self, option: bool) -> bool {
        if option {
            self.seen_bid && self.seen_ask && self.model.is_some()
        } else {
            self.seen_bid && self.seen_ask && self.seen_last
        }
    }

    /// Spot: the last trade, else the midpoint, else the previous close.
    fn spot(&self) -> Option<(f64, &'static str)> {
        if let Some(l) = self.last {
            return Some((l, "ibkr:last"));
        }
        if let (Some(b), Some(a)) = (self.bid, self.ask) {
            if a >= b {
                return Some(((a + b) / 2.0, "ibkr:mid"));
            }
        }
        self.close.map(|c| (c, "ibkr:close"))
    }
}

/// Open one line, read it until it has what a row needs or [`LINE_WAIT`] is up, close it.
async fn read_line(client: &ibapi::Client, c: &Contract, option: bool) -> Result<Line, ibapi::Error> {
    let builder = client.market_data(c);
    let builder = if option { builder.generic_ticks(GENERIC_TICKS) } else { builder };
    let mut sub = builder.subscribe().await?;
    let deadline = tokio::time::Instant::now() + LINE_WAIT;
    let mut line = Line::default();
    loop {
        let item = match tokio::time::timeout_at(deadline, sub.next()).await {
            Ok(Some(item)) => item,
            Ok(None) | Err(_) => break,
        };
        match item {
            Err(e) => {
                sub.cancel().await;
                return Err(e);
            }
            // Delayed prices (66, 67, 68) and the delayed model are other tick types and fall
            // through the matches below: not a quote.
            Ok(TickTypes::Price(t)) => line.price(t.tick_type, t.price),
            Ok(TickTypes::PriceSize(t)) => {
                line.price(t.price_tick_type, t.price);
                line.size(t.size_tick_type, t.size);
            }
            Ok(TickTypes::Size(t)) => line.size(t.tick_type, t.size),
            Ok(TickTypes::Generic(t)) if t.tick_type == TickType::OptionImpliedVol => {
                line.iv = sane(Some(t.value), 1e-6, 10.0);
            }
            Ok(TickTypes::OptionComputation(o)) if o.field == TickType::ModelOption => line.model(&o),
            Ok(TickTypes::Notice(n)) => match n.code {
                354 | 10089 => {
                    line.unsubscribed = true;
                    break;
                }
                200 => {
                    line.unknown = true;
                    break;
                }
                // Informational: delayed data offered, part of the data not subscribed.
                10000..=10999 | 2100..=2169 => {}
                code => {
                    line.refused = Some(format!("Interactive Brokers error {code}: {}", n.message));
                    break;
                }
            },
            Ok(TickTypes::SnapshotEnd) => break,
            Ok(_) => {}
        }
        if line.done(option) {
            break;
        }
    }
    sub.cancel().await;
    Ok(line)
}

async fn ibkr_spot(
    session: &Arc<session::Session>,
    underlying: &str,
    asset_type: &str,
) -> Result<(Option<f64>, Option<String>, Vec<Notice>)> {
    let c = resolve::contract_for(session, underlying, asset_type).await?;
    // A market-data line is not a historical request: sent at once, outside IB's pacing.
    let line = session
        .ask(&format!("{underlying} quote"), move |client| {
            let c = c.clone();
            async move { read_line(&client, &c, false).await }
        })
        .await?;
    let mut notices = Vec::new();
    if line.unsubscribed {
        notices.push(Notice {
            code: "missing_subscription",
            message: format!(
                "Interactive Brokers refused live data on {underlying} (error 354): subscribe to its \
                 exchange's market data in Client Portal"
            ),
        });
    }
    if let Some(r) = &line.refused {
        notices.push(Notice { code: "spot_refused", message: format!("{underlying}: {r}") });
    }
    let spot = line.spot();
    Ok((spot.map(|s| s.0), spot.map(|s| s.1.to_string()), notices))
}

fn ib_expiry(e: Date) -> String {
    format!("{:04}{:02}{:02}", e.year(), e.month() as u8, e.day())
}

async fn ibkr_snapshot(
    http: &reqwest::Client,
    secrets: &HashMap<String, String>,
    want: &Want<'_>,
    p: Progress<'_>,
) -> Result<Snapshot> {
    let session = session::get(secrets).await?;
    let underlying = want.underlying.trim();
    let symbol = contract::build(underlying, want.asset_type)?.symbol.0.clone();
    let first = *want.expiries.iter().min().ok_or_else(|| anyhow!("no expiry asked for"))?;
    let last = *want.expiries.iter().max().unwrap_or(&first);
    p(0, 1, "listing the chain");
    let (mut spot, mut spot_source, mut notices) = ibkr_spot(&session, underlying, want.asset_type).await?;
    let (lo, hi) = window(want, spot)?;
    let layout = chains::option_layout("ibkr", http, secrets, underlying, want.asset_type, first, last, lo, hi).await?;
    let mut wanted: Vec<(Date, f64, bool)> = Vec::new();
    for (i, e) in want.expiries.iter().enumerate() {
        if !layout.expiries.contains(e) {
            return Err(not_listed("Interactive Brokers", underlying, *e));
        }
        p(i, want.expiries.len(), &format!("strikes on {e}"));
        let ks = chains::ib_strikes(secrets, &symbol, &layout.trading_class, *e).await?;
        for k in ks.into_iter().filter(|k| *k >= lo && *k <= hi) {
            wanted.push((*e, k, true));
            wanted.push((*e, k, false));
        }
    }
    refuse_size(underlying, wanted.len())?;

    let as_of = OffsetDateTime::now_utc();
    let total = wanted.len();
    let done = AtomicUsize::new(0);
    p(0, total, "quotes");
    let class = layout.trading_class.clone();
    let lines: Vec<((Date, f64, bool), Result<Line, String>)> = futures::stream::iter(wanted)
        .map(|(e, k, call)| {
            let mut c = Contract::option(&symbol, &ib_expiry(e), k, if call { "C" } else { "P" });
            c.trading_class = class.clone();
            let label = chains::occ(&symbol, e, call, k);
            let (session, done) = (&session, &done);
            async move {
                let r = session
                    .ask(&label, move |client| {
                        let c = c.clone();
                        async move { read_line(&client, &c, true).await }
                    })
                    .await
                    .map_err(|e| e.to_string());
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                p(n, total, &label);
                ((e, k, call), r)
            }
        })
        .buffer_unordered(IBKR_CHAIN_PARALLEL)
        .collect()
        .await;

    let (mut unsubscribed, mut refused, mut no_line, mut unknown) = (0, 0, 0, 0);
    let mut quotes = Vec::with_capacity(lines.len());
    for ((e, k, call), r) in lines {
        let mut q = RawQuote { ticker: chains::occ(&symbol, e, call, k), expiry: Some(e), strike: k, call, ..RawQuote::default() };
        match r {
            Err(msg) => {
                if line_limit(&msg) {
                    no_line += 1;
                } else {
                    refused += 1;
                }
                q.error = Some(msg);
            }
            Ok(l) if l.unknown => {
                unknown += 1;
                continue;
            }
            Ok(l) => {
                if l.unsubscribed {
                    unsubscribed += 1;
                }
                if let Some(msg) = &l.refused {
                    if line_limit(msg) {
                        no_line += 1;
                    } else {
                        refused += 1;
                    }
                    q.error = Some(msg.clone());
                }
                let m = l.model.unwrap_or_default();
                if spot.is_none() {
                    if let Some(u) = m.underlying {
                        spot = Some(u);
                        spot_source = Some("ibkr:model".into());
                    }
                }
                q.bid = l.bid;
                q.ask = l.ask;
                q.last = l.last;
                q.volume = l.volume.or(l.side_volume);
                q.open_interest = if call { l.oi_call } else { l.oi_put };
                q.provider_iv = m.iv.or(l.iv);
                q.provider_delta = m.delta;
                q.provider_gamma = m.gamma;
                q.provider_vega = m.vega;
                q.provider_theta = m.theta;
                q.provider_price = m.price;
                q.source = Sources {
                    quote: (q.bid.is_some() || q.ask.is_some()).then_some("ibkr"),
                    last: q.last.map(|_| "ibkr"),
                    volume: q.volume.map(|_| "ibkr"),
                    open_interest: q.open_interest.map(|_| "ibkr"),
                    greeks: (q.provider_iv.is_some() || q.provider_delta.is_some()).then_some("ibkr"),
                };
            }
        }
        quotes.push(q);
    }
    if unsubscribed > 0 {
        notices.push(Notice {
            code: "missing_subscription",
            message: format!(
                "Interactive Brokers refused live option quotes on {unsubscribed} contracts (error 354): \
                 the account has no OPRA subscription. Subscribe to OPRA (US options) in Client Portal; \
                 those rows keep their contract with bid and ask empty"
            ),
        });
    }
    if no_line > 0 {
        notices.push(Notice {
            code: "lines_refused",
            message: format!(
                "{no_line} contracts got no market-data line (error 101: the account's line limit, shared \
                 with live charts). Narrow the strike window or close live charts, then refresh"
            ),
        });
    }
    if refused > 0 {
        notices.push(Notice {
            code: "lines_failed",
            message: format!("{refused} contracts could not be read; each row carries its error"),
        });
    }
    if unknown > 0 {
        notices.push(Notice {
            code: "not_listed",
            message: format!("{unknown} contracts in the window are not listed on their expiry and are left out"),
        });
    }
    if spot.is_none() {
        notices.push(no_spot(underlying));
    }
    p(total, total, "done");
    Ok(Snapshot {
        provider: "ibkr".into(),
        underlying: underlying.to_string(),
        spot,
        spot_source,
        as_of,
        trading_class: layout.trading_class,
        strike_lo: lo,
        strike_hi: hi,
        quotes,
        notices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::macros::{date, datetime};

    #[test]
    fn a_massive_row_keeps_what_massive_said_and_where() {
        let r = json!({
            "details": {"contract_type": "put", "expiration_date": "2026-12-18", "strike_price": 650.0, "ticker": "O:SPY261218P00650000"},
            "last_quote": {"bid": 0.0, "ask": 1.25, "timeframe": "DELAYED"},
            "last_trade": {"price": 1.1, "timeframe": "REAL-TIME"},
            "day": {"volume": 120.0},
            "open_interest": 3400.0,
            "implied_volatility": 0.21,
            "greeks": {"delta": -0.12, "gamma": 0.004, "theta": -0.08, "vega": 0.35},
            "underlying_asset": {"price": 700.5}
        });
        let q = massive_row(&r).unwrap();
        assert_eq!(q.ticker, "SPY261218P00650000");
        assert_eq!((q.call, q.strike, q.expiry), (false, 650.0, Some(date!(2026 - 12 - 18))));
        assert_eq!((q.bid, q.ask, q.last), (Some(0.0), Some(1.25), Some(1.1)));
        assert_eq!((q.provider_iv, q.provider_delta, q.provider_vega), (Some(0.21), Some(-0.12), Some(0.35)));
        assert_eq!(q.source.quote, Some("massive:delayed"));
        assert_eq!(q.source.last, Some("massive"));
        assert_eq!(massive_spot(&r), Some((700.5, false)));
        // No ask, no greeks: nothing is filled in.
        let bare = json!({"details": {"contract_type": "call", "expiration_date": "2026-12-18", "strike_price": 800.0}, "last_quote": {"bid": 0.0, "ask": 0.0}});
        let q = massive_row(&bare).unwrap();
        assert_eq!((q.ask, q.provider_iv, q.source.greeks), (None, None, None));
    }

    #[test]
    fn rows_derive_against_the_snapshot_spot_and_sort_calls_first() {
        let snap = Snapshot {
            spot: Some(100.0),
            as_of: datetime!(2026-11-02 15:00 UTC),
            quotes: vec![
                RawQuote { expiry: Some(date!(2026 - 12 - 18)), strike: 100.0, call: false, bid: Some(2.0), ask: Some(2.2), ..RawQuote::default() },
                RawQuote { expiry: Some(date!(2026 - 12 - 18)), strike: 100.0, call: true, bid: Some(2.5), ask: Some(2.7), ..RawQuote::default() },
                RawQuote { expiry: Some(date!(2026 - 11 - 20)), strike: 105.0, call: true, bid: None, ask: None, ..RawQuote::default() },
            ],
            ..Snapshot::default()
        };
        let model = Pricing { style: crate::options::pricing::Style::European, rate: 0.04, dividend: 0.0, dividends: &[] };
        let rows = rows(&snap, model);
        assert_eq!(rows.iter().map(|r| (r.expiry.as_str(), r.call)).collect::<Vec<_>>(), vec![("2026-11-20", true), ("2026-12-18", true), ("2026-12-18", false)]);
        assert_eq!(rows[1].dte, 46);
        assert!(rows[1].derived.iv_mid.is_some() && rows[0].derived.mid.is_none());
        let v = serde_json::to_value(&rows[1]).unwrap();
        assert!(v.get("iv_mid").is_some() && v.get("provider_iv").is_some(), "derived columns sit flat beside the raw ones");
    }

    #[test]
    fn an_ib_line_ignores_empty_sides_and_reads_its_spot_in_order() {
        let mut l = Line::default();
        l.price(TickType::Bid, -1.0);
        l.price(TickType::Ask, 1.2);
        l.price(TickType::DelayedLast, 9.0);
        assert_eq!((l.bid, l.ask, l.last), (None, Some(1.2), None));
        assert!(l.seen_bid && !l.done(false));
        l.price(TickType::Close, 1.0);
        assert_eq!(l.spot(), Some((1.0, "ibkr:close")));
        l.price(TickType::Bid, 1.0);
        assert_eq!(l.spot(), Some((1.1, "ibkr:mid")));
        l.price(TickType::Last, 1.15);
        assert_eq!(l.spot(), Some((1.15, "ibkr:last")));
        assert!(l.done(false) && !l.done(true));
    }

    #[test]
    fn a_missing_window_centres_on_the_spot_or_is_refused() {
        let exps = [date!(2026 - 12 - 18)];
        let limited = AtomicBool::new(false);
        let want = |lo, hi| Want { underlying: "SPY", asset_type: "etf", expiries: &exps, strike_lo: lo, strike_hi: hi, rate_limited: &limited };
        assert_eq!(window(&want(Some(1.0), Some(2.0)), None).unwrap(), (1.0, 2.0));
        let (lo, hi) = window(&want(None, None), Some(100.0)).unwrap();
        assert!((lo - 60.0).abs() < 1e-9 && (hi - 140.0).abs() < 1e-9);
        assert_eq!(window(&want(Some(90.0), None), Some(100.0)).unwrap().0, 90.0);
        assert!(window(&want(None, None), None).unwrap_err().to_string().contains("strike_lo"));
    }

    #[test]
    fn only_error_101_is_blamed_on_the_line_limit() {
        assert!(line_limit("Interactive Brokers error 101: Max number of tickers has been reached"));
        assert!(line_limit("[101] Max number of tickers has been reached"));
        assert!(!line_limit("Interactive Brokers error 10197: No market data during competing live session"));
        assert!(!line_limit("Interactive Brokers error 300: Can't find EId with tickerId"));
    }

    #[test]
    fn a_window_over_the_cap_is_refused_with_the_count() {
        let e = refuse_size("SPY", MAX_CONTRACTS + 2).unwrap_err().to_string();
        assert!(e.contains(&format!("{}", MAX_CONTRACTS + 2)), "{e}");
        assert!(refuse_size("SPY", MAX_CONTRACTS).is_ok());
    }
}
