//! Twice-weekly refresh job for the Managers' Portfolios module.
//!
//! It scrapes Dataroma's managers summary, then walks each manager and replaces its holdings in
//! the cache. A short delay between manager fetches keeps us polite to the upstream.
//!
//! Cadence is roughly twice a week, but each cycle waits a *random* slice of the window before
//! scraping (and startup jitters too). Across many self-hosted instances this spreads the load so
//! everyone doesn't hit Dataroma at the same instant. On startup it refreshes only if the cache is
//! empty or stale (past the refresh window), so a restart loop doesn't re-scrape. Manual refresh
//! is exposed via the API (the button on the page).

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::mportfolios;
use otw_store::mportfolios as store;

/// Base period between refreshes: twice a week (~3.5 days).
const PERIOD: Duration = Duration::from_secs(7 * 24 * 60 * 60 / 2);
/// Extra random spread added on top of each wait, so instances desync (up to +12h).
const MAX_JITTER: Duration = Duration::from_secs(12 * 60 * 60);
/// Consider the cache stale (refresh on startup) once it's older than the base period.
const STALE_AFTER: time::Duration = time::Duration::seconds(PERIOD.as_secs() as i64);
/// Politeness delay between per-manager detail fetches.
const PER_MANAGER_DELAY: Duration = Duration::from_millis(400);

/// Guards against concurrent refreshes (background loop vs. a manual API trigger).
pub type RefreshLock = Arc<Mutex<()>>;

pub fn new_lock() -> RefreshLock {
    Arc::new(Mutex::new(()))
}

/// A uniformly random `Duration` in `[0, max]`, seeded from the OS RNG.
fn random_up_to(max: Duration) -> Duration {
    let mut buf = [0u8; 8];
    if getrandom::fill(&mut buf).is_err() {
        return Duration::ZERO;
    }
    let frac = u64::from_le_bytes(buf) as f64 / u64::MAX as f64;
    max.mul_f64(frac)
}

pub fn spawn(pool: PgPool, lock: RefreshLock) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(8)).await;
        // First run: only if empty or stale. Jitter so restarts don't sync up.
        let fresh = matches!(
            store::last_refreshed(&pool).await,
            Ok(Some(ts)) if time::OffsetDateTime::now_utc() - ts < STALE_AFTER
        );
        if fresh {
            tracing::info!("mportfolios cache is fresh; skipping startup refresh");
        } else {
            tokio::time::sleep(random_up_to(MAX_JITTER)).await;
            if let Err(e) = refresh(&pool, &lock).await {
                tracing::error!("mportfolios initial refresh failed: {e:#}");
            }
        }
        loop {
            // Twice a week, plus a random offset each cycle so instances stay desynced.
            tokio::time::sleep(PERIOD + random_up_to(MAX_JITTER)).await;
            if let Err(e) = refresh(&pool, &lock).await {
                tracing::error!("mportfolios refresh failed: {e:#}");
            }
        }
    });
}

/// Scrape the whole Dataroma superinvestor set into the cache. Per-manager failures are logged
/// and skipped so one bad page doesn't abort the run.
pub async fn refresh(pool: &PgPool, lock: &RefreshLock) -> anyhow::Result<()> {
    let _guard = lock.lock().await;
    let client = mportfolios::client()?;

    let managers = mportfolios::fetch_managers(&client).await?;
    tracing::info!("mportfolios: scraping {} managers", managers.len());

    let mut ok = 0u32;
    for mut m in managers {
        match mportfolios::fetch_holdings(&client, &m.slug).await {
            Ok((period, holdings)) => {
                m.period = period;
                if let Err(e) = store::replace_portfolio(pool, &m, &holdings).await {
                    tracing::warn!("mportfolios: persist {} failed: {e:#}", m.slug);
                } else {
                    ok += 1;
                }
            }
            Err(e) => tracing::warn!("mportfolios: fetch {} failed: {e:#}", m.slug),
        }
        tokio::time::sleep(PER_MANAGER_DELAY).await;
    }
    tracing::info!("mportfolios: refreshed {ok} manager(s)");
    Ok(())
}
