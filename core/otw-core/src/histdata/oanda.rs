//! OANDA v20 candles — FX and CFDs, from the same personal access token the account uses.
//!
//! `GET /v3/accounts/{id}/instruments/{instrument}/candles`. Three things shape this
//! connector:
//!
//!   - **The token is the account's.** OANDA has no market-data-only credential and no
//!     keyless endpoint, so the connector asks for the same token plus the account number.
//!     The environment (fxTrade or fxPractice) is a host, not a flag.
//!   - **A day is not a day by default.** Daily and weekly candles align to 17:00
//!     America/New_York unless told otherwise, which is the FX session but not the epoch
//!     day every other dataset here is stored on. `dailyAlignment=0` and
//!     `alignmentTimezone=UTC` pin them to midnight UTC, and the weekly one to Monday, so a
//!     series downloaded here lines up with one downloaded anywhere else.
//!   - **Only complete candles are stored.** The last row of a live window is the bar being
//!     formed; writing it would persist a partial bar that the next append would not know
//!     to correct.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use super::{Capability, Chunk, ConfigField, Connector, SymbolHit};

const LIVE: &str = "https://api-fxtrade.oanda.com";
const PRACTICE: &str = "https://api-fxpractice.oanda.com";

pub struct Oanda;

static FIELDS: &[ConfigField] = &[
    ConfigField {
        name: "account_id",
        label: "Account ID",
        placeholder: "001-004-1234567-001",
        kind: "text",
        required: true,
        help: "The v20 account number the candles are read through. OANDA serves prices per \
               account, because which instruments exist depends on the division the account \
               is in.",
    },
    ConfigField {
        name: "env",
        label: "Environment",
        placeholder: "live",
        kind: "text",
        required: false,
        help: "live or practice. fxTrade and fxPractice are different hosts with different \
               tokens and different account numbers. Leave empty for live.",
    },
];

static CAP: Capability = Capability {
    provider: "oanda",
    label: "OANDA",
    website: "https://www.oanda.com",
    docs_url: "https://developer.oanda.com/rest-live-v20/pricing-ep/",
    rate_limit: "120 requests per second per token. A candle request returns up to 5000 bars, \
                 so a long history is a few dozen calls.",
    required_secrets: &["api_token"],
    // CURRENCY instruments are fx; the index, commodity and metal CFDs are not an asset type
    // of their own here, so they are downloaded as fx pairs of their quote currency would be.
    asset_types: &["fx"],
    timeframes: &["1m", "5m", "15m", "1h", "4h", "1d", "1w"],
    adjusted: false,
    max_bars_per_req: 5000,
    // 120 req/s is the published ceiling; 10 a second is plenty for a download and leaves
    // the account's own calls room.
    min_interval_ms: 100,
    searchable: true,
    config_fields: FIELDS,
    testable: true,
    // Live prices are an HTTP stream, not a WebSocket: see `stream_note`.
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "OANDA streams prices over a long-lived HTTP response rather than a \
                  WebSocket, which this app's live charts do not speak. History downloads \
                  and the account side work; the live candle does not.",
};

#[async_trait::async_trait]
impl Connector for Oanda {
    fn capability(&self) -> &'static Capability {
        &CAP
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        match config.get("env").map(|s| s.trim().to_ascii_lowercase()) {
            None => {}
            Some(v) if v.is_empty() || v == "live" || v == "practice" => {}
            Some(other) => {
                return Err(anyhow!(
                    "the OANDA environment is \"live\" or \"practice\", not \"{other}\""
                ))
            }
        }
        if let Some(id) = config.get("account_id").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if id.split('-').count() != 4 || !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
                return Err(anyhow!(
                    "an OANDA account id looks like 001-004-1234567-001, not \"{id}\""
                ));
            }
        }
        Ok(())
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let body = get(client, secrets, &format!("{}/instruments", account_path(secrets)?), &[])
            .await?;
        let n = body
            .get("instruments")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        Ok(format!(
            "{} account {}, {n} instrument(s) priced",
            env_label(secrets),
            crate::brokers::require(secrets, "account_id")?
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
        let instrument = ticker.trim().to_uppercase();
        if instrument.is_empty() {
            return Err(anyhow!("name the OANDA instrument to download (EUR_USD, SPX500_USD)"));
        }
        let body = get(
            client,
            secrets,
            &format!(
                "{}/instruments/{}/candles",
                account_path(secrets)?,
                super::enc(&instrument)
            ),
            &[
                ("price".into(), "M".into()),
                ("granularity".into(), granularity(timeframe)?.into()),
                ("from".into(), from.format(&Rfc3339)?),
                ("to".into(), to.format(&Rfc3339)?),
                // Midnight UTC, Monday weeks: the anchoring every other dataset here uses.
                // OANDA's own default is the 17:00 New York session roll.
                ("dailyAlignment".into(), "0".into()),
                ("alignmentTimezone".into(), "UTC".into()),
                ("weeklyAlignment".into(), "Monday".into()),
            ],
        )
        .await?;
        let rows = body
            .get("candles")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("OANDA answered no candle list for {instrument}"))?;
        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            // The forming candle is not a bar yet: storing it would persist a partial one
            // nothing later would come back to correct.
            if !r.get("complete").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let mid = r
                .get("mid")
                .ok_or_else(|| anyhow!("OANDA candle carries no midpoint prices"))?;
            let ts = OffsetDateTime::parse(
                r.get("time").and_then(Value::as_str).unwrap_or_default(),
                &Rfc3339,
            )
            .map_err(|e| anyhow!("OANDA candle time: {e}"))?;
            bars.push(Bar {
                ts,
                open: price(mid, "o")?,
                high: price(mid, "h")?,
                low: price(mid, "l")?,
                close: price(mid, "c")?,
                // "Number of prices created in the range": a tick count, the only volume a
                // dealing desk publishes.
                volume: r.get("volume").and_then(Value::as_f64).unwrap_or(0.0),
                adj_open: None,
                adj_high: None,
                adj_low: None,
                adj_close: None,
            });
        }
        bars.sort_by_key(|b| b.ts);
        Ok(Chunk { bars })
    }

    /// The account's own instrument list, fetched once per process and filtered locally: it
    /// is a few hundred names and it changes on the scale of months.
    async fn search_symbols(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        query: &str,
        _asset_type: &str,
        limit: usize,
    ) -> Result<Vec<SymbolHit>> {
        let key = format!("oanda:{}", crate::brokers::require(secrets, "account_id")?);
        if let Some(all) = super::cached_symbols(&key) {
            return Ok(super::filter_symbols(&all, query, limit));
        }
        let body = get(client, secrets, &format!("{}/instruments", account_path(secrets)?), &[])
            .await?;
        let all: Vec<SymbolHit> = body
            .get("instruments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|i| {
                let symbol = i.get("name").and_then(Value::as_str)?.to_string();
                Some(SymbolHit {
                    name: i.get("displayName").and_then(Value::as_str).unwrap_or("").to_string(),
                    asset_type: "fx".into(),
                    // The instrument's own kind, as OANDA files it: CURRENCY, CFD or METAL.
                    exchange: i.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
                    symbol,
                })
            })
            .collect();
        super::cache_symbols(&key, &all);
        Ok(super::filter_symbols(&all, query, limit))
    }
}

/// Canonical timeframe → OANDA granularity token.
fn granularity(tf: &str) -> Result<&'static str> {
    Ok(match tf {
        "1m" => "M1",
        "5m" => "M5",
        "15m" => "M15",
        "1h" => "H1",
        "4h" => "H4",
        "1d" => "D",
        "1w" => "W",
        other => return Err(anyhow!("OANDA does not serve a {other} candle")),
    })
}

fn price(mid: &Value, key: &str) -> Result<f64> {
    mid.get(key)
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("OANDA candle field {key}"))
}

fn base(secrets: &HashMap<String, String>) -> &'static str {
    match secrets.get("env").map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "practice" => PRACTICE,
        _ => LIVE,
    }
}

fn env_label(secrets: &HashMap<String, String>) -> &'static str {
    if base(secrets) == PRACTICE { "Practice" } else { "Live" }
}

fn account_path(secrets: &HashMap<String, String>) -> Result<String> {
    Ok(format!(
        "/v3/accounts/{}",
        super::enc(crate::brokers::require(secrets, "account_id")?)
    ))
}

async fn get(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    path: &str,
    params: &[(String, String)],
) -> Result<Value> {
    let token = crate::brokers::require(secrets, "api_token")?;
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", super::enc(k), super::enc(v)))
        .collect();
    let url = if query.is_empty() {
        format!("{}{path}", base(secrets))
    } else {
        format!("{}{path}?{}", base(secrets), query.join("&"))
    };
    let res = crate::rate::send(
        CAP.provider,
        client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept-Datetime-Format", "RFC3339"),
    )
    .await
    .context("oanda request")?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = super::trim_err(&body);
        return Err(match status.as_u16() {
            401 => anyhow!(
                "OANDA refused the token ({detail}). Check it was generated for the {} \
                 environment this connector is set to.",
                env_label(secrets).to_ascii_lowercase()
            ),
            400 | 404 => anyhow!(
                "OANDA does not serve this request ({detail}). Check the instrument is \
                 spelled as OANDA spells it (EUR_USD, SPX500_USD) and that the account id \
                 is right."
            ),
            _ => anyhow!("OANDA answered {status}: {detail}"),
        });
    }
    serde_json::from_str(&body).context("oanda decode")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forming_candle_is_not_stored() {
        let body = serde_json::json!({"candles": [
            {"time":"2026-09-21T00:00:00.000000000Z","complete":true,"volume":42,
             "mid":{"o":"1.1","h":"1.2","l":"1.0","c":"1.15"}},
            {"time":"2026-09-21T01:00:00.000000000Z","complete":false,"volume":3,
             "mid":{"o":"1.15","h":"1.16","l":"1.14","c":"1.16"}}
        ]});
        let rows = body.get("candles").unwrap().as_array().unwrap();
        let kept: Vec<_> = rows
            .iter()
            .filter(|r| r.get("complete").and_then(Value::as_bool).unwrap_or(false))
            .collect();
        assert_eq!(kept.len(), 1);
        let mid = kept[0].get("mid").unwrap();
        assert_eq!(price(mid, "o").unwrap(), 1.1);
        assert_eq!(price(mid, "c").unwrap(), 1.15);
    }

    #[test]
    fn timeframes_map_or_are_refused() {
        assert_eq!(granularity("1m").unwrap(), "M1");
        assert_eq!(granularity("4h").unwrap(), "H4");
        assert_eq!(granularity("1w").unwrap(), "W");
        assert!(granularity("30m").is_err());
    }
}
