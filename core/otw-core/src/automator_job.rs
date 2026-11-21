//! Background tick for the Automator.
//!
//! Wakes every minute, claims the schedules that are due, and starts one run each. Three
//! rules live here and nowhere else:
//!
//! - **No overlap.** A workflow that is still running never starts a second run; the
//!   occurrence is dropped and the schedule advances to the next future one. That is what
//!   makes an hourly workflow that occasionally takes 70 minutes safe.
//! - **Catch-up on boot.** A schedule marked `catch_up` whose occurrence passed while the
//!   app was down fires once at startup, whatever the number of occurrences missed. One
//!   catch-up run, not a backlog.
//! - **Watchdog.** A run that outlives its workflow's `max_runtime_secs` is closed as
//!   `timeout`, so a wedged run cannot block its workflow forever.

use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use otw_store::automator::{self as store, Schedule};

use crate::automator::{engine, schedule, Graph, RunMode};
use crate::AppState;

const TICK: Duration = Duration::from_secs(60);
/// Schedules claimed per tick. Well above any realistic single-user install.
const BATCH: i64 = 20;

pub fn spawn(state: AppState) {
    tracing::info!("automator scheduler started");
    tokio::spawn(async move {
        // Let migrations and the router settle before the first pass.
        tokio::time::sleep(Duration::from_secs(8)).await;
        if let Err(e) = catch_up(&state).await {
            tracing::error!("automator catch-up failed: {e:#}");
        }
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            if let Err(e) = run_due(&state).await {
                tracing::error!("automator tick failed: {e:#}");
            }
        }
    });
}

/// One pass: close wedged runs, then start whatever is due.
async fn run_due(state: &AppState) -> anyhow::Result<()> {
    watchdog(&state.pool).await?;
    let due = store::claim_due_schedules(&state.pool, BATCH).await?;
    for s in due {
        start(state, &s, "schedule").await;
        // Advance past every missed occurrence: a run that was skipped is not queued.
        let next = schedule::next_from_now(&s);
        if let Err(e) = store::mark_schedule_ran(&state.pool, s.id, next).await {
            tracing::error!("advancing automator schedule {}: {e:#}", s.id);
        }
    }
    Ok(())
}

/// Startup pass: fire the schedules that came due while the process was down, once each.
async fn catch_up(state: &AppState) -> anyhow::Result<()> {
    for s in store::overdue_schedules(&state.pool).await? {
        let next = schedule::next_from_now(&s);
        if s.catch_up {
            tracing::info!("automator: catching up schedule {}", s.id);
            start(state, &s, "catchup").await;
            store::mark_schedule_ran(&state.pool, s.id, next).await?;
        } else {
            // No catch-up: the missed occurrences are simply gone.
            store::set_next_run(&state.pool, s.id, next).await?;
        }
    }
    Ok(())
}

/// Start one run for a schedule. Failures are logged and never stop the tick.
async fn start(state: &AppState, s: &Schedule, trigger: &str) {
    let Ok(Some(workflow)) = store::get_workflow(&state.pool, s.workflow_id).await else {
        tracing::warn!("automator schedule {} points at a missing workflow", s.id);
        return;
    };
    // A pinned schedule runs its revision; an unpinned one always runs the latest save.
    let (graph_value, version_id) = match s.version_id {
        Some(v) => match store::version_graph(&state.pool, v).await {
            Ok(Some(g)) => (g, Some(v)),
            _ => (workflow.graph.clone(), workflow.version_id),
        },
        None => (workflow.graph.clone(), workflow.version_id),
    };
    let graph = match Graph::parse(&graph_value).and_then(|g| {
        crate::automator::validate(&g)?;
        Ok(g)
    }) {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!("automator workflow \"{}\" is not runnable: {e}", workflow.name);
            return;
        }
    };
    if graph.nodes.is_empty() {
        return;
    }

    match store::start_run(
        &state.pool,
        workflow.id,
        version_id,
        Some(s.id),
        trigger,
        "live",
        &serde_json::json!({}),
    )
    .await
    {
        // `None` is the no-overlap rule doing its job: the previous run is still going.
        Ok(None) => tracing::info!(
            "automator: \"{}\" is still running, occurrence skipped",
            workflow.name
        ),
        Ok(Some(run)) => {
            tracing::info!("automator: starting \"{}\" ({trigger})", workflow.name);
            engine::spawn(state.clone(), workflow, graph, run, RunMode::Live);
        }
        Err(e) => tracing::error!("starting automator run: {e:#}"),
    }
}

/// Close runs that outlived their workflow's limit. Their task may still be in flight; the
/// engine checks the run's status through its own deadline and stops on the same boundary.
async fn watchdog(pool: &PgPool) -> anyhow::Result<()> {
    let stale: Vec<Uuid> = store::stale_runs(pool).await?;
    for id in stale {
        tracing::warn!("automator run {id} exceeded its limit, closing it as timed out");
        store::finish_run(pool, id, "timeout", "the run exceeded its time limit", 0).await?;
    }
    Ok(())
}
