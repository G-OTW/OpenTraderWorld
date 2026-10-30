//! Quant derivatives tools that read a provider directly rather than a stored dataset.
//!
//! - `POST /api/quant/market/futures`    a futures product → term structure, roll, basis
//! - `POST /api/quant/market/options`    an underlying's option chain → implied-volatility surface
//! - `POST /api/quant/market/ivhistory`  an underlying → implied against realized volatility
//! - `GET  /api/quant/market/tasks/{id}` progress, then the result
//!
//! A curve or a chain is dozens of provider requests, paced by the provider (IB's rolling
//! window, Massive's five a minute on a free key), so each request starts a task and the page
//! polls it. What was fetched is kept for a few hours, keyed by what was fetched and not by
//! the analysis settings, so changing a rate or a roll rule recomputes without a request.
//! The connector has to be granted to the quant module, like any other data module.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::histdata::chains::{self, FutContract, OptContract};
use crate::quant::derivs;
use crate::{ApiError, AppState};
use otw_store::connectors as conn_store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/quant/market/futures", post(futures))
        .route("/api/quant/market/options", post(options))
        .route("/api/quant/market/ivhistory", post(iv_history))
        .route("/api/quant/market/tasks/{id}", get(task))
}

/// How long a finished task and a fetched data set are kept.
const KEEP: Duration = Duration::from_secs(6 * 3600);
/// More dated contracts than this is a window worth narrowing rather than waiting for.
const MAX_CONTRACTS: usize = 40;

// ── Tasks ────────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Task {
    status: &'static str,
    done: usize,
    total: usize,
    step: String,
    result: Option<Value>,
    error: Option<String>,
    at: Instant,
}

fn tasks() -> &'static Mutex<HashMap<Uuid, Task>> {
    static T: OnceLock<Mutex<HashMap<Uuid, Task>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_task(id: Uuid, f: impl FnOnce(&mut Task)) {
    if let Ok(mut m) = tasks().lock() {
        if let Some(t) = m.get_mut(&id) {
            f(t);
            t.at = Instant::now();
        }
    }
}

/// Start `work` in the background and hand back the id the page polls.
fn spawn<F>(work: F) -> Json<Value>
where
    F: FnOnce(Uuid) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<Value>> + Send>>,
{
    let id = Uuid::new_v4();
    if let Ok(mut m) = tasks().lock() {
        m.retain(|_, t| t.at.elapsed() < KEEP);
        m.insert(
            id,
            Task { status: "running", done: 0, total: 0, step: String::new(), result: None, error: None, at: Instant::now() },
        );
    }
    let fut = work(id);
    tokio::spawn(async move {
        let out = fut.await;
        set_task(id, |t| match out {
            Ok(v) => {
                t.status = "done";
                t.result = Some(v);
            }
            Err(e) => {
                t.status = "error";
                t.error = Some(format!("{e:#}"));
            }
        });
    });
    Json(json!({ "task": id }))
}

fn progress(id: Uuid) -> impl Fn(usize, usize, &str) + Send + Sync {
    move |done, total, step| {
        let step = step.to_string();
        set_task(id, |t| {
            t.done = done;
            t.total = total;
            t.step = step;
        })
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TaskQuery {
    /// `summary` drops the per-day rows (futures curve, IV history) and keeps everything else.
    #[serde(default)]
    view: Option<String>,
}

async fn task(Path(id): Path<Uuid>, Query(q): Query<TaskQuery>) -> Result<Json<Value>, ApiError> {
    let mut t = tasks()
        .lock()
        .ok()
        .and_then(|m| m.get(&id).cloned())
        .ok_or_else(|| ApiError::not_found("task not found or expired"))?;
    if q.view.as_deref() == Some("summary") {
        if let Some(r) = t.result.as_mut().and_then(|v| v.get_mut("result")).and_then(Value::as_object_mut) {
            if let Some(rows) = r.remove("rows") {
                r.insert("row_count".into(), json!(rows.as_array().map_or(0, Vec::len)));
                if let Some(last) = rows.as_array().and_then(|a| a.last()) {
                    r.insert("last_row".into(), last.clone());
                }
            }
        }
    }
    Ok(Json(json!({
        "status": t.status,
        "done": t.done,
        "total": t.total,
        "step": t.step,
        "result": t.result,
        "error": t.error,
    })))
}

// ── Fetched data, kept a few hours ───────────────────────────────────────────────────────

fn cache() -> &'static Mutex<HashMap<String, (Instant, Arc<dyn std::any::Any + Send + Sync>)>> {
    static C: OnceLock<Mutex<HashMap<String, (Instant, Arc<dyn std::any::Any + Send + Sync>)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached<T: Clone + Send + Sync + 'static>(key: &str) -> Option<T> {
    let m = cache().lock().ok()?;
    let (at, v) = m.get(key)?;
    (at.elapsed() < KEEP).then(|| v.downcast_ref::<T>().cloned()).flatten()
}

fn store<T: Send + Sync + 'static>(key: String, v: T) {
    if let Ok(mut m) = cache().lock() {
        m.retain(|_, (at, _)| at.elapsed() < KEEP);
        m.insert(key, (Instant::now(), Arc::new(v)));
    }
}

// ── Connector ────────────────────────────────────────────────────────────────────────────

struct Source {
    id: Uuid,
    provider: String,
    secrets: HashMap<String, String>,
}

async fn source(state: &AppState, id: Uuid) -> Result<Source, ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::forbidden("the demo does not reach market-data providers"));
    }
    let c = conn_store::get(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("connector not found"))?;
    if !c.allows("quant") {
        return Err(ApiError::bad_request(&format!(
            "connector '{}' is not granted to Quant. Grant it from the connector settings",
            c.name
        )));
    }
    if !chains::PROVIDERS.contains(&c.provider.as_str()) {
        return Err(ApiError::bad_request(&format!(
            "connector '{}' ({}) does not list futures contracts or option chains. Use an \
             Interactive Brokers or a Massive connector",
            c.name, c.provider
        )));
    }
    let secrets = conn_store::load_creds(&state.pool, &state.cipher, id).await?;
    Ok(Source { id, provider: c.provider, secrets })
}

fn jd(d: Date) -> i32 {
    d.to_julian_day()
}

fn day_pairs(v: &[(Date, f64)]) -> Vec<(i32, f64)> {
    v.iter().map(|(d, p)| (jd(*d), *p)).collect()
}

// ── Futures curve ────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct FuturesBody {
    connector_id: Uuid,
    root: String,
    #[serde(default = "default_years")]
    years: i64,
    #[serde(default = "default_ahead")]
    months_ahead: i64,
    #[serde(default)]
    spot_ticker: Option<String>,
    #[serde(default = "default_spot_type")]
    spot_asset_type: String,
    #[serde(default)]
    rate: Option<f64>,
    #[serde(default)]
    yield_rate: f64,
    #[serde(default = "default_roll_days")]
    roll_days: i32,
}
fn default_years() -> i64 {
    2
}
fn default_ahead() -> i64 {
    15
}
fn default_spot_type() -> String {
    "index".into()
}
fn default_roll_days() -> i32 {
    5
}

#[derive(Clone)]
struct FuturesData {
    series: Vec<derivs::FutSeries>,
    spot: Option<Vec<(i32, f64)>>,
    notes: Vec<String>,
}

async fn futures(State(state): State<AppState>, Json(b): Json<FuturesBody>) -> Result<Json<Value>, ApiError> {
    let root = b.root.trim().to_uppercase();
    if root.is_empty() {
        return Err(ApiError::bad_request("root is required, e.g. ES or ES@CME"));
    }
    if !(1..=5).contains(&b.years) || !(0..=36).contains(&b.months_ahead) || !(0..=60).contains(&b.roll_days) {
        return Err(ApiError::bad_request("years is 1 to 5, months_ahead 0 to 36, roll_days 0 to 60"));
    }
    if b.rate.is_some_and(|r| !(-0.1..=0.5).contains(&r)) || !(-0.1..=0.5).contains(&b.yield_rate) {
        return Err(ApiError::bad_request("rate and yield are fractions, e.g. 0.045"));
    }
    let src = source(&state, b.connector_id).await?;
    let spot = b.spot_ticker.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let key = format!("fut|{}|{root}|{}|{}|{:?}|{}", src.id, b.years, b.months_ahead, spot, b.spot_asset_type);
    Ok(spawn(move |id| {
        Box::pin(async move {
            let data = match cached::<FuturesData>(&key) {
                Some(d) => d,
                None => {
                    let d = fetch_futures(&state, &src, &root, b.years, b.months_ahead, spot.as_deref(), &b.spot_asset_type, id).await?;
                    store(key, d.clone());
                    d
                }
            };
            let curve = derivs::futures_curve(&data.series, data.spot.as_deref(), b.rate, b.yield_rate, b.roll_days)
                .ok_or_else(|| anyhow::anyhow!("no {root} contract returned a price"))?;
            Ok(json!({ "provider": src.provider, "root": root, "notes": data.notes, "result": curve }))
        })
    }))
}

#[allow(clippy::too_many_arguments)]
async fn fetch_futures(
    state: &AppState,
    src: &Source,
    root: &str,
    years: i64,
    months_ahead: i64,
    spot: Option<&str>,
    spot_type: &str,
    id: Uuid,
) -> anyhow::Result<FuturesData> {
    let p = progress(id);
    let now = OffsetDateTime::now_utc();
    let from = now - time::Duration::days(365 * years);
    let to = now.date() + time::Duration::days(31 * months_ahead);
    p(0, 1, "listing contracts");
    let list: Vec<FutContract> =
        chains::futures_contracts(&src.provider, &state.http, &src.secrets, root, from.date(), to).await?;
    if list.len() > MAX_CONTRACTS {
        anyhow::bail!(
            "{root} has {} contracts in that window. Shorten the history or the months ahead (at most {MAX_CONTRACTS})",
            list.len()
        );
    }
    let total = list.len() + 1 + usize::from(spot.is_some());
    let mut series = Vec::new();
    let mut notes = Vec::new();
    for (i, c) in list.iter().enumerate() {
        p(i + 1, total, &c.local);
        // A contract trades for a year or two before it expires; nothing older is needed.
        let start = from.max(OffsetDateTime::new_utc(c.last_trade, time::Time::MIDNIGHT) - time::Duration::days(450));
        let end = now.min(OffsetDateTime::new_utc(c.last_trade, time::Time::MIDNIGHT) + time::Duration::days(1));
        if start >= end {
            continue;
        }
        match chains::daily_closes(&src.provider, &state.http, &src.secrets, &c.ticker, "future", start, end).await {
            Ok(closes) => series.push(derivs::FutSeries { ticker: c.local.clone(), last_trade: jd(c.last_trade), closes: day_pairs(&closes) }),
            Err(e) => notes.push(format!("{}: {e:#}", c.local)),
        }
    }
    let spot = match spot {
        Some(t) => {
            p(total - 1, total, t);
            Some(day_pairs(&chains::daily_closes(&src.provider, &state.http, &src.secrets, t, spot_type, from, now).await?))
        }
        None => None,
    };
    p(total, total, "done");
    Ok(FuturesData { series, spot, notes })
}

// ── Option chain → IV surface ────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct OptionsBody {
    connector_id: Uuid,
    underlying: String,
    #[serde(default = "default_underlying_type")]
    asset_type: String,
    #[serde(default = "default_expiries")]
    expiries: usize,
    #[serde(default = "default_strikes")]
    strikes: usize,
    #[serde(default = "default_min_days")]
    min_days: i64,
    #[serde(default = "default_max_days")]
    max_days: i64,
    #[serde(default = "default_rate")]
    rate: f64,
    #[serde(default)]
    dividend: f64,
}
fn default_underlying_type() -> String {
    "etf".into()
}
fn default_expiries() -> usize {
    4
}
fn default_strikes() -> usize {
    5
}
fn default_min_days() -> i64 {
    7
}
fn default_max_days() -> i64 {
    120
}
fn default_rate() -> f64 {
    0.04
}

#[derive(Clone)]
struct ChainData {
    spot: f64,
    as_of: Date,
    stamp: String,
    quotes: Vec<derivs::OptQuote>,
    stale: usize,
    missing: usize,
}

/// Expiries spread geometrically between the shortest and the longest wanted, each the
/// listed one nearest its target.
fn pick_expiries(listed: &[Date], today: Date, n: usize, min_days: i64, max_days: i64) -> Vec<Date> {
    let lo = (min_days.max(1)) as f64;
    let hi = (max_days as f64).max(lo);
    let mut out: Vec<Date> = Vec::new();
    for i in 0..n {
        let target = if n == 1 { lo } else { lo * (hi / lo).powf(i as f64 / (n - 1) as f64) };
        let best = listed
            .iter()
            .filter(|e| {
                let d = (**e - today).whole_days();
                d >= min_days && d <= max_days && !out.contains(e)
            })
            .min_by(|a, b| {
                let da = ((**a - today).whole_days() as f64 - target).abs();
                let db = ((**b - today).whole_days() as f64 - target).abs();
                da.total_cmp(&db)
            });
        if let Some(e) = best {
            out.push(*e);
        }
    }
    out.sort();
    out
}

/// `n` out-of-the-money strikes on each side of the spot, spread over about 1.6 standard
/// deviations of a 25% volatility to that expiry, so the 25-delta points fall inside them.
fn pick_strikes(listed: &[f64], spot: f64, days: i64, n: usize) -> Vec<(f64, bool)> {
    let width = (1.6 * 0.25 * (days.max(1) as f64 / 365.0).sqrt()).max(0.02);
    let mut out: Vec<(f64, bool)> = Vec::new();
    for (call, sign) in [(false, -1.0), (true, 1.0)] {
        for i in 0..n {
            let m = sign * width * (i as f64 + 0.5) / n as f64;
            let target = spot * m.exp();
            let side: Vec<f64> = listed.iter().copied().filter(|k| if call { *k >= spot } else { *k < spot }).collect();
            if let Some(k) = side.iter().copied().min_by(|a, b| (a - target).abs().total_cmp(&(b - target).abs())) {
                if !out.iter().any(|(x, c)| *c == call && (x - k).abs() < 1e-9) {
                    out.push((k, call));
                }
            }
        }
    }
    out
}

async fn options(State(state): State<AppState>, Json(b): Json<OptionsBody>) -> Result<Json<Value>, ApiError> {
    let underlying = b.underlying.trim().to_uppercase();
    if underlying.is_empty() {
        return Err(ApiError::bad_request("underlying is required, e.g. SPY"));
    }
    if !(1..=8).contains(&b.expiries) || !(2..=12).contains(&b.strikes) {
        return Err(ApiError::bad_request("expiries is 1 to 8, strikes per side 2 to 12"));
    }
    if !(1..=730).contains(&b.max_days) || !(0..=b.max_days).contains(&b.min_days) {
        return Err(ApiError::bad_request("days to expiry run from 0 to 730, the minimum below the maximum"));
    }
    if !(-0.1..=0.5).contains(&b.rate) || !(-0.1..=0.5).contains(&b.dividend) {
        return Err(ApiError::bad_request("rate and dividend are fractions, e.g. 0.04"));
    }
    if !["equity", "etf", "index"].contains(&b.asset_type.as_str()) {
        return Err(ApiError::bad_request("the underlying is an equity, an ETF or an index"));
    }
    let src = source(&state, b.connector_id).await?;
    let key = format!("opt|{}|{underlying}|{}|{}|{}|{}|{}", src.id, b.asset_type, b.expiries, b.strikes, b.min_days, b.max_days);
    Ok(spawn(move |id| {
        Box::pin(async move {
            let data = match cached::<ChainData>(&key) {
                Some(d) => d,
                None => {
                    let d = fetch_chain(&state, &src, &underlying, &b, id).await?;
                    store(key, d.clone());
                    d
                }
            };
            let surface = derivs::iv_surface(data.spot, jd(data.as_of), &data.quotes, b.rate, b.dividend);
            Ok(json!({
                "provider": src.provider,
                "underlying": underlying,
                "stamp": data.stamp,
                "stale": data.stale,
                "missing": data.missing,
                "result": surface,
            }))
        })
    }))
}

async fn fetch_chain(state: &AppState, src: &Source, underlying: &str, b: &OptionsBody, id: Uuid) -> anyhow::Result<ChainData> {
    let p = progress(id);
    let now = OffsetDateTime::now_utc();
    p(0, 1, underlying);
    // The underlying at the instant the options are priced: IB's last hourly close, whose
    // hour the option midpoints are then read at; Massive's last session close.
    let (spot, stamp, hourly) = if src.provider == "ibkr" {
        let h = chains::ib_hourly(&src.secrets, underlying, &b.asset_type).await?;
        let (ts, px) = h.last().map(|(t, c)| (*t, *c)).ok_or_else(|| anyhow::anyhow!("no recent {underlying} price"))?;
        (px, ts, true)
    } else {
        let d = chains::daily_closes(&src.provider, &state.http, &src.secrets, underlying, &b.asset_type, now - time::Duration::days(10), now).await?;
        let (day, px) = *d.last().ok_or_else(|| anyhow::anyhow!("no recent {underlying} price"))?;
        (px, OffsetDateTime::new_utc(day, time::Time::MIDNIGHT), false)
    };
    let as_of = stamp.date();
    p(0, 1, "listing the chain");
    let layout = chains::option_layout(
        &src.provider,
        &state.http,
        &src.secrets,
        underlying,
        &b.asset_type,
        as_of + time::Duration::days(b.min_days.max(1)),
        as_of + time::Duration::days(b.max_days),
        spot * 0.6,
        spot * 1.4,
    )
    .await?;
    let exps = pick_expiries(&layout.expiries, as_of, b.expiries, b.min_days, b.max_days);
    let mut wanted: Vec<OptContract> = Vec::new();
    for e in &exps {
        let listed = if src.provider == "ibkr" {
            p(0, 1, &format!("strikes on {e}"));
            chains::ib_strikes(&src.secrets, underlying, &layout.trading_class, *e).await?
        } else {
            layout.strikes.get(e).cloned().unwrap_or_default()
        };
        for (k, call) in pick_strikes(&listed, spot, (*e - as_of).whole_days(), b.strikes) {
            wanted.push(OptContract { ticker: chains::occ(underlying, *e, call, k), expiry: *e, strike: k, call });
        }
    }
    if wanted.is_empty() {
        anyhow::bail!("no {underlying} strikes around {spot:.2} on the chosen expiries");
    }
    let total = wanted.len();
    let (mut quotes, mut stale, mut missing) = (Vec::new(), 0, 0);
    for (i, o) in wanted.iter().enumerate() {
        p(i, total, &o.ticker);
        let prices = chains::option_price(&src.provider, &state.http, &src.secrets, o, underlying, &layout.trading_class).await?;
        if prices.is_empty() {
            missing += 1;
            continue;
        }
        // Only a price taken when the underlying was: the same hour at IB, the same session
        // at Massive. Anything else is a stale print against today's spot.
        let hit = if hourly {
            prices.iter().find(|(_, t)| *t == stamp)
        } else {
            prices.iter().find(|(_, t)| chains::trading_date(*t) == as_of)
        };
        match hit {
            Some((px, _)) => quotes.push(derivs::OptQuote {
                ticker: o.ticker.clone(),
                expiry: jd(o.expiry),
                strike: o.strike,
                call: o.call,
                price: *px,
            }),
            None => stale += 1,
        }
    }
    p(total, total, "done");
    if quotes.is_empty() {
        anyhow::bail!(
            "none of the {total} {underlying} options had a price at the same time as the underlying \
             ({missing} without any price, {stale} priced at another time)"
        );
    }
    Ok(ChainData {
        spot,
        as_of,
        stamp: stamp.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
        quotes,
        stale,
        missing,
    })
}

// ── Implied against realized ─────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct IvHistoryBody {
    connector_id: Uuid,
    underlying: String,
    #[serde(default = "default_underlying_type")]
    asset_type: String,
    #[serde(default = "default_iv_years")]
    years: i32,
    #[serde(default = "default_window")]
    window: usize,
    #[serde(default = "default_lookback")]
    lookback: usize,
}
fn default_iv_years() -> i32 {
    3
}
fn default_window() -> usize {
    21
}
fn default_lookback() -> usize {
    252
}

async fn iv_history(State(state): State<AppState>, Json(b): Json<IvHistoryBody>) -> Result<Json<Value>, ApiError> {
    let underlying = b.underlying.trim().to_uppercase();
    if underlying.is_empty() {
        return Err(ApiError::bad_request("underlying is required, e.g. SPY"));
    }
    if !(1..=10).contains(&b.years) || !(5..=126).contains(&b.window) || !(20..=756).contains(&b.lookback) {
        return Err(ApiError::bad_request("years is 1 to 10, window 5 to 126 days, lookback 20 to 756 days"));
    }
    let src = source(&state, b.connector_id).await?;
    if src.provider != "ibkr" {
        return Err(ApiError::bad_request(
            "implied volatility history comes from Interactive Brokers. Pick an IB connector",
        ));
    }
    let key = format!("ivh|{}|{underlying}|{}|{}", src.id, b.asset_type, b.years);
    Ok(spawn(move |id| {
        Box::pin(async move {
            let h = match cached::<Arc<chains::VolHistory>>(&key) {
                Some(h) => h,
                None => {
                    let p = progress(id);
                    let h = Arc::new(chains::ib_vol_history(&src.secrets, &underlying, &b.asset_type, b.years, &p).await?);
                    store(key, h.clone());
                    h
                }
            };
            let closes: HashMap<Date, f64> = h.closes.iter().copied().collect();
            let hv: HashMap<Date, f64> = h.hv.iter().copied().collect();
            let mut days = Vec::new();
            let (mut iv, mut px, mut hvs) = (Vec::new(), Vec::new(), Vec::new());
            for (d, v) in &h.iv {
                if let Some(c) = closes.get(d) {
                    days.push(jd(*d));
                    iv.push(*v);
                    px.push(*c);
                    hvs.push(hv.get(d).copied().unwrap_or(f64::NAN));
                }
            }
            let out = derivs::iv_history(&days, &iv, &px, Some(&hvs), b.window, b.lookback)
                .ok_or_else(|| anyhow::anyhow!("too few days with both an implied volatility and a close"))?;
            Ok(json!({ "provider": src.provider, "underlying": underlying, "result": out }))
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn expiries_spread_between_the_bounds_without_repeats() {
        let today = date!(2026 - 09 - 30);
        let listed: Vec<Date> = (1..200).map(|d| today + time::Duration::days(d)).collect();
        let got = pick_expiries(&listed, today, 4, 7, 120);
        let days: Vec<i64> = got.iter().map(|e| (*e - today).whole_days()).collect();
        assert_eq!(days, vec![7, 18, 47, 120]);
        // Fewer listed than asked: each one at most once.
        let few = [today + time::Duration::days(30)];
        assert_eq!(pick_expiries(&few, today, 4, 7, 120).len(), 1);
    }

    #[test]
    fn strikes_sit_out_of_the_money_on_their_own_side() {
        let listed: Vec<f64> = (500..=900).map(|k| k as f64).collect();
        let got = pick_strikes(&listed, 700.0, 30, 4);
        assert_eq!(got.len(), 8);
        assert!(got.iter().all(|(k, call)| if *call { *k >= 700.0 } else { *k < 700.0 }));
        // The widest put reaches about 1.6 σ of a 25% volatility over 30 days.
        let lo = got.iter().filter(|(_, c)| !c).map(|(k, _)| *k).fold(f64::MAX, f64::min);
        let want = 700.0 * (-1.6 * 0.25 * (30.0f64 / 365.0).sqrt() * 3.5 / 4.0).exp();
        assert!((lo - want).abs() <= 1.0, "{lo} {want}");
    }
}
