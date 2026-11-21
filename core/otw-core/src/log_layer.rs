//! A tracing layer that persists log events into `app_logs` so the Settings "Logs"
//! section can fetch and filter them in-app.
//!
//! Events are filtered by the runtime-adjustable minimum level (otw_store::logs), then
//! handed to a background task over an unbounded channel — keeping logging off the hot
//! path and out of the request flow. The task batches inserts and periodically trims to
//! the retention bound.

use std::fmt::Write as _;

use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

struct Record {
    level: String,
    target: String,
    message: String,
}

#[derive(Clone)]
pub struct DbLogLayer {
    tx: mpsc::UnboundedSender<Record>,
}

impl DbLogLayer {
    /// Build the layer and spawn its writer task. Must be called inside a Tokio runtime.
    pub fn new(pool: PgPool) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Record>();
        tokio::spawn(async move {
            let mut since_trim = 0u32;
            while let Some(rec) = rx.recv().await {
                if let Err(e) =
                    otw_store::logs::insert(&pool, &rec.level, &rec.target, &rec.message).await
                {
                    // Don't recurse through the layer; write straight to stderr.
                    eprintln!("app_logs insert failed: {e:#}");
                    continue;
                }
                since_trim += 1;
                if since_trim >= 1000 {
                    since_trim = 0;
                    if let Err(e) = otw_store::logs::trim(&pool).await {
                        eprintln!("app_logs trim failed: {e:#}");
                    }
                }
            }
        });
        Self { tx }
    }
}

/// Collects the `message` field and any other fields into a single string.
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            let _ = write!(self.message, "{}={value:?}", field.name());
        }
    }
}

impl<S: Subscriber> Layer<S> for DbLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let target = event.metadata().target();
        // Never capture sqlx's own query logs: at debug/trace each app_logs insert would
        // emit a query log that we'd capture and re-insert, an unbounded loop.
        if target.starts_with("sqlx") {
            return;
        }
        let level = event.metadata().level().as_str().to_ascii_lowercase();
        if !otw_store::logs::should_capture(&level) {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let _ = self.tx.send(Record {
            level,
            target: target.to_string(),
            message: visitor.message,
        });
    }
}
