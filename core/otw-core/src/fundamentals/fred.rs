//! FRED (Federal Reserve Bank of St. Louis): the US macro catalog. Free key, 120 requests
//! per minute. It also mirrors most BLS, BEA and Census series, so one key covers the bulk
//! of the US Macro page.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use time::OffsetDateTime;

use crate::histdata::{enc, Capability, Chunk, Connector};
use otw_store::fundamentals::SeriesMeta;

pub const PROVIDER: &str = "fred";
const BASE: &str = "https://api.stlouisfed.org/fred";

pub struct Fred;

static CAP: Capability = Capability {
    provider: PROVIDER,
    label: "FRED (St. Louis Fed)",
    website: "https://fred.stlouisfed.org",
    docs_url: "https://fred.stlouisfed.org/docs/api/fred/",
    rate_limit: "Free key (fredaccount.stlouisfed.org): 120 requests per minute. One request per \
                 series refresh, one per search.",
    required_secrets: &["api_key"],
    asset_types: &[],
    timeframes: &[],
    adjusted: false,
    max_bars_per_req: 0,
    min_interval_ms: 500,
    searchable: false,
    config_fields: &[],
    testable: true,
    fundamentals: &["macro"],
    stream_asset_types: &[],
    stream_timeframes: &[],
    stream_note: "",
};

#[async_trait::async_trait]
impl Connector for Fred {
    fn capability(&self) -> &'static Capability {
        &CAP
    }

    async fn fetch_chunk(
        &self,
        _client: &reqwest::Client,
        _secrets: &HashMap<String, String>,
        _ticker: &str,
        _asset_type: &str,
        _timeframe: &str,
        _from: OffsetDateTime,
        _to: OffsetDateTime,
    ) -> Result<Chunk> {
        super::refuse_chunk(&CAP).await
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let s = meta(client, secrets, "GDP").await?;
        Ok(format!("key accepted: {} ({})", s.title, s.code))
    }
}

fn key(secrets: &HashMap<String, String>) -> Result<String> {
    secrets
        .get("api_key")
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            anyhow!("FRED needs an 'api_key' credential: get a free key at fredaccount.stlouisfed.org")
        })
}

async fn get(client: &reqwest::Client, url: &str) -> Result<Value> {
    // `without_url`: the key rides in the query string and must not reach a log or a row.
    let resp = crate::rate::send(PROVIDER, client.get(url).timeout(super::edgar::TIMEOUT))
        .await
        .map_err(|e| e.without_url())
        .context("FRED request")?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.without_url()).context("FRED response")?;
    if !status.is_success() {
        // FRED explains itself in `error_message` (bad key, unknown series id).
        let msg = body
            .get("error_message")
            .and_then(Value::as_str)
            .unwrap_or("request refused");
        return Err(anyhow!("FRED: {msg} (HTTP {})", status.as_u16()));
    }
    Ok(body)
}

fn frequency(short: &str) -> &'static str {
    match short {
        "D" => "D",
        "W" | "BW" => "W",
        "M" => "M",
        "Q" => "Q",
        "SA" | "A" => "A",
        _ => "",
    }
}

fn to_meta(s: &Value) -> Option<SeriesMeta> {
    let text = |k: &str| s.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let code = text("id");
    if code.is_empty() {
        return None;
    }
    Some(SeriesMeta {
        provider: PROVIDER.into(),
        code,
        title: text("title"),
        category: "other".into(),
        // FRED is US-centric but carries foreign series too; the title says which.
        country: "US".into(),
        unit: text("units_short"),
        frequency: frequency(&text("frequency_short")).into(),
        seasonal_adj: matches!(text("seasonal_adjustment_short").as_str(), "SA" | "SAAR"),
        notes: text("notes").chars().take(2000).collect(),
    })
}

pub async fn search(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    q: &str,
) -> Result<Vec<SeriesMeta>> {
    let key = key(secrets)?;
    let url = format!(
        "{BASE}/series/search?search_text={}&api_key={key}&file_type=json&limit=25\
         &order_by=popularity&sort_order=desc",
        enc(q)
    );
    let body = get(client, &url).await?;
    Ok(body
        .get("seriess")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(to_meta).collect())
        .unwrap_or_default())
}

pub async fn meta(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    code: &str,
) -> Result<SeriesMeta> {
    let key = key(secrets)?;
    let url = format!("{BASE}/series?series_id={}&api_key={key}&file_type=json", enc(code));
    let body = get(client, &url).await?;
    body.get("seriess")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(to_meta)
        .ok_or_else(|| anyhow!("FRED has no series {code}"))
}

pub async fn fetch(
    client: &reqwest::Client,
    secrets: &HashMap<String, String>,
    code: &str,
) -> Result<(SeriesMeta, Vec<(time::Date, f64)>)> {
    let m = meta(client, secrets, code).await?;
    let key = key(secrets)?;
    let url = format!(
        "{BASE}/series/observations?series_id={}&api_key={key}&file_type=json",
        enc(code)
    );
    let body = get(client, &url).await?;
    let obs = body
        .get("observations")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|o| {
                    let date = super::parse_date(o.get("date")?.as_str()?)?;
                    // "." marks a missing observation: a gap, not a zero.
                    let value = o.get("value")?.as_str()?.parse::<f64>().ok()?;
                    Some((date, value))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((m, obs))
}
