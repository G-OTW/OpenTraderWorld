//! Notifications pushed to the open app the moment their row exists.
//!
//! Every insert into `notifications` fires `pg_notify('otw_notifications', id)` (migration
//! `0183_notifications_notify.sql`), whoever inserted it. One task holds one `LISTEN`
//! connection, reloads each announced row by id with the unread count, and fans it out on a
//! broadcast channel; `GET /api/notifications/stream` relays that channel as SSE.
//!
//! The stream is a fast path, not the record: a notification sent while the listener was
//! reconnecting, or while a viewer lagged, is never replayed. Instead a `resync` event tells
//! the page to run its unread poll, which reads the table and catches up.

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use sqlx::postgres::PgListener;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::AppState;
use otw_store::reminders::{self, Notification};

/// The Postgres channel the trigger announces on.
pub const CHANNEL: &str = "otw_notifications";

/// Pushes a slow viewer may fall behind by before it is told to resync.
const BUFFER: usize = 256;

/// Wait before retrying a listener that could not connect or failed to receive.
const RETRY_EVERY: Duration = Duration::from_secs(5);

/// One message on the fan-out channel.
#[derive(Debug, Clone)]
pub enum Push {
    /// A new row, as the unread poll returns it, with the unread count right after it.
    Notification { notification: Notification, unread: i64 },
    /// Pushes may have been missed (the listener reconnected): poll to catch up.
    Resync,
}

/// The fan-out every open stream subscribes to. Cheap to clone.
#[derive(Clone)]
pub struct NotifHub {
    tx: broadcast::Sender<Push>,
}

impl NotifHub {
    /// A hub with no listener behind it (tests); [`spawn`] is the one that is fed.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BUFFER);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Push> {
        self.tx.subscribe()
    }
}

/// Start the listener task and return the hub it feeds. The task never exits: a lost
/// connection is reconnected, a failed load is logged and skipped.
pub fn spawn(pool: PgPool) -> NotifHub {
    let hub = NotifHub::new();
    let tx = hub.tx.clone();
    tokio::spawn(async move { listen_forever(pool, tx).await });
    hub
}

async fn listen_forever(pool: PgPool, tx: broadcast::Sender<Push>) {
    loop {
        let mut listener = match connect(&pool).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("notifications stream: listen failed, retrying: {e:#}");
                tokio::time::sleep(RETRY_EVERY).await;
                continue;
            }
        };
        // Anything inserted before this connection listened was not announced.
        let _ = tx.send(Push::Resync);
        loop {
            match listener.try_recv().await {
                Ok(Some(n)) => {
                    let Some(id) = parse_id(n.payload()) else {
                        tracing::warn!("notifications stream: ignoring payload {:?}", n.payload());
                        continue;
                    };
                    match load(&pool, id).await {
                        // No viewer open is not an error: the send just has no receiver.
                        Ok(Some(push)) => {
                            let _ = tx.send(push);
                        }
                        // Deleted before we read it: nothing to show.
                        Ok(None) => {}
                        Err(e) => tracing::warn!("notifications stream: loading {id}: {e:#}"),
                    }
                }
                // The connection dropped; the next `try_recv` reconnects and re-listens.
                // What was announced meanwhile is gone, so the pages poll.
                Ok(None) => {
                    tracing::info!("notifications stream: listener reconnecting");
                    let _ = tx.send(Push::Resync);
                }
                Err(e) => {
                    tracing::warn!("notifications stream: receive failed, reconnecting: {e:#}");
                    tokio::time::sleep(RETRY_EVERY).await;
                    break;
                }
            }
        }
    }
}

async fn connect(pool: &PgPool) -> anyhow::Result<PgListener> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(CHANNEL).await?;
    Ok(listener)
}

async fn load(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Push>> {
    let Some(notification) = reminders::get_notification(pool, id).await? else {
        return Ok(None);
    };
    let unread = reminders::unread_count(pool).await?;
    Ok(Some(Push::Notification { notification, unread }))
}

/// The row id a `NOTIFY` payload carries.
fn parse_id(payload: &str) -> Option<Uuid> {
    Uuid::parse_str(payload.trim()).ok()
}

/// The `notification` event's JSON: the row in the unread poll's shape, and the count.
fn notification_data(notification: &Notification, unread: i64) -> Value {
    json!({ "notification": notification, "unread": unread })
}

/// What one fan-out message (or a lag) becomes on the wire.
fn to_event(msg: Result<Push, tokio_stream::wrappers::errors::BroadcastStreamRecvError>) -> Event {
    match msg {
        Ok(Push::Notification { notification, unread }) => Event::default()
            .event("notification")
            .data(notification_data(&notification, unread).to_string()),
        // A viewer that fell behind lost pushes: same answer as a reconnect.
        Ok(Push::Resync) | Err(_) => Event::default().event("resync").data(""),
    }
}

/// GET /api/notifications/stream: one `notification` event per new row
/// (`{ notification, unread }`), `resync` when pushes may have been missed, and the shared
/// `ping` keep-alive.
pub async fn stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = BroadcastStream::new(state.notifications.subscribe()).map(|m| Ok(to_event(m)));
    Sse::new(events).keep_alive(crate::sse::keep_alive())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn row() -> Notification {
        Notification {
            id: Uuid::parse_str("0b5e1f2a-8c3d-4e6f-9a1b-2c3d4e5f6a7b").unwrap(),
            reminder_id: None,
            name: "BTCUSDT crossed 70000".into(),
            kind: "chart".into(),
            linked_id: None,
            details: "Price alert".into(),
            url: "/histviz?chart=1".into(),
            link_label: "Open chart".into(),
            read_at: None,
            created_at: datetime!(2026-11-05 10:00:00 UTC),
        }
    }

    #[test]
    fn parse_id_reads_the_trigger_payload() {
        let id = "0b5e1f2a-8c3d-4e6f-9a1b-2c3d4e5f6a7b";
        assert_eq!(parse_id(id), Some(Uuid::parse_str(id).unwrap()));
        assert_eq!(parse_id(&format!(" {id}\n")), Some(Uuid::parse_str(id).unwrap()));
        assert_eq!(parse_id(""), None);
        assert_eq!(parse_id("not-an-id"), None);
    }

    #[test]
    fn notification_data_has_the_poll_row_and_the_count() {
        let v = notification_data(&row(), 3);
        assert_eq!(v["unread"], 3);
        let n = &v["notification"];
        assert_eq!(n["id"], "0b5e1f2a-8c3d-4e6f-9a1b-2c3d4e5f6a7b");
        assert_eq!(n["kind"], "chart");
        assert_eq!(n["name"], "BTCUSDT crossed 70000");
        assert_eq!(n["details"], "Price alert");
        assert_eq!(n["url"], "/histviz?chart=1");
        assert_eq!(n["link_label"], "Open chart");
        assert_eq!(n["created_at"], "2026-11-05T10:00:00Z");
        assert!(n["read_at"].is_null());
        // Same shape as one entry of GET /api/notifications/unread.
        assert_eq!(*n, serde_json::to_value(row()).unwrap());
    }

    #[tokio::test]
    async fn hub_fans_out_to_every_subscriber() {
        let hub = NotifHub::new();
        let mut a = hub.subscribe();
        let mut b = hub.subscribe();
        hub.tx.send(Push::Notification { notification: row(), unread: 1 }).unwrap();
        for rx in [&mut a, &mut b] {
            match rx.recv().await.unwrap() {
                Push::Notification { notification, unread } => {
                    assert_eq!(notification.kind, "chart");
                    assert_eq!(unread, 1);
                }
                Push::Resync => panic!("expected a notification"),
            }
        }
    }

    #[tokio::test]
    async fn a_lagging_viewer_is_told_to_resync() {
        let (tx, rx) = broadcast::channel(1);
        let mut s = BroadcastStream::new(rx);
        tx.send(Push::Resync).unwrap();
        tx.send(Push::Notification { notification: row(), unread: 2 }).unwrap();
        // The first push was overwritten: the stream reports the lag first.
        let lagged = s.next().await.unwrap();
        assert!(lagged.is_err());
        let _ = to_event(lagged); // a lag maps to `resync`, never a panic
        assert!(matches!(s.next().await.unwrap(), Ok(Push::Notification { .. })));
    }
}
