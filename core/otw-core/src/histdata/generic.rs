//! REST-JSON equity/multi-asset providers: Alpha Vantage, EODHD, Yahoo, Alpaca, Massive (Polygon).
//!
//! Each has a distinct response shape, so the connector carries a `Kind` discriminant and
//! a per-kind URL builder + parser. They share capability plumbing. Equity providers that
//! expose adjusted closes populate the adj_* fields; the rest leave them None.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, Date, OffsetDateTime, Time};

use otw_store::histdata::Bar;

use super::{Capability, Chunk, Connector};

#[derive(Clone, Copy)]
pub enum Kind {
    AlphaVantage,
    Eodhd,
    Yahoo,
    Alpaca,
    Massive,
}

pub struct Generic {
    kind: Kind,
    cap: &'static Capability,
}

// ── Capability tables ──────────────────────────────────────────────────────────

static AV: Capability = Capability {
    provider: "alphavantage",
    label: "Alpha Vantage",
    website: "https://alphavantage.co",
    docs_url: "https://www.alphavantage.co/documentation/",
    rate_limit: "Free key: 25 requests/day and 5/min — very tight. Over-limit returns a JSON note (surfaced as a job error), not 429. Consider a premium key for bulk history.",
    required_secrets: &["api_key"],
    asset_types: &["equity", "etf", "crypto", "fx"],
    timeframes: &["1m", "5m", "15m", "1h", "1d"],
    // Free daily endpoint (TIME_SERIES_DAILY) is unadjusted; the adjusted variant is premium.
    adjusted: false,
    max_bars_per_req: 5000,
};
static EODHD: Capability = Capability {
    provider: "eodhd",
    label: "EODHD",
    website: "https://eodhd.com",
    docs_url: "https://eodhd.com/financial-apis/",
    rate_limit: "Paid key. Daily API-call allowance by plan (intraday costs more calls per request); 429 when exhausted. Downloads auto-retry with backoff.",
    required_secrets: &["api_key"],
    asset_types: &["equity", "etf", "fx", "crypto"],
    timeframes: &["1m", "5m", "1h", "1d", "1w"],
    adjusted: true,
    max_bars_per_req: 5000,
};
static YAHOO: Capability = Capability {
    provider: "yahoo",
    label: "Yahoo Finance",
    website: "https://finance.yahoo.com",
    docs_url: "https://query1.finance.yahoo.com/v8/finance/chart/AAPL",
    rate_limit: "Unofficial/keyless endpoint — no published limits. Aggressive polling can get your IP throttled (429) or blocked; 1m data is capped to recent weeks. Best-effort only.",
    required_secrets: &[],
    asset_types: &["equity", "etf", "index", "crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "1d", "1w"],
    adjusted: true,
    max_bars_per_req: 5000,
};
static ALPACA: Capability = Capability {
    provider: "alpaca",
    label: "Alpaca",
    website: "https://alpaca.markets",
    docs_url: "https://docs.alpaca.markets/reference/stockbars",
    rate_limit: "Free key: 200 requests/min. Free plan serves IEX equities (15-min delayed, limited history) and the indicative options feed (15-min delayed, history only from Feb 2024); SIP/OPRA full history needs a paid plan. Downloads auto-retry with backoff.",
    required_secrets: &["api_key", "api_secret"],
    asset_types: &["equity", "crypto", "option"],
    timeframes: &["1m", "5m", "15m", "1h", "1d"],
    adjusted: true,
    max_bars_per_req: 10000,
};

static MASSIVE: Capability = Capability {
    provider: "massive",
    label: "Massive (Polygon.io)",
    website: "https://massive.com",
    docs_url: "https://massive.com/docs/rest/quickstart",
    rate_limit: "Free key: 5 requests/minute (over-limit returns HTTP 429, auto-retried with backoff — the cap makes wide intraday ranges page slowly). Free plans are end-of-day: use the 1d timeframe. Stocks, ETFs, crypto, forex, options, indices and futures all work at EOD with ~2 years of history; intraday and older ranges need a paid plan. A few tickers stay premium even at EOD (e.g. the I:SPX index) and return NOT_AUTHORIZED, surfaced as a clear job error rather than empty data. Paid plans add intraday, real-time and full history.",
    required_secrets: &["api_key"],
    // Aggregates cover every Polygon market. Options/futures use the O:/ (and vendor)
    // ticker prefixes; crypto is X:BTCUSD, fx is C:EURUSD, indices I:SPX.
    asset_types: &["equity", "etf", "option", "future", "crypto", "fx", "index"],
    timeframes: &["1m", "5m", "15m", "1h", "1d", "1w"],
    // Aggregates fold splits/dividends straight into OHLC when adjusted=true (no separate
    // adj_* column), so we request unadjusted and leave adj_* None for consistency with peers.
    adjusted: false,
    max_bars_per_req: 50000,
};

pub fn alphavantage() -> Generic { Generic { kind: Kind::AlphaVantage, cap: &AV } }
pub fn eodhd() -> Generic { Generic { kind: Kind::Eodhd, cap: &EODHD } }
pub fn yahoo() -> Generic { Generic { kind: Kind::Yahoo, cap: &YAHOO } }
pub fn alpaca() -> Generic { Generic { kind: Kind::Alpaca, cap: &ALPACA } }
pub fn massive() -> Generic { Generic { kind: Kind::Massive, cap: &MASSIVE } }

// ── Helpers ────────────────────────────────────────────────────────────────────

fn need(secrets: &HashMap<String, String>, name: &str, label: &str) -> Result<String> {
    secrets
        .get(name)
        .filter(|v| !v.trim().is_empty())
        .cloned()
        .ok_or_else(|| anyhow!("{label} requires a '{name}' credential — set it on the settings page"))
}

/// Parse a "YYYY-MM-DD" or "YYYY-MM-DD HH:MM:SS" stamp (UTC) into OffsetDateTime.
fn parse_stamp(s: &str) -> Result<OffsetDateTime> {
    let s = s.trim();
    if let Ok(odt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Ok(odt);
    }
    let (date_part, time_part) = s.split_once([' ', 'T']).unwrap_or((s, "00:00:00"));
    let date = Date::parse(
        date_part,
        &time::format_description::well_known::Iso8601::DATE,
    )
    .context("parsing date")?;
    let t: Vec<u8> = time_part
        .split(':')
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let time = Time::from_hms(
        t.first().copied().unwrap_or(0),
        t.get(1).copied().unwrap_or(0),
        t.get(2).copied().unwrap_or(0),
    )
    .unwrap_or(Time::MIDNIGHT);
    Ok(date.with_time(time).assume_utc())
}

fn f(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

// ── Connector impl ───────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl Connector for Generic {
    fn capability(&self) -> &'static Capability {
        self.cap
    }

    async fn fetch_chunk(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Chunk> {
        match self.kind {
            Kind::Yahoo => fetch_yahoo(client, ticker, timeframe, from, to).await,
            Kind::Alpaca => fetch_alpaca(client, secrets, ticker, asset_type, timeframe, from, to).await,
            Kind::Eodhd => fetch_eodhd(client, secrets, ticker, timeframe, from, to).await,
            Kind::AlphaVantage => fetch_av(client, secrets, ticker, timeframe).await,
            Kind::Massive => fetch_massive(client, secrets, ticker, asset_type, timeframe, from, to).await,
        }
    }
}

// ── Yahoo (chart API, keyless) ───────────────────────────────────────────────────

fn yahoo_interval(tf: &str) -> &'static str {
    match tf {
        "1m" => "1m",
        "5m" => "5m",
        "15m" => "15m",
        "1h" => "1h",
        "1w" => "1wk",
        _ => "1d",
    }
}

/// Yahoo refuses (rather than clamps) any intraday window whose start predates the depth
/// it keeps for that interval, failing the whole request. Depths per Yahoo's chart API:
/// 1m ~30d, 5m/15m ~60d, 1h ~730d. Daily and weekly are unlimited.
fn yahoo_max_days(interval: &str) -> Option<i64> {
    match interval {
        "1m" => Some(30),
        "5m" | "15m" => Some(60),
        "1h" => Some(730),
        _ => None,
    }
}

async fn fetch_yahoo(
    client: &reqwest::Client,
    ticker: &str,
    timeframe: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Chunk> {
    let interval = yahoo_interval(timeframe);
    // Pull the floor in by a day so a request sitting exactly on the boundary (a 730-day
    // window for 1h, say) doesn't tip past it while the request is in flight.
    let from = match yahoo_max_days(interval) {
        Some(max) => from.max(to - time::Duration::days(max - 1)),
        None => from,
    };
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval={}&period1={}&period2={}",
        ticker, interval, from.unix_timestamp(), to.unix_timestamp()
    );
    // Yahoo returns a descriptive JSON error body even on 4xx (e.g. hourly data is
    // limited to the last 730 days), so parse the body regardless of status rather
    // than letting error_for_status() discard it.
    let body: Value = crate::rate::send("yahoo", client.get(&url))
        .await
        .context("yahoo request")?
        .json()
        .await
        .context("yahoo decode")?;

    if let Some(desc) = body.pointer("/chart/error/description").and_then(Value::as_str) {
        // Yahoo returns HTTP 200 with a body error even when throttled; flag those.
        if desc.to_ascii_lowercase().contains("too many requests") {
            crate::rate::note_limited("yahoo", &url, desc);
        }
        return Err(anyhow!("yahoo: {desc}"));
    }

    let res = body
        .pointer("/chart/result/0")
        .ok_or_else(|| anyhow!("yahoo: no data (bad symbol?)"))?;
    let ts = res
        .pointer("/timestamp")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("yahoo: no timestamps"))?;
    let q = res
        .pointer("/indicators/quote/0")
        .ok_or_else(|| anyhow!("yahoo: no quote"))?;
    let adj = res.pointer("/indicators/adjclose/0/adjclose").and_then(Value::as_array);
    let col = |name: &str| q.get(name).and_then(Value::as_array);
    let (opens, highs, lows, closes, vols) =
        (col("open"), col("high"), col("low"), col("close"), col("volume"));

    let mut bars = Vec::with_capacity(ts.len());
    for (i, t) in ts.iter().enumerate() {
        let Some(t) = t.as_i64() else { continue };
        let get = |c: &Option<&Vec<Value>>| c.and_then(|a| a.get(i)).and_then(f);
        let (Some(o), Some(h), Some(l), Some(c)) =
            (get(&opens), get(&highs), get(&lows), get(&closes))
        else {
            continue; // Yahoo emits nulls for gaps; skip them.
        };
        let adj_close = adj.and_then(|a| a.get(i)).and_then(f);
        bars.push(Bar {
            ts: OffsetDateTime::from_unix_timestamp(t)?,
            open: o,
            high: h,
            low: l,
            close: c,
            volume: get(&vols).unwrap_or(0.0),
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close,
        });
    }
    Ok(Chunk { bars })
}

// ── Alpaca (v2 market data) ──────────────────────────────────────────────────────

async fn fetch_alpaca(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    ticker: &str,
    asset_type: &str,
    timeframe: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Chunk> {
    let key = need(secrets, "api_key", "Alpaca")?;
    let secret = need(secrets, "api_secret", "Alpaca")?;
    let tf = match timeframe {
        "1m" => "1Min",
        "5m" => "5Min",
        "15m" => "15Min",
        "1h" => "1Hour",
        _ => "1Day",
    };
    // Crypto and options use the multi-symbol endpoints ({bars:{SYM:[..]}}); equities use the
    // per-symbol path ({bars:[..]}). Options are the bare OCC symbol (no vendor prefix) —
    // validate it so a malformed contract fails clearly instead of returning an empty result.
    let nested = matches!(asset_type, "crypto" | "option");
    let sym = match asset_type {
        "option" => crate::histdata::parse_option_symbol(ticker)?.occ(),
        _ => ticker.to_string(),
    };
    let base = match asset_type {
        "crypto" => "https://data.alpaca.markets/v1beta3/crypto/us/bars".to_string(),
        "option" => "https://data.alpaca.markets/v1beta1/options/bars".to_string(),
        _ => format!("https://data.alpaca.markets/v2/stocks/{sym}/bars"),
    };
    let url = format!(
        "{base}?{}timeframe={tf}&start={}&end={}&limit=10000",
        if nested { format!("symbols={sym}&") } else { String::new() },
        from.format(&Rfc3339)?,
        to.format(&Rfc3339)?,
    );
    let body: Value = crate::rate::send(
        "alpaca",
        client
            .get(&url)
            .header("APCA-API-KEY-ID", key)
            .header("APCA-API-SECRET-KEY", secret),
    )
    .await
    .context("alpaca request")?
    .error_for_status()
    .context("alpaca status")?
        .json()
        .await
        .context("alpaca decode")?;

    // Stocks: { bars: [..] }; crypto/options: { bars: { SYM: [..] } }. Index the nested object by
    // key rather than a JSON Pointer — crypto symbols contain a '/' (e.g. BTC/USD) which Pointer
    // would treat as a path separator and fail to find.
    let arr = if nested {
        body.get("bars").and_then(|b| b.get(&sym)).and_then(Value::as_array)
    } else {
        body.get("bars").and_then(Value::as_array)
    }
    .ok_or_else(|| anyhow!("alpaca: no bars (bad symbol?)"))?;

    let mut bars = Vec::with_capacity(arr.len());
    for b in arr {
        // { t, o, h, l, c, v }
        let ts = b.get("t").and_then(Value::as_str).ok_or_else(|| anyhow!("alpaca bar time"))?;
        bars.push(Bar {
            ts: parse_stamp(ts)?,
            open: b.get("o").and_then(f).unwrap_or(0.0),
            high: b.get("h").and_then(f).unwrap_or(0.0),
            low: b.get("l").and_then(f).unwrap_or(0.0),
            close: b.get("c").and_then(f).unwrap_or(0.0),
            volume: b.get("v").and_then(f).unwrap_or(0.0),
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        });
    }
    Ok(Chunk { bars })
}

// ── EODHD ────────────────────────────────────────────────────────────────────────

async fn fetch_eodhd(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    ticker: &str,
    timeframe: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Chunk> {
    let key = need(secrets, "api_key", "EODHD")?;
    // EOD (daily/weekly) vs intraday have different endpoints + time params.
    if matches!(timeframe, "1d" | "1w") {
        let iso = &time::format_description::well_known::Iso8601::DATE;
        let period = if timeframe == "1w" { "w" } else { "d" };
        let url = format!(
            "https://eodhd.com/api/eod/{ticker}?from={}&to={}&period={period}&api_token={key}&fmt=json",
            from.date().format(iso)?, to.date().format(iso)?
        );
        let arr: Vec<Value> = crate::rate::send("eodhd", client.get(&url)).await.context("eodhd request")?
            .error_for_status().context("eodhd status")?.json().await.context("eodhd decode")?;
        let mut bars = Vec::with_capacity(arr.len());
        for b in &arr {
            let ts = b.get("date").and_then(Value::as_str).ok_or_else(|| anyhow!("eodhd date"))?;
            bars.push(Bar {
                ts: parse_stamp(ts)?,
                open: b.get("open").and_then(f).unwrap_or(0.0),
                high: b.get("high").and_then(f).unwrap_or(0.0),
                low: b.get("low").and_then(f).unwrap_or(0.0),
                close: b.get("close").and_then(f).unwrap_or(0.0),
                volume: b.get("volume").and_then(f).unwrap_or(0.0),
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: b.get("adjusted_close").and_then(f),
            });
        }
        return Ok(Chunk { bars });
    }
    let interval = match timeframe {
        "1m" => "1m",
        "5m" => "5m",
        _ => "1h",
    };
    let url = format!(
        "https://eodhd.com/api/intraday/{ticker}?interval={interval}&from={}&to={}&api_token={key}&fmt=json",
        from.unix_timestamp(), to.unix_timestamp()
    );
    let arr: Vec<Value> = client.get(&url).send().await.context("eodhd request")?
        .error_for_status().context("eodhd status")?.json().await.context("eodhd decode")?;
    let mut bars = Vec::with_capacity(arr.len());
    for b in &arr {
        let t = b.get("timestamp").and_then(Value::as_i64).ok_or_else(|| anyhow!("eodhd ts"))?;
        bars.push(Bar {
            ts: OffsetDateTime::from_unix_timestamp(t)?,
            open: b.get("open").and_then(f).unwrap_or(0.0),
            high: b.get("high").and_then(f).unwrap_or(0.0),
            low: b.get("low").and_then(f).unwrap_or(0.0),
            close: b.get("close").and_then(f).unwrap_or(0.0),
            volume: b.get("volume").and_then(f).unwrap_or(0.0),
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        });
    }
    Ok(Chunk { bars })
}

// ── Alpha Vantage (returns a full series per call; we filter to [from,to)) ────────

async fn fetch_av(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    ticker: &str,
    timeframe: &str,
) -> Result<Chunk> {
    let key = need(secrets, "api_key", "Alpha Vantage")?;
    // TIME_SERIES_DAILY is the free daily endpoint (unadjusted → adj_close None). The
    // _ADJUSTED variant AND outputsize=full are both premium even for daily, so use
    // outputsize=compact (last 100 bars) to stay within the free tier. Intraday full is
    // also premium; we keep it available for paying keys. A failed request surfaces AV's
    // own error message.
    let url = if timeframe == "1d" {
        format!("https://www.alphavantage.co/query?function=TIME_SERIES_DAILY&symbol={ticker}&outputsize=compact&apikey={key}")
    } else {
        let iv = match timeframe {
            "1m" => "1min", "5m" => "5min", "15m" => "15min", _ => "60min",
        };
        format!("https://www.alphavantage.co/query?function=TIME_SERIES_INTRADAY&symbol={ticker}&interval={iv}&outputsize=full&apikey={key}")
    };
    let body: Value = crate::rate::send("alphavantage", client.get(&url)).await.context("alphavantage request")?
        .error_for_status().context("alphavantage status")?.json().await.context("alphavantage decode")?;

    if let Some(note) = body.get("Note").or_else(|| body.get("Information")).and_then(Value::as_str) {
        // AV signals its 25/day + 5/min free-tier limit in a 200 body note, not a 429; flag it.
        crate::rate::note_limited("alphavantage", &url, note);
        return Err(anyhow!("alphavantage: {note}")); // rate limit / key issue
    }
    let series = body
        .as_object()
        .and_then(|o| o.iter().find(|(k, _)| k.starts_with("Time Series")).map(|(_, v)| v))
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("alphavantage: no series (bad symbol?)"))?;

    let mut bars = Vec::with_capacity(series.len());
    for (stamp, ohlc) in series {
        bars.push(Bar {
            ts: parse_stamp(stamp)?,
            open: ohlc.get("1. open").and_then(f).unwrap_or(0.0),
            high: ohlc.get("2. high").and_then(f).unwrap_or(0.0),
            low: ohlc.get("3. low").and_then(f).unwrap_or(0.0),
            close: ohlc.get("4. close").and_then(f).unwrap_or(0.0),
            volume: ohlc.get("6. volume").or_else(|| ohlc.get("5. volume")).and_then(f).unwrap_or(0.0),
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: ohlc.get("5. adjusted close").and_then(f),
        });
    }
    bars.sort_by_key(|b| b.ts);
    Ok(Chunk { bars })
}

// ── Massive (Polygon.io aggregates) ────────────────────────────────────────────────

/// Map our canonical timeframe to a Polygon `(multiplier, timespan)` pair.
fn massive_span(tf: &str) -> (u32, &'static str) {
    match tf {
        "1m" => (1, "minute"),
        "5m" => (5, "minute"),
        "15m" => (15, "minute"),
        "1h" => (1, "hour"),
        "1w" => (1, "week"),
        _ => (1, "day"),
    }
}

/// Prefix the user's symbol with Polygon's per-market marker. Equities/ETFs are bare; options
/// use the `O:`-prefixed OCC symbol; crypto is `X:`, fx `C:`, indices `I:`. The form submits a
/// prefix-free symbol (e.g. `SPY251219C00650000`, `BTCUSD`); we add the marker here so the same
/// input works across providers.
fn massive_ticker(ticker: &str, asset_type: &str) -> Result<String> {
    let t = ticker.trim();
    Ok(match asset_type {
        "option" => format!("O:{}", super::parse_option_symbol(t)?.occ()),
        "crypto" => format!("X:{}", t.strip_prefix("X:").unwrap_or(t).to_ascii_uppercase()),
        "fx" => format!("C:{}", t.strip_prefix("C:").unwrap_or(t).to_ascii_uppercase()),
        "index" => format!("I:{}", t.strip_prefix("I:").unwrap_or(t).to_ascii_uppercase()),
        _ => t.to_ascii_uppercase(),
    })
}

/// GET a Massive endpoint and validate its envelope. A served response carries status OK or
/// DELAYED (free tier is delayed); anything else — ERROR (bad request), NOT_AUTHORIZED (window
/// outside the plan's history depth) — carries a human `message`/`error` we surface so the job
/// fails with the real reason rather than looking like an empty range. Returns the parsed body.
async fn massive_get(client: &reqwest::Client, url: &str) -> Result<Value> {
    // error_for_status() keeps a 429 (free-tier over-limit) as a retryable reqwest error so the
    // worker backs off rather than treating it as a permanent failure.
    let body: Value = crate::rate::send("massive", client.get(url))
        .await
        .context("massive request")?
        .error_for_status()
        .context("massive status")?
        .json()
        .await
        .context("massive decode")?;
    let status = body.get("status").and_then(Value::as_str).unwrap_or("");
    if !status.eq_ignore_ascii_case("OK") && !status.eq_ignore_ascii_case("DELAYED") {
        let msg = body.get("message").or_else(|| body.get("error")).and_then(Value::as_str)
            .unwrap_or("request failed");
        // Some over-limit conditions arrive as an ERROR envelope rather than a 429.
        if msg.to_ascii_lowercase().contains("exceeded") || status.eq_ignore_ascii_case("ERROR")
            && msg.to_ascii_lowercase().contains("limit")
        {
            crate::rate::note_limited("massive", url, msg);
        }
        return Err(anyhow!("massive ({status}): {msg}"));
    }
    Ok(body)
}

async fn fetch_massive(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    ticker: &str,
    asset_type: &str,
    timeframe: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Chunk> {
    let key = need(secrets, "api_key", "Massive")?;
    // Futures live on a separate service (`/futures/v1/aggs`) with different params, timestamp
    // units and response fields, so they get their own fetch. Everything else (equity/etf/
    // option/crypto/fx/index) shares the v2 aggregates endpoint below.
    if asset_type == "future" {
        return fetch_massive_futures(client, &key, ticker, timeframe, from, to).await;
    }
    let sym = massive_ticker(ticker, asset_type)?;
    let (mult, span) = massive_span(timeframe);
    // Millisecond bounds give us exact intraday windows; `to` is inclusive on Polygon's side,
    // so subtract 1ms to keep chunk edges non-overlapping (the worker pages [cursor, chunk_to)).
    let from_ms = from.unix_timestamp_nanos() / 1_000_000;
    let to_ms = (to.unix_timestamp_nanos() / 1_000_000).saturating_sub(1).max(from_ms);
    // The index aggregates endpoint has no `adjusted` param (index values aren't split-adjusted)
    // and its bars carry no volume; every other market accepts adjusted + returns volume. Request
    // unadjusted OHLC so peers stay consistent (we leave the adj_* columns None across the board).
    let adjusted = if asset_type == "index" { "" } else { "adjusted=false&" };
    let url = format!(
        "https://api.massive.com/v2/aggs/ticker/{sym}/range/{mult}/{span}/{from_ms}/{to_ms}\
         ?{adjusted}sort=asc&limit=50000&apiKey={key}"
    );
    let body = massive_get(client, &url).await?;
    // No `results` under an OK status just means the window held no bars (weekend, pre-listing,
    // before the ticker existed) — return an empty chunk.
    let Some(arr) = body.get("results").and_then(Value::as_array) else {
        return Ok(Chunk { bars: Vec::new() });
    };

    let mut bars = Vec::with_capacity(arr.len());
    for b in arr {
        // { t: ms, o, h, l, c, v, vw, n }
        let t = b.get("t").and_then(Value::as_i64).ok_or_else(|| anyhow!("massive bar time"))?;
        bars.push(Bar {
            ts: OffsetDateTime::from_unix_timestamp_nanos(t as i128 * 1_000_000)?,
            open: b.get("o").and_then(f).unwrap_or(0.0),
            high: b.get("h").and_then(f).unwrap_or(0.0),
            low: b.get("l").and_then(f).unwrap_or(0.0),
            close: b.get("c").and_then(f).unwrap_or(0.0),
            volume: b.get("v").and_then(f).unwrap_or(0.0),
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        });
    }
    Ok(Chunk { bars })
}

// ── Massive futures (separate service; CT sessions, ns timestamps) ──────────────────

/// Map our canonical timeframe to a futures `resolution` string. Futures uses `session` for the
/// daily candle (a trading session) and has no native weekly rollup below month, so `1w` maps to
/// `1week` which the API does support.
fn massive_futures_resolution(tf: &str) -> &'static str {
    match tf {
        "1m" => "1min",
        "5m" => "5min",
        "15m" => "15min",
        "1h" => "1hour",
        "1w" => "1week",
        _ => "1session",
    }
}

async fn fetch_massive_futures(
    client: &reqwest::Client,
    key: &str,
    ticker: &str,
    timeframe: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Chunk> {
    // Futures tickers embed the contract month (e.g. GCJ5 = April 2025 gold); pass through
    // uppercased, no prefix. window_start bounds are nanosecond Unix timestamps; the range is
    // [from, to) to match the worker's non-overlapping paging (gte/lt).
    let sym = ticker.trim().to_ascii_uppercase();
    let res = massive_futures_resolution(timeframe);
    let from_ns = from.unix_timestamp_nanos();
    let to_ns = to.unix_timestamp_nanos();
    let url = format!(
        "https://api.massive.com/futures/v1/aggs/{sym}\
         ?resolution={res}&window_start.gte={from_ns}&window_start.lt={to_ns}\
         &sort=window_start.asc&limit=50000&apiKey={key}"
    );
    let body = massive_get(client, &url).await?;
    let Some(arr) = body.get("results").and_then(Value::as_array) else {
        return Ok(Chunk { bars: Vec::new() });
    };

    let mut bars = Vec::with_capacity(arr.len());
    for b in arr {
        // { window_start: ns, open, high, low, close, volume, ... }
        let ns = b.get("window_start").and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("massive futures bar time"))?;
        bars.push(Bar {
            ts: OffsetDateTime::from_unix_timestamp_nanos(ns as i128)?,
            open: b.get("open").and_then(f).unwrap_or(0.0),
            high: b.get("high").and_then(f).unwrap_or(0.0),
            low: b.get("low").and_then(f).unwrap_or(0.0),
            close: b.get("close").and_then(f).unwrap_or(0.0),
            volume: b.get("volume").and_then(f).unwrap_or(0.0),
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        });
    }
    Ok(Chunk { bars })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn massive_futures_resolution_maps() {
        assert_eq!(massive_futures_resolution("1d"), "1session");
        assert_eq!(massive_futures_resolution("1h"), "1hour");
        assert_eq!(massive_futures_resolution("5m"), "5min");
        assert_eq!(massive_futures_resolution("1w"), "1week");
    }

    #[test]
    fn massive_ticker_prefixes_per_market() {
        // Equity/ETF: bare, uppercased. Options: O:+OCC. Crypto/fx/index carry their marker.
        assert_eq!(massive_ticker("aapl", "equity").unwrap(), "AAPL");
        assert_eq!(massive_ticker("SPY260706C00500000", "option").unwrap(), "O:SPY260706C00500000");
        assert_eq!(massive_ticker("btcusd", "crypto").unwrap(), "X:BTCUSD");
        assert_eq!(massive_ticker("eurusd", "fx").unwrap(), "C:EURUSD");
        assert_eq!(massive_ticker("spx", "index").unwrap(), "I:SPX");
        // Already-prefixed input isn't double-prefixed (append replays stored symbols).
        assert_eq!(massive_ticker("X:BTCUSD", "crypto").unwrap(), "X:BTCUSD");
        assert!(massive_ticker("NOTANOPTION", "option").is_err());
    }
}
