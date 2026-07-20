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

/// One Yahoo chart call: (daily closes ascending as (unix_sec, close), live price, exchange).
async fn yahoo_chart(ticker: &str) -> Result<(Vec<(i64, f64)>, Option<f64>, Option<String>)> {
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
    Ok((series, live, exchange))
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
fn compute_quote(history: &[(i64, f64)], live: f64, now_sec: i64) -> Value {
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
    crypto_spot: Option<&HashMap<String, f64>>,
) -> Result<bool> {
    let now_sec = OffsetDateTime::now_utc().unix_timestamp();
    match item.provider.as_str() {
        "yahoo" => {
            let (series, live, exchange) = yahoo_chart(&item.provider_id).await?;
            let live = live
                .or_else(|| series.last().map(|(_, v)| *v))
                .ok_or_else(|| anyhow!("no price for {}", item.provider_id))?;
            let quote = compute_quote(&series, live, now_sec);
            store::set_item_quote(pool, item.id, &quote, exchange.as_deref()).await?;
            Ok(true)
        }
        _ => {
            let live = match crypto_spot {
                Some(batch) => batch.get(&item.provider_id).copied(),
                None => prices::crypto_prices(&[item.provider_id.clone()])
                    .await?
                    .get(&item.provider_id)
                    .copied(),
            };
            let (history, fetched) = match cached_history(item, now_sec) {
                Some(h) => (h, false),
                None => (coingecko_history(&item.provider_id).await?, true),
            };
            let live = live
                .or_else(|| history.last().map(|(_, v)| *v))
                .ok_or_else(|| anyhow!("no price for {}", item.provider_id))?;
            let quote = compute_quote(&history, live, now_sec);
            store::set_item_quote(pool, item.id, &quote, None).await?;
            Ok(fetched)
        }
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
        prices::crypto_prices(&crypto_ids).await.unwrap_or_default()
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
        let row = otw_store::histdata::get_connector(pool, connector_id)
            .await?
            .ok_or_else(|| anyhow!("the list's quote connector no longer exists"))?;
        let connector = crate::histdata::connector_for(&row.provider)?;
        let cap = connector.capability();
        let secrets =
            otw_store::histdata::load_creds(pool, crate::histdata_cipher::get(), connector_id)
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

    let quote = compute_quote(&history, live, now_sec);
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
        let q = compute_quote(&history, 132.0, now);
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
        let q = compute_quote(&history, 102.0, now);
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
        let q = compute_quote(&history, 103.0, now);
        assert!((q["change_24h"].as_f64().unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn short_history_yields_null_long_horizons() {
        let now = 1_000 * DAY;
        let history = vec![(999 * DAY, 50.0)];
        let q = compute_quote(&history, 55.0, now);
        assert!(q["change_24h"].as_f64().is_some());
        assert!(q["change_30d"].is_null());
    }
}
