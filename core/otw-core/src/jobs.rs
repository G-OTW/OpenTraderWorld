//! Background jobs, gathered in one place for the Automator's Jobs tab.
//!
//! Each job keeps its settings where its module keeps them (a journal's broker sync, a
//! portfolio's, a mailbox account): this module reads them into one list, applies the
//! tab's actions back to them, and runs the scheduled broker syncs. The built-in loops
//! (FX catch-up, managers' portfolios) keep their own cadence and only consult the pause
//! switch and write to the shared run log.
//!
//! Job ids: `journal-sync:<category>`, `portfolio-sync:<portfolio>`, `mailbox:<account>`,
//! `fx`, `mportfolios`.

use std::collections::HashSet;
use std::future::Future;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::jobs as store;
use otw_store::{journal_import as jstore, portfolios_import as pstore};

use crate::AppState;

pub const FX: &str = "fx";
pub const MPORTFOLIOS: &str = "mportfolios";

/// Broker sync intervals the tab and the modals accept, in minutes.
pub const MIN_INTERVAL: i32 = 15;
pub const MAX_INTERVAL: i32 = 7 * 24 * 60;

const TICK: Duration = Duration::from_secs(60);

// ── Run log and guard ────────────────────────────────────────────────────────

/// What a run reports when it did not fail.
pub struct Outcome {
    /// ok | warning
    pub status: &'static str,
    pub summary: String,
}

impl Outcome {
    pub fn ok(summary: impl Into<String>) -> Self {
        Self { status: "ok", summary: summary.into() }
    }
}

fn running() -> &'static Mutex<HashSet<String>> {
    static RUNNING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(HashSet::new()))
}

fn is_running(id: &str) -> bool {
    running().lock().map(|s| s.contains(id)).unwrap_or(false)
}

/// Releases the running mark however the run ends.
struct Guard(String);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut s) = running().lock() {
            s.remove(&self.0);
        }
    }
}

/// Run `work` as job `id`, logged in `job_runs`. A job already running is refused rather
/// than run twice: a manual run and the schedule landing together would pull the same
/// window side by side.
pub async fn recorded<F>(pool: &sqlx::PgPool, id: &str, trigger: &str, work: F) -> Result<Outcome>
where
    F: Future<Output = Result<Outcome>>,
{
    let _guard = {
        let mut s = running().lock().map_err(|_| anyhow!("job registry poisoned"))?;
        if !s.insert(id.to_string()) {
            return Err(anyhow!("this job is already running"));
        }
        Guard(id.to_string())
    };
    let run = store::start_run(pool, id, trigger).await.ok();
    let result = work.await;
    if let Some(run) = run {
        let (status, summary, error) = match &result {
            Ok(o) => (o.status, Some(o.summary.as_str()), None),
            Err(e) => ("failed", None, Some(format!("{e:#}"))),
        };
        store::finish_run(pool, run, status, summary, error.as_deref()).await.ok();
    }
    result
}

// ── Scheduler ────────────────────────────────────────────────────────────────

/// Wake every minute and run the broker syncs whose interval elapsed, one at a time.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        if let Err(e) = store::close_orphan_runs(&state.pool).await {
            tracing::warn!("jobs: {e:#}");
        }
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            match jstore::due_broker_syncs(&state.pool).await {
                Ok(due) => {
                    for sync in due {
                        let id = format!("journal-sync:{}", sync.category_id);
                        if !is_running(&id) {
                            run_journal(&state, &sync, "schedule").await.ok();
                        }
                    }
                }
                Err(e) => tracing::warn!("jobs: {e:#}"),
            }
            match pstore::due_broker_syncs(&state.pool).await {
                Ok(due) => {
                    for sync in due {
                        let id = format!("portfolio-sync:{}", sync.portfolio_id);
                        if !is_running(&id) {
                            run_portfolio(&state, &sync, "schedule").await.ok();
                        }
                    }
                }
                Err(e) => tracing::warn!("jobs: {e:#}"),
            }
        }
    });
}

async fn run_journal(state: &AppState, sync: &jstore::BrokerSync, trigger: &str) -> Result<Outcome> {
    use crate::journal_import::broker;
    let id = format!("journal-sync:{}", sync.category_id);
    let result = recorded(&state.pool, &id, trigger, async {
        let r = broker::run_scheduled(&state.pool, &state.cipher, sync).await?;
        let summary = format!(
            "{} imported, {} updated, {} unchanged, {} failed, {} waiting for an answer",
            r.imported, r.updated, r.duplicates, r.failed, r.conflicts
        );
        let status = if r.conflicts > 0 || r.failed > 0 { "warning" } else { "ok" };
        // Said once: a new trade or a new question. A pass that changed nothing is quiet.
        if r.imported > 0 || r.updated > 0 || r.new_conflicts > 0 {
            let mut details = format!("{} new trade(s), {} updated.", r.imported, r.updated);
            if r.new_conflicts > 0 {
                details.push_str(&format!(
                    " {} instrument(s) held back: the fills do not say whether they open or \
                     close a position. Answer them in the journal's pending tasks.",
                    r.new_conflicts
                ));
            }
            notify(state, broker::MODULE, "Journal: broker sync", &details).await;
        }
        Ok(Outcome { status, summary })
    })
    .await;
    after_run(state, broker::MODULE, "Journal", sync.last_error.as_deref(), &result).await;
    let error = result.as_ref().err().map(|e| format!("{e:#}"));
    jstore::mark_broker_run(&state.pool, sync.category_id, error.as_deref()).await.ok();
    result
}

async fn run_portfolio(state: &AppState, sync: &pstore::BrokerSync, trigger: &str) -> Result<Outcome> {
    use crate::portfolios_import::broker;
    let id = format!("portfolio-sync:{}", sync.portfolio_id);
    let result = recorded(&state.pool, &id, trigger, async {
        let r = broker::run_scheduled(&state.pool, &state.cipher, sync).await?;
        let summary = format!(
            "{} operation(s) written, {} line(s) in agreement, {} error(s), {} waiting for an answer",
            r.report.imported,
            r.report.skipped + r.report.duplicates,
            r.report.errors.len(),
            r.conflicts
        );
        let status = if r.conflicts > 0 || !r.report.errors.is_empty() { "warning" } else { "ok" };
        if r.report.imported > 0 || r.new_conflicts > 0 {
            let mut details = format!("{} operation(s) written to align on the broker.", r.report.imported);
            if r.new_conflicts > 0 {
                details.push_str(&format!(
                    " {} holding(s) held back: no asset matches them, or no price to file them \
                     at. Open the broker import to answer.",
                    r.new_conflicts
                ));
            }
            notify(state, broker::MODULE, "Portfolio: broker sync", &details).await;
        }
        Ok(Outcome { status, summary })
    })
    .await;
    after_run(state, broker::MODULE, "Portfolio", sync.last_error.as_deref(), &result).await;
    let error = result.as_ref().err().map(|e| format!("{e:#}"));
    pstore::mark_broker_run(&state.pool, sync.portfolio_id, error.as_deref()).await.ok();
    result
}

/// A failure is said when it is new: a broker down for a day would otherwise send the
/// same message at every pass.
async fn after_run(
    state: &AppState,
    module: &str,
    label: &str,
    previous_error: Option<&str>,
    result: &Result<Outcome>,
) {
    if let Err(e) = result {
        let msg = format!("{e:#}");
        if msg != "this job is already running" && previous_error != Some(msg.as_str()) {
            notify(state, module, &format!("{label}: broker sync failed"), &msg).await;
        }
    }
}

async fn notify(state: &AppState, module: &str, title: &str, details: &str) {
    match otw_store::reminders::add_notification(&state.pool, title, details).await {
        Ok(n) => {
            crate::notif_send::dispatch(&state.pool, &state.cipher, &state.http, &n, module, None)
                .await;
        }
        Err(e) => tracing::warn!("jobs notification: {e:#}"),
    }
}

// ── The list ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct JobView {
    pub id: String,
    /// journal_sync | portfolio_sync | mailbox | fx | mportfolios
    pub kind: &'static str,
    /// The module the job belongs to, for its label and its link.
    pub module: &'static str,
    /// What it works on: a journal, a portfolio, a mailbox. Empty for built-in jobs.
    pub target: String,
    /// The broker account a sync reads.
    pub account: Option<String>,
    pub interval_minutes: i32,
    /// Built-in jobs keep the cadence they ship with.
    pub interval_editable: bool,
    pub paused: bool,
    /// Built-in jobs and mailboxes are paused, never deleted from here.
    pub deletable: bool,
    pub running: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_run_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_run_at: Option<OffsetDateTime>,
    /// ok | warning | failed, from the last logged run.
    pub last_status: Option<String>,
    pub last_summary: Option<String>,
    pub last_error: Option<String>,
    /// Conflicts waiting for an answer (broker syncs).
    pub conflicts: i64,
    /// Where the job is configured.
    pub link: String,
}

fn next_of(
    paused: bool,
    last: Option<OffsetDateTime>,
    interval_minutes: i32,
) -> Option<OffsetDateTime> {
    if paused {
        return None;
    }
    Some(
        last.map(|l| l + time::Duration::minutes(interval_minutes as i64))
            .unwrap_or_else(OffsetDateTime::now_utc),
    )
}

pub async fn list(state: &AppState) -> Result<Vec<JobView>> {
    let pool = &state.pool;
    let runs = store::last_runs(pool).await?;
    let accounts = otw_store::brokers::list(pool).await?;
    let account_name = |id: Option<Uuid>| {
        id.and_then(|id| accounts.iter().find(|a| a.id == id).map(|a| a.name.clone()))
    };
    let conflicts = |module: &str, target: Uuid| {
        let module = module.to_string();
        async move {
            store::list_conflicts(pool, &module, Some(target))
                .await
                .map(|c| c.len() as i64)
                .unwrap_or(0)
        }
    };
    let mut out = Vec::new();

    let categories = otw_store::journal::list_categories(pool).await?;
    for s in jstore::scheduled_broker_syncs(pool).await? {
        let id = format!("journal-sync:{}", s.category_id);
        let last = runs.get(&id);
        out.push(JobView {
            kind: "journal_sync",
            module: "journal",
            target: categories
                .iter()
                .find(|c| c.id == s.category_id)
                .map(|c| c.name.clone())
                .unwrap_or_default(),
            account: account_name(s.account_id),
            interval_minutes: s.interval_minutes,
            interval_editable: true,
            paused: s.auto_paused,
            deletable: true,
            running: is_running(&id),
            last_run_at: s.last_run_at,
            next_run_at: next_of(s.auto_paused, s.last_run_at, s.interval_minutes),
            last_status: last.map(|r| r.status.clone()),
            last_summary: last.and_then(|r| r.summary.clone()),
            last_error: s.last_error.clone(),
            conflicts: conflicts("journal", s.category_id).await,
            link: "/journal?view=import".into(),
            id,
        });
    }

    let portfolios = otw_store::portfolios::list_portfolios(pool).await?;
    for s in pstore::scheduled_broker_syncs(pool).await? {
        let id = format!("portfolio-sync:{}", s.portfolio_id);
        let last = runs.get(&id);
        out.push(JobView {
            kind: "portfolio_sync",
            module: "portfolios",
            target: portfolios
                .iter()
                .find(|p| p.id == s.portfolio_id)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            account: account_name(s.account_id),
            interval_minutes: s.interval_minutes,
            interval_editable: true,
            paused: s.auto_paused,
            deletable: true,
            running: is_running(&id),
            last_run_at: s.last_run_at,
            next_run_at: next_of(s.auto_paused, s.last_run_at, s.interval_minutes),
            last_status: last.map(|r| r.status.clone()),
            last_summary: last.and_then(|r| r.summary.clone()),
            last_error: s.last_error.clone(),
            conflicts: conflicts("portfolios", s.portfolio_id).await,
            link: "/portfolios".into(),
            id,
        });
    }

    for a in otw_store::mailbox::list_accounts(pool).await? {
        let minutes = (a.interval_secs / 60).max(1);
        out.push(JobView {
            id: format!("mailbox:{}", a.id),
            kind: "mailbox",
            module: "mailbox",
            target: a.name.clone(),
            account: None,
            interval_minutes: minutes,
            interval_editable: true,
            paused: !a.enabled,
            deletable: false,
            running: false,
            last_run_at: a.last_poll_at,
            next_run_at: next_of(!a.enabled, a.last_poll_at, minutes),
            last_status: Some(if a.last_error.is_some() { "failed" } else { "ok" }.to_string())
                .filter(|_| a.last_poll_at.is_some()),
            last_summary: None,
            last_error: a.last_error.clone(),
            conflicts: 0,
            link: "/mailbox".into(),
        });
    }

    for (id, minutes, link) in [
        (FX, (crate::fx_job::TICK.as_secs() / 60) as i32, "/journal?view=pending"),
        (MPORTFOLIOS, (crate::mportfolios_job::PERIOD.as_secs() / 60) as i32, "/mportfolios"),
    ] {
        let last = runs.get(id);
        let paused = !store::system_enabled(pool, id).await;
        out.push(JobView {
            id: id.to_string(),
            kind: id,
            module: "system",
            target: String::new(),
            account: None,
            interval_minutes: minutes,
            interval_editable: false,
            paused,
            deletable: false,
            running: is_running(id),
            last_run_at: last.map(|r| r.started_at),
            next_run_at: next_of(paused, last.map(|r| r.started_at), minutes),
            last_status: last.map(|r| r.status.clone()),
            last_summary: last.and_then(|r| r.summary.clone()),
            last_error: last.and_then(|r| r.error.clone()),
            conflicts: 0,
            link: link.into(),
        });
    }
    Ok(out)
}

// ── Actions ──────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct Patch {
    #[serde(default)]
    pub paused: Option<bool>,
    #[serde(default)]
    pub interval_minutes: Option<i32>,
}

pub fn check_interval(minutes: i32) -> Result<()> {
    if !(MIN_INTERVAL..=MAX_INTERVAL).contains(&minutes) {
        return Err(anyhow!(
            "the interval must be between {MIN_INTERVAL} minutes and {} days",
            MAX_INTERVAL / 1440
        ));
    }
    Ok(())
}

/// Split `kind:uuid`.
fn parse(id: &str) -> Result<(&str, Option<Uuid>)> {
    match id.split_once(':') {
        Some((kind, rest)) => {
            let uuid = Uuid::parse_str(rest).map_err(|_| anyhow!("unknown job"))?;
            Ok((kind, Some(uuid)))
        }
        None => Ok((id, None)),
    }
}

pub async fn update(state: &AppState, id: &str, patch: &Patch) -> Result<()> {
    let pool = &state.pool;
    if let Some(m) = patch.interval_minutes {
        check_interval(m)?;
    }
    match parse(id)? {
        ("journal-sync", Some(cat)) => {
            let s = jstore::get_broker_sync(pool, cat)
                .await?
                .filter(|s| s.auto_enabled)
                .ok_or_else(|| anyhow!("unknown job"))?;
            jstore::set_broker_schedule(
                pool,
                cat,
                true,
                patch.paused.unwrap_or(s.auto_paused),
                patch.interval_minutes.unwrap_or(s.interval_minutes),
            )
            .await?;
        }
        ("portfolio-sync", Some(pf)) => {
            let s = pstore::get_broker_sync(pool, pf)
                .await?
                .filter(|s| s.auto_enabled)
                .ok_or_else(|| anyhow!("unknown job"))?;
            pstore::set_broker_schedule(
                pool,
                pf,
                true,
                patch.paused.unwrap_or(s.auto_paused),
                patch.interval_minutes.unwrap_or(s.interval_minutes),
            )
            .await?;
        }
        ("mailbox", Some(acc)) => {
            otw_store::mailbox::update_account(
                pool,
                acc,
                &otw_store::mailbox::AccountPatch {
                    enabled: patch.paused.map(|p| !p),
                    interval_secs: patch.interval_minutes.map(|m| m * 60),
                    ..Default::default()
                },
            )
            .await?
            .ok_or_else(|| anyhow!("unknown job"))?;
        }
        (FX, None) | (MPORTFOLIOS, None) => {
            if patch.interval_minutes.is_some() {
                return Err(anyhow!("a built-in job keeps the cadence it ships with"));
            }
            if let Some(p) = patch.paused {
                store::set_system_enabled(pool, id, !p).await?;
            }
        }
        _ => return Err(anyhow!("unknown job")),
    }
    Ok(())
}

/// Remove a broker sync's schedule. The sync itself (last window, answers to conflicts)
/// stays, so the modal still reopens on it.
pub async fn delete(state: &AppState, id: &str) -> Result<()> {
    let pool = &state.pool;
    let found = match parse(id)? {
        ("journal-sync", Some(cat)) => jstore::set_broker_schedule(pool, cat, false, false, 60).await?,
        ("portfolio-sync", Some(pf)) => {
            pstore::set_broker_schedule(pool, pf, false, false, 1440).await?
        }
        ("mailbox", Some(_)) | (FX, None) | (MPORTFOLIOS, None) => {
            return Err(anyhow!("this job can be paused, not deleted"))
        }
        _ => false,
    };
    if !found {
        return Err(anyhow!("unknown job"));
    }
    Ok(())
}

/// Start a job now, in the background. Returns once it is started, not finished.
pub async fn run_now(state: &AppState, id: &str) -> Result<()> {
    if is_running(id) {
        return Err(anyhow!("this job is already running"));
    }
    let pool = &state.pool;
    match parse(id)? {
        ("journal-sync", Some(cat)) => {
            let s = jstore::get_broker_sync(pool, cat)
                .await?
                .filter(|s| s.auto_enabled)
                .ok_or_else(|| anyhow!("unknown job"))?;
            let state = state.clone();
            tokio::spawn(async move {
                run_journal(&state, &s, "manual").await.ok();
            });
        }
        ("portfolio-sync", Some(pf)) => {
            let s = pstore::get_broker_sync(pool, pf)
                .await?
                .filter(|s| s.auto_enabled)
                .ok_or_else(|| anyhow!("unknown job"))?;
            let state = state.clone();
            tokio::spawn(async move {
                run_portfolio(&state, &s, "manual").await.ok();
            });
        }
        ("mailbox", Some(acc)) => otw_store::mailbox::schedule_soon(pool, acc, 5).await?,
        (FX, None) => {
            let pool = pool.clone();
            tokio::spawn(async move {
                recorded(&pool, FX, "manual", crate::fx_job::run_once(&pool)).await.ok();
            });
        }
        (MPORTFOLIOS, None) => {
            let (pool, lock) = (pool.clone(), state.mportfolios_refresh.clone());
            tokio::spawn(async move {
                recorded(&pool, MPORTFOLIOS, "manual", async {
                    crate::mportfolios_job::refresh(&pool, &lock).await?;
                    Ok(Outcome::ok("managers refreshed"))
                })
                .await
                .ok();
            });
        }
        _ => return Err(anyhow!("unknown job")),
    }
    Ok(())
}
