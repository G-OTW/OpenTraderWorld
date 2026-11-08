//! Time alerts (`op = at_time`), fired on their instant.
//!
//! A time alert reads no bar, so it has no reason to wait for the minute loop of
//! [`crate::histviz_alerts`] and its settle delay. One task holds every time alert: it asks the
//! store for the next instant, sleeps until it, fires what is due through
//! [`crate::histviz_alerts::fire_time`] (the trade first, then the notification, then the
//! channels), and asks again. A create, an update or a delete wakes it ([`poke`]), so a new
//! instant is picked up at once.
//!
//! The sleep is capped ([`CAP`]): a missed wake-up or a clock step costs at most that much.
//! [`otw_store::histviz::record_time_fire`] disables the row as it fires, so an instant fires
//! once whatever wakes the task.

use std::sync::LazyLock;
use std::time::Duration;

use sqlx::PgPool;
use time::OffsetDateTime;
use tokio::sync::Notify;

use otw_store::histviz as store;

use crate::histviz_alerts::{self as alerts, AlertCx};

/// Longest sleep between two looks at the store, whatever the next instant.
const CAP: Duration = Duration::from_secs(60);
/// Wait before the first pass, so a restart does not race the rest of the boot.
const WARMUP: Duration = Duration::from_secs(5);

/// The scheduler's wake-up. A static rather than a field of the state, so the API reaches it
/// from wherever it changes an alert.
static POKE: LazyLock<Notify> = LazyLock::new(Notify::new);

/// Time alerts changed: look at the store now rather than at the next planned wake-up.
pub fn poke() {
    POKE.notify_one();
}

pub fn spawn(pool: PgPool, cx: AlertCx) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        loop {
            let wait = match pass(&pool, &cx).await {
                Ok(next) => until(next, OffsetDateTime::now_utc()),
                Err(e) => {
                    tracing::error!("time alerts: pass failed: {e:#}");
                    CAP
                }
            };
            tokio::select! {
                _ = POKE.notified() => {}
                _ = tokio::time::sleep(wait) => {}
            }
        }
    });
}

/// Fire every time alert that is due, then say when the next one is.
async fn pass(pool: &PgPool, cx: &AlertCx) -> anyhow::Result<Option<OffsetDateTime>> {
    let now = OffsetDateTime::now_utc();
    for a in store::due_time_alerts(pool, now).await? {
        if let Err(e) = alerts::fire_time(pool, cx, &a).await {
            let msg = alerts::short(&format!("{e:#}"));
            tracing::warn!("time alert {} ({}): {msg}", a.id, a.ticker);
            let _ = store::mark_error(pool, a.id, &msg).await;
        } else if let Some(at) = a.fire_at {
            let late_ms = (OffsetDateTime::now_utc() - at).whole_milliseconds() as i64;
            tracing::info!(alert = %a.id, ticker = %a.ticker, late_ms, "time alert fired");
        }
    }
    Ok(store::next_time_alert_at(pool, now).await?)
}

/// How long to sleep before the next look: until `next`, never past [`CAP`], at once when it is
/// already due.
fn until(next: Option<OffsetDateTime>, now: OffsetDateTime) -> Duration {
    let Some(next) = next else { return CAP };
    let ms = (next - now).whole_milliseconds();
    if ms <= 0 {
        return Duration::ZERO;
    }
    Duration::from_millis(ms as u64).min(CAP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn the_task_sleeps_until_the_next_instant() {
        let now = datetime!(2026-11-05 18:09:58.250 UTC);
        assert_eq!(until(Some(datetime!(2026-11-05 18:10:00 UTC)), now), Duration::from_millis(1_750));
        assert_eq!(until(Some(datetime!(2026-11-05 18:09:58.251 UTC)), now), Duration::from_millis(1));
    }

    #[test]
    fn a_due_or_past_instant_is_fired_at_once() {
        let now = datetime!(2026-11-05 18:10:00 UTC);
        assert_eq!(until(Some(now), now), Duration::ZERO);
        assert_eq!(until(Some(datetime!(2026-11-05 18:09:00 UTC)), now), Duration::ZERO);
    }

    #[test]
    fn the_sleep_is_capped_so_a_missed_poke_is_picked_up() {
        let now = datetime!(2026-11-05 18:10:00 UTC);
        assert_eq!(until(None, now), CAP);
        assert_eq!(until(Some(datetime!(2026-11-05 19:00:00 UTC)), now), CAP);
        assert_eq!(until(Some(datetime!(2026-11-05 18:11:00 UTC)), now), CAP);
        assert_eq!(until(Some(datetime!(2026-11-05 18:10:59.999 UTC)), now), Duration::from_millis(59_999));
    }

    /// Over a scratch database (`OTW_TEST_DATABASE_URL`, skipped without it): a due time alert
    /// fires once and pauses, and the pass names the next instant.
    #[tokio::test]
    async fn a_pass_fires_what_is_due_once_and_names_the_next_instant() {
        let Ok(url) = std::env::var("OTW_TEST_DATABASE_URL") else { return };
        let pool = otw_store::connect_and_migrate(&url).await.unwrap();
        let cx = AlertCx { cipher: otw_store::crypto::SecretCipher::from_master("mock-tests").unwrap(), http: reqwest::Client::new() };
        let now = OffsetDateTime::now_utc();
        let make = |name: String, at: OffsetDateTime| {
            let pool = pool.clone();
            async move {
                store::create_alert(
                    &pool,
                    store::AlertInput {
                        name: &name,
                        provider: "binance",
                        asset_type: "crypto",
                        ticker: "BTCUSDT",
                        timeframe: "1m",
                        connector_id: None,
                        lhs: &serde_json::json!({ "kind": "price", "field": "close" }),
                        rhs: &serde_json::json!({ "kind": "const", "value": 0.0 }),
                        inputs: &serde_json::json!({}),
                        op: "at_time",
                        channels: &serde_json::json!([]),
                        mode: "background",
                        frequency: "once",
                        cooldown_secs: 0,
                        session_id: None,
                        enabled: true,
                        expires_at: None,
                        fire_at: Some(at),
                    },
                )
                .await
                .unwrap()
            }
        };
        let due = make(format!("due {}", uuid::Uuid::new_v4()), now - time::Duration::seconds(1)).await;
        let later = make(format!("later {}", uuid::Uuid::new_v4()), now + time::Duration::hours(1)).await;

        let next = pass(&pool, &cx).await.unwrap();
        assert!(next.is_some_and(|t| t > now && t <= later.fire_at.unwrap()), "{next:?}");
        let fired = store::get_alert(&pool, due.id).await.unwrap().unwrap();
        assert!(!fired.enabled && fired.fire_count == 1);
        let told: i64 = sqlx::query_scalar("SELECT count(*) FROM notifications WHERE linked_id = $1").bind(due.id).fetch_one(&pool).await.unwrap();
        assert_eq!(told, 1);
        // Paused as it fired: a second pass does not fire it again.
        pass(&pool, &cx).await.unwrap();
        assert_eq!(store::get_alert(&pool, due.id).await.unwrap().unwrap().fire_count, 1);
        assert!(store::get_alert(&pool, later.id).await.unwrap().unwrap().enabled);
        store::delete_alert(&pool, due.id).await.unwrap();
        store::delete_alert(&pool, later.id).await.unwrap();
    }
}
