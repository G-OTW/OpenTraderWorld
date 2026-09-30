//! FOREX.com / StoneX price bars — FX and CFD OHLC on the account's own login.
//!
//! `GET /market/{MarketId}/barhistorybetween`, authenticated with the same session the
//! [broker account](crate::brokers) opens: StoneX issues no market-data-only credential, so
//! the connector asks for the same username, password and AppKey.
//!
//! Three properties shape it:
//!
//!   - **A market is a number.** The API takes a `MarketId`; a human types `EUR/USD`. The
//!     name is resolved against StoneX's own search and, when it matches more than one
//!     market, the error lists them: a ticker that cannot be resolved is a refusal naming
//!     the fix, never a best-effort pick.
//!   - **A bar is stamped at its open**, which is what every dataset here stores, so nothing
//!     is shifted. There is **no volume**: a dealing desk publishes prices, not size, and the
//!     field is left at zero rather than filled with a plausible number.
//!   - **A daily bar is the venue's session, not the UTC day.** StoneX rolls its FX day at
//!     the New York close, so a `1d` bar here opens in the evening of the previous calendar
//!     day. That is the period the venue actually traded, and it is stored as such; it is
//!     also why a daily series from here does not sit on top of one from a UTC-day provider.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::Value;
use time::OffsetDateTime;

use otw_store::histdata::Bar;

use crate::brokers::forexcom as fx;

use super::{Capability, Chunk, Connector, SymbolHit};

pub struct ForexCom;

static CAP: Capability = Capability {
    provider: "forexcom",
    label: "FOREX.com (StoneX)",
    website: "https://www.forex.com",
    docs_url: "https://docs.labs.gaincapital.com/",
    rate_limit: "500 requests per 5 seconds. One bar request returns at most 4000 bars, and \
                 the session token is shared with the broker account on the same login.",
    required_secrets: &["username", "password", "app_key"],
    asset_types: &["fx"],
    // See the module note: the four-hourly and weekly bars have no documented anchor here,
    // so they are refused rather than stored out of step.
    timeframes: &["1m", "5m", "15m", "1h", "1d"],
    adjusted: false,
    max_bars_per_req: 4000,
    // 500 requests per 5 seconds is far more than a download needs; stay gentle, the same
    // session serves the account side.
    min_interval_ms: 200,
    searchable: true,
    config_fields: &[],
    testable: true,
    // StoneX streams over Lightstreamer, not a WebSocket: see `stream_note`.
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "FOREX.com streams prices over Lightstreamer rather than a WebSocket, which \
                  this app's live charts do not speak. History downloads and the account \
                  side work; the live candle does not.",
};

#[async_trait::async_trait]
impl Connector for ForexCom {
    fn capability(&self) -> &'static Capability {
        &CAP
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let accounts = fx::account_tree(client, secrets).await?;
        let markets = search(client, secrets, &accounts.client_id, "EUR/USD", 5).await?;
        Ok(format!(
            "signed in as client {}, {} market(s) match EUR/USD",
            accounts.client_id,
            markets.len()
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
        let (interval, span) = bar_interval(timeframe)?;
        let market_id = resolve(client, secrets, ticker).await?;
        let body = fx::get(
            client,
            secrets,
            &format!("/market/{}/barhistorybetween", super::enc(&market_id)),
            &[
                ("interval".into(), interval.to_string()),
                ("span".into(), span.to_string()),
                ("fromTimestampUTC".into(), from.unix_timestamp().to_string()),
                ("toTimestampUTC".into(), to.unix_timestamp().to_string()),
                ("maxResults".into(), CAP.max_bars_per_req.to_string()),
                // The mid, so a stored bar is not one side of the spread.
                ("priceType".into(), "MID".into()),
            ],
        )
        .await?;
        // `PartialPriceBar` is the period still forming and is deliberately left out:
        // storing it would persist a bar nothing later comes back to correct.
        let rows = body
            .get("PriceBars")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("FOREX.com answered no bar list for {ticker}"))?;
        let mut bars = Vec::with_capacity(rows.len());
        for r in rows {
            let ts = fx::wcf_date(r, "BarDate")
                .ok_or_else(|| anyhow!("FOREX.com bar carries no readable date"))?;
            bars.push(Bar {
                ts,
                open: fx::num(r, "Open"),
                high: fx::num(r, "High"),
                low: fx::num(r, "Low"),
                close: fx::num(r, "Close"),
                // StoneX publishes no size on a price bar, and zero says so rather than
                // inventing one.
                volume: 0.0,
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
        let accounts = fx::account_tree(client, secrets).await?;
        Ok(search(client, secrets, &accounts.client_id, query, limit)
            .await?
            .into_iter()
            .map(|(id, name)| SymbolHit {
                symbol: name.clone(),
                // The id is what the API takes, so it belongs on the line: a name that
                // matches two markets is typed as its number instead.
                name: format!("{name} (market {id})"),
                asset_type: "fx".into(),
                exchange: "FOREX.com".into(),
            })
            .collect())
    }
}

/// The market id behind a ticker: the number itself when one was typed, otherwise the one
/// market whose name matches. Two matches is an error listing them, never a pick.
async fn resolve(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    ticker: &str,
) -> Result<String> {
    let wanted = ticker.trim();
    if wanted.is_empty() {
        return Err(anyhow!(
            "name the FOREX.com market to download (EUR/USD, or its market id)"
        ));
    }
    if wanted.chars().all(|c| c.is_ascii_digit()) {
        return Ok(wanted.to_string());
    }
    let accounts = fx::account_tree(client, secrets).await?;
    let hits = search(client, secrets, &accounts.client_id, wanted, 20).await?;
    let exact: Vec<&(String, String)> = hits
        .iter()
        .filter(|(_, name)| name.eq_ignore_ascii_case(wanted))
        .collect();
    match exact.as_slice() {
        [(id, _)] => Ok(id.clone()),
        [] => Err(anyhow!(
            "FOREX.com lists no market called \"{wanted}\". Its search answered [{}].",
            hits.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>().join(", ")
        )),
        many => Err(anyhow!(
            "\"{wanted}\" matches {} markets on this account. Download by market id instead: \
             [{}].",
            many.len(),
            many.iter().map(|(i, n)| format!("{n} = {i}")).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Markets whose name starts with `query`, as (id, name).
async fn search(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    client_account_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<(String, String)>> {
    let body = fx::get(
        client,
        secrets,
        "/market/search",
        &[
            ("searchByMarketName".into(), "true".into()),
            ("searchByMarketCode".into(), "true".into()),
            ("cfdProductType".into(), "true".into()),
            ("query".into(), query.trim().to_string()),
            ("maxResults".into(), limit.clamp(1, 200).to_string()),
            ("ClientAccountId".into(), client_account_id.to_string()),
        ],
    )
    .await?;
    Ok(body
        .get("Markets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = fx::num(m, "MarketId");
            let name = m.get("Name").and_then(Value::as_str)?.to_string();
            (id > 0.0).then(|| (format!("{id:.0}"), name))
        })
        .collect())
}

/// Canonical timeframe → StoneX `interval` and `span`.
fn bar_interval(tf: &str) -> Result<(&'static str, u32)> {
    Ok(match tf {
        "1m" => ("MINUTE", 1),
        "5m" => ("MINUTE", 5),
        "15m" => ("MINUTE", 15),
        "1h" => ("HOUR", 1),
        "1d" => ("DAY", 1),
        "4h" => {
            return Err(anyhow!(
                "FOREX.com does not say where it starts counting a four-hour bar, so it is \
                 not offered rather than stored out of step. Download 1h and read it at 4h \
                 instead."
            ))
        }
        other => return Err(anyhow!("FOREX.com does not serve a {other} candle here")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timeframe_without_a_known_anchor_is_refused_by_name() {
        assert_eq!(bar_interval("15m").unwrap(), ("MINUTE", 15));
        assert_eq!(bar_interval("1h").unwrap(), ("HOUR", 1));
        assert_eq!(bar_interval("1d").unwrap(), ("DAY", 1));
        let e = bar_interval("4h").unwrap_err();
        assert!(format!("{e}").contains("1h"), "{e}");
        assert!(bar_interval("1w").is_err());
    }
}
