//! Daily FX catch-up job for the Trading Journal.
//!
//! On start it backfills a window of recent history (so existing trades convert), then
//! ticks every few hours to pull each new business day's close up to yesterday.
//! Weekends/holidays have no ECB publication; those are skipped (the breakdown carries
//! forward the prior rate). Each tick also:
//!   - retries **pending** dates (e.g. a trade logged before the backfill window, or a
//!     past fetch failure) — Frankfurter serves any historical date back to ECB coverage,
//!     so most of these resolve without manual entry;
//!   - retries **incomplete** dates (a partial upstream answer stored fewer than the full
//!     quote set) so a half-covered date doesn't strand its currencies forever.
//! Dates a fetch genuinely can't cover stay pending for the user to fill in manually;
//! each is attempted at most once per process run to avoid hammering the sources.
//!
//! Idempotent: dates already fully covered are skipped, so re-running is cheap.

use std::collections::HashSet;
use std::time::Duration;

use sqlx::PgPool;
use time::{Date, Month, OffsetDateTime, Weekday};

use crate::fx;
use otw_store::journal_fx;

/// How many days back to backfill on first run (calendar days; weekends are skipped).
const BACKFILL_DAYS: i64 = 120;
/// A date within this many days of today is recent enough to accept the "latest" backup.
const BACKUP_WINDOW_DAYS: i64 = 3;
/// How often to wake and catch up.
const TICK: Duration = Duration::from_secs(6 * 60 * 60);
/// Max pending / incomplete dates retried per tick (politeness to the upstream APIs).
const RETRY_BATCH: i64 = 25;

/// Earliest date the primary source can serve (ECB reference rates start 1999-01-04).
/// Anything older can only be resolved manually.
fn source_floor() -> Date {
    Date::from_calendar_date(1999, Month::January, 4).expect("valid constant date")
}

/// Spawn the FX catch-up loop. Returns immediately; runs until the process exits.
pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        // Small initial delay so startup/migrations settle and we don't race the first
        // request burst.
        tokio::time::sleep(Duration::from_secs(5)).await;
        // Dates already attempted this run (pending/incomplete retries) — a date that
        // still failed or stayed partial is not re-fetched until the next process start.
        let mut attempted: HashSet<Date> = HashSet::new();
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            if let Err(e) = catch_up(&pool).await {
                tracing::error!("fx catch-up failed: {e:#}");
            }
            if let Err(e) = retry_pending(&pool, &mut attempted).await {
                tracing::error!("fx pending retry failed: {e:#}");
            }
            if let Err(e) = retry_incomplete(&pool, &mut attempted).await {
                tracing::error!("fx incomplete retry failed: {e:#}");
            }
        }
    });
}

fn is_business_day(d: Date) -> bool {
    !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday)
}

/// The last business day on or before `d` (rates for a weekend date are the prior close).
fn business_day_on_or_before(mut d: Date) -> Date {
    while !is_business_day(d) {
        d = d.previous_day().unwrap_or(d);
    }
    d
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

/// Try to resolve pending dates automatically. A pending date can be a weekend/holiday
/// (a trade's date), so the fetch targets the last business day on or before it — once a
/// rate exists at-or-before the date, the breakdown's carry-forward read works and the
/// pending task can be cleared.
async fn retry_pending(pool: &PgPool, attempted: &mut HashSet<Date>) -> anyhow::Result<()> {
    let floor = source_floor();
    let today = OffsetDateTime::now_utc().date();
    let mut resolved = 0u32;
    let mut tried = 0i64;
    for p in journal_fx::list_pending(pool).await? {
        if tried >= RETRY_BATCH {
            break;
        }
        let d = p.pending_date;
        let target = business_day_on_or_before(d);
        if target < floor {
            continue; // before the source's coverage — manual entry only
        }
        // Already fully covered (e.g. resolved since the task was created, or by an
        // earlier retry this tick that shares the same business day): just clear.
        if journal_fx::has_full_coverage(pool, target).await? {
            journal_fx::clear_pending(pool, d).await?;
            resolved += 1;
            continue;
        }
        if !attempted.insert(target) {
            continue; // already tried this run
        }
        tried += 1;
        let recent = (today - target).whole_days() <= BACKUP_WINDOW_DAYS;
        match fx::fetch_rates(target, recent).await {
            Ok(Some(f)) => {
                journal_fx::upsert_rates(pool, target, &f.rates, f.source).await?;
                // The upsert clears the exact-date task; a weekend/holiday task points at
                // a different date than the business day just stored — clear it too.
                if d != target {
                    journal_fx::clear_pending(pool, d).await?;
                }
                resolved += 1;
            }
            Ok(None) => {} // still uncoverable — stays pending for manual entry
            Err(e) => tracing::warn!("fx pending retry failed for {target}: {e:#}"),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if resolved > 0 {
        tracing::info!("fx pending retry resolved {resolved} date(s)");
    }
    Ok(())
}

/// Re-fetch dates stored with fewer than the full quote set, merging in what the source
/// can now supply. A date that stays partial keeps working via per-quote carry-forward.
async fn retry_incomplete(pool: &PgPool, attempted: &mut HashSet<Date>) -> anyhow::Result<()> {
    let floor = source_floor();
    let today = OffsetDateTime::now_utc().date();
    for day in journal_fx::incomplete_dates(pool, RETRY_BATCH).await? {
        if day < floor || !is_business_day(day) || !attempted.insert(day) {
            continue;
        }
        let recent = (today - day).whole_days() <= BACKUP_WINDOW_DAYS;
        match fx::fetch_rates(day, recent).await {
            Ok(Some(f)) => {
                journal_fx::upsert_rates(pool, day, &f.rates, f.source).await?;
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("fx incomplete retry failed for {day}: {e:#}"),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(())
}
