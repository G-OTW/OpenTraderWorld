//! Options API: chain, strategy builder, volatility.
//!
//! - `GET          /api/options/expiries`            an underlying's expiries and strikes
//! - `GET          /api/options/dividends`           an underlying's coming cash dividends
//! - `POST         /api/options/chain`               a chain read → task id
//! - `GET          /api/options/tasks/{id}`          progress, then the chain (or surface)
//! - `POST         /api/options/strategy/evaluate`   a strategy across spot; no provider
//! - `GET/POST     /api/options/strategies`          saved strategies; `PUT/DELETE …/{id}`
//! - `POST         /api/options/surface`             the quant IV surface, granted to Options
//! - `POST         /api/options/ivhistory`           the quant IV history, granted to Options
//!
//! Provider reads go through a connector granted to the module (Interactive Brokers, Massive
//! or, for the chain, Alpaca). A chain is kept 30 seconds, so a rate or a filter change reads it again for free.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use otw_store::fundamentals as fund_store;
use otw_store::options::{self as store, StrategyInput};

use crate::fundamentals::datasets;
use crate::histdata::chains;
use crate::market_task::{chain_source, progress, spawn};
use crate::options::pricing::{Model, Style};
use crate::options::strategy::{self, Kind, Leg, Regime, Smile};
use crate::options::{cache, chain, dividends, metrics, MAX_EXPIRIES};
use crate::quant_market_api::{self as quant_market, IvHistoryBody, OptionsBody};
use crate::{ApiError, AppState};

/// A whole chain read, every request included.
const CHAIN_TIMEOUT: Duration = Duration::from_secs(90);
/// Legs one strategy holds at most.
const MAX_LEGS: usize = 12;
const MAX_STEPS: usize = 1001;
const UNDERLYING_TYPES: &[&str] = &["equity", "etf", "index"];
/// Cash dividends one read carries at most: two years of a monthly payer, and then some.
const MAX_DIVIDENDS: usize = 40;
/// Smile points one expiry carries at most.
const MAX_SMILE_POINTS: usize = 400;
/// How far ahead the dividend schedule runs: the longest listing read.
const DIVIDEND_DAYS: i64 = 730;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/options/expiries", get(expiries))
        .route("/api/options/dividends", get(dividends_ahead))
        .route("/api/options/chain", post(chain_task))
        .route("/api/options/tasks/{id}", get(crate::market_task::task))
        .route("/api/options/strategy/evaluate", post(evaluate))
        .route("/api/options/strategies", get(list_strategies).post(create_strategy))
        .route("/api/options/strategies/{id}", put(update_strategy).delete(delete_strategy))
        .route("/api/options/surface", post(surface))
        .route("/api/options/ivhistory", post(iv_history))
}

/// The underlying, checked before it goes into a provider URL: letters, digits, `.`, `:`
/// and `-` only, upper-cased.
fn underlying(v: &str) -> Result<String, ApiError> {
    let u = v.trim();
    if u.is_empty() {
        return Err(ApiError::bad_request("underlying is required: pick it from the symbol search, e.g. SPY"));
    }
    if !u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-')) {
        return Err(ApiError::bad_request(
            "underlying is one symbol as the symbol search returned it, e.g. SPY: letters, digits, '.', ':' and '-' only",
        ));
    }
    Ok(u.to_ascii_uppercase())
}

/// A strike window from strike_lo to strike_hi, both zero or more. One strike alone is
/// lo == hi; null leaves that side to the spot.
fn strike_window(lo: Option<f64>, hi: Option<f64>) -> Result<(), ApiError> {
    let bad = |v: Option<f64>| v.is_some_and(|x| !(x.is_finite() && x >= 0.0));
    if bad(lo) || bad(hi) || lo.zip(hi).is_some_and(|(l, h)| h < l) {
        return Err(ApiError::bad_request(
            "the strike window runs from strike_lo to a strike_hi at or above it, both zero or more (null = around the spot)",
        ));
    }
    Ok(())
}

fn asset_type(v: &str) -> Result<&str, ApiError> {
    if UNDERLYING_TYPES.contains(&v) {
        Ok(v)
    } else {
        Err(ApiError::bad_request("asset_type is equity, etf or index: options on other underlyings are not read"))
    }
}

fn date(what: &str, v: &str) -> Result<Date, ApiError> {
    chains::iso_date(v.trim())
        .filter(|_| v.trim().len() == 10)
        .ok_or_else(|| ApiError::bad_request(&format!("{what} {v:?} is not a date. Write it YYYY-MM-DD")))
}

fn rate_and_dividend(rate: f64, dividend: f64) -> Result<(), ApiError> {
    if !(-0.1..=0.5).contains(&rate) || !(-0.1..=0.5).contains(&dividend) {
        return Err(ApiError::bad_request("rate and dividend are fractions between -0.1 and 0.5, e.g. 0.04"));
    }
    Ok(())
}

/// A cash dividend the model prices with.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DividendBody {
    /// YYYY-MM-DD.
    ex_date: String,
    /// Per share.
    amount: f64,
}

/// Cash dividends, checked. They and a yield would count the same income twice.
fn cash_dividends(list: &[DividendBody], yield_: f64) -> Result<Vec<(Date, f64)>, ApiError> {
    if list.len() > MAX_DIVIDENDS {
        return Err(ApiError::bad_request(&format!("at most {MAX_DIVIDENDS} dividends, {} were sent", list.len())));
    }
    if !list.is_empty() && yield_ != 0.0 {
        return Err(ApiError::bad_request("send either cash dividends or a dividend yield, not both: they count the same income"));
    }
    list.iter()
        .map(|d| {
            if !(d.amount.is_finite() && d.amount > 0.0) {
                return Err(ApiError::bad_request("a dividend's amount is per share, above zero"));
            }
            Ok((date("ex_date", &d.ex_date)?, d.amount))
        })
        .collect()
}

fn provider_name(p: &str) -> &str {
    match p {
        "massive" => "Massive",
        "ibkr" => "Interactive Brokers",
        "alpaca" => "Alpaca",
        other => other,
    }
}

fn today() -> Date {
    metrics::ny_date(OffsetDateTime::now_utc())
}

fn stamp(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

// ── Expiries ─────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ExpiriesQuery {
    /// A data connector granted to Options (Interactive Brokers, Massive or Alpaca).
    connector_id: Uuid,
    /// The underlying as the symbol search returned it.
    underlying: String,
    /// equity | etf | index.
    asset_type: String,
}

async fn expiries(State(state): State<AppState>, Query(q): Query<ExpiriesQuery>) -> Result<Json<Value>, ApiError> {
    let u = underlying(&q.underlying)?;
    let kind = asset_type(&q.asset_type)?;
    let src = chain_source(&state, q.connector_id, "options", "Options").await?;
    let list = tokio::time::timeout(CHAIN_TIMEOUT, chain::expiries(&src.provider, &state.http, &src.secrets, &u, kind))
        .await
        .map_err(|_| ApiError::timeout("listing the expiries took longer than 90 s. Try again in a minute"))?
        // The provider's reason names the fix: a bad key, an index on Alpaca, an unknown symbol.
        .map_err(|e| ApiError::bad_gateway(&format!("{e:#}")))?;
    let today = metrics::ny_date(list.as_of);
    let expiries: Vec<Value> = list
        .expiries
        .iter()
        .map(|(d, ks)| json!({ "date": d.to_string(), "dte": (*d - today).whole_days(), "strikes": ks }))
        .collect();
    Ok(Json(json!({
        "provider": list.provider,
        "underlying": list.underlying,
        "spot": list.spot,
        "spot_source": list.spot_source,
        "as_of": stamp(list.as_of),
        "trading_class": list.trading_class,
        "notices": list.notices,
        "expiries": expiries,
    })))
}

// ── Dividends ────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DividendsQuery {
    underlying: String,
    /// equity | etf | index.
    asset_type: String,
}

/// The cash dividends coming on an underlying, from the Fundamentals dividend dataset: those
/// announced, then the pattern carried forward and marked `projected`. Nothing to read (an
/// index, no provider connected) is an empty list with a `notice`, never an error: the chain
/// still prices, on the yield the user typed.
async fn dividends_ahead(State(state): State<AppState>, Query(q): Query<DividendsQuery>) -> Result<Json<Value>, ApiError> {
    let u = underlying(&q.underlying)?;
    let kind = asset_type(&q.asset_type)?;
    let empty = |notice: String| Json(json!({ "dividends": [], "provider": null, "notice": notice }));
    if kind == "index" {
        return Ok(empty("an index pays no cash dividend of its own: type its yield".into()));
    }
    if crate::demo::enabled() {
        return Ok(empty("the demo does not reach data providers".into()));
    }
    let subject = crate::fundamentals::edgar::normalize_ticker(&u);
    let mut snap = fund_store::snapshot(&state.pool, &subject, "dividends").await?;
    if snap.as_ref().is_none_or(datasets::stale) {
        let asked = datasets::refresh(&state, "dividends", &subject, None, false).await;
        match (asked, &snap) {
            (Ok(_), _) => snap = fund_store::snapshot(&state.pool, &subject, "dividends").await?,
            (Err(e), None) => return Ok(empty(format!("no dividend data for {u}: {e:#}. Type a yield instead"))),
            (Err(_), Some(_)) => {}
        }
    }
    let Some(snap) = snap else {
        return Ok(empty(format!("no dividend data for {u}: connect a provider of the Fundamentals dividend dataset, or type a yield")));
    };
    let history: Vec<(Date, f64)> = snap
        .data
        .get("dividends")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let day = r.get("ex_date").and_then(Value::as_str).and_then(chains::iso_date)?;
            Some((day, r.get("amount").and_then(Value::as_f64)?))
        })
        .collect();
    let now = today();
    let ahead: Vec<Value> = dividends::upcoming(&history, now, now + time::Duration::days(DIVIDEND_DAYS))
        .iter()
        .map(|d| json!({ "ex_date": d.ex_date.to_string(), "amount": d.amount, "projected": d.projected }))
        .collect();
    let notice = ahead.is_empty().then(|| format!("{u} shows no regular dividend: none is priced in"));
    Ok(Json(json!({ "dividends": ahead, "provider": datasets::label(&snap.provider), "notice": notice })))
}

// ── Chain ────────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ChainBody {
    connector_id: Uuid,
    underlying: String,
    asset_type: String,
    /// Listed expiries (YYYY-MM-DD) from GET /api/options/expiries, 1 to 8.
    expiries: Vec<String>,
    /// Lowest strike read; null = spot x 0.6.
    #[serde(default)]
    strike_lo: Option<f64>,
    /// Highest strike read; null = spot x 1.4.
    #[serde(default)]
    strike_hi: Option<f64>,
    /// Continuous, as a fraction. Default 0.04.
    #[serde(default = "default_rate")]
    rate: f64,
    /// Continuous dividend yield, as a fraction. Default 0.
    #[serde(default)]
    dividend: f64,
    /// Cash dividends, from GET /api/options/dividends. Replace the yield, which must be 0.
    #[serde(default)]
    dividends: Vec<DividendBody>,
    /// Read the provider again even when the same chain was read in the last 30 s.
    #[serde(default)]
    force: bool,
}
fn default_rate() -> f64 {
    0.04
}

async fn chain_task(State(state): State<AppState>, Json(b): Json<ChainBody>) -> Result<Json<Value>, ApiError> {
    let u = underlying(&b.underlying)?;
    let kind = asset_type(&b.asset_type)?.to_string();
    if b.expiries.is_empty() || b.expiries.len() > MAX_EXPIRIES {
        return Err(ApiError::bad_request(&format!(
            "a chain call reads 1 to {MAX_EXPIRIES} expiries, {} were asked for. Pick fewer",
            b.expiries.len()
        )));
    }
    let today = today();
    let mut exps = Vec::with_capacity(b.expiries.len());
    for e in &b.expiries {
        let d = date("expiry", e)?;
        if d < today {
            return Err(ApiError::bad_request(&format!("{d} has expired. Pick a listed expiry from GET /api/options/expiries")));
        }
        exps.push(d);
    }
    exps.sort();
    exps.dedup();
    let (lo, hi) = (b.strike_lo, b.strike_hi);
    strike_window(lo, hi)?;
    rate_and_dividend(b.rate, b.dividend)?;
    let cash = cash_dividends(&b.dividends, b.dividend)?;
    let style = Style::of(&kind);
    let src = chain_source(&state, b.connector_id, "options", "Options").await?;
    let key = cache::key(src.id, &u, &kind, &exps, lo, hi);
    let (rate, dividend, force) = (b.rate, b.dividend, b.force);
    Ok(spawn(move |id| {
        Box::pin(async move {
            let snap = match (!force).then(|| cache::get(&key)).flatten() {
                Some(s) => s,
                None => {
                    let p = progress(id);
                    let limited = AtomicBool::new(false);
                    let want = chain::Want {
                        underlying: &u,
                        asset_type: &kind,
                        expiries: &exps,
                        strike_lo: lo,
                        strike_hi: hi,
                        rate_limited: &limited,
                    };
                    let read = chain::snapshot(&src.provider, &state.http, &src.secrets, &want, &p);
                    let s = tokio::time::timeout(CHAIN_TIMEOUT, read).await.map_err(|_| {
                        // Waiting out the provider's rate limit is what ran the clock down: a
                        // narrower window costs the same requests, fewer expiries cost fewer.
                        if limited.load(Ordering::Relaxed) {
                            anyhow::anyhow!(
                                "the chain read took longer than 90 s: {} refused requests over its rate limit and \
                                 each one was waited out. Pick fewer expiries per call",
                                provider_name(&src.provider)
                            )
                        } else {
                            anyhow::anyhow!("the chain read took longer than 90 s. Narrow the strike window or pick fewer expiries")
                        }
                    })??;
                    let s = Arc::new(s);
                    cache::put(key, s.clone());
                    s
                }
            };
            Ok(json!({
                "provider": snap.provider,
                "underlying": snap.underlying,
                "spot": snap.spot,
                "spot_source": snap.spot_source,
                "as_of": stamp(snap.as_of),
                "rate": rate,
                "dividend": dividend,
                "trading_class": snap.trading_class,
                "strike_lo": snap.strike_lo,
                "strike_hi": snap.strike_hi,
                "notices": snap.notices,
                "style": if style == Style::American { "american" } else { "european" },
                "rows": chain::rows(&snap, Model { style, rate, dividend, dividends: &cash }),
            }))
        })
    }))
}

// ── Strategy ─────────────────────────────────────────────────────────────────

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct LegBody {
    /// call | put | stock.
    kind: String,
    /// Required on an option leg.
    #[serde(default)]
    strike: Option<f64>,
    /// YYYY-MM-DD, required on an option leg.
    #[serde(default)]
    expiry: Option<String>,
    /// Signed: + long, - short. Contracts for an option (100 shares each), shares for stock.
    #[serde(default)]
    qty: Option<f64>,
    /// Premium paid per share (per unit for stock).
    #[serde(default)]
    price: Option<f64>,
    /// Implied volatility as a fraction (0.22), needed while the leg is still open.
    #[serde(default)]
    iv: Option<f64>,
}

/// One leg checked for shape: kind, strike and expiry where they belong, finite numbers.
fn leg(i: usize, l: &LegBody) -> Result<Leg, ApiError> {
    let n = i + 1;
    let kind = match l.kind.as_str() {
        "call" => Kind::Call,
        "put" => Kind::Put,
        "stock" => Kind::Stock,
        _ => return Err(ApiError::bad_request(&format!("leg {n}: kind is call, put or stock"))),
    };
    let qty = l.qty.filter(|q| q.is_finite() && *q != 0.0 && q.abs() <= 1e6)
        .ok_or_else(|| ApiError::bad_request(&format!("leg {n}: qty is a non-zero number, + long and - short")))?;
    let price = l.price.filter(|p| p.is_finite() && *p >= 0.0)
        .ok_or_else(|| ApiError::bad_request(&format!("leg {n}: price is the premium paid per share, zero or more")))?;
    if l.iv.is_some_and(|v| !(v.is_finite() && v > 0.0 && v <= 10.0)) {
        return Err(ApiError::bad_request(&format!("leg {n}: iv is a fraction above 0 and at most 10, e.g. 0.22")));
    }
    if kind == Kind::Stock {
        return Ok(Leg { kind, strike: 0.0, expiry: None, qty, price, iv: None });
    }
    let strike = l.strike.filter(|k| k.is_finite() && *k > 0.0)
        .ok_or_else(|| ApiError::bad_request(&format!("leg {n}: an option leg needs a positive strike")))?;
    let expiry = l.expiry.as_deref()
        .ok_or_else(|| ApiError::bad_request(&format!("leg {n}: an option leg needs an expiry (YYYY-MM-DD)")))?;
    let expiry = date(&format!("leg {n}: expiry"), expiry)?;
    Ok(Leg { kind, strike, expiry: Some(expiry), qty, price, iv: l.iv })
}

fn legs(list: &[LegBody]) -> Result<Vec<Leg>, ApiError> {
    if list.is_empty() || list.len() > MAX_LEGS {
        return Err(ApiError::bad_request(&format!("a strategy has 1 to {MAX_LEGS} legs, {} were sent", list.len())));
    }
    list.iter().enumerate().map(|(i, l)| leg(i, l)).collect()
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RangeBody {
    lo: f64,
    hi: f64,
    /// Points across the range, 2 to 1001. Default 121.
    #[serde(default = "default_steps")]
    steps: usize,
}
fn default_steps() -> usize {
    121
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct EvaluateBody {
    /// The underlying's price.
    spot: f64,
    /// Continuous, as a fraction. Default 0.04.
    #[serde(default = "default_rate")]
    rate: f64,
    #[serde(default)]
    dividend: f64,
    /// Cash dividends, from GET /api/options/dividends. Replace the yield, which must be 0.
    #[serde(default)]
    dividends: Vec<DividendBody>,
    /// american (default, listed equity and ETF options) or european (index options).
    #[serde(default)]
    style: Option<String>,
    legs: Vec<LegBody>,
    /// `expiry` (default) or a date YYYY-MM-DD for the dashed curve, today at the earliest.
    #[serde(default)]
    at: Option<String>,
    /// Absolute shift added to every leg's iv, e.g. 0.05 for five vol points. Default 0.
    #[serde(default)]
    iv_shift: f64,
    /// Volatility for the probability of profit. Default: the nearest-expiry leg closest to
    /// the money.
    #[serde(default)]
    atm_iv: Option<f64>,
    /// How volatility moves with spot: sticky_strike (default) or sticky_delta, which reads
    /// `smile`.
    #[serde(default)]
    vol_regime: Option<String>,
    /// The chain's smiles, also used for the probability of profit.
    #[serde(default)]
    smile: Vec<SmileBody>,
    /// Spot range of the payoff. Default spot x 0.7 to x 1.3, 121 points.
    #[serde(default)]
    range: Option<RangeBody>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SmileBody {
    /// YYYY-MM-DD.
    expiry: String,
    /// [moneyness (strike / spot), implied volatility as a fraction] pairs.
    points: Vec<[f64; 2]>,
}

fn smiles(list: &[SmileBody]) -> Result<Vec<Smile>, ApiError> {
    if list.len() > MAX_EXPIRIES {
        return Err(ApiError::bad_request(&format!("at most {MAX_EXPIRIES} smiles, {} were sent", list.len())));
    }
    list.iter()
        .map(|m| {
            let good = |p: &[f64; 2]| p[0].is_finite() && p[0] > 0.0 && p[1].is_finite() && p[1] > 0.0 && p[1] <= 10.0;
            if m.points.len() > MAX_SMILE_POINTS || !m.points.iter().all(good) {
                return Err(ApiError::bad_request(&format!(
                    "a smile holds at most {MAX_SMILE_POINTS} [moneyness, volatility] pairs, both above zero, volatility at most 10"
                )));
            }
            Ok(Smile::smoothed(date("expiry", &m.expiry)?, m.points.iter().map(|p| (p[0], p[1])).collect()))
        })
        .collect()
}

async fn evaluate(Json(b): Json<EvaluateBody>) -> Result<Json<Value>, ApiError> {
    if !(b.spot.is_finite() && b.spot > 0.0) {
        return Err(ApiError::bad_request("spot is the underlying's price, above zero"));
    }
    rate_and_dividend(b.rate, b.dividend)?;
    if !(-1.0..=1.0).contains(&b.iv_shift) {
        return Err(ApiError::bad_request("iv_shift is an absolute fraction between -1 and 1, e.g. 0.05 for five points"));
    }
    if b.atm_iv.is_some_and(|v| !(v.is_finite() && v > 0.0 && v <= 10.0)) {
        return Err(ApiError::bad_request("atm_iv is a fraction above 0 and at most 10, e.g. 0.2"));
    }
    let legs = legs(&b.legs)?;
    let today = today();
    for (i, l) in legs.iter().enumerate() {
        if let Some(e) = l.expiry.filter(|e| *e < today) {
            return Err(ApiError::bad_request(&format!("leg {}: it expired on {e}. Remove it or pick a later expiry", i + 1)));
        }
    }
    let nearest = legs.iter().filter_map(|l| l.expiry).min();
    let at = match b.at.as_deref().map(str::trim) {
        None | Some("") | Some("expiry") => None,
        Some(v) => {
            let d = date("at", v)?;
            if d < today {
                return Err(ApiError::bad_request(&format!("at {d} is in the past. Pick today or later, or expiry")));
            }
            Some(d)
        }
    };
    // A leg still open on a day the curve is read on is priced by the model: it needs a vol.
    for day in [nearest, at].into_iter().flatten() {
        for (i, l) in legs.iter().enumerate() {
            if l.expiry.is_some_and(|e| e > day) {
                match l.iv {
                    None => {
                        return Err(ApiError::bad_request(&format!(
                            "leg {}: it is still open on {day}, so it needs an iv (a fraction, e.g. 0.22)",
                            i + 1
                        )))
                    }
                    Some(v) if v + b.iv_shift <= 0.0 => {
                        return Err(ApiError::bad_request(&format!(
                            "leg {}: its iv plus iv_shift is {:.4}, at or below zero. Raise the shift",
                            i + 1,
                            v + b.iv_shift
                        )))
                    }
                    _ => {}
                }
            }
        }
    }
    let (lo, hi, steps) = match &b.range {
        Some(r) => (r.lo, r.hi, r.steps),
        None => (b.spot * 0.7, b.spot * 1.3, default_steps()),
    };
    if !(lo.is_finite() && hi.is_finite() && lo >= 0.0 && hi > lo) || !(2..=MAX_STEPS).contains(&steps) {
        return Err(ApiError::bad_request(&format!(
            "range runs from lo to a higher hi, both zero or more, over 2 to {MAX_STEPS} steps"
        )));
    }
    let cash = cash_dividends(&b.dividends, b.dividend)?;
    let style = match b.style.as_deref() {
        None | Some("american") => Style::American,
        Some("european") => Style::European,
        Some(other) => return Err(ApiError::bad_request(&format!("style is american or european, not {other:?}"))),
    };
    let regime = match b.vol_regime.as_deref() {
        None | Some("sticky_strike") => Regime::StickyStrike,
        Some("sticky_delta") => Regime::StickyDelta,
        Some(other) => return Err(ApiError::bad_request(&format!("vol_regime is sticky_strike or sticky_delta, not {other:?}"))),
    };
    let smiles = smiles(&b.smile)?;
    let (spot, rate, dividend, iv_shift, atm_iv) = (b.spot, b.rate, b.dividend, b.iv_shift, b.atm_iv);
    // A tree per leg per point of every curve: off the async threads.
    let out = tokio::task::spawn_blocking(move || {
        strategy::evaluate(&strategy::Input {
            spot,
            model: Model { style, rate, dividend, dividends: &cash },
            legs: &legs,
            today,
            at,
            iv_shift,
            atm_iv,
            regime,
            smiles: &smiles,
            lo,
            hi,
            steps,
        })
    })
    .await
    .map_err(|e| ApiError::internal(&e.to_string()))?;
    Ok(Json(json!(out)))
}

// ── Saved strategies ─────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct StrategyBody {
    name: String,
    underlying: String,
    /// equity | etf | index.
    asset_type: String,
    /// ibkr | massive, the provider the legs were priced from.
    #[serde(default)]
    provider: Option<String>,
    /// The legs, same shape as POST /api/options/strategy/evaluate takes them.
    legs: Vec<LegBody>,
    #[serde(default)]
    notes: String,
}

/// The checked body, ready to store.
fn strategy_row(b: &StrategyBody) -> Result<(String, String, Value), ApiError> {
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(ApiError::bad_request("name is required, at most 200 characters"));
    }
    let u = underlying(&b.underlying)?;
    asset_type(&b.asset_type)?;
    if let Some(p) = b.provider.as_deref().filter(|p| !chains::CHAIN_PROVIDERS.contains(p)) {
        return Err(ApiError::bad_request(&format!("provider {p:?} lists no option chains: ibkr, massive, alpaca or none")));
    }
    if b.notes.chars().count() > 10_000 {
        return Err(ApiError::bad_request("notes hold at most 10000 characters"));
    }
    legs(&b.legs)?;
    let stored = serde_json::to_value(&b.legs).map_err(|e| ApiError::internal(&e.to_string()))?;
    Ok((name.to_string(), u, stored))
}

async fn list_strategies(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(store::list(&state.pool).await?)))
}

async fn create_strategy(State(state): State<AppState>, Json(b): Json<StrategyBody>) -> Result<Json<Value>, ApiError> {
    let (name, u, legs) = strategy_row(&b)?;
    let input = StrategyInput {
        name: &name,
        underlying: &u,
        asset_type: &b.asset_type,
        provider: b.provider.as_deref(),
        legs: &legs,
        notes: b.notes.trim(),
    };
    Ok(Json(json!(store::create(&state.pool, &input).await?)))
}

async fn update_strategy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<StrategyBody>,
) -> Result<Json<Value>, ApiError> {
    let (name, u, legs) = strategy_row(&b)?;
    let input = StrategyInput {
        name: &name,
        underlying: &u,
        asset_type: &b.asset_type,
        provider: b.provider.as_deref(),
        legs: &legs,
        notes: b.notes.trim(),
    };
    let row = store::update(&state.pool, id, &input).await?.ok_or_else(|| ApiError::not_found("strategy"))?;
    Ok(Json(json!(row)))
}

async fn delete_strategy(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !store::delete(&state.pool, id).await? {
        return Err(ApiError::not_found("strategy"));
    }
    Ok(Json(json!({ "deleted": true })))
}

// ── Volatility ───────────────────────────────────────────────────────────────

/// The quant IV surface over a connector granted to Options.
async fn surface(State(state): State<AppState>, Json(b): Json<OptionsBody>) -> Result<Json<Value>, ApiError> {
    quant_market::surface(state, b, "options", "Options").await
}

/// The quant IV history over a connector granted to Options.
async fn iv_history(State(state): State<AppState>, Json(b): Json<IvHistoryBody>) -> Result<Json<Value>, ApiError> {
    quant_market::vol_history(state, b, "options", "Options").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_strike_alone_is_a_window_and_a_reversed_one_is_not() {
        assert!(strike_window(Some(500.0), Some(500.0)).is_ok());
        assert!(strike_window(Some(500.0), Some(600.0)).is_ok());
        assert!(strike_window(None, Some(600.0)).is_ok());
        assert!(strike_window(Some(600.0), Some(500.0)).is_err());
        assert!(strike_window(Some(-1.0), None).is_err());
        assert!(strike_window(Some(f64::NAN), None).is_err());
    }

    #[test]
    fn an_underlying_holds_only_what_a_provider_url_takes() {
        assert_eq!(underlying(" brk.b ").ok().as_deref(), Some("BRK.B"));
        assert_eq!(underlying("I:SPX").ok().as_deref(), Some("I:SPX"));
        assert_eq!(underlying("ES-USD").ok().as_deref(), Some("ES-USD"));
        for bad in ["", "SPY QQQ", "SPY/../x", "SPY?apiKey=x", "SPY&x=1", "SPY%20", "ÉTF"] {
            assert!(underlying(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_leg_without_a_price_or_qty_is_refused_naming_it() {
        let l = |qty, price| LegBody { kind: "stock".into(), strike: None, expiry: None, qty, price, iv: None };
        assert!(leg(0, &l(Some(1.0), Some(10.0))).is_ok());
        assert!(leg(1, &l(Some(1.0), None)).err().is_some_and(|e| e.message().starts_with("leg 2: price")));
        assert!(leg(2, &l(None, Some(10.0))).err().is_some_and(|e| e.message().starts_with("leg 3: qty")));
    }
}
