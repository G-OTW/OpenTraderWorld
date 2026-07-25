//! Background tick for the RemindMe module.
//!
//! Wakes every minute, fires any reminders whose `next_fire_at` is due (writing
//! notifications and advancing each reminder's schedule), and logs how many fired.
//! Each fired notification is also pushed to every enabled external channel
//! (email/telegram/slack/discord). The frontend polls the unread endpoint to surface
//! in-app slide-in toasts.

use std::time::Duration;

use otw_store::crypto::SecretCipher;
use reqwest::Client;
use sqlx::PgPool;

use crate::notif_send;

/// How often to check for due reminders.
const TICK: Duration = Duration::from_secs(60);

/// Spawn the reminder tick loop. Returns immediately; runs until the process exits.
pub fn spawn(pool: PgPool, cipher: SecretCipher, http: Client) {
    tokio::spawn(async move {
        // Small initial delay so startup/migrations settle.
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            match otw_store::reminders::fire_due(&pool).await {
                Ok(fired) if !fired.is_empty() => {
                    tracing::info!("fired {} reminder notification(s)", fired.len());
                    for n in &fired {
                        notif_send::dispatch(&pool, &cipher, &http, n).await;
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::error!("reminder tick failed: {e:#}"),
            }
        }
    });
}
