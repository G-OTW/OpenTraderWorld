//! Price & symbol resolution for the Portfolio Tracker.
//!
//! Two providers, both quoted in USD (the store converts USD → portfolio currency at read time):
//!   - **crypto** → CoinGecko (keyless demo). Symbol search resolves against the cached
//!     `/coins/list` (refetched at most every ~30 min, per CoinGecko's rate limit); spot prices
//!     come from `/simple/price?vs_currencies=usd`.
//!   - **stock/etf** → Yahoo Finance `query1.../v8/finance/chart` (last close) for spot, and
//!     `query1.../v1/finance/search` for symbol search.
//!
//! Hardcoding USD as the quote sidesteps the BTCUSD vs BTC/USD vs BTC-USD ambiguity: we store the
//! provider's own id (coin id / Yahoo ticker) and always price it in USD.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::portfolios as store;

const COINGECKO: &str = "https://api.coingecko.com/api/v3";
const YAHOO: &str = "https://query1.finance.yahoo.com";
/// CoinGecko asks callers not to hit /coins/list more than ~once per 30 min.
const COIN_LIST_TTL: Duration = Duration::from_secs(30 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent("OpenTraderWorld/portfolios")
        .build()
        .map_err(|e| anyhow!("building http client: {e}"))
}

/// A symbol search hit the user picks from when adding an asset.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub asset_class: String,
    pub provider: String,
    pub provider_id: String,
    pub symbol: String,
    pub name: String,
}

// ── Symbol search ─────────────────────────────────────────────────────────────

/// CoinGecko's /coins/list has thousands of look-alikes sharing a ticker (dozens of "BTC"s). It
/// carries no ranking, so for the well-known tickers we pin the canonical coin id; an exact-symbol
/// query for one of these surfaces the real coin first regardless of list order.
const CANONICAL: &[(&str, &str)] = &[
    ("btc", "bitcoin"),
    ("eth", "ethereum"),
    ("usdt", "tether"),
    ("usdc", "usd-coin"),
    ("bnb", "binancecoin"),
    ("xrp", "ripple"),
    ("sol", "solana"),
    ("ada", "cardano"),
    ("doge", "dogecoin"),
    ("trx", "tron"),
    ("dot", "polkadot"),
    ("matic", "matic-network"),
    ("ltc", "litecoin"),
    ("shib", "shiba-inu"),
    ("avax", "avalanche-2"),
    ("link", "chainlink"),
    ("xlm", "stellar"),
    ("atom", "cosmos"),
    ("xmr", "monero"),
    ("etc", "ethereum-classic"),
    ("bch", "bitcoin-cash"),
    ("uni", "uniswap"),
    ("near", "near"),
    ("apt", "aptos"),
    ("arb", "arbitrum"),
    ("op", "optimism"),
];

/// Search crypto by symbol/name against the cached coin list (case-insensitive substring).
/// Canonical exact-symbol matches rank first, then exact id/symbol/name, prefix, substring.
/// Refreshes the cache if stale/empty.
pub async fn search_crypto(pool: &PgPool, query: &str) -> Result<Vec<SearchHit>> {
    let coins = coin_list(pool).await?;
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let canonical_id = CANONICAL.iter().find(|(sym, _)| *sym == q).map(|(_, id)| *id);
    let arr = coins.as_array().cloned().unwrap_or_default();
    let mut hits: Vec<(u8, SearchHit)> = Vec::new();
    for c in arr {
        let id = c.get("id").and_then(Value::as_str).unwrap_or("");
        let symbol = c.get("symbol").and_then(Value::as_str).unwrap_or("");
        let name = c.get("name").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let sym_l = symbol.to_lowercase();
        let name_l = name.to_lowercase();
        let id_l = id.to_lowercase();
        // The canonical coin for a well-known ticker wins outright; then an exact id match (query
        // "bitcoin" → id "bitcoin"); then exact symbol/name, prefix, substring.
        let rank = if canonical_id == Some(id_l.as_str()) {
            0
        } else if id_l == q {
            1
        } else if sym_l == q || name_l == q {
            1
        } else if sym_l.starts_with(&q) || name_l.starts_with(&q) {
            2
        } else if sym_l.contains(&q) || name_l.contains(&q) {
            3
        } else {
            continue;
        };
        hits.push((
            rank,
            SearchHit {
                asset_class: "crypto".into(),
                provider: "coingecko".into(),
                provider_id: id.into(),
                symbol: symbol.to_uppercase(),
                name: name.into(),
            },
        ));
    }
    // Within a rank, prefer the shorter coin id — canonical coins ("bitcoin") tend to have clean,
    // short ids while look-alikes are longer ("big-tom-coin", "batcat").
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.provider_id.len().cmp(&b.1.provider_id.len())));
    Ok(hits.into_iter().take(25).map(|(_, h)| h).collect())
}

/// Search stocks/ETFs via Yahoo's search endpoint.
pub async fn search_stock(query: &str) -> Result<Vec<SearchHit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let url = format!("{YAHOO}/v1/finance/search?q={}&quotesCount=25&newsCount=0", urlencoding(q));
    let body: Value = crate::rate::send("yahoo", client()?.get(&url))
        .await
        .context("yahoo search request")?
        .error_for_status()
        .context("yahoo search status")?
        .json()
        .await
        .context("yahoo search decode")?;
    let quotes = body.get("quotes").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut hits = Vec::new();
    for qt in quotes {
        let symbol = qt.get("symbol").and_then(Value::as_str).unwrap_or("");
        if symbol.is_empty() {
            continue;
        }
        let qtype = qt.get("quoteType").and_then(Value::as_str).unwrap_or("");
        // Keep equities/ETFs; skip futures/options/currencies/indices we don't price here.
        let asset_class = match qtype {
            "ETF" => "etf",
            "EQUITY" => "stock",
            _ => continue,
        };
        let name = qt
            .get("longname")
            .or_else(|| qt.get("shortname"))
            .and_then(Value::as_str)
            .unwrap_or(symbol);
        hits.push(SearchHit {
            asset_class: asset_class.into(),
            provider: "yahoo".into(),
            provider_id: symbol.into(),
            symbol: symbol.into(),
            name: name.into(),
        });
    }
    Ok(hits)
}

// ── Spot prices (USD) ─────────────────────────────────────────────────────────

/// Fetch the latest USD spot for a batch of CoinGecko coin ids. Missing ids are omitted.
pub async fn crypto_prices(ids: &[String]) -> Result<std::collections::HashMap<String, f64>> {
    let mut out = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let url = format!(
        "{COINGECKO}/simple/price?ids={}&vs_currencies=usd",
        urlencoding(&ids.join(","))
    );
    let body: Value = crate::rate::send("coingecko", client()?.get(&url))
        .await
        .context("coingecko price request")?
        .error_for_status()
        .context("coingecko price status")?
        .json()
        .await
        .context("coingecko price decode")?;
    if let Some(obj) = body.as_object() {
        for (id, v) in obj {
            if let Some(p) = v.get("usd").and_then(Value::as_f64) {
                out.insert(id.clone(), p);
            }
        }
    }
    Ok(out)
}

/// Fetch the latest USD price (most recent close) for a Yahoo ticker.
pub async fn stock_price(ticker: &str) -> Result<Option<f64>> {
    let url = format!("{YAHOO}/v8/finance/chart/{}?interval=1d&range=5d", urlencoding(ticker));
    let body: Value = crate::rate::send("yahoo", client()?.get(&url))
        .await
        .context("yahoo price request")?
        .error_for_status()
        .context("yahoo price status")?
        .json()
        .await
        .context("yahoo price decode")?;
    let res = body.pointer("/chart/result/0");
    // Prefer the live regularMarketPrice, else the last non-null close.
    if let Some(p) = res
        .and_then(|r| r.pointer("/meta/regularMarketPrice"))
        .and_then(Value::as_f64)
    {
        return Ok(Some(p));
    }
    let closes = res
        .and_then(|r| r.pointer("/indicators/quote/0/close"))
        .and_then(Value::as_array);
    let last = closes
        .and_then(|a| a.iter().rev().find_map(Value::as_f64));
    Ok(last)
}

// ── Exchange spot prices (keyless, treated as USD) ─────────────────────────────
//
// Each exchange has a trivial keyless last-price endpoint. Stablecoin quotes (USDT/USD) are taken
// as ≈ USD, consistent with the module's USD-quote design. The user types the exact provider pair
// (BTCUSDT / BTC-USD / XXBTZUSD differ), so we don't guess or normalize — we fetch what they gave.

const BINANCE: &str = "https://api.binance.com";
const KRAKEN: &str = "https://api.kraken.com";
const COINBASE: &str = "https://api.exchange.coinbase.com";

/// Binance last trade price, e.g. pair "BTCUSDT".
pub async fn binance_price(pair: &str) -> Result<Option<f64>> {
    let url = format!("{BINANCE}/api/v3/ticker/price?symbol={}", urlencoding(pair));
    let body: Value = crate::rate::send("binance", client()?.get(&url)).await.context("binance request")?.json().await.context("binance decode")?;
    Ok(body.get("price").and_then(Value::as_str).and_then(|s| s.parse::<f64>().ok()))
}

/// Kraken last trade price, e.g. pair "XBTUSDT" / "XXBTZUSD". Kraken keys the result by its own
/// canonical pair name (not always the query), so we read the single entry under `result`.
pub async fn kraken_price(pair: &str) -> Result<Option<f64>> {
    let url = format!("{KRAKEN}/0/public/Ticker?pair={}", urlencoding(pair));
    let body: Value = crate::rate::send("kraken", client()?.get(&url)).await.context("kraken request")?.json().await.context("kraken decode")?;
    // { "error": [...], "result": { "XXBTZUSD": { "c": ["<last>", "<lot>"] } } }
    let last = body
        .get("result")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(|v| v.pointer("/c/0"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok());
    Ok(last)
}

/// Coinbase Exchange last trade price, e.g. product "BTC-USD".
pub async fn coinbase_price(product: &str) -> Result<Option<f64>> {
    let url = format!("{COINBASE}/products/{}/ticker", urlencoding(product));
    let body: Value = crate::rate::send("coinbase", client()?.get(&url)).await.context("coinbase request")?.json().await.context("coinbase decode")?;
    Ok(body.get("price").and_then(Value::as_str).and_then(|s| s.parse::<f64>().ok()))
}

/// Fetch the current USD spot for one asset, honoring a `spot_provider` override. Returns
/// `Ok(None)` when the source has no price for the symbol (→ unresolved), `Err` on transport
/// failure. `crypto` is a prefetched CoinGecko batch used when the asset prices via CoinGecko.
async fn spot_for(
    a: &store::Asset,
    crypto: &std::collections::HashMap<String, f64>,
) -> Result<Option<f64>> {
    match a.spot_provider.as_deref() {
        Some("binance") => binance_price(&a.spot_symbol).await,
        Some("kraken") => kraken_price(&a.spot_symbol).await,
        Some("coinbase") => coinbase_price(&a.spot_symbol).await,
        Some("yahoo") => stock_price(&a.spot_symbol).await,
        Some("coingecko") => Ok(crypto_prices(&[a.spot_symbol.clone()]).await?.get(&a.spot_symbol).copied()),
        // No override: price by the default provider the asset was resolved against.
        _ => match a.provider.as_str() {
            "coingecko" => Ok(crypto.get(&a.provider_id).copied()),
            _ => stock_price(&a.provider_id).await,
        },
    }
}

/// The result of reconciling one asset against its (possibly overridden) price source.
#[derive(Debug, Clone, Serialize)]
pub struct AssetRecon {
    pub asset_id: Uuid,
    pub symbol: String,
    pub source: String,
    /// "ok" | "unresolved". ("manual" is set by the user, not by a check.)
    pub status: String,
    pub price_usd: Option<f64>,
    pub note: String,
}

/// The price source an asset resolves against, as a label ("coingecko"/"binance"/…).
fn source_label(a: &store::Asset) -> &str {
    a.spot_provider.as_deref().unwrap_or(&a.provider)
}

/// Check every asset in a portfolio against its price source, persisting an 'ok'/'unresolved'
/// status per asset. Assets the user marked 'manual' are left untouched (still skipped by refresh).
/// Does not change stored prices — it only verifies resolvability. Returns per-asset results.
pub async fn reconcile_portfolio(pool: &PgPool, pf: &store::Portfolio) -> Result<Vec<AssetRecon>> {
    let assets = store::list_assets(pool, pf.id).await?;
    // Batch the default-CoinGecko lookups (assets with no override, provider coingecko).
    let crypto_ids: Vec<String> = assets
        .iter()
        .filter(|a| a.spot_provider.is_none() && a.provider == "coingecko")
        .map(|a| a.provider_id.clone())
        .collect();
    let crypto = crypto_prices(&crypto_ids).await.unwrap_or_default();

    let mut out = Vec::with_capacity(assets.len());
    for a in &assets {
        let source = source_label(a).to_string();
        if a.recon_status == "manual" {
            out.push(AssetRecon {
                asset_id: a.id,
                symbol: a.symbol.clone(),
                source,
                status: "manual".into(),
                price_usd: a.last_price_usd,
                note: "excluded from auto-refresh".into(),
            });
            continue;
        }
        let (status, price, note) = match spot_for(a, &crypto).await {
            Ok(Some(p)) => ("ok", Some(p), String::new()),
            Ok(None) => ("unresolved", None, "no price returned for this symbol".into()),
            Err(e) => ("unresolved", None, format!("source error: {e}")),
        };
        store::set_recon(pool, a.id, status, &note).await?;
        out.push(AssetRecon { asset_id: a.id, symbol: a.symbol.clone(), source, status: status.into(), price_usd: price, note });
    }
    Ok(out)
}

/// Fetch and persist the spot for a single freshly-added asset, returning the updated row
/// (or `None` when the source has no price for it — the caller keeps the unpriced asset).
/// Used by `add_asset` so a new row isn't blank until the next portfolio-wide refresh.
pub async fn price_new_asset(pool: &PgPool, a: &store::Asset) -> Result<Option<store::Asset>> {
    // `spot_for` reads default-provider CoinGecko assets out of a prefetched batch, so fetch
    // this one id up front rather than handing it an empty map (which would always miss).
    let crypto = if a.spot_provider.is_none() && a.provider == "coingecko" {
        crypto_prices(std::slice::from_ref(&a.provider_id)).await.unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };
    let Some(p) = spot_for(a, &crypto).await? else {
        return Ok(None);
    };
    store::set_asset_price(pool, a.id, p).await?;
    store::get_asset(pool, a.id).await
}

// ── Refresh a portfolio's asset prices ─────────────────────────────────────────

/// Re-price every asset in a portfolio (USD), persist the spots, then snapshot today's value.
/// Assets marked 'manual' or 'unresolved' are skipped (their price is left as-is).
pub async fn refresh_portfolio(pool: &PgPool, pf: &store::Portfolio) -> Result<()> {
    let assets = store::list_assets(pool, pf.id).await?;
    // Prefetch CoinGecko for the default-priced crypto assets (single batched call).
    let crypto_ids: Vec<String> = assets
        .iter()
        .filter(|a| a.spot_provider.is_none() && a.provider == "coingecko")
        .map(|a| a.provider_id.clone())
        .collect();
    let crypto = crypto_prices(&crypto_ids).await.unwrap_or_default();

    for a in &assets {
        if matches!(a.recon_status.as_str(), "manual" | "unresolved") {
            continue;
        }
        // Tolerate individual source failures; a persistent one shows up on the next reconcile.
        if let Ok(Some(p)) = spot_for(a, &crypto).await {
            store::set_asset_price(pool, a.id, p).await?;
        }
    }
    store::snapshot_today(pool, pf).await?;
    store::mark_refreshed(pool, pf.id).await?;
    Ok(())
}

// ── CoinGecko coin-list cache ──────────────────────────────────────────────────

/// Return the cached coin list, refetching from CoinGecko when missing or older than the TTL.
async fn coin_list(pool: &PgPool) -> Result<Value> {
    if let Some((coins, fetched)) = store::coingecko_cache(pool).await? {
        let age = OffsetDateTime::now_utc() - fetched;
        let fresh = age.whole_seconds() >= 0 && (age.whole_seconds() as u64) < COIN_LIST_TTL.as_secs();
        let nonempty = coins.as_array().map(|a| !a.is_empty()).unwrap_or(false);
        if fresh && nonempty {
            return Ok(coins);
        }
    }
    match fetch_coin_list().await {
        Ok(coins) => {
            store::set_coingecko_cache(pool, &coins).await?;
            Ok(coins)
        }
        // On a fetch failure, fall back to whatever we have cached (even if stale).
        Err(e) => {
            if let Some((coins, _)) = store::coingecko_cache(pool).await? {
                if coins.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                    tracing::warn!("coingecko coin-list refresh failed, using stale cache: {e:#}");
                    return Ok(coins);
                }
            }
            Err(e)
        }
    }
}

async fn fetch_coin_list() -> Result<Value> {
    let body: Value = client()?
        .get(format!("{COINGECKO}/coins/list"))
        .send()
        .await
        .context("coingecko coins/list request")?
        .error_for_status()
        .context("coingecko coins/list status")?
        .json()
        .await
        .context("coingecko coins/list decode")?;
    if !body.is_array() {
        return Err(anyhow!("coingecko coins/list: unexpected response"));
    }
    Ok(body)
}

/// Minimal percent-encoding for query values (alnum and a few safe chars pass through).
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
