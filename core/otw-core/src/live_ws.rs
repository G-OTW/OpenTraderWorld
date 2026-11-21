//! `GET /api/live/ws`: every live stream of a browser tab over one WebSocket.
//!
//! Over plain HTTP/1.1 (the `local` and `lan` modes) a browser holds six connections per
//! host across all of its tabs, and each open server-sent event stream is one of them. A
//! chart window alone keeps three (bars, broker events, notifications), so two windows used
//! to take every slot and a form save waited behind them. Here a tab opens one socket and
//! multiplexes its streams over it; the SSE routes stay as they are for scripts and `curl`.
//!
//! The protocol is JSON text frames.
//!
//! Client:
//! - `{"op":"subscribe","id":"<sub id>","path":"/api/histviz/streams?instruments=…"}`: open
//!   the stream an SSE route serves, by that route's path and query string. Same validation,
//!   same grants, same demo rules. Subscribing again with a live id replaces it.
//! - `{"op":"unsubscribe","id":"<sub id>"}`: stop it.
//!
//! Server:
//! - `{"id":…,"event":"open","data":null}`: the route accepted the subscription; its events
//!   follow.
//! - `{"id":…,"event":"<name>","data":<json or null>}`: one event of that stream, exactly
//!   the name and payload its SSE route sends.
//! - `{"id":…,"event":"error","data":{"status":…,"message":…}}`: the route refused
//!   (bad query, not granted, demo slot full, unknown path). The subscription is gone.
//! - `{"id":…,"event":"end","data":null}`: the stream ended on the server side (the SSE
//!   route would have closed its response). The subscription is gone.
//! - `{"op":"ping"}` every [`crate::sse::PING_EVERY`], so the client can tell a quiet
//!   socket from a dead one.
//!
//! `open`, `error` and `end` are reserved: no stream route may name an event so.
//!
//! Each subscription runs in its own task; unsubscribing aborts it and closing the socket
//! aborts them all, which drops the stream and with it the hub's ref-count and the demo's
//! live slot, as the end of an SSE response does. Memory is bounded: frames wait in one
//! bounded queue per socket, and a client that leaves it full for [`STALL`] is too slow to
//! follow and is disconnected (it reconnects and replays from its newest bar).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, Method, Uri},
    response::Response,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;

use crate::sse::{Frame, Frames};
use crate::{ApiError, AppState};

/// Streams one socket may carry. A busy chart window holds four or five; this bounds what a
/// scripted client can park on one connection.
const MAX_SUBS: usize = 32;
/// Frames queued for one socket. Bursts (a workspace's catch-up bars) wait here and in the
/// subscriptions behind it, which simply pause while it is full.
const QUEUE: usize = 512;
/// How long the queue may stay full, or a write to the client may take, before the client
/// is judged too slow and disconnected.
const STALL: Duration = Duration::from_secs(10);
/// Largest frame a client may send: a subscribe carries one URL.
const MAX_IN: usize = 64 * 1024;
/// Longest subscription id: it is echoed on every frame.
const MAX_ID: usize = 64;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/live/ws", get(upgrade))
}

/// The upgrade. It sits behind the session guard like every `/api` route, and checks its
/// origin itself: the same-origin guard lets a `GET` through, but a socket is readable by
/// the page that opened it, wherever that page came from.
async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    crate::security::check_same_origin(&headers)?;
    Ok(ws
        .max_message_size(MAX_IN)
        .on_upgrade(move |socket| serve(state, socket)))
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum ClientMsg {
    Subscribe { id: String, path: String },
    Unsubscribe { id: String },
}

/// Where a subscription's task writes, shared by every task of one socket.
#[derive(Clone)]
struct Out {
    tx: mpsc::Sender<String>,
    /// Raised by a task that waited [`STALL`] for room: the socket closes.
    stalled: Arc<Notify>,
}

impl Out {
    /// Queue a frame; false once the socket is gone or too slow, and the task should stop.
    async fn send(&self, frame: String) -> bool {
        match tokio::time::timeout(STALL, self.tx.send(frame)).await {
            Ok(Ok(())) => true,
            Ok(Err(_)) => false,
            Err(_) => {
                self.stalled.notify_one();
                false
            }
        }
    }
}

async fn serve(state: AppState, socket: WebSocket) {
    // A signed-in tab is open for as long as its socket is: `app` chart alerts watch meanwhile.
    let _tab = state.live_alerts.tab_opened();
    let (mut sink, mut incoming) = socket.split();
    let (tx, mut rx) = mpsc::channel::<String>(QUEUE);
    let out = Out { tx, stalled: Arc::new(Notify::new()) };
    let mut subs: HashMap<String, JoinHandle<()>> = HashMap::new();
    let mut ping = tokio::time::interval(crate::sse::PING_EVERY);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping.tick().await; // the first tick is immediate; the socket has just said hello

    let close = loop {
        let frame = tokio::select! {
            msg = incoming.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    on_message(&state, text.as_str(), &out, &mut subs);
                    continue;
                }
                // Binary frames mean nothing here; pings are answered by the library.
                Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break None,
            },
            Some(frame) = rx.recv() => frame,
            _ = ping.tick() => r#"{"op":"ping"}"#.to_string(),
            _ = out.stalled.notified() => break Some(CloseFrame {
                // 1013: try again later. The client reconnects and replays what it missed.
                code: 1013,
                reason: "too slow to follow these streams".into(),
            }),
        };
        match tokio::time::timeout(STALL, sink.send(Message::Text(frame.into()))).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => break None,
        }
    };
    for (_, task) in subs.drain() {
        task.abort();
    }
    if let Some(frame) = close {
        let _ = tokio::time::timeout(STALL, sink.send(Message::Close(Some(frame)))).await;
    }
}

fn on_message(state: &AppState, text: &str, out: &Out, subs: &mut HashMap<String, JoinHandle<()>>) {
    // A frame that is not the protocol is ignored: nothing to answer it with an id.
    let Ok(msg) = serde_json::from_str::<ClientMsg>(text) else { return };
    match msg {
        ClientMsg::Unsubscribe { id } => {
            if let Some(task) = subs.remove(&id) {
                task.abort();
            }
        }
        ClientMsg::Subscribe { id, path } => {
            if id.len() > MAX_ID {
                return;
            }
            if let Some(task) = subs.remove(&id) {
                task.abort();
            }
            // A refused or ended subscription stays in the map until the client lets go of
            // it, but it holds nothing and does not count.
            subs.retain(|_, task| !task.is_finished());
            let task = if subs.len() >= MAX_SUBS {
                // Answered from a task too: this loop is the one that drains the queue, and
                // must never wait on it.
                let refused = ApiError::too_many(&format!("one connection carries at most {MAX_SUBS} streams"));
                let (frame, out) = (error_frame(&id, &refused), out.clone());
                tokio::spawn(async move {
                    out.send(frame).await;
                })
            } else {
                tokio::spawn(run(state.clone(), id.clone(), path, out.clone()))
            };
            subs.insert(id, task);
        }
    }
}

/// One subscription: open the route's stream, then forward its frames until it ends, the
/// client unsubscribes (the task is aborted) or the socket goes.
async fn run(state: AppState, id: String, path: String, out: Out) {
    let mut frames = match open(&state, &path).await {
        Ok(frames) => frames,
        Err(e) => {
            out.send(error_frame(&id, &e)).await;
            return;
        }
    };
    let id_json = serde_json::to_string(&id).unwrap_or_default();
    if !out.send(control_frame(&id_json, "open")).await {
        return;
    }
    while let Some(frame) = frames.next().await {
        if !out.send(event_frame(&id_json, &frame)).await {
            return;
        }
    }
    out.send(control_frame(&id_json, "end")).await;
}

/// The stream an SSE route serves, by that route's path and query string.
async fn open(state: &AppState, path: &str) -> Result<Frames, ApiError> {
    let uri: Uri = path.parse().map_err(|_| ApiError::bad_request("path must be a stream route with its query"))?;
    let route = uri.path();
    // The demo gate in front of the router only saw the upgrade: each stream is held to the
    // rule of the route it names, as its own request would have been.
    if crate::demo::enabled() && !crate::demo::allowed(&Method::GET, route) {
        return Err(ApiError::forbidden_code("demo_disabled", "this action is disabled in the public demo"));
    }
    match route {
        "/api/histviz/stream" => crate::histdata_api::bars_frames(state, query(&uri)?).await,
        "/api/histviz/streams" => crate::histdata_api::workspace_frames(state, query(&uri)?).await,
        "/api/robots/streams" => crate::histdata_api::robot_workspace_frames(state, query(&uri)?).await,
        "/api/scanner/streams" => crate::histdata_api::scanner_board_frames(state, query(&uri)?).await,
        "/api/brokers/events" => crate::brokers_api::events_frames(state, query(&uri)?).await,
        "/api/notifications/stream" => Ok(crate::notifications_stream::frames(state)),
        "/api/feeds/stream" => Ok(crate::feeds_api::stream_frames(state)),
        "/api/fundamentals/stream" => crate::fundamentals_api::points_frames(state, query(&uri)?),
        other => Err(ApiError::not_found(&format!("{other} is not a live stream"))),
    }
}

/// The route's query, parsed exactly as its `Query` extractor parses it.
fn query<T: DeserializeOwned>(uri: &Uri) -> Result<T, ApiError> {
    Query::<T>::try_from_uri(uri)
        .map(|Query(q)| q)
        .map_err(|e| ApiError::bad_request(&e.body_text()))
}

fn control_frame(id_json: &str, event: &str) -> String {
    format!(r#"{{"id":{id_json},"event":"{event}","data":null}}"#)
}

/// A stream event. Its payload is already JSON (a route never sends anything else), so it
/// is written as is rather than parsed and serialized again.
fn event_frame(id_json: &str, frame: &Frame) -> String {
    let event = serde_json::to_string(frame.event).unwrap_or_default();
    let data = if frame.data.is_empty() { "null" } else { frame.data.as_str() };
    format!(r#"{{"id":{id_json},"event":{event},"data":{data}}}"#)
}

fn error_frame(id: &str, e: &ApiError) -> String {
    json!({
        "id": id,
        "event": "error",
        "data": { "status": e.status_code().as_u16(), "message": e.message() },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_carry_the_route_event_verbatim() {
        let id = serde_json::to_string("a\"b").unwrap();
        let f = Frame::json("bar", &json!({ "c": 1.5, "i": 2 }));
        let v: serde_json::Value = serde_json::from_str(&event_frame(&id, &f)).unwrap();
        assert_eq!(v, json!({ "id": "a\"b", "event": "bar", "data": { "c": 1.5, "i": 2 } }));
        let v: serde_json::Value = serde_json::from_str(&event_frame(&id, &Frame::empty("resync"))).unwrap();
        assert_eq!(v, json!({ "id": "a\"b", "event": "resync", "data": null }));
        let v: serde_json::Value = serde_json::from_str(&control_frame(&id, "end")).unwrap();
        assert_eq!(v, json!({ "id": "a\"b", "event": "end", "data": null }));
    }

    #[test]
    fn a_refusal_names_its_status() {
        let v: serde_json::Value =
            serde_json::from_str(&error_frame("s1", &ApiError::bad_request("ticker is required"))).unwrap();
        assert_eq!(v, json!({ "id": "s1", "event": "error", "data": { "status": 400, "message": "ticker is required" } }));
    }
}
