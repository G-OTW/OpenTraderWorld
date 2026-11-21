//! One convention for every server-sent event stream the UI keeps open.
//!
//! The keep-alive is a **named `ping` event**, not the default SSE comment: a browser's
//! `EventSource` never surfaces comments, so a page could not tell a quiet stream from a dead
//! one. With a ping it can: any event, ping included, proves the connection is up, and the
//! client watchdog (`frontend/src/lib/live/stream.js`) reconnects once none has arrived for a
//! few intervals. A new stream route gets the same supervision by using [`respond`].
//!
//! Every live route builds its events as [`Frame`]s, not as SSE events: the same stream is
//! served on its own SSE route and as one subscription of the tab's multiplexed socket
//! (`live_ws`), and only the last step, [`respond`] or the socket, picks the wire format.

use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::{Stream, StreamExt};

/// How often an idle stream says it is alive. The client allows three of these to go missing.
pub const PING_EVERY: Duration = Duration::from_secs(15);

/// The keep-alive every SSE route uses.
pub fn keep_alive() -> KeepAlive {
    KeepAlive::new().interval(PING_EVERY).event(Event::default().event("ping").data(""))
}

/// One event of a live stream, whatever carries it: a name and its JSON payload, already
/// serialized (empty for an event that carries nothing).
pub struct Frame {
    pub event: &'static str,
    pub data: String,
}

impl Frame {
    /// An event whose payload is `data`, written as JSON.
    pub fn json<T: serde::Serialize + ?Sized>(event: &'static str, data: &T) -> Self {
        Self { event, data: serde_json::to_string(data).unwrap_or_default() }
    }

    /// An event with no payload.
    pub fn empty(event: &'static str) -> Self {
        Self { event, data: String::new() }
    }

    /// The same event as SSE writes it.
    pub fn into_sse(self) -> Event {
        Event::default().event(self.event).data(self.data)
    }
}

/// A live stream of [`Frame`]s, boxed so a transport can hold any route's.
pub type Frames = Pin<Box<dyn Stream<Item = Frame> + Send>>;

/// A live stream served as its own SSE response, with the shared keep-alive.
pub fn respond(frames: Frames) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(frames.map(|f| Ok(f.into_sse()))).keep_alive(keep_alive())
}
