//! The robots' tick loop, by priority.
//!
//! One task, draining whatever is due, then sleeping until the soonest planned tick (at most
//! [`TICK`]): a robot ticks the instant its bar closes, not on the next poll. A session already running is not
//! claimed (`claim_due` filters on it), so a one-minute rule over a five-minute simulation
//! keeps its cadence instead of building a backlog it can never clear.
//!
//! Catch-up needs no code of its own: an occurrence that came due while the engine was down
//! is simply still in the past when the loop starts, so it is claimed once and re-planned
//! forward from the rule. Three days offline fire one run, not three.

use std::time::Duration;

use time::OffsetDateTime;

use otw_store::robots as store;

use crate::{robots as paper, AppState};

/// The longest the loop sleeps. It wakes earlier, at the planned instant, when a tick is due
/// sooner: a robot is planned on its bar's close and fires then, not up to a poll later.
const TICK: Duration = Duration::from_secs(1);
/// How often status reports and exclusion boundaries are looked at.
const CALENDAR_CYCLE: Duration = Duration::from_secs(1);
/// Robots ticking at once: each is a full simulation, and the machine also serves the UI. A
/// cap keeps one crowded close from monopolizing the blocking pool.
const MAX_PER_TICK: usize = 8;

/// How often the guards of every robot are read for what time restores.
const GUARD_CYCLE: Duration = Duration::from_secs(30);

/// Send the status reports that are due.
async fn report_pass(state: &AppState) {
    match store::due_reports(&state.pool, OffsetDateTime::now_utc()).await {
        Ok(due) => {
            for s in &due {
                paper::report(state, s).await;
            }
        }
        Err(e) => tracing::error!("robots: listing due reports: {e:#}"),
    }
}

/// Enter and leave exclusion periods. On the way in a robot set to leave the market closes
/// what it holds; on the way out it trades again (its entries were only held). Said on the
/// robot's channels only when it holds something.
async fn exclusion_pass(state: &AppState) {
    let sessions = match store::list_sessions(&state.pool).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("robots: listing for the exclusion pass: {e:#}");
            return;
        }
    };
    let now = OffsetDateTime::now_utc();
    for s in sessions.iter().filter(|s| s.status == "active") {
        let why = paper::excluded(s, now);
        match (why, s.excluded_at) {
            (Some(why), None) => {
                if let Err(e) = store::set_excluded(&state.pool, s.id, Some(now)).await {
                    tracing::error!("robots: entering the exclusion of \"{}\": {e:#}", s.name);
                    continue;
                }
                let leave = paper::exclusions::Exclusions::of(&s.exclusions).leave_market;
                let book = paper::book(state, s).await;
                let holding = paper::holds_position(&book);
                let closed = if leave && holding {
                    match paper::live::close_all(state, s, "exclusion").await {
                        Ok(n) => n,
                        Err(e) => {
                            tracing::error!("robots: leaving the market for \"{}\": {e:#}", s.name);
                            0
                        }
                    }
                } else {
                    0
                };
                let message = if closed > 0 {
                    format!("The bot is {why}: no new entry, {closed} position(s) closed.")
                } else {
                    format!("The bot is {why}: no new entry until it ends, exits are still managed.")
                };
                let title = format!("{}: exclusion", s.name);
                let payload = serde_json::json!({ "exclusion": why, "closed": closed });
                let _ = store::add_event(&state.pool, s.id, "exclusion", &payload, &title, &message, holding).await;
                if holding {
                    paper::deliver_events(state, s).await;
                }
                paper::live::poke();
                if s.on_broker() {
                    paper::exec::poke();
                }
            }
            (None, Some(_)) => {
                if let Err(e) = store::set_excluded(&state.pool, s.id, None).await {
                    tracing::error!("robots: leaving the exclusion of \"{}\": {e:#}", s.name);
                    continue;
                }
                let holding = paper::holds_position(&paper::book(state, s).await);
                let title = format!("{}: exclusion over", s.name);
                let message = "The exclusion period is over: the bot takes new entries again.".to_string();
                let _ = store::add_event(&state.pool, s.id, "exclusion", &serde_json::json!({}), &title, &message, holding).await;
                if holding {
                    paper::deliver_events(state, s).await;
                }
                paper::live::poke();
                if s.on_broker() {
                    paper::exec::poke();
                }
            }
            _ => {}
        }
    }
}

/// Read the guards of every robot that holds something, for what the time restored.
async fn guard_pass(state: &AppState) {
    let sessions = match store::list_sessions(&state.pool).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("robots: listing for the guard cycle: {e:#}");
            return;
        }
    };
    let timed = |s: &&store::Session| {
        let g = paper::guards::State::of(&s.guard_state);
        g.halt_kind.as_deref() == Some("pause") || matches!(g.scale_restore, Some(paper::guards::Restore::AfterDays { .. }))
    };
    for s in sessions.iter().filter(timed) {
        if let Err(e) = paper::run_guards(state, s).await {
            tracing::warn!("robots: guards of \"{}\": {e:#}", s.name);
        }
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        // A crash leaves `running` set, which would make a session unclaimable forever.
        match store::release_orphans(&state.pool).await {
            Ok(n) if n > 0 => tracing::info!("paper: released {n} interrupted session(s)"),
            Err(e) => tracing::error!("paper: releasing interrupted sessions: {e:#}"),
            _ => {}
        }
        let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_PER_TICK));
        let mut guard_cycle = tokio::time::Instant::now();
        let mut calendar_cycle = tokio::time::Instant::now();
        loop {
            if calendar_cycle.elapsed() >= CALENDAR_CYCLE {
                calendar_cycle = tokio::time::Instant::now();
                exclusion_pass(&state).await;
                report_pass(&state).await;
            }
            // Guards that run out on their own (a pause, a scale restored after some days) are
            // read on a cycle, not only when the robot ticks.
            if guard_cycle.elapsed() >= GUARD_CYCLE {
                guard_cycle = tokio::time::Instant::now();
                guard_pass(&state).await;
            }
            // Due robots tick side by side: one waiting for its provider's bar never delays
            // another's close. At most MAX_PER_TICK at once.
            loop {
                let Ok(permit) = slots.clone().try_acquire_owned() else { break };
                let now = OffsetDateTime::now_utc();
                let session = match store::claim_due(&state.pool, now).await {
                    Ok(Some(s)) => s,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("paper: claiming a due session: {e:#}");
                        break;
                    }
                };
                let state = state.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let name = session.name.clone();
                    let planned = session.next_run_at;
                    let t = paper::run_once(&state, &session, false).await;
                    // How late after its bar's close the robot decided: the latency to watch.
                    if let Some(at) = planned {
                        let lag = OffsetDateTime::now_utc() - at;
                        tracing::info!("robots: \"{name}\" ticked {} ms after its close (claimed at +{} ms)", lag.whole_milliseconds(), (now - at).whole_milliseconds());
                    }
                    if !t.error.is_empty() {
                        tracing::warn!("paper: session \"{name}\" failed: {}", t.error);
                    } else if t.changed() {
                        tracing::info!("paper: session \"{name}\": {} opened, {} closed", t.opened.len(), t.closed.len());
                    }
                });
            }
            // Sleep until the soonest planned tick, never longer than TICK.
            let wait = match store::next_due_at(&state.pool).await {
                Ok(Some(at)) => {
                    // A floor so a due robot waiting for a free slot is not polled in a spin.
                    let ms = (at - OffsetDateTime::now_utc()).whole_milliseconds().clamp(20, TICK.as_millis() as i128);
                    Duration::from_millis(ms as u64)
                }
                _ => TICK,
            };
            tokio::time::sleep(wait).await;
        }
    });
}
