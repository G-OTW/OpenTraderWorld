//! FX rate fetching for the Trading Journal.
//!
//! Primary source is Frankfurter (https://frankfurter.dev) — free, no key, serves
//! historical rates for any date against any base. Backup is open.er-api.com, which only
//! serves the latest rates (used when Frankfurter is unavailable and the target date is
//! today/yesterday). All rates are normalised to a USD base before storage. A date no
//! source can cover is recorded as pending for the user to fill in manually.
//!
//! Only the journal's major currencies are requested (otw_store::journal_fx::FX_QUOTES).

use anyhow::{Context, Result};
use std::time::Duration;
use time::{format_description::well_known::Iso8601, Date};

use otw_store::journal_fx::FX_QUOTES;

const FRANKFURTER: &str = "https://api.frankfurter.dev/v2/rates";
const ER_API_LATEST: &str = "https://open.er-api.com/v6/latest/USD";
const TIMEOUT: Duration = Duration::from_secs(15);

/// USD-based rates for one date, ready to upsert: (quote, rate) with the source tag.
pub struct FetchedRates {
    pub source: &'static str,
    pub rates: Vec<(String, f64)>,
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent("OpenTraderWorld/journal-fx")
        .build()
        .context("building fx http client")
}

/// One element of the Frankfurter v2 array response.
#[derive(serde::Deserialize)]
struct FrankRow {
    quote: String,
    rate: f64,
}

/// Fetch USD-based rates for `date` from Frankfurter. Requests USD base directly so no
/// re-basing is needed. Returns the quotes the API actually provided.
async fn fetch_frankfurter(client: &reqwest::Client, date: Date) -> Result<Vec<(String, f64)>> {
    let day = date.format(&Iso8601::DATE).context("formatting date")?;
    let quotes = FX_QUOTES.join(",");
    let url = format!("{FRANKFURTER}?date={day}&base=USD&quotes={quotes}");
    let rows: Vec<FrankRow> = crate::rate::send("frankfurter", client.get(&url))
        .await
        .context("frankfurter request")?
        .error_for_status()
        .context("frankfurter status")?
        .json()
        .await
        .context("frankfurter decode")?;
    Ok(rows.into_iter().map(|r| (r.quote, r.rate)).collect())
}

#[derive(serde::Deserialize)]
struct ErApiResp {
    result: String,
    rates: std::collections::HashMap<String, f64>,
}

/// Fetch the latest USD-based rates from the backup (er-api). No historical support, so
/// the caller only uses this for recent dates.
async fn fetch_er_api(client: &reqwest::Client) -> Result<Vec<(String, f64)>> {
    let resp: ErApiResp = crate::rate::send("er-api", client.get(ER_API_LATEST))
        .await
        .context("er-api request")?
        .error_for_status()
        .context("er-api status")?
        .json()
        .await
        .context("er-api decode")?;
    if resp.result != "success" {
        anyhow::bail!("er-api returned result={}", resp.result);
    }
    Ok(FX_QUOTES
        .iter()
        .filter_map(|q| resp.rates.get(*q).map(|r| (q.to_string(), *r)))
        .collect())
}

/// Fetch USD-based rates for `date`, trying Frankfurter first and falling back to er-api
/// (only when `allow_backup`, i.e. the date is recent enough that "latest" is acceptable).
/// A partial Frankfurter set is kept as the last resort — some coverage beats none (the
/// catch-up job retries incomplete dates to fill the gaps). Returns `Ok(None)` only when
/// no source supplied any usable rate.
pub async fn fetch_rates(date: Date, allow_backup: bool) -> Result<Option<FetchedRates>> {
    let client = client()?;

    let mut partial: Option<Vec<(String, f64)>> = None;
    match fetch_frankfurter(&client, date).await {
        Ok(rates) if rates.len() >= FX_QUOTES.len() => {
            return Ok(Some(FetchedRates { source: "frankfurter", rates }));
        }
        Ok(rates) => {
            tracing::warn!("frankfurter returned {} of {} quotes for {date}", rates.len(), FX_QUOTES.len());
            // Prefer a full set from the backup when allowed; keep this as last resort.
            if !rates.is_empty() {
                partial = Some(rates);
            }
        }
        Err(e) => tracing::warn!("frankfurter fetch failed for {date}: {e:#}"),
    }

    if allow_backup {
        match fetch_er_api(&client).await {
            Ok(rates) if rates.len() >= FX_QUOTES.len() => {
                return Ok(Some(FetchedRates { source: "er-api", rates }));
            }
            Ok(rates) if !rates.is_empty() && partial.is_none() => {
                return Ok(Some(FetchedRates { source: "er-api", rates }));
            }
            Ok(_) => tracing::warn!("er-api returned no usable quotes"),
            Err(e) => tracing::warn!("er-api fetch failed: {e:#}"),
        }
    }

    Ok(partial.map(|rates| FetchedRates { source: "frankfurter", rates }))
}
