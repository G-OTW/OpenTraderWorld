//! Portfolio Tracker: price fetching + the daily refresh job.

pub mod prices;

use std::time::Duration;

use sqlx::PgPool;
use time::OffsetDateTime;

use otw_store::portfolios as store;

/// How often the loop wakes to check whether the daily refresh is due.
const TICK: Duration = Duration::from_secs(60 * 60);

/// Spawn the daily refresh loop: once per UTC day it re-prices and snapshots every portfolio that
/// has `auto_refresh` on. Runs immediately on start (so a freshly-toggled portfolio gets a point),
/// then hourly checks the date to fire once more each day.
pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let mut last_run: Option<time::Date> = None;
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            let today = OffsetDateTime::now_utc().date();
            if last_run == Some(today) {
                continue;
            }
            if let Err(e) = run_due(&pool).await {
                tracing::error!("portfolios daily refresh failed: {e:#}");
            } else {
                last_run = Some(today);
            }
        }
    });
}

/// Refresh every auto-refresh portfolio once.
async fn run_due(pool: &PgPool) -> anyhow::Result<()> {
    let portfolios = store::list_auto_refresh(pool).await?;
    if portfolios.is_empty() {
        return Ok(());
    }
    tracing::info!("portfolios: daily refresh of {} portfolio(s)", portfolios.len());
    for pf in &portfolios {
        if let Err(e) = prices::refresh_portfolio(pool, pf).await {
            tracing::error!("portfolio {} refresh failed: {e:#}", pf.id);
        }
        // Be gentle with the free public APIs between portfolios.
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}
