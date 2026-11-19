//! US Treasury: the daily par yield curve (home.treasury.gov CSV) and FiscalData series
//! (api.fiscaldata.treasury.gov). Keyless; still a connector so grants and the rate
//! dashboard apply.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;

use crate::histdata::{enc, Capability, Chunk, Connector};
use otw_store::fundamentals::SeriesMeta;

pub const PROVIDER: &str = "treasury";

pub struct Treasury;

static CAP: Capability = Capability {
    provider: PROVIDER,
    label: "US Treasury",
    website: "https://fiscaldata.treasury.gov",
    docs_url: "https://fiscaldata.treasury.gov/api-documentation/",
    rate_limit: "Keyless, no published cap. The yield curve is cached for an hour.",
    required_secrets: &[],
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
impl Connector for Treasury {
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
        _secrets: &HashMap<String, String>,
    ) -> Result<String> {
        let c = yield_curve(client).await?;
        Ok(format!("yield curve as of {}", c.as_of))
    }
}

// ── FiscalData series ───────────────────────────────────────────────────────

/// FiscalData datasets exposed as series: (code, title, endpoint, date field, value field,
/// unit, frequency, category).
const SERIES: &[(&str, &str, &str, &str, &str, &str, &str, &str)] = &[(
    "debt_to_penny",
    "Total public debt outstanding",
    "v2/accounting/od/debt_to_penny",
    "record_date",
    "tot_pub_debt_out_amt",
    "USD",
    "D",
    "fiscal",
)];

fn meta_of(s: &(&str, &str, &str, &str, &str, &str, &str, &str)) -> SeriesMeta {
    SeriesMeta {
        provider: PROVIDER.into(),
        code: s.0.into(),
        title: s.1.into(),
        category: s.7.into(),
        country: "US".into(),
        unit: s.5.into(),
        frequency: s.6.into(),
        seasonal_adj: false,
        notes: String::new(),
    }
}

/// The Treasury's catalog is a fixed list; search filters it.
pub fn search(q: &str) -> Vec<SeriesMeta> {
    let q = q.to_lowercase();
    SERIES
        .iter()
        .filter(|s| q.is_empty() || s.0.contains(&q) || s.1.to_lowercase().contains(&q))
        .map(meta_of)
        .collect()
}

pub async fn fetch(client: &reqwest::Client, code: &str) -> Result<(SeriesMeta, Vec<(time::Date, f64)>)> {
    let s = SERIES
        .iter()
        .find(|s| s.0 == code)
        .ok_or_else(|| anyhow!("US Treasury has no series {code}"))?;
    let url = format!(
        "https://api.fiscaldata.treasury.gov/services/api/fiscal_service/{}?fields={},{}\
         &sort={}&page%5Bsize%5D=10000",
        s.2,
        s.3,
        s.4,
        enc(s.3)
    );
    let body: Value = crate::rate::send(PROVIDER, client.get(&url).timeout(super::edgar::TIMEOUT))
        .await
        .context("FiscalData request")?
        .error_for_status()
        .context("FiscalData status")?
        .json()
        .await
        .context("FiscalData response")?;
    let obs = body
        .get("data")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let d = super::parse_date(r.get(s.3)?.as_str()?)?;
                    let v = r.get(s.4)?.as_str()?.parse::<f64>().ok()?;
                    Some((d, v))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((meta_of(s), obs))
}

// ── Par yield curve ─────────────────────────────────────────────────────────

/// Tenors shown, and the CSV column each one reads.
const TENORS: &[(&str, &str)] = &[
    ("1M", "1 Mo"),
    ("3M", "3 Mo"),
    ("6M", "6 Mo"),
    ("1Y", "1 Yr"),
    ("2Y", "2 Yr"),
    ("3Y", "3 Yr"),
    ("5Y", "5 Yr"),
    ("7Y", "7 Yr"),
    ("10Y", "10 Yr"),
    ("20Y", "20 Yr"),
    ("30Y", "30 Yr"),
];

#[derive(Debug, Clone, Serialize)]
pub struct Curve {
    pub label: String,
    pub date: String,
    pub values: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct YieldCurve {
    pub as_of: String,
    pub tenors: Vec<&'static str>,
    /// Today, a month ago and a year ago (the nearest published day on or before each).
    pub curves: Vec<Curve>,
}

static CURVE_CACHE: Mutex<Option<(Instant, YieldCurve)>> = Mutex::new(None);
const CURVE_TTL: Duration = Duration::from_secs(3600);

/// One year of daily par yields, newest first.
async fn year_rows(client: &reqwest::Client, year: i32) -> Result<Vec<(time::Date, Vec<Option<f64>>)>> {
    let url = format!(
        "https://home.treasury.gov/resource-center/data-chart-center/interest-rates/\
         daily-treasury-rates.csv/{year}/all?type=daily_treasury_yield_curve\
         &field_tdr_date_value={year}&page&_format=csv"
    );
    let text = crate::rate::send(PROVIDER, client.get(&url).timeout(super::edgar::TIMEOUT))
        .await
        .context("Treasury yield curve request")?
        .error_for_status()
        .context("Treasury yield curve status")?
        .text()
        .await
        .context("Treasury yield curve response")?;
    let mut rdr = csv::Reader::from_reader(text.as_bytes());
    let headers = rdr.headers().context("yield curve header")?.clone();
    let cols: Vec<Option<usize>> = TENORS
        .iter()
        .map(|(_, h)| headers.iter().position(|x| x == *h))
        .collect();
    let fmt = time::macros::format_description!("[month]/[day]/[year]");
    let mut out = Vec::new();
    for rec in rdr.records().flatten() {
        let Some(date) = rec.get(0).and_then(|d| time::Date::parse(d, &fmt).ok()) else {
            continue;
        };
        let values = cols
            .iter()
            .map(|c| c.and_then(|i| rec.get(i)).and_then(|v| v.trim().parse::<f64>().ok()))
            .collect();
        out.push((date, values));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(out)
}

pub async fn yield_curve(client: &reqwest::Client) -> Result<YieldCurve> {
    if let Ok(guard) = CURVE_CACHE.lock() {
        if let Some((at, c)) = guard.as_ref() {
            if at.elapsed() < CURVE_TTL {
                return Ok(c.clone());
            }
        }
    }
    let year = OffsetDateTime::now_utc().year();
    let mut rows = year_rows(client, year).await?;
    rows.extend(year_rows(client, year - 1).await?);
    let (latest, _) = *rows.first().ok_or_else(|| anyhow!("the Treasury published no yield curve"))?;
    let on_or_before = |target: time::Date, label: &str| -> Option<Curve> {
        rows.iter().find(|(d, _)| *d <= target).map(|(d, v)| Curve {
            label: label.into(),
            date: d.to_string(),
            values: v.clone(),
        })
    };
    let curves = [
        on_or_before(latest, "today"),
        on_or_before(latest - time::Duration::days(30), "1m"),
        on_or_before(latest - time::Duration::days(365), "1y"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let curve = YieldCurve {
        as_of: latest.to_string(),
        tenors: TENORS.iter().map(|(t, _)| *t).collect(),
        curves,
    };
    if let Ok(mut guard) = CURVE_CACHE.lock() {
        *guard = Some((Instant::now(), curve.clone()));
    }
    Ok(curve)
}
