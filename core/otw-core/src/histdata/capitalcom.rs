//! Capital.com historical prices — CFDs on FX, indices, commodities, shares and crypto.
//!
//! `GET /api/v1/prices/{epic}`, signed with the same session the
//! [broker account](crate::brokers) opens: Capital.com issues no market-data-only key.
//!
//! Three properties shape it:
//!
//!   - **Every price has two sides.** A candle arrives as a bid OHLC and an ask OHLC, and
//!     what is stored is the **mid** of the two, which is the number a chart is read on and
//!     the one the live feed folds to as well.
//!   - **An instrument is an epic**, Capital.com's own name for a market (`GOLD`,
//!     `EURUSD`, `US500`). It is what the API takes, and the symbol search returns it.
//!   - **A daily bar is the venue's session, not the UTC day.** A CFD desk rolls its day at
//!     its own hour, and `snapshotTimeUTC` is the period the venue actually traded. It is
//!     stored as such, which is why a daily series from here does not sit on top of one from
//!     a UTC-day provider.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use crate::brokers::capitalcom as cc;

use super::{Capability, Chunk, ConfigField, Connector, SymbolHit};

pub struct CapitalCom;

static FIELDS: &[ConfigField] = &[ConfigField {
    name: "env",
    label: "Environment",
    placeholder: "live",
    kind: "text",
    required: false,
    help: "live or demo. The demo platform is a different host with its own key. Leave empty \
           for live.",
}];

static CAP: Capability = Capability {
    provider: "capitalcom",
    label: "Capital.com",
    website: "https://capital.com",
    docs_url: "https://open-api.capital.com/",
    rate_limit: "10 requests per second per user, and one sign-in per second. One price \
                 request returns at most 1000 bars.",
    required_secrets: &["api_key", "identifier", "api_password"],
    asset_types: &["fx", "index", "equity", "crypto"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 1000,
    // 10 requests a second is the ceiling; 5 a second leaves the account side room.
    min_interval_ms: 200,
    searchable: true,
    config_fields: FIELDS,
    testable: true,
    stream_asset_types: &["fx", "index", "equity", "crypto"],
    // Capital.com publishes a candle channel per resolution, and the ones it serves are the
    // ones downloaded here.
    stream_timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    stream_note: "Live candles ride the same login as the history and count against the same \
                  session, which allows 40 instruments at once. Capital.com streams the bid \
                  and the ask as two separate candles and the chart shows their mid, so a \
                  pane fills once both sides have ticked.",
};

#[async_trait::async_trait]
impl Connector for CapitalCom {
    fn capability(&self) -> &'static Capability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => Ok(()),
            Some(v) if v.is_empty() || v == "live" || v == "demo" => Ok(()),
            Some(other) => Err(anyhow!(
                "the Capital.com environment is \"live\" or \"demo\", not \"{other}\""
            )),
        }
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let body = cc::get(
            client,
            secrets,
            "/api/v1/markets",
            &[("searchTerm".into(), "gold".into())],
        )
        .await?;
        let n = body.get("markets").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        Ok(format!("sign-in accepted, {n} market(s) match \"gold\""))
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
        let epic = ticker.trim().to_uppercase();
        if epic.is_empty() {
            return Err(anyhow!(
                "name the Capital.com epic to download (GOLD, EURUSD, US500)"
            ));
        }
        let body = cc::get(
            client,
            secrets,
            &format!("/api/v1/prices/{}", super::enc(&epic)),
            &[
                ("resolution".into(), resolution(timeframe)?.into()),
                ("from".into(), cc::local_stamp(from)?),
                ("to".into(), cc::local_stamp(to)?),
                ("max".into(), CAP.max_bars_per_req.to_string()),
            ],
        )
        .await?;
        let rows = body
            .get("prices")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("Capital.com answered no price list for {epic}"))?;
        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            let ts = cc::utc_stamp(r, "snapshotTimeUTC")
                .ok_or_else(|| anyhow!("Capital.com price carries no readable time"))?;
            bars.push(Bar {
                ts,
                open: mid(r, "openPrice")?,
                high: mid(r, "highPrice")?,
                low: mid(r, "lowPrice")?,
                close: mid(r, "closePrice")?,
                volume: cc::num(r, "lastTradedVolume"),
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            });
        }
        bars.sort_by_key(|b| b.ts);
        Ok(Chunk { bars })
    }

    async fn search_symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        query: &str,
        _asset_type: &str,
        limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let body = cc::get(
            client,
            secrets,
            "/api/v1/markets",
            &[("searchTerm".into(), q.to_string())],
        )
        .await?;
        Ok(body
            .get("markets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(limit)
            .filter_map(|m| {
                let kind = m.get("instrumentType").and_then(Value::as_str).unwrap_or("");
                Some(SymbolHit {
                    symbol: m.get("epic").and_then(Value::as_str)?.to_string(),
                    name: m
                        .get("instrumentName")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    asset_type: asset_type_of(kind).to_string(),
                    // Every one of them is a CFD, and the line says which kind.
                    exchange: format!("Capital.com {kind}"),
                })
            })
            .collect())
    }
}

/// The mid of the two sides Capital.com deals on, which is what a chart is read at.
fn mid(row: &Value, key: &str) -> Result<f64> {
    let side = row
        .get(key)
        .ok_or_else(|| anyhow!("Capital.com price has no {key}"))?;
    let (bid, ask) = (cc::num(side, "bid"), cc::num(side, "ask"));
    match (bid > 0.0, ask > 0.0) {
        (true, true) => Ok((bid + ask) / 2.0),
        // One side only: the number is still the venue's, so it is used rather than dropped.
        (true, false) => Ok(bid),
        (false, true) => Ok(ask),
        _ => Err(anyhow!("Capital.com price {key} carries neither side")),
    }
}

/// Canonical timeframe → Capital.com resolution.
pub(crate) fn resolution(tf: &str) -> Result<&'static str> {
    Ok(match tf {
        "1m" => "MINUTE",
        "5m" => "MINUTE_5",
        "15m" => "MINUTE_15",
        "1h" => "HOUR",
        "4h" => "HOUR_4",
        "1d" => "DAY",
        "1w" => "WEEK",
        other => return Err(anyhow!("Capital.com does not serve a {other} candle")),
    })
}

/// Capital.com's instrument type in the download form's vocabulary.
fn asset_type_of(instrument_type: &str) -> &'static str {
    match instrument_type.to_ascii_uppercase().as_str() {
        "CURRENCIES" => "fx",
        "INDICES" => "index",
        "CRYPTOCURRENCIES" => "crypto",
        _ => "equity",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_candle_is_the_mid_of_the_two_sides() {
        // The docs' own worked example.
        let row = serde_json::json!({
            "snapshotTime": "2022-04-06T15:18:00", "snapshotTimeUTC": "2022-04-06T13:18:00",
            "openPrice": {"bid": 24.356, "ask": 24.376},
            "closePrice": {"bid": 24.378, "ask": 24.398},
            "highPrice": {"bid": 24.378, "ask": 24.398},
            "lowPrice": {"bid": 24.355, "ask": 24.375},
            "lastTradedVolume": 187
        });
        assert!((mid(&row, "openPrice").unwrap() - 24.366).abs() < 1e-9);
        assert!((mid(&row, "lowPrice").unwrap() - 24.365).abs() < 1e-9);
        // One side only is still the venue's number.
        let one = serde_json::json!({"openPrice": {"bid": 10.0}});
        assert_eq!(mid(&one, "openPrice").unwrap(), 10.0);
        assert!(mid(&serde_json::json!({"openPrice": {}}), "openPrice").is_err());
    }

    #[test]
    fn timeframes_map_or_are_refused() {
        assert_eq!(resolution("15m").unwrap(), "MINUTE_15");
        assert_eq!(resolution("4h").unwrap(), "HOUR_4");
        assert_eq!(resolution("1w").unwrap(), "WEEK");
        assert!(resolution("30m").is_err());
    }
}
