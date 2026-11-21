//! Background tick for the RemindMe module.
//!
//! Wakes on each wall-clock minute boundary (hh:mm:00), fires any reminders whose
//! `next_fire_at` is due (writing notifications and advancing each reminder's schedule),
//! and logs how many fired.
//! Each fired notification is also pushed to every enabled external channel
//! (email/telegram/slack/discord). The frontend polls the unread endpoint to surface
//! in-app slide-in toasts.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use otw_store::crypto::SecretCipher;
use reqwest::Client;
use sqlx::PgPool;

use crate::notif_send;

/// How often to check for due reminders: once a minute, on the minute.
const TICK: Duration = Duration::from_secs(60);

/// Spawn the reminder tick loop. Returns immediately; runs until the process exits.
pub fn spawn(pool: PgPool, cipher: SecretCipher, http: Client) {
    tokio::spawn(async move {
        // Small initial delay so startup/migrations settle.
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            // Re-aligned every pass so a reminder due at hh:mm:00 fires at hh:mm:00, not up
            // to a minute later depending on when the process started.
            let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
            tokio::time::sleep(until_next_tick(since_epoch)).await;
            match otw_store::reminders::fire_due(&pool).await {
                Ok(fired) if !fired.is_empty() => {
                    tracing::info!("fired {} reminder notification(s)", fired.len());
                    for n in &fired {
                        // Reminders have no per-reminder destination choice: every enabled
                        // channel granted to RemindMe gets it.
                        notif_send::dispatch(&pool, &cipher, &http, n, "remindme", None).await;
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::error!("reminder tick failed: {e:#}"),
            }
        }
    });
}

/// Time left until the next minute boundary, given the time since the Unix epoch.
/// Exactly on a boundary, waits a full minute (that boundary was just handled).
fn until_next_tick(since_epoch: Duration) -> Duration {
    let tick_ms = TICK.as_millis();
    let into = since_epoch.as_millis() % tick_ms;
    Duration::from_millis((tick_ms - into) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_until_the_next_minute_boundary() {
        // 19:51:25.300 → 34.7 s to 19:52:00.
        let t = Duration::from_millis(1_795_114_285_300);
        assert_eq!(until_next_tick(t), Duration::from_millis(34_700));
        // Just before a boundary.
        assert_eq!(until_next_tick(Duration::from_millis(59_999)), Duration::from_millis(1));
        // On a boundary: the next one, a minute later.
        assert_eq!(until_next_tick(Duration::from_secs(120)), TICK);
    }
}
