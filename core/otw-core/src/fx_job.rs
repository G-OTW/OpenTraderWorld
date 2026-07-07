//! Daily FX catch-up job for the Trading Journal.
//!
//! On start it backfills a window of recent history (so existing trades convert), then
//! ticks hourly to pull each new business day's close up to yesterday. Weekends/holidays
//! have no ECB publication; those are skipped (the breakdown carries forward the prior
//! rate). A business day that no source can supply is recorded as a pending task.
//!
//! Idempotent: dates already fully covered are skipped, so re-running is cheap.

use std::time::Duration;

use sqlx::PgPool;
use time::{Date, OffsetDateTime, Weekday};

use crate::fx;
use otw_store::journal_fx;

/// How many days back to backfill on first run (calendar days; weekends are skipped).
const BACKFILL_DAYS: i64 = 120;
/// A date within this many days of today is recent enough to accept the "latest" backup.
const BACKUP_WINDOW_DAYS: i64 = 3;
/// How often to wake and catch up.
const TICK: Duration = Duration::from_secs(6 * 60 * 60);

/// Spawn the FX catch-up loop. Returns immediately; runs until the process exits.
pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        // Small initial delay so startup/migrations settle and we don't race the first
        // request burst.
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            if let Err(e) = catch_up(&pool).await {
                tracing::error!("fx catch-up failed: {e:#}");
            }
        }
    });
}

fn is_business_day(d: Date) -> bool {
    !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday)
}

/// Fetch every uncovered business day from our start bound up to yesterday.
async fn catch_up(pool: &PgPool) -> anyhow::Result<()> {
    let yesterday = journal_fx::yesterday_utc();

    // Start the day after our latest stored rate, or BACKFILL_DAYS back on first run.
    let start = match journal_fx::latest_rate_date(pool).await? {
        Some(latest) => latest.next_day().unwrap_or(latest),
        None => yesterday - time::Duration::days(BACKFILL_DAYS),
    };
    if start > yesterday {
        return Ok(()); // already current
    }

    let today = OffsetDateTime::now_utc().date();
    let mut day = start;
    let mut fetched = 0u32;
    while day <= yesterday {
        if is_business_day(day) && !journal_fx::has_full_coverage(pool, day).await? {
            let recent = (today - day).whole_days() <= BACKUP_WINDOW_DAYS;
            match fx::fetch_rates(day, recent).await {
                Ok(Some(f)) => {
                    journal_fx::upsert_rates(pool, day, &f.rates, f.source).await?;
                    fetched += 1;
                }
                Ok(None) => {
                    journal_fx::mark_pending(pool, day, "no source returned rates").await?;
                }
                Err(e) => {
                    journal_fx::mark_pending(pool, day, "fetch error").await?;
                    tracing::warn!("fx fetch error for {day}: {e:#}");
                }
            }
            // Be polite to the upstream APIs during a backfill burst.
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        day = match day.next_day() {
            Some(d) => d,
            None => break,
        };
    }
    if fetched > 0 {
        tracing::info!("fx catch-up stored {fetched} day(s) up to {yesterday}");
    }
    Ok(())
}
