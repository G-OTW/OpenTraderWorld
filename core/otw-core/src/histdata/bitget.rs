//! Bitget candles — keyless public OHLCV for spot and USDT-M futures.
//!
//! `GET /api/v2/spot/market/history-candles` and its `mix` twin. Three things shape this
//! connector:
//!
//!   - **The same ticker exists twice.** `BTCUSDT` is a spot pair *and* a perpetual, and
//!     they are different series with different prices. Which market a connector reads is a
//!     setting, not a guess at the ticker.
//!   - **A day is not the epoch day by default.** Bitget's `1day` candle opens at 16:00 UTC
//!     (midnight in UTC+8). The `utc` variants (`1Dutc`, `1Wutc`) open on midnight UTC and
//!     Monday, which is where every other dataset here is anchored, so those are what this
//!     connector asks for.
//!   - **History pages backwards.** `history-candles` answers the last N bars *before*
//!     `endTime`, 200 at a time. The worker's window never holds more than
//!     `max_bars_per_req` bars, so one call per window covers it and the rows before `from`
//!     are dropped.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{Capability, Chunk, ConfigField, Connector, SymbolHit};

const BASE: &str = "https://api.bitget.com";
/// How long after a period closes Bitget is still rewriting its candle. Measured at around
/// twenty-five seconds on the spot board: the bar for a minute that has just ended comes back
/// with part of its volume, then settles. Anything younger than this is left for the next
/// pass rather than stored half-built, which costs one late bar on a `1m` download and
/// nothing at all on a daily one.
const SETTLE_MS: i64 = 60_000;

/// The futures product type this connector serves; see `market`.
const FUTURES: &str = "USDT-FUTURES";

pub struct Bitget;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "market",
    label: "Market",
    placeholder: "spot",
    kind: "text",
    required: false,
    help: "spot or usdt-futures. BTCUSDT is a spot pair and a perpetual at once, and the \
           two are different series, so which one this connector downloads is stated rather \
           than inferred. Leave empty for spot.",
}];

static CAP: Capability = Capability {
    provider: "bitget",
    label: "Bitget",
    website: "https://www.bitget.com",
    docs_url: "https://www.bitget.com/docs/catalog/classic-spot-market/classic-spot-market",
    rate_limit: "Keyless. 20 requests/second per IP on the public candle endpoints, 6000 per \
                 minute overall. A candle page is 200 bars.",
    required_secrets: &[],
    asset_types: &["crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 200,
    // 20 req/s is the published ceiling; 5 a second leaves room and never trips it.
    min_interval_ms: 200,
    searchable: true,
    config_fields: FIELDS,
    testable: false,
    stream_asset_types: &["crypto"],
    // Bitget publishes a candle channel per interval, so every downloadable timeframe
    // streams natively. The daily and weekly channels are the UTC-anchored ones, like the
    // download.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles are public and keyless, like the history. The connector's \
                  Market setting picks the socket: spot and USDT-M futures are separate \
                  feeds for the same ticker.",
};

/// Which book a connector reads. Spot unless its setting says otherwise.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Market {
    Spot,
    Futures,
}

pub(crate) fn market(config: &HashMap<String, String>) -> Result<Market> {
    match config.get("market").map(|s| s.trim().to_ascii_lowercase()) {
        None => Ok(Market::Spot),
        Some(v) if v.is_empty() || v == "spot" => Ok(Market::Spot),
        Some(v) if v == "usdt-futures" || v == "futures" => Ok(Market::Futures),
        Some(other) => Err(anyhow!(
            "the Bitget market is \"spot\" or \"usdt-futures\", not \"{other}\""
        )),
    }
}

#[async_trait::async_trait]
impl Connector for Bitget {
    fn capability(&self) -> &'static Capability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        market(config).map(|_| ())
    }

    async fn fetch_chunk(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        _asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Chunk> {
        let m = market(secrets)?;
        let symbol = ticker.trim().to_uppercase();
        if symbol.is_empty() {
            return Err(anyhow!("name the Bitget pair to download (BTCUSDT, ETHUSDT)"));
        }
        let mut url = format!(
            "{BASE}/api/v2/{}/market/history-candles?symbol={}&granularity={}&endTime={}&limit={}",
            match m {
                Market::Spot => "spot",
                Market::Futures => "mix",
            },
            super::enc(&symbol),
            granularity(m, timeframe)?,
            ms(to),
            CAP.max_bars_per_req,
        );
        if m == Market::Futures {
            url.push_str(&format!("&productType={FUTURES}"));
        }
        let body: Value = crate::rate::send(CAP.provider, client.get(&url))
            .await
            .context("bitget request")?
            .json()
            .await
            .context("bitget decode")?;
        let code = body.get("code").and_then(Value::as_str).unwrap_or_default();
        if code != "00000" {
            let msg = body.get("msg").and_then(Value::as_str).unwrap_or_default();
            return Err(anyhow!(
                "Bitget refused this request ({code} {msg}). Check the pair is spelled as \
                 Bitget spells it (BTCUSDT) and that it trades on the connector's market."
            ));
        }
        let rows = body
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("Bitget answered no candle list for {symbol}"))?;
        let from_ms = ms(from);
        let to_ms = ms(to);
        // Bitget's candle carries neither a close time nor a confirm flag, and the archive
        // answers the period still running as if it were finished, so how old a bar is is
        // the only thing that says whether it is final.
        let step_ms = super::timeframe_secs(timeframe)? * 1000;
        let closed_before = ms(OffsetDateTime::now_utc()) - step_ms - SETTLE_MS;
        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            // [ openTime, open, high, low, close, baseVolume, quoteVolume, … ]
            let open_ms = r
                .get(0)
                .and_then(cell)
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or_else(|| anyhow!("bitget candle open time"))?;
            // The page ends at `to` but may start before `from`: the window is closed here.
            if open_ms < from_ms || open_ms >= to_ms {
                continue;
            }
            // A partial bar stored is a partial high, low, close and volume that nothing
            // later comes back to correct.
            if open_ms > closed_before {
                continue;
            }
            let num = |i: usize| -> Result<f64> {
                r.get(i)
                    .and_then(cell)
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| anyhow!("bitget candle field {i}"))
            };
            bars.push(Bar {
                ts: OffsetDateTime::from_unix_timestamp(open_ms / 1000)?,
                open: num(1)?,
                high: num(2)?,
                low: num(3)?,
                close: num(4)?,
                volume: num(5)?,
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            });
        }
        bars.sort_by_key(|b| b.ts);
        Ok(Chunk { bars })
    }

    /// Bitget publishes its whole universe in one keyless call, so the list is fetched once
    /// per process and per market, then filtered locally.
    async fn search_symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        query: &str,
        _asset_type: &str,
        limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        let m = market(secrets)?;
        let key = match m {
            Market::Spot => "bitget:spot",
            Market::Futures => "bitget:usdt-futures",
        };
        if let Some(all) = super::cached_symbols(key) {
            return Ok(super::filter_symbols(&all, query, limit));
        }
        let url = match m {
            Market::Spot => format!("{BASE}/api/v2/spot/public/symbols"),
            Market::Futures => format!("{BASE}/api/v2/mix/market/contracts?productType={FUTURES}"),
        };
        let body: Value = crate::rate::send(CAP.provider, client.get(&url))
            .await
            .context("bitget symbols request")?
            .json()
            .await
            .context("bitget symbols decode")?;
        let all: Vec<SymbolHit> = body
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|s| {
                // A delisted or halted pair is not worth offering: it downloads nothing.
                matches!(
                    s.get("status").or_else(|| s.get("symbolStatus")).and_then(Value::as_str),
                    None | Some("online") | Some("normal")
                )
            })
            .filter_map(|s| {
                let symbol = s.get("symbol").and_then(Value::as_str)?.to_string();
                let base = s.get("baseCoin").and_then(Value::as_str).unwrap_or("");
                let quote = s.get("quoteCoin").and_then(Value::as_str).unwrap_or("");
                Some(SymbolHit {
                    symbol,
                    name: format!("{base}/{quote}"),
                    asset_type: "crypto".into(),
                    exchange: match m {
                        Market::Spot => "Bitget".into(),
                        Market::Futures => "Bitget USDT-M futures".into(),
                    },
                })
            })
            .collect();
        super::cache_symbols(key, &all);
        Ok(super::filter_symbols(&all, query, limit))
    }
}

/// Canonical timeframe → Bitget granularity token. The two books spell them differently,
/// and the daily and weekly ones are the UTC-anchored variants on purpose: Bitget's plain
/// `1day` opens at 16:00 UTC.
pub(crate) fn granularity(m: Market, tf: &str) -> Result<&'static str> {
    Ok(match (m, tf) {
        (Market::Spot, "1m") => "1min",
        (Market::Spot, "5m") => "5min",
        (Market::Spot, "15m") => "15min",
        (Market::Spot, "1h") => "1h",
        (Market::Spot, "4h") => "4h",
        (Market::Spot, "1d") => "1Dutc",
        (Market::Spot, "1w") => "1Wutc",
        (Market::Futures, "1m") => "1m",
        (Market::Futures, "5m") => "5m",
        (Market::Futures, "15m") => "15m",
        (Market::Futures, "1h") => "1H",
        (Market::Futures, "4h") => "4H",
        (Market::Futures, "1d") => "1Dutc",
        (Market::Futures, "1w") => "1Wutc",
        (_, other) => return Err(anyhow!("Bitget does not serve a {other} candle")),
    })
}

/// A candle cell is a string; a number is accepted too rather than dropped.
fn cell(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn ms(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_is_the_utc_day_on_both_books() {
        // Bitget's plain 1day/1D candle opens at 16:00 UTC; the utc variant is the one that
        // agrees with every other dataset here.
        assert_eq!(granularity(Market::Spot, "1d").unwrap(), "1Dutc");
        assert_eq!(granularity(Market::Futures, "1d").unwrap(), "1Dutc");
        // The two books do not spell the intraday ones the same way.
        assert_eq!(granularity(Market::Spot, "1m").unwrap(), "1min");
        assert_eq!(granularity(Market::Futures, "1m").unwrap(), "1m");
        assert!(granularity(Market::Spot, "30m").is_err());
    }

    #[test]
    fn the_market_setting_is_checked_rather_than_guessed() {
        assert!(matches!(market(&HashMap::new()).unwrap(), Market::Spot));
        let f = HashMap::from([("market".to_string(), "usdt-futures".to_string())]);
        assert!(matches!(market(&f).unwrap(), Market::Futures));
        let bad = HashMap::from([("market".to_string(), "coin-futures".to_string())]);
        assert!(market(&bad).is_err());
    }
}
