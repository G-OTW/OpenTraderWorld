//! TradeStation v3 bar charts — equities, options and futures, on the account's own key.
//!
//! `GET /v3/marketdata/barcharts/{symbol}`, authenticated with the same OAuth refresh token
//! the [broker account](crate::brokers) uses: TradeStation issues no market-data-only
//! credential, and the `MarketData` scope rides on the same key.
//!
//! Two properties of the feed shape what this connector offers:
//!
//!   - **A bar is stamped at its close, not at its open.** TradeStation timestamps an
//!     interval with the moment it finished; every dataset here stores the moment it
//!     started, so the interval is subtracted. A daily bar is stamped at the session close,
//!     so its open is that session's date at midnight UTC.
//!   - **Intraday bars are anchored on the session, not on the epoch.** A US equity session
//!     opens at 9:30, so a 60-minute bar runs 9:30 to 10:30 and does not line up with the
//!     hourly candle every other provider here stores. One, five and fifteen minutes divide
//!     that opening evenly and do line up; the hourly, four-hourly and weekly ones are
//!     refused rather than stored a half-hour out of step with the rest of the library.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, Time};

use otw_store::histdata::Bar;

use crate::brokers::tradestation as ts;

use super::{Capability, Chunk, ConfigField, Connector, SymbolHit};

pub struct TradeStation;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or sim. Simulated trading is a different host with its own key. Leave empty \
           for live.",
}];

static CAP: Capability = Capability {
    provider: "tradestation",
    label: "TradeStation",
    website: "https://www.tradestation.com",
    docs_url: "https://api.tradestation.com/docs/",
    rate_limit: "500 bar-chart requests per 5 minutes, plus a credit allowance for how much \
                 history is asked for per minute. One intraday request returns at most \
                 57,600 bars.",
    required_secrets: &["client_id", "client_secret", "refresh_token"],
    asset_types: &["equity", "etf", "option", "future", "index"],
    // See the module note: the hourly, four-hourly and weekly bars are anchored on the
    // session rather than on the epoch, so they are not offered.
    timeframes: &["1m", "5m", "15m", "1d"],
    // Bar charts are not split-adjusted on this endpoint.
    adjusted: false,
    max_bars_per_req: 57_600,
    // 500 requests per 5 minutes is 1.67 a second; 600 ms leaves room for the account side.
    min_interval_ms: 600,
    searchable: true,
    config_fields: FIELDS,
    testable: true,
    // TradeStation streams over a long-lived HTTP response, not a WebSocket: see the note.
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "TradeStation streams bars and quotes over a long-lived HTTP response \
                  rather than a WebSocket, which this app's live charts do not speak. \
                  History downloads and the account side work; the live candle does not.",
};

#[async_trait::async_trait]
impl Connector for TradeStation {
    fn capability(&self) -> &'static Capability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => Ok(()),
            Some(v) if v.is_empty() || v == "live" || v == "sim" => Ok(()),
            Some(other) => Err(anyhow!(
                "the TradeStation environment is \"live\" or \"sim\", not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let body = ts::get(client, secrets, "/v3/marketdata/symbols/SPY", &[]).await?;
        let row = body
            .get("Symbols")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .ok_or_else(|| {
                anyhow!(
                    "the key reached TradeStation but market data answered nothing. Check \
                     the sign-in granted the MarketData scope."
                )
            })?;
        Ok(format!(
            "market data reached, {} priced on {} in {}",
            row.get("Symbol").and_then(Value::as_str).unwrap_or("SPY"),
            row.get("Exchange").and_then(Value::as_str).unwrap_or(""),
            row.get("Currency").and_then(Value::as_str).unwrap_or(""),
        ))
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
        let symbol = ticker.trim().to_uppercase();
        if symbol.is_empty() {
            return Err(anyhow!(
                "name the TradeStation symbol to download (AAPL, @ES, ESH26)"
            ));
        }
        let (unit, interval) = bar_unit(timeframe)?;
        let body = ts::get(
            client,
            secrets,
            &format!("/v3/marketdata/barcharts/{}", super::enc(&symbol)),
            &[
                ("unit".into(), unit.to_string()),
                ("interval".into(), interval.to_string()),
                ("firstdate".into(), from.format(&Rfc3339)?),
                ("lastdate".into(), to.format(&Rfc3339)?),
            ],
        )
        .await?;
        let rows = body
            .get("Bars")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("TradeStation answered no bar list for {symbol}"))?;
        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            // The forming bar is not a bar yet: storing it would persist a partial one that
            // nothing later would come back to correct.
            if r.get("BarStatus").and_then(Value::as_str) == Some("Open") {
                continue;
            }
            let close_ts = OffsetDateTime::parse(
                r.get("TimeStamp").and_then(Value::as_str).unwrap_or_default(),
                &Rfc3339,
            )
            .map_err(|e| anyhow!("TradeStation bar timestamp: {e}"))?;
            let num = |key: &str| -> Result<f64> {
                r.get(key)
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| anyhow!("TradeStation bar field {key}"))
            };
            bars.push(Bar {
                ts: opened_at(close_ts, unit, interval),
                open: num("Open")?,
                high: num("High")?,
                low: num("Low")?,
                close: num("Close")?,
                volume: num("TotalVolume").unwrap_or(0.0),
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            });
        }
        bars.sort_by_key(|b| b.ts);
        Ok(Chunk { bars })
    }

    /// TradeStation has no free-text symbol search on this API, but it will say whether a
    /// symbol exists and what it is, which is what a picker needs to confirm a guess.
    async fn search_symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        query: &str,
        _asset_type: &str,
        _limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        let symbol = query.trim().to_uppercase();
        if symbol.is_empty() {
            return Ok(Vec::new());
        }
        let body = ts::get(
            client,
            secrets,
            &format!("/v3/marketdata/symbols/{}", super::enc(&symbol)),
            &[],
        )
        .await?;
        Ok(body
            .get("Symbols")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| {
                Some(SymbolHit {
                    symbol: s.get("Symbol").and_then(Value::as_str)?.to_string(),
                    name: s.get("Description").and_then(Value::as_str).unwrap_or("").to_string(),
                    asset_type: asset_type_of(
                        s.get("AssetType").and_then(Value::as_str).unwrap_or(""),
                    )
                    .to_string(),
                    exchange: s.get("Exchange").and_then(Value::as_str).unwrap_or("").to_string(),
                })
            })
            .collect())
    }
}

/// Canonical timeframe → TradeStation `unit` and `interval`.
fn bar_unit(tf: &str) -> Result<(&'static str, u32)> {
    Ok(match tf {
        "1m" => ("Minute", 1),
        "5m" => ("Minute", 5),
        "15m" => ("Minute", 15),
        "1d" => ("Daily", 1),
        "1h" | "4h" => {
            return Err(anyhow!(
                "TradeStation builds intraday bars from the session open, so its {tf} candle \
                 starts at 9:30 rather than on the hour and would not line up with the rest \
                 of the library. Download 15m and read it at {tf} instead."
            ))
        }
        other => return Err(anyhow!("TradeStation does not serve a {other} candle here")),
    })
}

/// When a bar opened, from the moment TradeStation says it closed.
///
/// An intraday bar covers the interval that ends at its stamp, so the interval is
/// subtracted. A daily bar is stamped at the session close, which is the same calendar day
/// in UTC for the markets this serves, so its open is that date at midnight.
fn opened_at(close: OffsetDateTime, unit: &str, interval: u32) -> OffsetDateTime {
    match unit {
        "Minute" => close - time::Duration::minutes(interval as i64),
        _ => close.replace_time(Time::MIDNIGHT),
    }
}

/// TradeStation's asset type in the download form's vocabulary.
fn asset_type_of(asset_type: &str) -> &'static str {
    match asset_type.to_ascii_uppercase().as_str() {
        "STOCK" => "equity",
        "STOCKOPTION" | "INDEXOPTION" => "option",
        "FUTURE" => "future",
        "INDEX" => "index",
        _ => "equity",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_is_stored_at_its_open_not_at_its_close() {
        let close = OffsetDateTime::parse("2026-09-21T14:35:00Z", &Rfc3339).unwrap();
        // A five-minute bar stamped 14:35 covers 14:30 to 14:35.
        let open = opened_at(close, "Minute", 5);
        assert_eq!(open.format(&Rfc3339).unwrap(), "2026-09-21T14:30:00Z");
        // A daily bar stamped at the session close belongs to that session's date.
        let day = OffsetDateTime::parse("2026-09-21T20:00:00Z", &Rfc3339).unwrap();
        assert_eq!(
            opened_at(day, "Daily", 1).format(&Rfc3339).unwrap(),
            "2026-09-21T00:00:00Z"
        );
    }

    #[test]
    fn a_session_anchored_timeframe_is_refused_by_name() {
        assert_eq!(bar_unit("15m").unwrap(), ("Minute", 15));
        assert_eq!(bar_unit("1d").unwrap(), ("Daily", 1));
        // The refusal says what to download instead rather than storing a bar half an hour
        // out of step with every other provider's.
        let e = bar_unit("1h").unwrap_err();
        assert!(format!("{e}").contains("15m"), "{e}");
        assert!(bar_unit("1w").is_err());
    }
}
