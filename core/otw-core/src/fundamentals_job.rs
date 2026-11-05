//! Scheduled refresh of what Fundamentals stores.
//!
//! Every stored series is refreshed once its frequency says a new value may be out (a
//! daily series twice a day, a monthly one daily, an annual one weekly). Followed
//! companies are refreshed from EDGAR once a day. A provider with no connector granted to
//! the module is skipped silently: the page already names that fix.
//!
//! What changed is notified as `fundamentals` through the channel broker: a series with a
//! new period, a followed company with new filings. A first fetch notifies nothing.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use time::OffsetDateTime;
use uuid::Uuid;

use crate::fundamentals::{self as fund, edgar};
use crate::AppState;
use otw_store::fundamentals as store;

const TICK: Duration = Duration::from_secs(15 * 60);
/// Leave the server time to start (and the user to look at it) before the first pass.
const FIRST_TICK: Duration = Duration::from_secs(2 * 60);
/// A series or company whose last refresh failed is retried this long after.
const RETRY: Duration = Duration::from_secs(6 * 3600);
/// Refreshes per pass, so a long catalog spreads over several passes.
const MAX_PER_TICK: usize = 12;
const COMPANY_EVERY: time::Duration = time::Duration::hours(24);
/// Filings counted as news. Insider forms (3, 4, 5) arrive daily for large companies and
/// have their own table on the Ownership tab.
const QUIET_FORMS: &[&str] = &["3", "4", "5", "3/A", "4/A", "5/A", "144"];

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_TICK).await;
        let mut failed: HashMap<Uuid, Instant> = HashMap::new();
        loop {
            if let Err(e) = tick(&state, &mut failed).await {
                tracing::warn!("fundamentals: scheduled refresh: {e:#}");
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// How long a series stays fresh, by frequency.
fn series_every(freq: &str) -> time::Duration {
    match freq {
        "D" => time::Duration::hours(12),
        "W" | "M" => time::Duration::hours(24),
        "Q" => time::Duration::days(3),
        "A" => time::Duration::days(7),
        _ => time::Duration::hours(24),
    }
}

fn due(last: Option<OffsetDateTime>, every: time::Duration, now: OffsetDateTime) -> bool {
    last.is_none_or(|t| now - t >= every)
}

fn retry_ok(failed: &HashMap<Uuid, Instant>, id: Uuid) -> bool {
    failed.get(&id).is_none_or(|t| t.elapsed() >= RETRY)
}

async fn tick(state: &AppState, failed: &mut HashMap<Uuid, Instant>) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let connectors = otw_store::connectors::list_for_module(&state.pool, fund::MODULE).await?;
    let granted: HashSet<String> = connectors.iter().map(|c| c.provider.clone()).collect();
    // A provider whose declared quota is spent waits for the next window, quietly.
    let mut spent = HashSet::new();
    for c in &connectors {
        if fund::skips::quota_spent(&state.pool, c.id).await.is_some() {
            spent.insert(c.provider.clone());
        }
    }
    let mut budget = MAX_PER_TICK;

    for s in store::list_series(&state.pool).await? {
        if budget == 0 {
            return Ok(());
        }
        if s.status == "pending"
            || !granted.contains(&s.provider)
            || spent.contains(&s.provider)
            || !due(s.last_refreshed, series_every(&s.frequency), now)
            || !retry_ok(failed, s.id)
        {
            continue;
        }
        budget -= 1;
        match fund::refresh_series_now(state, &s).await {
            Ok(true) => {
                failed.remove(&s.id);
                if s.last_period.is_some() {
                    notify_series(state, &s).await;
                }
            }
            Ok(false) => {}
            Err(e) => {
                failed.insert(s.id, Instant::now());
                tracing::warn!("fundamentals: {}:{} refresh failed: {e:#}", s.provider, s.code);
            }
        }
    }

    if !granted.contains(edgar::PROVIDER) || spent.contains(edgar::PROVIDER) {
        return Ok(());
    }
    for c in store::list_companies(&state.pool).await? {
        if budget == 0 {
            return Ok(());
        }
        if !c.followed || c.status == "pending" || !due(c.last_refreshed, COMPANY_EVERY, now) || !retry_ok(failed, c.id) {
            continue;
        }
        budget -= 1;
        let before: HashSet<String> = filings(state, c.id).await?.into_iter().map(|d| d.accession).collect();
        match fund::refresh_company_now(state, &c).await {
            Ok(true) => {
                failed.remove(&c.id);
                if c.last_refreshed.is_some() {
                    let new: Vec<store::DocumentRow> = filings(state, c.id)
                        .await?
                        .into_iter()
                        .filter(|d| !before.contains(&d.accession) && !QUIET_FORMS.contains(&d.form.as_str()))
                        .collect();
                    if !new.is_empty() {
                        notify_filings(state, &c, &new).await;
                    }
                }
            }
            Ok(false) => {}
            Err(e) => {
                failed.insert(c.id, Instant::now());
                tracing::warn!("fundamentals: {} refresh failed: {e:#}", c.ticker);
            }
        }
    }
    Ok(())
}

async fn filings(state: &AppState, company: Uuid) -> anyhow::Result<Vec<store::DocumentRow>> {
    store::documents(&state.pool, Some(company), Some("filing"), None, 200).await
}

/// A new period for a series: its value, unit and period.
async fn notify_series(state: &AppState, before: &store::SeriesRow) {
    let Ok(Some(after)) = store::get_series(&state.pool, before.id).await else { return };
    let (Some(period), Some(value)) = (after.last_period, after.last) else { return };
    if before.last_period.is_some_and(|p| p >= period) {
        return;
    }
    let details = format!("{value} {} for the period starting {period} ({})", after.unit, after.provider)
        .replace("  ", " ");
    notify(state, &format!("{} updated", after.title), &details, "/fundamentals").await;
}

async fn notify_filings(state: &AppState, c: &store::CompanyRow, new: &[store::DocumentRow]) {
    let forms: Vec<&str> = new.iter().map(|d| d.form.as_str()).collect();
    let title = if new.len() == 1 {
        format!("{}: new {} filing", c.ticker, forms[0])
    } else {
        format!("{}: {} new filings", c.ticker, new.len())
    };
    let details = new
        .iter()
        .take(10)
        .map(|d| format!("{} {}: {}", d.filed_at, d.form, d.title))
        .collect::<Vec<_>>()
        .join("\n");
    notify(state, &title, &details, &format!("/fundamentals/company?t={}", c.ticker)).await;
}

async fn notify(state: &AppState, title: &str, details: &str, link: &str) {
    match otw_store::reminders::add_linked_notification(&state.pool, fund::MODULE, title, details, link).await {
        Ok(mut n) => {
            // The link is a path inside the app: it means nothing in an email or a chat.
            n.url.clear();
            crate::notif_send::dispatch(&state.pool, &state.cipher, &state.http, &n, fund::MODULE, None).await;
        }
        Err(e) => tracing::warn!("fundamentals notification: {e:#}"),
    }
}
