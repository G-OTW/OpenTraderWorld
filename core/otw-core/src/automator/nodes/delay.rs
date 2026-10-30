//! `delay`: wait between two blocks.
//!
//! Counted against the run's own deadline: a delay can never push a run past
//! `max_runtime_secs`, it just wakes up early and the run ends normally.
//!
//! It also wakes up regularly to look for a stop request. Without that, pressing Stop on a
//! run parked in a five-minute wait would do nothing visible for five minutes.

use serde_json::{json, Value};

use super::{NodeCtx, Outcome};

const MAX_SECONDS: u64 = 300;

pub fn validate(config: &Value) -> Result<(), String> {
    let secs = config.get("seconds").and_then(|v| v.as_u64()).unwrap_or(0);
    if secs == 0 || secs > MAX_SECONDS {
        return Err(format!("seconds must be between 1 and {MAX_SECONDS}"));
    }
    Ok(())
}

/// How often the wait looks up to see whether the run was stopped.
const SLICE: std::time::Duration = std::time::Duration::from_secs(2);

pub async fn run(ctx: &NodeCtx<'_>) -> Result<Outcome, String> {
    let asked = ctx.node.config.get("seconds").and_then(|v| v.as_u64()).unwrap_or(1).min(MAX_SECONDS);
    let started = std::time::Instant::now();
    let target = std::time::Duration::from_secs(asked)
        .min(ctx.deadline.saturating_duration_since(started));

    let mut stopped = false;
    while started.elapsed() < target {
        let left = target - started.elapsed();
        tokio::time::sleep(left.min(SLICE)).await;
        if left > SLICE
            && otw_store::automator::cancel_requested(&ctx.state.pool, ctx.run_id)
                .await
                .unwrap_or(false)
        {
            // The engine sees the same flag on its next check and ends the run; the step
            // records that the wait was cut short rather than pretending it completed.
            stopped = true;
            break;
        }
    }
    Ok(Outcome::new(
        json!({ "seconds": asked }),
        json!({ "waited_ms": started.elapsed().as_millis() as u64, "stopped": stopped }),
    ))
}
