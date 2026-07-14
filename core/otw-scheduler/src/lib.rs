//! Interval scheduler for the news-feed module.
//!
//! A single tokio task wakes on a fixed tick, claims any feeds whose `next_run_at`
//! is due (DB-side `FOR UPDATE SKIP LOCKED`, so it's safe even if run more than
//! once), fetches each, inserts new items (deduplicated), and records health.
//! New items are broadcast so the SSE endpoint can push them live.

use std::sync::Arc;
use std::time::Duration;

use otw_store::crypto::SecretCipher;
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::broadcast;

/// An event emitted when a feed poll produced new items.
#[derive(Debug, Clone, Serialize)]
pub struct FeedEvent {
    pub feed_id: uuid::Uuid,
    pub feed_name: String,
    pub new_items: u64,
}

/// Handle shared with the HTTP layer: subscribe for live events, or trigger a
/// one-off poll of a single feed (used by a "Refresh now" button).
#[derive(Clone)]
pub struct Scheduler {
    pool: PgPool,
    cipher: Arc<SecretCipher>,
    tx: broadcast::Sender<FeedEvent>,
}

impl Scheduler {
    pub fn new(pool: PgPool, cipher: SecretCipher) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            pool,
            cipher: Arc::new(cipher),
            tx,
        }
    }

    /// Subscribe to live feed events (new items pushed by polls).
    pub fn subscribe(&self) -> broadcast::Receiver<FeedEvent> {
        self.tx.subscribe()
    }

    /// Start the background poll loop. Returns immediately; the loop runs until
    /// the process exits.
    pub fn spawn(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            // A short tick keeps latency low; actual cadence is per-feed via
            // next_run_at, so most ticks claim nothing.
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            loop {
                tick.tick().await;
                if let Err(e) = this.run_due().await {
                    tracing::error!("feed scheduler tick failed: {e:#}");
                }
            }
        });
    }

    /// Poll every feed that is currently due.
    async fn run_due(&self) -> anyhow::Result<()> {
        let due = otw_store::feeds::claim_due_feeds(&self.pool, 20).await?;
        for feed in due {
            self.poll_one(&feed).await;
        }
        Ok(())
    }

    /// Poll a single feed now (also used directly by the HTTP "refresh" route).
    pub async fn poll_feed_id(&self, id: uuid::Uuid) -> anyhow::Result<u64> {
        let feed = otw_store::feeds::get_feed(&self.pool, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("feed not found"))?;
        Ok(self.poll_one(&feed).await)
    }

    /// Refresh every source once, regardless of dashboard state (the "Refresh
    /// all" button is an explicit manual action). Returns total new items.
    pub async fn poll_all_enabled(&self) -> anyhow::Result<u64> {
        let feeds = otw_store::feeds::list_feeds(&self.pool).await?;
        let mut total = 0u64;
        for feed in feeds {
            total += self.poll_one(&feed).await;
        }
        Ok(total)
    }

    /// Refresh every source linked to one dashboard once. Returns total new items.
    pub async fn poll_dashboard(&self, dashboard_id: uuid::Uuid) -> anyhow::Result<u64> {
        let ids = otw_store::feeds::dashboard_source_ids(&self.pool, dashboard_id).await?;
        let mut total = 0u64;
        for id in ids {
            if let Some(feed) = otw_store::feeds::get_feed(&self.pool, id).await? {
                total += self.poll_one(&feed).await;
            }
        }
        Ok(total)
    }

    async fn poll_one(&self, feed: &otw_store::feeds::Feed) -> u64 {
        let secrets = match otw_store::feeds::load_secrets(&self.pool, &self.cipher, feed.id).await {
            Ok(s) => s,
            Err(e) => {
                let _ =
                    otw_store::feeds::mark_error(&self.pool, feed.id, &format!("secrets: {e}")).await;
                return 0;
            }
        };

        let fetched = otw_feeds::fetch(feed, &secrets).await;
        record_feed_rate(feed, &fetched);
        // Count the poll against the feed's declared quota (one request per poll,
        // success or not — the provider billed it either way). No-op when untracked.
        if feed.kind == "api" {
            if let Err(e) =
                otw_store::api_quota::bump(&self.pool, &format!("feed:{}", feed.id)).await
            {
                tracing::debug!("feed quota bump failed: {e:#}");
            }
        }
        match fetched {
            Ok(items) => match otw_store::feeds::insert_items(&self.pool, feed.id, &items).await {
                Ok(n) => {
                    let _ = otw_store::feeds::mark_success(&self.pool, feed.id).await;
                    if n > 0 {
                        tracing::info!("feed '{}' fetched {} new item(s)", feed.name, n);
                        // A lagging/absent receiver is fine; ignore send errors.
                        let _ = self.tx.send(FeedEvent {
                            feed_id: feed.id,
                            feed_name: feed.name.clone(),
                            new_items: n,
                        });
                    }
                    n
                }
                Err(e) => {
                    let _ =
                        otw_store::feeds::mark_error(&self.pool, feed.id, &format!("store: {e}")).await;
                    0
                }
            },
            Err(e) => {
                // Fetch errors may echo the upstream response (e.g. Alpha Vantage
                // repeats the API key in its rate-limit message), so scrub any
                // secret values before persisting.
                let msg = sanitize_error(&format!("{e:#}"), &secrets);
                let _ = otw_store::feeds::mark_error(&self.pool, feed.id, &msg).await;
                0
            }
        }
    }
}

/// Record an **API** feed poll on the rate dashboard (observe-only). Plain RSS feeds are
/// skipped — they're arbitrary public endpoints with no rate limits worth tracking; only
/// `kind == "api"` feeds (a user-configured API with its own quota) are recorded, keyed by
/// the feed's name so each shows as its own provider. A success/other error counts as a
/// plain call; an over-limit-looking error is flagged `Limited` (dashboard event + WARN log).
fn record_feed_rate<T>(feed: &otw_store::feeds::Feed, result: &anyhow::Result<T>) {
    use otw_store::api_rate::Outcome;
    if feed.kind != "api" {
        return;
    }
    let host = feed
        .config
        .get("url")
        .and_then(|v| v.as_str())
        .map(host_of)
        .unwrap_or_default();
    match result {
        Ok(_) => otw_store::api_rate::record(&feed.name, &host, Outcome::Ok, None, ""),
        Err(e) => {
            let msg = format!("{e:#}").to_ascii_lowercase();
            let limited = msg.contains("429")
                || msg.contains("too many requests")
                || msg.contains("rate limit");
            let outcome = if limited { Outcome::Limited } else { Outcome::Error };
            let detail = if limited { "feed poll rate-limited" } else { "" };
            otw_store::api_rate::record(&feed.name, &host, outcome, None, detail);
        }
    }
}

/// Bare host (no scheme/port/path) from a URL, for dashboard grouping.
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?'])
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Max length we persist for a feed error message; upstream notices can be long.
const MAX_ERROR_LEN: usize = 240;

/// Redact any secret values appearing in an error message and cap its length, so
/// stored/displayed errors never leak credentials (some APIs echo the key back).
fn sanitize_error(msg: &str, secrets: &std::collections::HashMap<String, String>) -> String {
    let mut out = msg.to_string();
    for value in secrets.values() {
        // Skip trivially short values to avoid mangling unrelated text.
        if value.len() >= 4 {
            out = out.replace(value, "***");
        }
    }
    if out.chars().count() > MAX_ERROR_LEN {
        let truncated: String = out.chars().take(MAX_ERROR_LEN).collect();
        out = format!("{truncated}…");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn redacts_secret_values() {
        let mut secrets = HashMap::new();
        secrets.insert("av_key".to_string(), "Y4M4NL2HOPINT299".to_string());
        let msg = "Information: We have detected your API key as Y4M4NL2HOPINT299 and ...";
        let out = sanitize_error(msg, &secrets);
        assert!(!out.contains("Y4M4NL2HOPINT299"));
        assert!(out.contains("***"));
    }

    #[test]
    fn truncates_long_messages() {
        let secrets = HashMap::new();
        let long = "x".repeat(1000);
        let out = sanitize_error(&long, &secrets);
        assert!(out.chars().count() <= MAX_ERROR_LEN + 1); // +1 for the ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn short_secrets_are_not_substituted() {
        let mut secrets = HashMap::new();
        secrets.insert("k".to_string(), "ab".to_string());
        let out = sanitize_error("abcdef the message", &secrets);
        assert_eq!(out, "abcdef the message");
    }
}
