//! Quote computation for the Watchlists module.
//!
//! Reuses the Portfolio Tracker's provider scheme (CoinGecko coin id / Yahoo ticker, all USD)
//! but adds what a watchlist needs beyond a spot: day/3d/7d/30d changes and a 30-day sparkline.
//! Those derive from a rolling daily-close series that is fetched, computed against the live
//! spot, and cached inside the item's `quote` JSONB — never stored as rows:
//!   - **stock/etf** → one Yahoo `v8/finance/chart?interval=1d&range=1mo` call per item; it
//!     carries the live price, the daily closes AND the exchange label, so spot + history cost
//!     a single request.
//!   - **crypto** → spot for the whole list in one batched CoinGecko `simple/price` call;
//!     the daily series comes from `coins/{id}/market_chart` and is refetched at most every
//!     `HISTORY_TTL` (the cached series is reused in between), keeping fast refresh cheap.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use sqlx::PgPool;
use time::OffsetDateTime;

use crate::histdata::{Capability, Connector};
use crate::portfolios::prices;
use otw_store::watchlists as store;

const COINGECKO: &str = "https://api.coingecko.com/api/v3";
const YAHOO: &str = "https://query1.finance.yahoo.com";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a cached crypto daily series stays valid before market_chart is refetched.
/// Daily closes only move once a day; 6h keeps even a 1-min refresh at 4 history calls/day.
const HISTORY_TTL: i64 = 6 * 3600;

/// Pause between consecutive remote per-item calls inside one refresh, so a big list
/// doesn't burst the free providers.
const INTER_CALL_PAUSE: Duration = Duration::from_millis(250);

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent("OpenTraderWorld/watchlists")
        .build()
        .map_err(|e| anyhow!("building http client: {e}"))
}

// ── Provider fetches ──────────────────────────────────────────────────────────

/// One Yahoo chart call: (daily closes ascending as (unix_sec, close), live price, exchange,
/// the extra fields the chart meta carries). The keyless chart endpoint serves the 52-week
/// range and the session volume; it serves no market cap, P/E or dividend yield, so those
/// stay null rather than being guessed.
async fn yahoo_chart(ticker: &str) -> Result<(Vec<(i64, f64)>, Option<f64>, Option<String>, Extras)> {
    // 2mo, not 1mo: the 30d change needs a close at-or-before now−30d, and a 1-month
    // window of *trading* days starts exactly at the cutoff and usually misses it.
    let url = format!(
        "{YAHOO}/v8/finance/chart/{}?interval=1d&range=2mo",
        urlencoding(ticker)
    );
    let body: Value = crate::rate::send("yahoo", client()?.get(&url))
        .await
        .context("yahoo chart request")?
        .error_for_status()
        .context("yahoo chart status")?
        .json()
        .await
        .context("yahoo chart decode")?;
    let res = body
        .pointer("/chart/result/0")
        .ok_or_else(|| anyhow!("yahoo chart: empty result"))?;
    let live = res.pointer("/meta/regularMarketPrice").and_then(Value::as_f64);
    let meta_num = |k: &str| res.pointer(&format!("/meta/{k}")).and_then(Value::as_f64).filter(|v| v.is_finite());
    let extras = Extras {
        high_52w: meta_num("fiftyTwoWeekHigh"),
        low_52w: meta_num("fiftyTwoWeekLow"),
        volume: meta_num("regularMarketVolume"),
        currency: res.pointer("/meta/currency").and_then(Value::as_str).map(str::to_string),
        ..Extras::default()
    };
    let exchange = res
        .pointer("/meta/fullExchangeName")
        .or_else(|| res.pointer("/meta/exchangeName"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let ts = res.pointer("/timestamp").and_then(Value::as_array).cloned().unwrap_or_default();
    let closes = res
        .pointer("/indicators/quote/0/close")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut series = Vec::with_capacity(ts.len());
    for (t, c) in ts.iter().zip(closes.iter()) {
        if let (Some(t), Some(c)) = (t.as_i64(), c.as_f64()) {
            series.push((t, c));
        }
    }
    Ok((series, live, exchange, extras))
}

/// CoinGecko rolling daily series for one coin, ascending (unix_sec, close). 35 days,
/// not 30: the 30d change needs a close at-or-before now−30d and the window's first
/// midnight point lands just inside the cutoff.
async fn coingecko_history(id: &str) -> Result<Vec<(i64, f64)>> {
    let url = format!(
        "{COINGECKO}/coins/{}/market_chart?vs_currency=usd&days=35&interval=daily",
        urlencoding(id)
    );
    let body: Value = crate::rate::send("coingecko", client()?.get(&url))
        .await
        .context("coingecko market_chart request")?
        .error_for_status()
        .context("coingecko market_chart status")?
        .json()
        .await
        .context("coingecko market_chart decode")?;
    let prices = body
        .get("prices")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut series = Vec::with_capacity(prices.len());
    for p in prices {
        if let (Some(ms), Some(v)) = (
            p.get(0).and_then(Value::as_i64),
            p.get(1).and_then(Value::as_f64),
        ) {
            series.push((ms / 1000, v));
        }
    }
    Ok(series)
}

/// One batched CoinGecko `coins/markets` call for a list's coins: the spot plus the market
/// cap and 24h volume it carries, keyed by coin id. Same single request the old
/// `simple/price` spot cost, with the fields the column picker shows.
async fn coingecko_markets(ids: &[String]) -> Result<HashMap<String, (f64, Extras)>> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let url = format!(
        "{COINGECKO}/coins/markets?vs_currency=usd&ids={}&per_page=250",
        urlencoding(&ids.join(","))
    );
    let body: Value = crate::rate::send("coingecko", client()?.get(&url))
        .await
        .context("coingecko markets request")?
        .error_for_status()
        .context("coingecko markets status")?
        .json()
        .await
        .context("coingecko markets decode")?;
    for c in body.as_array().cloned().unwrap_or_default() {
        let (Some(id), Some(price)) = (
            c.get("id").and_then(Value::as_str),
            c.get("current_price").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let num = |k: &str| c.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
        out.insert(
            id.to_string(),
            (
                price,
                Extras {
                    mcap: num("market_cap"),
                    volume: num("total_volume"),
                    currency: Some("USD".into()),
                    ..Extras::default()
                },
            ),
        );
    }
    Ok(out)
}

/// Quote fields beyond the price, each only when the source actually serves it. A field
/// that stays `None` is stored as null and shown as "n/a", never as 0.
#[derive(Debug, Clone, Default)]
pub struct Extras {
    pub mcap: Option<f64>,
    pub volume: Option<f64>,
    pub high_52w: Option<f64>,
    pub low_52w: Option<f64>,
    /// The currency the price is quoted in, when the source says.
    pub currency: Option<String>,
}

/// Wilder's RSI over `closes` (ascending), `period` long. `None` until there are
/// `period + 1` closes: an RSI on less history is a different number.
pub fn rsi(closes: &[f64], period: usize) -> Option<f64> {
    if period == 0 || closes.len() < period + 1 {
        return None;
    }
    let mut gain = 0.0;
    let mut loss = 0.0;
    for w in closes[..=period].windows(2) {
        let d = w[1] - w[0];
        if d > 0.0 { gain += d } else { loss -= d }
    }
    let n = period as f64;
    let (mut avg_g, mut avg_l) = (gain / n, loss / n);
    for w in closes[period..].windows(2) {
        let d = w[1] - w[0];
        avg_g = (avg_g * (n - 1.0) + d.max(0.0)) / n;
        avg_l = (avg_l * (n - 1.0) + (-d).max(0.0)) / n;
    }
    if avg_l == 0.0 {
        return Some(if avg_g == 0.0 { 50.0 } else { 100.0 });
    }
    Some(100.0 - 100.0 / (1.0 + avg_g / avg_l))
}

// ── Quote computation ─────────────────────────────────────────────────────────

/// Percent change of `live` vs `reference`.
fn pct(live: f64, reference: f64) -> Option<f64> {
    (reference != 0.0).then(|| (live - reference) / reference * 100.0)
}

/// Collapse a raw series to one close per UTC day (the day's last value), ascending.
fn daily_closes(series: &[(i64, f64)]) -> Vec<(i64, f64)> {
    let mut out: Vec<(i64, f64)> = Vec::with_capacity(series.len());
    let mut sorted: Vec<(i64, f64)> = series.to_vec();
    sorted.sort_by_key(|(t, _)| *t);
    for (t, v) in sorted {
        match out.last() {
            Some((last_t, _)) if last_t / 86_400 == t / 86_400 => {
                *out.last_mut().unwrap() = (t, v);
            }
            _ => out.push((t, v)),
        }
    }
    out
}

/// Build the cached quote JSON from a daily series and the live spot.
///
/// Every horizon compares the live spot against the last completed close at or before
/// now − N days. For 24h that lands on the previous session's close while the market is
/// open (the usual intraday "day change") and on the close before the just-finished
/// session after hours — so a closed market shows the last session's move, not 0%.
/// `spark` is the completed daily closes with the live spot appended.
fn compute_quote(history: &[(i64, f64)], live: f64, now_sec: i64, extras: &Extras) -> Value {
    let daily = daily_closes(history);
    let today = now_sec / 86_400;
    // Completed sessions only: the provider's point for the current UTC day is a partial
    // close the live spot supersedes.
    let completed: Vec<(i64, f64)> = daily.iter().copied().filter(|(t, _)| t / 86_400 < today).collect();

    let ref_at = |days: i64| -> Option<f64> {
        let cutoff = now_sec - days * 86_400;
        completed.iter().rev().find(|(t, _)| *t <= cutoff).map(|(_, v)| *v)
    };

    let mut spark: Vec<f64> = completed.iter().map(|(_, v)| *v).collect();
    spark.push(live);
    // RSI(14) on the same completed closes plus the live spot, before the spark is capped.
    let rsi_14 = rsi(&spark, 14);
    // The card sparkline wants ~a month of shape, not unbounded growth.
    if spark.len() > 31 {
        let cut = spark.len() - 31;
        spark.drain(..cut);
    }

    json!({
        "price_usd": live,
        "change_24h": ref_at(1).and_then(|r| pct(live, r)),
        "change_3d": ref_at(3).and_then(|r| pct(live, r)),
        "change_7d": ref_at(7).and_then(|r| pct(live, r)),
        "change_30d": ref_at(30).and_then(|r| pct(live, r)),
        "spark": spark,
        "history": history.iter().map(|(t, v)| json!([t, v])).collect::<Vec<_>>(),
        "history_at": now_sec,
        "rsi_14": rsi_14,
        "mcap": extras.mcap,
        "volume": extras.volume,
        "high_52w": extras.high_52w,
        "low_52w": extras.low_52w,
        "range_src": extras.high_52w.map(|_| "quote"),
        "currency": extras.currency,
        // No current source serves these; kept explicit so the table says "n/a".
        "pe": null,
        "div_yield": null,
    })
}

/// Read the cached daily series back out of an item's quote, if still within `HISTORY_TTL`.
fn cached_history(item: &store::Item, now_sec: i64) -> Option<Vec<(i64, f64)>> {
    let at = item.quote.get("history_at").and_then(Value::as_i64)?;
    if now_sec - at >= HISTORY_TTL {
        return None;
    }
    let raw = item.quote.get("history")?.as_array()?;
    let mut series = Vec::with_capacity(raw.len());
    for p in raw {
        series.push((p.get(0)?.as_i64()?, p.get(1)?.as_f64()?));
    }
    (!series.is_empty()).then_some(series)
}

// ── Refresh ───────────────────────────────────────────────────────────────────

/// Re-quote a single item. `crypto_spot` is a prefetched CoinGecko batch when refreshing a
/// whole list; pass `None` to fetch the single spot here (the add-item path). Returns whether
/// a remote per-item call was made (so callers can pace).
async fn quote_item(
    pool: &PgPool,
    item: &store::Item,
    crypto_spot: Option<&HashMap<String, (f64, Extras)>>,
) -> Result<bool> {
    let now_sec = OffsetDateTime::now_utc().unix_timestamp();
    match item.provider.as_str() {
        "yahoo" => {
            let (series, live, exchange, extras) = yahoo_chart(&item.provider_id).await?;
            let live = live
                .or_else(|| series.last().map(|(_, v)| *v))
                .ok_or_else(|| anyhow!("no price for {}", item.provider_id))?;
            let mut quote = compute_quote(&series, live, now_sec, &extras);
            fill_stored_range(pool, item, chart_coords(item, None).as_ref(), &mut quote, live, now_sec).await;
            store::set_item_quote(pool, item.id, &quote, exchange.as_deref()).await?;
            Ok(true)
        }
        _ => {
            let spot = match crypto_spot {
                Some(batch) => batch.get(&item.provider_id).cloned(),
                None => match coingecko_markets(&[item.provider_id.clone()]).await {
                    Ok(m) => m.get(&item.provider_id).cloned(),
                    // The markets call failing is no reason to show no price at all.
                    Err(_) => prices::crypto_prices(&[item.provider_id.clone()])
                        .await?
                        .get(&item.provider_id)
                        .map(|p| (*p, Extras::default())),
                },
            };
            let (history, fetched) = match cached_history(item, now_sec) {
                Some(h) => (h, false),
                None => (coingecko_history(&item.provider_id).await?, true),
            };
            let (live, extras) = match spot {
                Some((p, e)) => (p, e),
                None => (
                    history
                        .last()
                        .map(|(_, v)| *v)
                        .ok_or_else(|| anyhow!("no price for {}", item.provider_id))?,
                    Extras::default(),
                ),
            };
            let mut quote = compute_quote(&history, live, now_sec, &extras);
            fill_stored_range(pool, item, chart_coords(item, None).as_ref(), &mut quote, live, now_sec).await;
            store::set_item_quote(pool, item.id, &quote, None).await?;
            Ok(fetched)
        }
    }
}

// ── Chart coordinates and stored candles ──────────────────────────────────────

/// Where an item's candles live in the data broker: the provider, its asset type and the
/// ticker in that provider's spelling. Derived from the item's resolved quote source, the
/// same way the quote itself is fetched: a connector source uses its ticker, the default
/// Yahoo path uses the exact Yahoo ticker. A crypto item on the default CoinGecko path has
/// no such coordinates (a coin id is not a market), so it gets none rather than a guess.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct Coords {
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
}

/// `connector_provider` = the provider of the item's resolved connector, when it resolves
/// to one that serves its asset class.
pub fn chart_coords(item: &store::Item, connector_provider: Option<&str>) -> Option<Coords> {
    let asset_type = histdata_asset_type(&item.asset_class).to_string();
    match connector_provider {
        Some(p) => Some(Coords {
            provider: p.to_string(),
            asset_type,
            ticker: match item.quote_ticker.trim() {
                "" => default_quote_ticker(p, &item.asset_class, &item.symbol),
                t => t.to_string(),
            },
        }),
        None if item.provider == "yahoo" => Some(Coords {
            provider: "yahoo".into(),
            asset_type,
            ticker: item.provider_id.clone(),
        }),
        None => None,
    }
}

/// The provider of each item's resolved connector (items on the default path are absent).
/// Reads the connector rows once per id; a connector that is gone or does not serve the
/// item's class leaves the item on the default path, as the refresh does.
pub async fn resolved_providers(
    pool: &PgPool,
    wl: &store::Watchlist,
    items: &[store::Item],
) -> HashMap<uuid::Uuid, String> {
    let mut providers: HashMap<uuid::Uuid, Option<(String, &'static Capability)>> = HashMap::new();
    let mut out = HashMap::new();
    for item in items {
        let Source::Connector(cid) = resolve_source(wl, item).0 else { continue };
        if !providers.contains_key(&cid) {
            let row = otw_store::connectors::get(pool, cid).await.ok().flatten();
            let entry = row.and_then(|r| {
                crate::histdata::connector_for(&r.provider).ok().map(|c| (r.provider.clone(), c.capability()))
            });
            providers.insert(cid, entry);
        }
        if let Some(Some((p, cap))) = providers.get(&cid) {
            if supports(cap, &item.asset_class) {
                out.insert(item.id, p.clone());
            }
        }
    }
    out
}

/// The stored daily dataset for these coordinates. An ETF may be filed as equity by one
/// provider and as etf by another; the ticker itself is never altered.
async fn stored_daily(pool: &PgPool, c: &Coords) -> Option<otw_store::histdata::Dataset> {
    let mut types = vec![c.asset_type.as_str()];
    match c.asset_type.as_str() {
        "etf" => types.push("equity"),
        "equity" => types.push("etf"),
        _ => {}
    }
    for at in types {
        if let Ok(Some(d)) =
            otw_store::histdata::find_dataset_any(pool, at, &c.ticker, "1d", Some(&c.provider)).await
        {
            return Some(d);
        }
    }
    None
}

/// Stored daily bars of the last `days`, ascending, as (unix_sec, high, low, close).
pub async fn stored_bars(pool: &PgPool, c: &Coords, days: i64) -> Option<Vec<(i64, f64, f64, f64)>> {
    let d = stored_daily(pool, c).await?;
    let from = OffsetDateTime::now_utc() - time::Duration::days(days);
    let bars = otw_store::histdata::read_bars(pool, d.id, Some(from), None, 1000).await.ok()?;
    Some(bars.iter().map(|b| (b.ts.unix_timestamp(), b.high, b.low, b.close)).collect())
}

/// The 52-week high and low out of stored daily bars, only when they reach back a full
/// year (a partial year would draw a range that is not the 52-week one). The live price
/// counts, so a new high today is inside the range.
pub fn range_from_bars(bars: &[(i64, f64, f64, f64)], live: f64, now_sec: i64) -> Option<(f64, f64)> {
    let first = bars.first()?.0;
    if first > now_sec - 358 * 86_400 {
        return None;
    }
    let hi = bars.iter().map(|b| b.1).fold(live, f64::max);
    let lo = bars.iter().map(|b| b.2).fold(live, f64::min);
    Some((hi, lo))
}

/// When the source served no 52-week range, fall back to a year of stored candles. The
/// stored answer is reused for `HISTORY_TTL` (daily bars do not move faster).
async fn fill_stored_range(
    pool: &PgPool,
    item: &store::Item,
    coords: Option<&Coords>,
    quote: &mut Value,
    live: f64,
    now_sec: i64,
) {
    if quote.get("high_52w").is_some_and(|v| !v.is_null()) {
        return;
    }
    let prev = &item.quote;
    let fresh = prev.get("range_src").and_then(Value::as_str) == Some("stored")
        && prev.get("range_at").and_then(Value::as_i64).is_some_and(|t| now_sec - t < HISTORY_TTL);
    let range = if fresh {
        prev.get("high_52w")
            .and_then(Value::as_f64)
            .zip(prev.get("low_52w").and_then(Value::as_f64))
            .map(|(h, l)| (h.max(live), l.min(live)))
    } else {
        match coords {
            Some(c) => stored_bars(pool, c, 366)
                .await
                .and_then(|b| range_from_bars(&b, live, now_sec)),
            None => None,
        }
    };
    if let Some((hi, lo)) = range {
        quote["high_52w"] = json!(hi);
        quote["low_52w"] = json!(lo);
        quote["range_src"] = json!("stored");
        quote["range_at"] = if fresh { prev["range_at"].clone() } else { json!(now_sec) };
    }
}

/// The quote source an item resolves to, plus whether the user pinned it explicitly
/// (an explicit incompatible choice errors; an inherited one falls back to Auto).
enum Source {
    Auto,
    Connector(uuid::Uuid),
}

fn resolve_source(wl: &store::Watchlist, item: &store::Item) -> (Source, bool) {
    match item.quote_source.trim() {
        "" => (
            wl.connector_id.map(Source::Connector).unwrap_or(Source::Auto),
            false,
        ),
        "auto" => (Source::Auto, true),
        s => match uuid::Uuid::parse_str(s) {
            Ok(id) => (Source::Connector(id), true),
            Err(_) => (Source::Auto, false),
        },
    }
}

fn supports(cap: &Capability, asset_class: &str) -> bool {
    cap.asset_types.contains(&histdata_asset_type(asset_class))
}

/// Quote one freshly added/edited item right away so it never sits price-less in the UI.
/// Routes through the item's resolved source; stores the failure reason on the item.
pub async fn refresh_item(pool: &PgPool, wl: &store::Watchlist, item: &store::Item) -> Result<()> {
    let (source, explicit) = resolve_source(wl, item);
    let res = match source {
        Source::Auto => quote_item(pool, item, None).await.map(|_| ()),
        Source::Connector(id) => match ConnectorCx::open(pool, id).await {
            Ok(cx) if supports(cx.cap, &item.asset_class) => {
                quote_via_connector(pool, &cx, item).await
            }
            // The list default doesn't cover this asset class → default providers.
            Ok(_) if !explicit => quote_item(pool, item, None).await.map(|_| ()),
            Ok(cx) => Err(anyhow!(
                "{} does not support {}",
                cx.cap.label,
                histdata_asset_type(&item.asset_class)
            )),
            Err(e) => Err(e),
        },
    };
    if let Err(e) = &res {
        let _ = store::set_item_error(pool, item.id, &format!("{e:#}")).await;
    }
    res
}

/// Refresh every item in a watchlist, then stamp `refreshed_at`. Items are partitioned by
/// their resolved source: Auto items keep the batched default path, connector items go
/// through their own connector (opened once each). Individual item failures are logged and
/// annotated on the item (previous quote stays); the list still stamps `refreshed_at` so
/// one bad symbol can't wedge it into a hot retry loop.
pub async fn refresh_watchlist(pool: &PgPool, wl: &store::Watchlist) -> Result<()> {
    let items = store::list_items(pool, wl.id).await?;

    // Resolve every item first; open each referenced connector once. An unopenable
    // connector (deleted creds, provider gone) errors its items, not the whole list.
    let mut ctxs: HashMap<uuid::Uuid, Option<ConnectorCx>> = HashMap::new();
    let mut auto_items: Vec<&store::Item> = Vec::new();
    let mut conn_items: Vec<(&store::Item, uuid::Uuid)> = Vec::new();
    for item in &items {
        let (source, explicit) = resolve_source(wl, item);
        let cid = match source {
            Source::Auto => {
                auto_items.push(item);
                continue;
            }
            Source::Connector(id) => id,
        };
        if !ctxs.contains_key(&cid) {
            let cx = match ConnectorCx::open(pool, cid).await {
                Ok(cx) => Some(cx),
                Err(e) => {
                    tracing::warn!("watchlist {} connector {cid} unavailable: {e:#}", wl.id);
                    None
                }
            };
            ctxs.insert(cid, cx);
        }
        match ctxs.get(&cid).and_then(|c| c.as_ref()) {
            Some(cx) if supports(cx.cap, &item.asset_class) => conn_items.push((item, cid)),
            Some(_) if !explicit => auto_items.push(item),
            Some(cx) => {
                let _ = store::set_item_error(
                    pool,
                    item.id,
                    &format!(
                        "{} does not support {}",
                        cx.cap.label,
                        histdata_asset_type(&item.asset_class)
                    ),
                )
                .await;
            }
            None => {
                let _ = store::set_item_error(pool, item.id, "quote connector unavailable").await;
            }
        }
    }

    // Auto path — one batched spot call covers every crypto item.
    let crypto_ids: Vec<String> = auto_items
        .iter()
        .filter(|i| i.provider == "coingecko")
        .map(|i| i.provider_id.clone())
        .collect();
    let crypto_spot = if crypto_ids.is_empty() {
        HashMap::new()
    } else {
        match coingecko_markets(&crypto_ids).await {
            Ok(m) => m,
            // Spot only, without the extra fields, rather than no price at all.
            Err(e) => {
                tracing::debug!("coingecko markets failed, falling back to simple/price: {e:#}");
                prices::crypto_prices(&crypto_ids)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, p)| (k, (p, Extras::default())))
                    .collect()
            }
        }
    };
    for item in &auto_items {
        match quote_item(pool, item, Some(&crypto_spot)).await {
            Ok(true) => tokio::time::sleep(INTER_CALL_PAUSE).await,
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(
                    "watchlist {} item {} ({}) quote failed: {e:#}",
                    wl.id,
                    item.id,
                    item.symbol
                );
                let _ = store::set_item_error(pool, item.id, &format!("{e:#}")).await;
            }
        }
    }

    // Connector path — per-item calls, paced.
    for (item, cid) in &conn_items {
        let cx = ctxs.get(cid).and_then(|c| c.as_ref()).expect("ctx opened above");
        match quote_via_connector(pool, cx, item).await {
            Ok(()) => tokio::time::sleep(INTER_CALL_PAUSE).await,
            Err(e) => {
                tracing::warn!(
                    "watchlist {} item {} ({}) quote via {} failed: {e:#}",
                    wl.id,
                    item.id,
                    item.symbol,
                    cx.cap.provider
                );
                let _ = store::set_item_error(pool, item.id, &format!("{e:#}")).await;
            }
        }
    }
    store::mark_refreshed(pool, wl.id).await?;
    Ok(())
}

// ── Custom-connector path (Historical Data providers) ─────────────────────────

/// Everything one connector-backed refresh needs, resolved once per list.
struct ConnectorCx {
    connector: Box<dyn Connector>,
    cap: &'static Capability,
    secrets: HashMap<String, String>,
    client: reqwest::Client,
}

impl ConnectorCx {
    async fn open(pool: &PgPool, connector_id: uuid::Uuid) -> Result<Self> {
        let row = otw_store::connectors::get(pool, connector_id)
            .await?
            .ok_or_else(|| anyhow!("the list's quote connector no longer exists"))?;
        let connector = crate::histdata::connector_for(&row.provider)?;
        let cap = connector.capability();
        let secrets =
            otw_store::connectors::load_creds(pool, crate::histdata_cipher::get(), connector_id)
                .await?;
        Ok(Self { connector, cap, secrets, client: crate::histdata::client()? })
    }
}

/// Watchlist asset_class → histdata asset_type vocabulary.
fn histdata_asset_type(class: &str) -> &'static str {
    match class {
        "stock" => "equity",
        "etf" => "etf",
        _ => "crypto",
    }
}

/// Best-guess ticker in a connector's own symbol format when the user hasn't pinned one.
/// Crypto items carry a bare coin symbol (BTC); exchanges want a USD pair in their spelling.
pub fn default_quote_ticker(provider: &str, asset_class: &str, symbol: &str) -> String {
    let s = symbol.to_ascii_uppercase();
    let crypto = asset_class == "crypto";
    match provider {
        "binance" => format!("{s}USDT"),
        "coinbase" => format!("{s}-USD"),
        "kraken" => format!("{s}USD"),
        "alpaca" if crypto => format!("{s}/USD"),
        "massive" if crypto => format!("{s}USD"),
        "yahoo" if crypto => format!("{s}-USD"),
        // EODHD addresses everything with an exchange suffix.
        "eodhd" if crypto => format!("{s}-USD.CC"),
        "eodhd" => format!("{s}.US"),
        _ => s,
    }
}

/// Quote one item through the list's connector: rolling 1d bars for the changes/spark
/// (cached under `HISTORY_TTL` like the CoinGecko path), plus the freshest 1m close as the
/// live spot when the provider supports intraday — that's what makes fast cadences live.
async fn quote_via_connector(pool: &PgPool, cx: &ConnectorCx, item: &store::Item) -> Result<()> {
    let asset_type = histdata_asset_type(&item.asset_class);
    if !cx.cap.asset_types.contains(&asset_type) {
        return Err(anyhow!("{} does not support {asset_type}", cx.cap.label));
    }
    let ticker = match item.quote_ticker.trim() {
        "" => default_quote_ticker(cx.cap.provider, &item.asset_class, &item.symbol),
        t => t.to_string(),
    };
    let now = OffsetDateTime::now_utc();
    let now_sec = now.unix_timestamp();

    let history = match cached_history(item, now_sec) {
        Some(h) => h,
        None => {
            // 70 calendar days of dailies: the 30d change needs a close at-or-before
            // now−30d even across sparse trading calendars.
            let chunk = cx
                .connector
                .fetch_chunk(
                    &cx.client,
                    &cx.secrets,
                    &ticker,
                    asset_type,
                    "1d",
                    now - time::Duration::days(70),
                    now,
                )
                .await?;
            let series: Vec<(i64, f64)> =
                chunk.bars.iter().map(|b| (b.ts.unix_timestamp(), b.close)).collect();
            if series.is_empty() {
                return Err(anyhow!("no daily bars for {ticker} (wrong provider symbol?)"));
            }
            series
        }
    };

    // Live spot: last 1m close of the past few hours. A miss (market closed, intraday
    // behind a paywall) falls back to the last daily close rather than failing the item.
    let live = if cx.cap.timeframes.contains(&"1m") {
        match cx
            .connector
            .fetch_chunk(
                &cx.client,
                &cx.secrets,
                &ticker,
                asset_type,
                "1m",
                now - time::Duration::hours(3),
                now,
            )
            .await
        {
            Ok(c) => c.bars.last().map(|b| b.close),
            Err(e) => {
                tracing::debug!("watchlist 1m spot for {ticker} unavailable: {e:#}");
                None
            }
        }
    } else {
        None
    };
    let live = live
        .or_else(|| history.last().map(|(_, v)| *v))
        .ok_or_else(|| anyhow!("no price for {ticker}"))?;

    let mut quote = compute_quote(&history, live, now_sec, &Extras::default());
    let coords = chart_coords(item, Some(cx.cap.provider));
    fill_stored_range(pool, item, coords.as_ref(), &mut quote, live, now_sec).await;
    store::set_item_quote(pool, item.id, &quote, None).await
}

/// Minimal percent-encoding for path/query values (same policy as portfolios::prices).
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b',' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    #[test]
    fn changes_use_horizon_cutoffs() {
        // 31 completed daily closes: 100, 101, … 130, then live 132 "today at noon".
        let now = 1_000 * DAY + DAY / 2;
        let history: Vec<(i64, f64)> = (0..31)
            .map(|i| ((1_000 - 31 + i) * DAY, 100.0 + i as f64))
            .collect();
        let q = compute_quote(&history, 132.0, now, &Extras::default());
        // 24h cutoff lands mid-yesterday → yesterday's midnight close 130 → +2/130.
        let c24 = q["change_24h"].as_f64().unwrap();
        assert!((c24 - 2.0 / 130.0 * 100.0).abs() < 1e-9);
        // 7 days back: cutoff = day 993 at noon → last close at or before it is day 993's 124.
        let c7 = q["change_7d"].as_f64().unwrap();
        assert!((c7 - (132.0 - 124.0) / 124.0 * 100.0).abs() < 1e-9);
        // Spark ends on the live price and is capped at 31 points.
        let spark = q["spark"].as_array().unwrap();
        assert_eq!(spark.last().unwrap().as_f64(), Some(132.0));
        assert!(spark.len() <= 31);
    }

    #[test]
    fn todays_partial_close_is_superseded_by_live() {
        let now = 1_000 * DAY + DAY / 2;
        // Yesterday's close 100, plus a partial point for today at 105 — live is 102.
        let history = vec![((999) * DAY, 100.0), (1_000 * DAY + 3_600, 105.0)];
        let q = compute_quote(&history, 102.0, now, &Extras::default());
        assert!((q["change_24h"].as_f64().unwrap() - 2.0).abs() < 1e-9);
        let spark = q["spark"].as_array().unwrap();
        assert_eq!(spark.len(), 2); // yesterday + live, no duplicated today point
    }

    #[test]
    fn closed_market_shows_last_sessions_move_not_zero() {
        // Stock-style bars at 13:30 UTC. After hours the live spot equals the last close;
        // the day change must be that session's move (vs the close before it), not 0%.
        let bar = 13 * 3_600 + 1_800;
        let history = vec![(998 * DAY + bar, 100.0), (999 * DAY + bar, 103.0)];
        let now = 1_000 * DAY + 10 * 3_600; // next morning, market closed
        let q = compute_quote(&history, 103.0, now, &Extras::default());
        assert!((q["change_24h"].as_f64().unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn rsi_matches_the_textbook_series() {
        // Wilder's 14-day example series (StockCharts). Over the first 14 changes the gains
        // sum to 3.34 and the losses to 1.40: RS = 2.3857, RSI = 70.46 (the article prints
        // 70.53 because it rounds the averages to 0.24 and 0.10 first).
        let closes = [
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
            45.61, 46.28, 46.28,
        ];
        assert!((rsi(&closes, 14).unwrap() - 70.464).abs() < 0.001);
        // Too short for the period: no number at all.
        assert!(rsi(&closes[..14], 14).is_none());
        let up: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        assert_eq!(rsi(&up, 14), Some(100.0));
    }

    #[test]
    fn fields_the_source_does_not_serve_stay_null() {
        let now = 1_000 * DAY;
        let q = compute_quote(&[(999 * DAY, 50.0)], 55.0, now, &Extras::default());
        for k in ["mcap", "volume", "pe", "div_yield", "high_52w", "low_52w", "rsi_14"] {
            assert!(q[k].is_null(), "{k} should be null");
        }
        let served = Extras { mcap: Some(1e9), high_52w: Some(60.0), low_52w: Some(40.0), ..Extras::default() };
        let q = compute_quote(&[(999 * DAY, 50.0)], 55.0, now, &served);
        assert_eq!(q["mcap"].as_f64(), Some(1e9));
        assert_eq!(q["range_src"], "quote");
        assert!(q["pe"].is_null());
    }

    #[test]
    fn stored_range_needs_a_full_year() {
        let now = 1_000 * DAY;
        let year: Vec<(i64, f64, f64, f64)> =
            (0..365).map(|i| (now - (365 - i) * DAY, 110.0, 90.0, 100.0)).collect();
        assert_eq!(range_from_bars(&year, 120.0, now), Some((120.0, 90.0)));
        assert_eq!(range_from_bars(&year[100..], 100.0, now), None);
        assert_eq!(range_from_bars(&[], 100.0, now), None);
    }

    #[test]
    fn chart_coordinates_follow_the_quote_source() {
        let mut item = store::Item {
            id: uuid::Uuid::nil(),
            watchlist_id: uuid::Uuid::nil(),
            asset_class: "stock".into(),
            provider: "yahoo".into(),
            provider_id: "AAPL".into(),
            symbol: "AAPL".into(),
            name: String::new(),
            exchange: String::new(),
            notes: String::new(),
            quote_ticker: String::new(),
            quote_source: String::new(),
            position: 0.0,
            quote: json!({}),
            quoted_at: None,
            section: None,
            flag: None,
            target_buy: None,
            target_sell: None,
            virt_qty: None,
            virt_price: None,
            virt_date: None,
        };
        let c = chart_coords(&item, None).unwrap();
        assert_eq!((c.provider.as_str(), c.asset_type.as_str(), c.ticker.as_str()), ("yahoo", "equity", "AAPL"));
        item.asset_class = "crypto".into();
        item.provider = "coingecko".into();
        item.provider_id = "bitcoin".into();
        item.symbol = "BTC".into();
        // A coin id is not a market: no coordinates on the default path.
        assert!(chart_coords(&item, None).is_none());
        assert_eq!(chart_coords(&item, Some("binance")).unwrap().ticker, "BTCUSDT");
        item.quote_ticker = "BTCUSDC".into();
        assert_eq!(chart_coords(&item, Some("binance")).unwrap().ticker, "BTCUSDC");
    }

    #[test]
    fn short_history_yields_null_long_horizons() {
        let now = 1_000 * DAY;
        let history = vec![(999 * DAY, 50.0)];
        let q = compute_quote(&history, 55.0, now, &Extras::default());
        assert!(q["change_24h"].as_f64().is_some());
        assert!(q["change_30d"].is_null());
    }
}
