//! Sparing provider quotas. An automatic refresh (a page opening on missing or stale data)
//! does not ask a provider that refused the same dataset and subject recently, nor one
//! whose declared quota is spent; it says so instead, and a manual refresh asks anyway.
//! How long a refusal holds depends on what it was: a plan does not change by itself, a
//! rate limit resets, a network error passes.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime, Time};
use uuid::Uuid;

use otw_store::fundamentals as store;

/// A provider an automatic refresh left alone, and when it will be asked again.
#[derive(Debug, Clone, Serialize)]
pub struct Skip {
    pub provider: String,
    pub label: String,
    /// `failed` (it refused recently) or `quota` (the connector's declared quota is spent).
    pub reason: &'static str,
    /// The refusal, for `failed`.
    pub error: String,
    /// When it refused, for `failed` (ms).
    pub failed_at: Option<i64>,
    /// Next automatic try (ms).
    pub retry_at: i64,
}

fn ms(t: OffsetDateTime) -> i64 {
    (t.unix_timestamp_nanos() / 1_000_000) as i64
}

/// The refusals in force for a dataset and subject. Empty on a manual refresh.
pub async fn failures(
    pool: &PgPool,
    dataset: &str,
    subject: &str,
    force: bool,
) -> anyhow::Result<HashMap<String, store::FetchFailure>> {
    if force {
        return Ok(HashMap::new());
    }
    store::fetch_failures(pool, dataset, subject).await
}

/// Why an automatic refresh should leave this provider alone, if it should.
pub async fn check(
    pool: &PgPool,
    failures: &HashMap<String, store::FetchFailure>,
    provider: &str,
    connector: Uuid,
    force: bool,
) -> Option<Skip> {
    if force {
        return None;
    }
    if let Some(f) = failures.get(provider) {
        return Some(Skip {
            provider: provider.into(),
            label: super::datasets::label(provider),
            reason: "failed",
            error: f.error.clone(),
            failed_at: Some(ms(f.failed_at)),
            retry_at: ms(f.retry_at),
        });
    }
    let resets = quota_spent(pool, connector).await?;
    Some(Skip {
        provider: provider.into(),
        label: super::datasets::label(provider),
        reason: "quota",
        error: String::new(),
        failed_at: None,
        retry_at: ms(resets),
    })
}

/// When the connector's declared quota (data broker) is spent, the instant it resets.
pub async fn quota_spent(pool: &PgPool, connector: Uuid) -> Option<OffsetDateTime> {
    let scope = crate::connectors_api::quota_scope(connector);
    let q = otw_store::api_quota::get(pool, &scope).await.ok().flatten()?;
    let max = q.max_requests?;
    (q.used >= max).then_some(q.resets_at)
}

/// Remember a refusal until the provider is worth asking again.
pub async fn record(pool: &PgPool, dataset: &str, subject: &str, provider: &str, error: &str) {
    let retry = retry_at(error, OffsetDateTime::now_utc());
    let _ = store::put_fetch_failure(pool, dataset, subject, provider, error, retry).await;
}

pub async fn clear(pool: &PgPool, dataset: &str, subject: &str, provider: &str) {
    let _ = store::clear_fetch_failure(pool, dataset, subject, provider).await;
}

/// A provider's answer with its own credentials masked: some echo the key back (Alpha
/// Vantage's rate-limit notice), and the answer is stored and shown.
pub fn redact(msg: &str, secrets: &HashMap<String, String>) -> String {
    secrets
        .values()
        .map(|v| v.trim())
        .filter(|v| v.len() >= 6)
        .fold(msg.to_string(), |m, v| m.replace(v, "[key]"))
}

/// When a provider that answered `error` is worth asking again.
pub fn retry_at(error: &str, now: OffsetDateTime) -> OffsetDateTime {
    let e = error.to_ascii_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| e.contains(w));
    // Rate limits first: their messages often advertise a premium plan too.
    // Alpha Vantage's per-second burst limit.
    if any(&["spreading out"]) {
        return now + Duration::minutes(15);
    }
    if any(&["http 429", "rate limit", "limit reached", "too many requests", "call frequency", "requests per", "quota"]) {
        // A daily allowance comes back at midnight UTC (Alpha Vantage, EODHD, FMP); a
        // per-minute one much sooner.
        if any(&["per day", "daily", "/day"]) {
            return (now + Duration::days(1)).replace_time(Time::MIDNIGHT);
        }
        return now + Duration::hours(1);
    }
    // A plan or an entitlement: it only changes when the user changes it.
    if any(&["http 402", "http 401", "http 403", "restricted", "premium", "subscription", "your plan", "not entitled", "upgrade"]) {
        return now + Duration::days(7);
    }
    // The provider does not have it (symbol, company, country).
    if any(&["http 404", "no data", "not found", "has no", "nothing matches", "does not cover", "unknown symbol"]) {
        return now + Duration::days(1);
    }
    // Network trouble or a server error: soon.
    if any(&["timed out", "timeout", "connection", "http 5", "dns"]) {
        return now + Duration::minutes(15);
    }
    now + Duration::hours(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn refusals_hold_by_kind() {
        let now = datetime!(2026-10-02 14:30 UTC);
        let plan = "Financial Modeling Prep: fmp answered HTTP 402: Restricted Endpoint";
        assert_eq!(retry_at(plan, now), now + Duration::days(7));
        let daily = "Alpha Vantage: standard API rate limit is 25 requests per day";
        assert_eq!(retry_at(daily, now), datetime!(2026-10-03 00:00 UTC));
        assert_eq!(retry_at("massive answered HTTP 429", now), now + Duration::hours(1));
        assert_eq!(retry_at("EODHD has no data for XYZ", now), now + Duration::days(1));
        assert_eq!(retry_at("request timed out", now), now + Duration::minutes(15));
        assert_eq!(retry_at("something odd", now), now + Duration::hours(1));
        let av = "our standard API rate limit is 25 requests per day. Please subscribe to any of the premium plans";
        assert_eq!(retry_at(av, now), datetime!(2026-10-03 00:00 UTC));
    }

    #[test]
    fn keys_are_masked() {
        let secrets = HashMap::from([("api_key".to_string(), "ABCDEF123".to_string())]);
        assert_eq!(redact("your API key as ABCDEF123 and", &secrets), "your API key as [key] and");
    }
}
