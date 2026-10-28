//! One convention for every server-sent event stream the UI keeps open.
//!
//! The keep-alive is a **named `ping` event**, not the default SSE comment: a browser's
//! `EventSource` never surfaces comments, so a page could not tell a quiet stream from a dead
//! one. With a ping it can: any event, ping included, proves the connection is up, and the
//! client watchdog (`frontend/src/lib/live/stream.js`) reconnects once none has arrived for a
//! few intervals. A new stream route gets the same supervision by using [`keep_alive`].

use std::time::Duration;

use axum::response::sse::{Event, KeepAlive};

/// How often an idle stream says it is alive. The client allows three of these to go missing.
pub const PING_EVERY: Duration = Duration::from_secs(15);

/// The keep-alive every SSE route uses.
pub fn keep_alive() -> KeepAlive {
    KeepAlive::new().interval(PING_EVERY).event(Event::default().event("ping").data(""))
}
