//! Paper trading tick loop.
//!
//! One task, one minute apart, draining whatever is due. A session already running is not
//! claimed (`claim_due` filters on it), so a one-minute rule over a five-minute simulation
//! keeps its cadence instead of building a backlog it can never clear.
//!
//! Catch-up needs no code of its own: an occurrence that came due while the engine was down
//! is simply still in the past when the loop starts, so it is claimed once and re-planned
//! forward from the rule. Three days offline fire one run, not three.

use std::time::Duration;

use time::OffsetDateTime;

use otw_store::paper as store;

use crate::{paper, AppState};

/// How often the queue is looked at. Sessions are planned on the bar grid and fire the second
/// a period closes, so the poll has to be finer than the finest rule: a one-minute poll would
/// round every occurrence up to a minute late and read the bar after the one it waited for.
const TICK: Duration = Duration::from_secs(1);
/// Sessions run one after another: each is a full simulation, and the machine also serves the
/// UI. A cap keeps one crowded minute from monopolizing the blocking pool.
const MAX_PER_TICK: usize = 8;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        // A crash leaves `running` set, which would make a session unclaimable forever.
        match store::release_orphans(&state.pool).await {
            Ok(n) if n > 0 => tracing::info!("paper: released {n} interrupted session(s)"),
            Err(e) => tracing::error!("paper: releasing interrupted sessions: {e:#}"),
            _ => {}
        }
        loop {
            for _ in 0..MAX_PER_TICK {
                let now = OffsetDateTime::now_utc();
                let session = match store::claim_due(&state.pool, now).await {
                    Ok(Some(s)) => s,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("paper: claiming a due session: {e:#}");
                        break;
                    }
                };
                let name = session.name.clone();
                let t = paper::run_once(&state, &session, false).await;
                if !t.error.is_empty() {
                    tracing::warn!("paper: session \"{name}\" failed: {}", t.error);
                } else if t.changed() {
                    tracing::info!(
                        "paper: session \"{name}\": {} opened, {} closed",
                        t.opened.len(),
                        t.closed.len()
                    );
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}
