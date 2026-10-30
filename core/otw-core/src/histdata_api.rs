//! HTTP API for the Historical Data module.
//!
//! Provider connectors themselves live in the data broker (`connectors_api`); this module
//! spends them: queueing downloads, serving stored bars, and the ad-hoc preview the chart
//! uses to look at an instrument before deciding to store it.
//!
//! - `POST /api/histdata/downloads`            queue a download job (connector_id or provider)
//! - `GET  /api/histdata/jobs`                 recent jobs (download-page progress)
//! - `DELETE /api/histdata/jobs/{id}`          cancel one job (running = stop at next chunk)
//! - `DELETE /api/histdata/jobs/batch/{id}`    cancel what is left of a batch
//! - `GET  /api/histdata/datasets`             catalog (management page)
//! - `POST /api/histdata/datasets/{id}/append` gap-fill a dataset (max(ts)→now)
//! - `GET  /api/histdata/datasets/{id}/bars`   OHLCV JSON for the visualization module
//! - `GET  /api/histdata/datasets/{id}/export` CSV download
//! - `GET  /api/histdata/datasets/{id}/check`  integrity check: missing candles, fill cost
//! - `POST /api/histdata/datasets/{id}/check/fill` queue downloads for the holes it found
//! - `DELETE /api/histdata/datasets/{id}`
//! - `POST /api/histdata/preview`              fetch bars live, store nothing
//! - `POST /api/histdata/preview/save`         store what the preview showed (consolidating)
//! - `GET  /api/histdata/symbols`              instrument lookup across granted connectors
//! - `POST /api/histviz/series`                one chart slice: stored bars + gap-filled fetch
//! - `GET  /api/histviz/stream`                SSE live bars for an instrument (writes nothing)

use std::convert::Infallible;
use std::pin::Pin;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, post},
    Json, Router,
};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::align;
use crate::histdata;
use crate::histdata_check::{self, Kind};
use crate::live::{self, hub::LiveTarget};
use crate::{ApiError, AppState};
use otw_store::connectors as conn_store;
use otw_store::histdata as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/histdata/downloads", post(start_download))
        .route("/api/histdata/preview", post(preview))
        .route("/api/histdata/preview/save", post(save_preview))
        .route("/api/histdata/symbols", get(symbols))
        .route("/api/histviz/series", post(series))
        .route("/api/histviz/series/batch", post(series_batch))
        .route("/api/histdata/jobs", get(jobs))
        .route("/api/histdata/jobs/batch/{id}", delete(cancel_batch))
        .route("/api/histdata/jobs/{id}", delete(cancel_job))
        .route("/api/histdata/datasets", get(datasets))
        .route("/api/histdata/datasets/{id}/append", post(append))
        .route("/api/histdata/datasets/{id}/bars", get(bars))
        .route("/api/histdata/datasets/{id}/export", get(export))
        .route("/api/histdata/datasets/{id}/check", get(check_dataset))
        .route("/api/histdata/datasets/{id}/check/fill", post(fill_checked_holes))
        .route("/api/histdata/datasets/{id}", delete(remove_dataset))
        .route(
            "/api/histviz/chart-settings",
            get(get_chart_settings).put(set_chart_settings),
        )
        // Live market data: the SSE stream of forming/closed bars for one instrument.
        // Addressed by coordinates, so watching a symbol never creates anything.
        .route("/api/histviz/stream", get(stream_bars))
        // The workspace form: one connection for every pane on screen.
        .route("/api/histviz/streams", get(stream_workspace))
        .route("/api/backtest/paper/streams", get(stream_paper_workspace))
        .route("/api/scanner/streams", get(stream_scanner_board))
}

// ── Live market data ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct StreamQuery {
    provider: String,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// Which connector of that provider to spend. Absent = the oldest one the chart is
    /// granted.
    connector: Option<Uuid>,
    /// The newest bar the viewer already has (RFC3339). What sits between it and the first
    /// live event is sent as catch-up before the stream starts. Absent = no catch-up.
    since: Option<String>,
    /// Also stream the venue's best bid and ask (`quote` events), when it publishes one.
    /// Off by default: on a busy pair it is the heaviest channel the socket carries.
    #[serde(default)]
    quotes: bool,
}

/// Fastest a viewer is sent quotes: the latest one each period, never a backlog. The eye reads
/// a line moving four times a second as live; a book ticker can fire hundreds of times.
const QUOTE_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// Most bars a catch-up will fetch. Past this the viewer is not slightly behind, it is
/// looking at an old window, and reloading the series is both cheaper and more correct than
/// walking it forward one live bar at a time.
const CATCH_UP_MAX: i64 = 300;

/// GET /api/histviz/stream?provider=&asset_type=&ticker=&timeframe=&connector= — Server-Sent
/// Events for an *instrument*, stored or not.
///
/// Three event kinds: `status` (the feed's condition, always sent first), `snapshot` (the
/// forming bar, if any) and `bar` per update — `closed` tells the client whether to replace
/// the last candle or append a new one. The feed is ref-counted: it starts on the first viewer
/// and tears down shortly after the last leaves.
///
/// `connector` names **which** of the user's connectors for this provider to spend. It is
/// optional and falls back to the oldest one the chart is granted, but never to one it is
/// not: two keys of the same provider are two entitlements and two connection seats.
///
/// Watching writes nothing. If these coordinates already have a dataset, the same feed also
/// records its closed bars into it — saving an instrument is what turns recording on.
async fn stream_bars(
    State(state): State<AppState>,
    Query(q): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let slot = live_slot()?;
    let stream = instrument_stream(&state, q, None, "histviz").await?;
    Ok(Sse::new(Slotted { inner: stream, _slot: slot })
        .keep_alive(crate::sse::keep_alive()))
}

/// A slot in the demo's live-stream budget, taken for the life of the response.
///
/// Outside demo mode this is free and unlimited; in the sandbox it is what keeps a scripted
/// client from parking hundreds of never-ending requests on a two-core host (see
/// `demo::live_slot`).
fn live_slot() -> Result<crate::demo::LiveSlot, ApiError> {
    crate::demo::live_slot().ok_or_else(|| {
        ApiError::too_many("the demo is streaming as many charts as it can right now — try again in a moment")
    })
}

/// An SSE body that releases its [`crate::demo::LiveSlot`] when the client goes away.
/// `EventStream` is a pinned box, so the wrapper is `Unpin` and the projection is a move.
struct Slotted {
    inner: EventStream,
    _slot: crate::demo::LiveSlot,
}

impl Stream for Slotted {
    type Item = Result<Event, Infallible>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(cx)
    }
}

/// GET /api/histviz/streams?instruments=[…] , the same feed for a whole workspace.
///
/// **One connection, N instruments.** A four-pane workspace used to be four SSE requests,
/// which HTTP/1.1 caps at six per host before anything else on the page gets a turn, and four
/// separate subscriptions to the hub. The instruments ride as one URL-encoded JSON array (the
/// browser's `EventSource` only speaks GET), and every event carries `i`, the instrument's
/// index in that array, so the client routes it to the right pane.
///
/// A pane that cannot stream does not fail the request: it gets a `stopped` status event of
/// its own naming the reason, and the other panes stream on.
async fn stream_workspace(
    State(state): State<AppState>,
    Query(q): Query<WorkspaceStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    stream_workspace_for(state, q, "histviz", WORKSPACE_MAX).await
}

/// The paper monitor shares the stream transport, but spends its own module's grants.
async fn stream_paper_workspace(
    State(state): State<AppState>,
    Query(q): Query<WorkspaceStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    stream_workspace_for(state, q, "backtest", WORKSPACE_MAX).await
}

/// The scanner board's live mode: the visible items of a board on one connection, on the
/// scanner's own grants. The hub ref-counts each instrument, so an item a chart pane also
/// shows is still one feed.
async fn stream_scanner_board(
    State(state): State<AppState>,
    Query(q): Query<WorkspaceStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    stream_workspace_for(state, q, "scanner", SCANNER_LIVE_MAX).await
}

async fn stream_workspace_for(
    state: AppState,
    q: WorkspaceStreamQuery,
    module: &'static str,
    max: usize,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    // One slot per connection, not per pane: the workspace form exists precisely so a
    // twelve-pane screen is one socket.
    let slot = live_slot()?;
    let items: Vec<StreamQuery> = serde_json::from_str(&q.instruments)
        .map_err(|e| ApiError::bad_request(&format!("instruments must be a JSON array: {e}")))?;
    if items.is_empty() {
        return Err(ApiError::bad_request("instruments is required"));
    }
    if items.len() > max {
        return Err(ApiError::bad_request(&format!("this stream carries at most {max} instruments")));
    }
    let mut streams: Vec<EventStream> = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        streams.push(match instrument_stream(&state, item, Some(i), module).await {
            Ok(s) => s,
            Err(e) => Box::pin(futures::stream::iter(std::iter::once(Ok(tagged_event(
                "status",
                Some(i),
                json!({ "state": "stopped", "code": "unsupported", "message": e.message() }),
            ))))),
        });
    }
    let merged: EventStream = Box::pin(futures::stream::select_all(streams));
    Ok(Sse::new(Slotted { inner: merged, _slot: slot })
        .keep_alive(crate::sse::keep_alive()))
}

/// Instruments one workspace stream may carry. The grid tops out at 3x4, and a pane holds one
/// instrument, so this is the shape of the screen rather than an arbitrary ceiling.
const WORKSPACE_MAX: usize = 12;
/// Live items one scanner board may hold; above it the board stays on bar close and says so.
const SCANNER_LIVE_MAX: usize = 100;

#[derive(Deserialize)]
struct WorkspaceStreamQuery {
    /// URL-encoded JSON array of the same object the single-instrument route takes as query
    /// parameters. JSON rather than repeated keys because a ticker carries `:`, `/` and `@`
    /// (`SAN:EUR`, `BTC/USD`, `7203@TSEJ:JPY`), and a hand-rolled separator would eventually
    /// meet one of them.
    instruments: String,
}

/// A boxed SSE stream, so the workspace route can merge instruments whose streams were built
/// on different paths (a live subscription, or a single status event saying why not).
type EventStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

/// One instrument's live stream: the condition, the bars that closed while we were
/// connecting, the bar forming now, then the feed. `tag` is the pane index for a workspace
/// stream, absent for the single-instrument route (whose payloads stay byte-identical).
async fn instrument_stream(
    state: &AppState,
    q: StreamQuery,
    tag: Option<usize>,
    module: &str,
) -> Result<EventStream, ApiError> {
    let ticker = q.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    histdata::validate_request(&q.provider, &q.asset_type, &q.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    // Live reach is narrower than download reach, and the refusal says which of the two
    // (the asset type or the timeframe) is the one that does not stream.
    if let Some(why) = live::stream_refusal(&q.provider, &q.asset_type, &q.timeframe) {
        return Err(ApiError::bad_request(&why));
    }
    let connector = pick_stream_connector(state, &q, module).await?;
    let dataset_id = store::find_dataset(&state.pool, &q.provider, &q.asset_type, ticker, &q.timeframe)
        .await?
        .map(|d| d.id);
    let target = LiveTarget {
        dataset_id,
        connector_id: connector.id,
        connector: connector.name,
        provider: q.provider,
        asset_type: q.asset_type,
        ticker: ticker.to_string(),
        timeframe: q.timeframe,
    };
    // The quote is its own subscription on the same socket: one per instrument whatever the
    // timeframes on screen, recording nothing, and a refusal of it never touches the bars.
    let quote_source = q
        .quotes
        .then(|| live::quote_source(&target.provider, &target.asset_type, &target.ticker))
        .flatten();
    let quote_target = quote_source.map(|_| LiveTarget {
        dataset_id: None,
        timeframe: live::QUOTE_TF.to_string(),
        ..target.clone()
    });
    // What closed between the viewer's newest bar and now. The series call and this
    // subscription are two round trips with a WS connect in between, and the minute-bar
    // providers only speak once a minute on top of that, so by the time the first live event
    // arrives the chart is routinely one or more candles behind — and appending the new one
    // next to the old made the hole look like a jump in time rather than a missing bar.
    let caught_up = catch_up(state, &target, q.since.as_deref()).await;

    // Read before the move: the line count below is about this account, and the target is
    // what names it.
    let (provider, connector_id, connector_name) =
        (target.provider.clone(), target.connector_id, target.connector.clone());
    // What the socket publishes for this instrument, carried on every status event: the dot
    // says connected, the grain says what is forming behind it.
    let grain = live::source_grain_label(
        &target.provider,
        &target.asset_type,
        &target.ticker,
        &target.timeframe,
    );

    let live::hub::Subscription { rx, snapshot, status, guard, .. } = state
        .live
        .subscribe(target)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let quotes: Option<EventStream> = match (q.quotes, quote_target) {
        (false, _) => None,
        // Asked for, and the venue streams none: said once, so the client can say why there
        // are no lines rather than wait for them.
        (true, None) => Some(Box::pin(futures::stream::once(async move {
            Ok(tagged_event("quote_status", tag, json!({ "state": "none" })))
        }))),
        (true, Some(t)) => match state.live.subscribe(t) {
            Ok(sub) => Some(quote_stream(sub, tag, quote_source.unwrap_or_default())),
            Err(e) => Some(Box::pin(futures::stream::once(async move {
                Ok(tagged_event(
                    "quote_status",
                    tag,
                    json!({ "state": "stopped", "code": "unsupported", "message": e.to_string() }),
                ))
            }))),
        },
    };

    // Interactive Brokers is the only provider whose live feed is rationed per *account*
    // rather than per connection: every instrument on screen holds one of the account's
    // market-data lines. IB never publishes the ceiling and simply starts refusing once it
    // is reached, so the count travels to the client, which can say so before the pane that
    // crosses it comes back empty.
    let lines = (provider == "ibkr").then(|| {
        tagged_event(
            "lines",
            tag,
            json!({
                "connector": connector_name,
                "used": state.live.lines_held(connector_id),
                "cap": live::ibkr::LINE_CAP,
            }),
        )
    });

    // Keep the ref-count guard alive for the stream's whole life by moving it into the map
    // closure — dropping the stream drops the guard, releasing the feed.
    // Order matters: the condition, then the bars that closed while we were connecting
    // (oldest first), then the bar forming right now.
    let opening = futures::stream::iter(
        std::iter::once(Ok(status_event(tag, &status, grain.as_deref())))
            .chain(lines.into_iter().map(Ok))
            .chain(
                caught_up
                    .into_iter()
                    .map(move |b| Ok(sse_event("bar", tag, &live::hub::bar_to_event(&b)))),
            )
            .chain(
                snapshot
                    .into_iter()
                    .map(move |e| Ok(sse_event("snapshot", tag, &e))),
            ),
    );
    let updates = BroadcastStream::new(rx).filter_map(move |m| {
        let grain = grain.clone();
        async move {
            match m {
                Ok(live::hub::LiveMsg::Bar(e)) => Some(Ok(sse_event("bar", tag, &e))),
                Ok(live::hub::LiveMsg::Status(s)) => {
                    Some(Ok(status_event(tag, &s, grain.as_deref())))
                }
                // A bar subscription carries no quote. A lagging viewer's dropped messages
                // are skipped; it resyncs from the snapshot.
                Ok(live::hub::LiveMsg::Quote(_)) | Err(_) => None,
            }
        }
    });
    let bars = opening.chain(updates).map(move |ev| {
        let _keep = &guard;
        ev
    });
    let Some(quotes) = quotes else { return Ok(Box::pin(bars)) };
    // The bars decide when the stream ends: their channel closing is what tells the viewer to
    // reconnect (a subscription restarted to start or stop recording), and a quote channel
    // still open must not keep a dead pane connected.
    let merged = futures::stream::select(
        bars.map(Some).chain(futures::stream::once(async { None })),
        quotes.map(Some),
    )
    .take_while(|ev| futures::future::ready(ev.is_some()))
    .filter_map(futures::future::ready);
    Ok(Box::pin(merged))
}

/// One instrument's quote subscription as SSE: the latest quote right away, then at most one
/// per [`QUOTE_EVERY`], always the newest (a value held back is sent when the period ends, so
/// the last move is never lost). Its status rides as `quote_status`, with `source` saying
/// where the quote comes from.
fn quote_stream(sub: live::hub::Subscription, tag: Option<usize>, source: &'static str) -> EventStream {
    use tokio::sync::broadcast::error::RecvError;
    use tokio::time::Instant;
    let live::hub::Subscription { rx, quote, status, guard, .. } = sub;
    let quote_event = move |q: &live::LiveQuote| tagged_event("quote", tag, q.to_wire());
    let status_of = move |s: &live::hub::LiveStatus| {
        tagged_event(
            "quote_status",
            tag,
            json!({ "state": s.state, "code": s.code, "message": s.message, "source": source }),
        )
    };
    let opening = futures::stream::iter(
        std::iter::once(Ok(status_of(&status))).chain(quote.iter().map(|q| Ok(quote_event(q)))).collect::<Vec<_>>(),
    );
    // (receiver, quote held back, when the last one went out, guard)
    // As if the last one went out a period ago, so the first quote is not held back.
    let start = (rx, None::<live::LiveQuote>, Instant::now().checked_sub(QUOTE_EVERY).unwrap_or_else(Instant::now), guard);
    let updates = futures::stream::unfold(start, move |(mut rx, mut held, mut sent, guard)| async move {
        loop {
            let due = sent + QUOTE_EVERY;
            let msg = match held {
                Some(_) => tokio::select! {
                    m = rx.recv() => Some(m),
                    _ = tokio::time::sleep_until(due) => None,
                },
                None => Some(rx.recv().await),
            };
            match msg {
                None => {
                    let q = held.take()?;
                    sent = Instant::now();
                    return Some((Ok(quote_event(&q)), (rx, held, sent, guard)));
                }
                Some(Ok(live::hub::LiveMsg::Quote(q))) => {
                    if Instant::now() >= due {
                        held = None;
                        sent = Instant::now();
                        return Some((Ok(quote_event(&q)), (rx, held, sent, guard)));
                    }
                    held = Some(q);
                }
                Some(Ok(live::hub::LiveMsg::Status(s))) => {
                    return Some((Ok(status_of(&s)), (rx, held, sent, guard)));
                }
                // Quotes superseded while this viewer was slow: the next one is the news.
                Some(Ok(live::hub::LiveMsg::Bar(_))) | Some(Err(RecvError::Lagged(_))) => {}
                Some(Err(RecvError::Closed)) => return None,
            }
        }
    });
    Box::pin(opening.chain(updates))
}

/// The connector this live feed spends: the one asked for, or the oldest the chart is
/// granted for that provider.
///
/// A named connector is checked three ways — it exists, it belongs to this provider, and
/// histviz is granted it — because the id arrives from the client and the grant is the whole
/// point of the broker. A provider with no granted connector is an error the user can act on
/// ("grant one"), never a silent fallback onto a key they parked for another module.
async fn pick_stream_connector(
    state: &AppState,
    q: &StreamQuery,
    module: &str,
) -> Result<conn_store::ConnectorRow, ApiError> {
    let viewer = if module == "backtest" { "paper trading" } else { "the chart" };
    if let Some(id) = q.connector {
        let c = conn_store::get(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::not_found("connector not found"))?;
        if c.provider != q.provider {
            return Err(ApiError::bad_request(&format!(
                "connector {} is a {} connector, not {}",
                c.name, c.provider, q.provider
            )));
        }
        if !c.allows(module) {
            return Err(ApiError::bad_request(&format!(
                "connector {} is not granted to {viewer}",
                c.name
            )));
        }
        return Ok(c);
    }
    conn_store::default_for_module(&state.pool, &q.provider, module)
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(&format!(
                "no {} connector is granted to {viewer} — add one, or grant an existing one \
                 to {viewer} in the data broker",
                q.provider
            ))
        })
}

/// The closed bars between what the viewer has and what the live feed will start sending.
///
/// Cheapest source first: the catalog covers it for free when the instrument is stored, and
/// only the tail the store cannot reach costs a provider request. Best-effort throughout —
/// a catch-up that fails must not stop the stream from opening, it only leaves the hole this
/// is trying to close, which is where we were before.
async fn catch_up(
    state: &AppState,
    target: &LiveTarget,
    since: Option<&str>,
) -> Vec<otw_store::histdata::Bar> {
    let Some(since) = since.and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok()) else {
        return Vec::new();
    };
    let Ok(tf) = histdata::timeframe_secs(&target.timeframe) else {
        return Vec::new();
    };
    let step = time::Duration::seconds(tf);
    let now = OffsetDateTime::now_utc();
    let Some((from, to)) = catch_up_window(since, step, now) else {
        return Vec::new();
    };

    let mut rows: Vec<otw_store::histdata::Bar> = Vec::new();
    if let Some(id) = target.dataset_id {
        rows = store::read_bars(&state.pool, id, Some(from), Some(to), CATCH_UP_MAX)
            .await
            .unwrap_or_default();
    }
    // Whatever the store could not answer, asked of the provider once. `newest` is the last
    // bar we hold, so a covered seam spends nothing — and the test is for a *closed* bar
    // being missing (two steps), not merely a period having started, or every subscribe
    // would buy one request for a bar that is still forming and gets filtered out below.
    let newest = rows.last().map(|b| b.ts).unwrap_or(since);
    if to - newest >= step * 2 {
        if let Ok(connector) = histdata::connector_for(&target.provider) {
            let secrets = conn_store::load_creds(&state.pool, &state.cipher, target.connector_id)
                .await
                .unwrap_or_default();
            match connector
                .fetch_chunk(
                    &state.http,
                    &secrets,
                    &target.ticker,
                    &target.asset_type,
                    &target.timeframe,
                    ask_from(newest + step, to, step),
                    to,
                )
                .await
            {
                Ok(chunk) => rows.extend(chunk.bars),
                Err(e) => tracing::warn!("live catch-up {}: {e:#}", target.ticker),
            }
        }
    }
    rows.retain(|b| b.ts >= from && b.ts + step <= to);
    rows.sort_by_key(|b| b.ts);
    rows.dedup_by_key(|b| b.ts);
    rows
}

/// The `[from, to]` a catch-up should ask about, or `None` when there is nothing to catch up.
///
/// `since` is the newest bar the viewer holds and may still be forming, so the window opens
/// at the next period. It is worth asking only once a *whole* period has passed since then:
/// a bar is closed when its own end is behind `now`, which is stated in periods rather than
/// by bucketing `now` so it holds for a provider whose bars are not epoch-aligned. Sending a
/// forming bar as a closed one would make the chart finalize a candle that is still moving —
/// the snapshot that follows is what owns that bar.
/// The fewest bars a provider request asks for.
///
/// A top-up after the last stored bar is a candle or two, and a window that short is a poor
/// request whoever serves it: Interactive Brokers refuses it outright (error 321, the
/// duration is invalid for the bar size) and a paging provider can answer a one-bar window
/// with nothing at all. Both leave the same hole at the right edge of the chart.
const MIN_ASK_BARS: i32 = 30;

/// Where to start asking for `(from, to)`: the window itself, widened backwards when it is
/// too short to be worth a request.
///
/// Widening is free. The extra bars are ones the store already holds, they come back to an
/// upsert or to a caller that filters and dedups against the window it actually asked for,
/// and it is one request either way.
fn ask_from(
    from: OffsetDateTime,
    to: OffsetDateTime,
    step: time::Duration,
) -> OffsetDateTime {
    (to - step * MIN_ASK_BARS).min(from)
}

fn catch_up_window(
    since: OffsetDateTime,
    step: time::Duration,
    now: OffsetDateTime,
) -> Option<(OffsetDateTime, OffsetDateTime)> {
    let from = since + step;
    // Nothing has closed since the viewer's last bar.
    if from + step > now {
        return None;
    }
    // Too far behind to walk forward one bar at a time: this is a stale window, not a seam,
    // and reloading the series is both cheaper and more correct.
    if (now - from).whole_seconds() / step.whole_seconds().max(1) > CATCH_UP_MAX {
        return None;
    }
    Some((from, now))
}

fn sse_event(kind: &str, tag: Option<usize>, e: &live::LiveEvent) -> Event {
    tagged_event(kind, tag, e.to_wire())
}

/// The feed's condition, as its own event kind. A `stopped` status is the end of the line —
/// the supervisor will not retry — so the client shows it and stops waiting for bars.
/// `grain` is what the socket publishes underneath (see [`live::source_grain_label`]).
fn status_event(tag: Option<usize>, s: &live::hub::LiveStatus, grain: Option<&str>) -> Event {
    tagged_event(
        "status",
        tag,
        json!({ "state": s.state, "code": s.code, "message": s.message, "grain": grain }),
    )
}

/// One SSE event, carrying the pane index when the stream serves a whole workspace. The
/// single-instrument route passes `None` and its payloads stay exactly what they were.
fn tagged_event(kind: &str, tag: Option<usize>, mut payload: Value) -> Event {
    if let (Some(i), Some(obj)) = (tag, payload.as_object_mut()) {
        obj.insert("i".into(), json!(i));
    }
    Event::default()
        .event(kind)
        .data(serde_json::to_string(&payload).unwrap_or_default())
}

// ── Visualization chart settings (single global JSON blob) ──────────────────────

const CHART_SETTINGS_KEY: &str = "histviz.chart_settings";

/// Saved chart settings, or `null` if never customized (client applies its defaults).
async fn get_chart_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let raw = otw_store::settings::get(&state.pool, CHART_SETTINGS_KEY)
        .await
        .map_err(ApiError::from)?;
    let out = match raw {
        Some(s) if !s.trim().is_empty() => serde_json::from_str::<Value>(&s).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    Ok(Json(out))
}

/// Replace the saved chart settings with the posted JSON object.
async fn set_chart_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let serialized = serde_json::to_string(&body)
        .map_err(|e| ApiError::bad_request(&format!("invalid settings: {e}")))?;
    otw_store::settings::set(&state.pool, CHART_SETTINGS_KEY, &serialized)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(body))
}

/// A library indicator was deleted: take its studies off the saved chart templates.
pub(crate) async fn forget_indicator_in_templates(pool: &sqlx::PgPool, id: Uuid) -> anyhow::Result<()> {
    let Some(raw) = otw_store::settings::get(pool, CHART_SETTINGS_KEY).await? else { return Ok(()) };
    let Ok(mut settings) = serde_json::from_str::<Value>(&raw) else { return Ok(()) };
    let id = id.to_string();
    let mut hit = false;
    if let Some(list) = settings.get_mut("chartTemplates").and_then(Value::as_array_mut) {
        for tpl in list {
            if let Some(studies) = tpl.get_mut("instances") {
                hit |= otw_store::histviz::drop_indicator_studies(studies, &id);
            }
        }
    }
    if hit {
        otw_store::settings::set(pool, CHART_SETTINGS_KEY, &serde_json::to_string(&settings)?).await?;
    }
    Ok(())
}

// ── Downloads ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DownloadBody {
    /// The connector to download through (preferred). When absent, `provider` must be
    /// set and the provider's default connector is used (MCP / legacy clients).
    connector_id: Option<Uuid>,
    /// Provider id, e.g. "binance" — see /api/connectors/providers.
    provider: Option<String>,
    /// One of the provider's asset_types, e.g. "crypto" — see /api/connectors/providers.
    asset_type: String,
    /// Symbol in the provider's format, e.g. "BTCUSDT" for binance.
    ticker: String,
    /// One of the provider's timeframes, e.g. "1h".
    timeframe: String,
    /// RFC3339 inclusive start, e.g. "2024-07-19T00:00:00Z".
    from: String,
    /// RFC3339 exclusive end.
    to: String,
    /// Extra symbols to queue alongside `ticker` (batch download).
    #[serde(default)]
    tickers: Vec<String>,
    /// Extra timeframes to queue alongside `timeframe` (batch download).
    #[serde(default)]
    timeframes: Vec<String>,
    /// Queue nothing: answer with the `estimate` block only. Lets the form show what a
    /// batch will cost in provider requests before the user commits to it.
    #[serde(default)]
    estimate_only: bool,
}

/// Ceiling on one batch, so a paste of a few hundred symbols can't flood the queue.
const MAX_BATCH_JOBS: usize = 100;

/// `first` plus `rest`, trimmed, empties dropped, order kept, duplicates removed.
fn merge_list(first: &str, rest: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in std::iter::once(first).chain(rest.iter().map(|s| s.as_str())) {
        let v = v.trim();
        if !v.is_empty() && !out.iter().any(|k| k == v) {
            out.push(v.to_string());
        }
    }
    out
}

fn parse_rfc3339(s: &str, field: &str) -> Result<OffsetDateTime, ApiError> {
    OffsetDateTime::parse(s.trim(), &Rfc3339)
        .map_err(|_| ApiError::bad_request(&format!("invalid {field} (need RFC3339)")))
}

async fn start_download(
    State(state): State<AppState>,
    Json(b): Json<DownloadBody>,
) -> Result<Json<Value>, ApiError> {
    // One request can queue several instruments: every ticker × every timeframe, all on the
    // same connector and the same window. A single pair behaves exactly as before.
    let tickers = merge_list(&b.ticker, &b.tickers);
    if tickers.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    let timeframes = merge_list(&b.timeframe, &b.timeframes);
    if timeframes.is_empty() {
        return Err(ApiError::bad_request("timeframe is required"));
    }
    if tickers.len() * timeframes.len() > MAX_BATCH_JOBS {
        return Err(ApiError::bad_request(&format!(
            "too many downloads at once ({} tickers × {} timeframes, max {MAX_BATCH_JOBS})",
            tickers.len(),
            timeframes.len()
        )));
    }
    // Resolve the connector: explicit id wins; a bare provider (MCP / legacy clients)
    // maps to that provider's default connector when one exists.
    let (connector_id, provider) = match (b.connector_id, b.provider.as_deref()) {
        (Some(id), _) => {
            let c = conn_store::get(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("connector not found"))?;
            (Some(id), c.provider)
        }
        (None, Some(p)) => {
            let c = conn_store::default_for(&state.pool, p).await?;
            (c.map(|c| c.id), p.to_string())
        }
        (None, None) => {
            return Err(ApiError::bad_request("connector_id or provider is required"));
        }
    };
    // Enforce the capability matrix server-side (the UI greys these out, but never trust it).
    // One unsupported timeframe in a batch is reported and skipped, not fatal; if none is
    // supported the request fails as a single download always did.
    let mut errors: Vec<Value> = Vec::new();
    let mut usable: Vec<&str> = Vec::new();
    for tf in &timeframes {
        match histdata::validate_request(&provider, &b.asset_type, tf) {
            Ok(_) => usable.push(tf.as_str()),
            Err(e) => errors.push(json!({ "timeframe": tf, "error": e.to_string() })),
        }
    }
    if usable.is_empty() {
        let msg = errors
            .first()
            .and_then(|e| e["error"].as_str().map(str::to_string))
            .unwrap_or_else(|| "unsupported timeframe".into());
        return Err(ApiError::bad_request(&msg));
    }
    let from = parse_rfc3339(&b.from, "from")?;
    let to = parse_rfc3339(&b.to, "to")?;
    if from >= to {
        return Err(ApiError::bad_request("'from' must be before 'to'"));
    }

    // What this will cost the provider, before anything is written. The same block rides
    // back with a real queueing, so the page can say why a batch is about to wait.
    let estimate = estimate_cost(&state, connector_id, &provider, &tickers, &usable, from, to).await;
    if b.estimate_only {
        return Ok(Json(json!({ "estimate": estimate, "errors": errors })));
    }

    // A batch is one submission: its jobs share a `batch_id` so the page can summarize them
    // and cancel what is left in one click. A lone download keeps no batch.
    let batch_id = (tickers.len() * usable.len() > 1).then(Uuid::new_v4);
    let mut queued: Vec<Value> = Vec::new();
    for ticker in &tickers {
        for timeframe in &usable {
            let dataset_id =
                store::upsert_dataset(&state.pool, &provider, &b.asset_type, ticker, timeframe)
                    .await?;
            let job_id = store::enqueue_job(
                &state.pool,
                &store::NewJob {
                    dataset_id,
                    connector_id,
                    provider: &provider,
                    asset_type: &b.asset_type,
                    ticker,
                    timeframe,
                    range_from: from,
                    range_to: to,
                    kind: "download",
                    batch_id,
                },
            )
            .await?;
            queued.push(json!({
                "ticker": ticker,
                "timeframe": timeframe,
                "job_id": job_id,
                "dataset_id": dataset_id,
            }));
        }
    }
    // `job_id`/`dataset_id` stay at the top level (first pair queued) so single-download
    // callers, MCP agents included, read the same shape they always have.
    Ok(Json(json!({
        "job_id": queued[0]["job_id"],
        "dataset_id": queued[0]["dataset_id"],
        "batch_id": batch_id,
        "jobs": queued,
        "errors": errors,
        "estimate": estimate,
    })))
}

/// How many provider requests a (batch) download needs, against the connector's declared
/// quota. Chunk count per pair mirrors the worker's own paging: one request per
/// `max_bars_per_req` bars of the window. Everything here is advisory — the quota is
/// observe-only at queue time, the worker is what actually waits for it.
async fn estimate_cost(
    state: &AppState,
    connector_id: Option<Uuid>,
    provider: &str,
    tickers: &[String],
    timeframes: &[&str],
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Value {
    let cap = match histdata::connector_for(provider) {
        Ok(c) => c.capability(),
        Err(_) => return Value::Null,
    };
    let span = (to - from).whole_seconds().max(0);
    let mut requests: i64 = 0;
    for tf in timeframes {
        let Ok(secs) = histdata::timeframe_secs(tf) else { continue };
        let step = (secs * cap.max_bars_per_req.max(1) as i64).max(1);
        // Ceiling division: a window shorter than one page is still one request.
        let chunks = ((span + step - 1) / step).max(1);
        requests += tickers.len() as i64 * chunks;
    }
    // Wall-clock floor: the worker paces requests to stay under the published limit.
    let seconds = requests * cap.min_interval_ms as i64 / 1000;
    let (used, max, period, resets_at) = quota_now(state, connector_id).await;
    json!({
        "jobs": tickers.len() * timeframes.len(),
        "requests": requests,
        "seconds": seconds,
        "quota_used": used,
        "quota_max": max,
        "quota_period": period,
        "quota_resets_at": resets_at,
        // True when the batch cannot fit in what is left of the window: the worker will
        // download what it can, then park until the quota rolls over.
        "over_quota": max.is_some_and(|m| used + requests > m),
    })
}

/// The connector's declared quota as it stands: (used, max, period, resets_at). Zero and no
/// cap when the connector declares none.
async fn quota_now(state: &AppState, connector_id: Option<Uuid>) -> (i64, Option<i64>, String, String) {
    let quota = match connector_id {
        Some(id) => otw_store::api_quota::get(&state.pool, &crate::connectors_api::quota_scope(id))
            .await
            .ok()
            .flatten(),
        None => None,
    };
    match quota {
        Some(q) => (q.used, q.max_requests, q.period, q.resets_at.format(&Rfc3339).unwrap_or_default()),
        None => (0, None, String::new(), String::new()),
    }
}

async fn jobs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let jobs = store::list_jobs(&state.pool, 50).await?;
    Ok(Json(json!({ "jobs": jobs })))
}

/// Stop one job. Queued or parked, it ends immediately; running, it is asked to stop and
/// the worker closes it at the next chunk boundary, keeping the bars already written.
async fn cancel_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let status = store::cancel_job(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::bad_request("job is already finished"))?;
    Ok(Json(json!({ "id": id, "status": status })))
}

/// Stop every job of a batch that has not finished yet.
async fn cancel_batch(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let cancelled = store::cancel_batch(&state.pool, id).await?;
    Ok(Json(json!({ "batch_id": id, "cancelled": cancelled })))
}

// ── Preview (look now, store later) ──────────────────────────────────────────────
//
// The chart can open any instrument a connector serves, not only what was downloaded:
// the preview fetches bars through the connector and returns them without writing a
// single row. Saving is a separate, explicit click — it queues an ordinary download job
// over the same window, so the bars land in the normal catalog and consolidate with
// whatever was already stored (`write_bars` upserts on (dataset, ts)).

/// Trailing bars a preview pulls when the client doesn't say, and the ceiling it may ask.
const PREVIEW_DEFAULT_BARS: i64 = 500;
const PREVIEW_MAX_BARS: i64 = 5_000;
/// Cap on provider round-trips for one preview: a connector that pages 200 bars at a time
/// must not turn one click into an unbounded burst against the user's API plan.
const PREVIEW_MAX_CHUNKS: usize = 12;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PreviewBody {
    /// Connector to fetch through (preferred); must be granted to the chart module.
    connector_id: Option<Uuid>,
    /// Provider id, when no connector is named (uses that provider's default connector).
    provider: Option<String>,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// Trailing bars to fetch (default 500, capped 5000).
    bars: Option<i64>,
}

/// Resolve `connector_id` / `provider` to a (connector, provider) pair, checking that the
/// connector is granted to `module`. Mirrors the download resolution, plus the grant.
async fn resolve_source(
    state: &AppState,
    connector_id: Option<Uuid>,
    provider: Option<&str>,
    module: &str,
) -> Result<(Option<Uuid>, String), ApiError> {
    match (connector_id, provider) {
        (Some(id), _) => {
            let c = conn_store::get(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("connector not found"))?;
            if !c.allows(module) {
                return Err(ApiError::bad_request(&format!(
                    "connector '{}' is not granted to {module}",
                    c.name
                )));
            }
            Ok((Some(id), c.provider))
        }
        (None, Some(p)) => {
            let c = conn_store::default_for(&state.pool, p).await?;
            Ok((c.map(|c| c.id), p.to_string()))
        }
        (None, None) => Err(ApiError::bad_request("connector_id or provider is required")),
    }
}

/// POST /api/histdata/preview — fetch the trailing window straight from the provider and
/// return it. Nothing is written; `stored` reports whether a dataset for these coordinates
/// already exists, so the UI can offer "save" vs "update".
async fn preview(
    State(state): State<AppState>,
    Json(b): Json<PreviewBody>,
) -> Result<Json<Value>, ApiError> {
    let ticker = b.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    let (connector_id, provider) =
        resolve_source(&state, b.connector_id, b.provider.as_deref(), "histviz").await?;
    let cap = histdata::validate_request(&provider, &b.asset_type, &b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let tf_secs = histdata::timeframe_secs(&b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let want = b.bars.unwrap_or(PREVIEW_DEFAULT_BARS).clamp(1, PREVIEW_MAX_BARS);

    let to = OffsetDateTime::now_utc();
    let from = to - time::Duration::seconds(tf_secs * want);
    let secrets = match connector_id {
        Some(id) => conn_store::load_creds(&state.pool, &state.cipher, id).await?,
        None => Default::default(),
    };
    let connector = histdata::connector_for(&provider)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let step = tf_secs * cap.max_bars_per_req.max(1) as i64;

    let mut rows: Vec<otw_store::histdata::Bar> = Vec::new();
    let mut cursor = from;
    let mut chunks = 0usize;
    while cursor < to && chunks < PREVIEW_MAX_CHUNKS {
        let chunk_to = (cursor + time::Duration::seconds(step)).min(to);
        if let Some(id) = connector_id {
            // Display-only counter; a failed write must not block the fetch.
            let _ = otw_store::api_quota::bump(&state.pool, &crate::connectors_api::quota_scope(id))
                .await;
        }
        let chunk = connector
            .fetch_chunk(
                &state.http,
                &secrets,
                ticker,
                &b.asset_type,
                &b.timeframe,
                cursor,
                chunk_to,
            )
            .await
            .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
        rows.extend(chunk.bars);
        cursor = chunk_to;
        chunks += 1;
    }
    // Providers may overlap pages or ignore the window; normalize to a clean series.
    rows.sort_by_key(|b| b.ts);
    rows.dedup_by_key(|b| b.ts);
    if rows.len() as i64 > want {
        rows.drain(..rows.len() - want as usize);
    }
    if rows.is_empty() {
        return Err(ApiError::bad_request(&format!(
            "{provider} returned no bars for {ticker} ({} {})",
            b.asset_type, b.timeframe
        )));
    }

    let stored = store::find_dataset(&state.pool, &provider, &b.asset_type, ticker, &b.timeframe)
        .await?
        .map(|d| json!({ "id": d.id, "bar_count": d.bar_count }));
    let mut out = bars_payload(&rows);
    out["provider"] = json!(provider);
    out["asset_type"] = json!(b.asset_type);
    out["ticker"] = json!(ticker);
    out["timeframe"] = json!(b.timeframe);
    out["stream"] = json!(live::stream_capable_for(&provider, &b.asset_type, &b.timeframe));
    out["stream_timeframes"] = json!(live::stream_timeframes(&provider));
    out["stream_note"] = json!(live::stream_note(&provider));
    out["stored"] = stored.unwrap_or(Value::Null);
    Ok(Json(out))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SavePreviewBody {
    connector_id: Option<Uuid>,
    provider: Option<String>,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// Trailing bars to persist — the same window the preview showed. Ignored when an
    /// explicit `from`/`to` pair is given.
    bars: Option<i64>,
    /// Explicit window start (RFC3339) — what the chart has actually loaded.
    from: Option<String>,
    /// Explicit window end (RFC3339). Defaults to now when only `from` is given.
    to: Option<String>,
}

/// POST /api/histdata/preview/save — persist what the chart is showing by queueing the
/// normal download job for that window. Re-downloading server-side (rather than trusting
/// the bars the browser holds) keeps a single write path and consolidates with existing
/// rows through the same upsert every download uses. Either give an explicit `from`/`to`
/// (the loaded window) or a trailing `bars` count.
async fn save_preview(
    State(state): State<AppState>,
    Json(b): Json<SavePreviewBody>,
) -> Result<Json<Value>, ApiError> {
    let ticker = b.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    let (connector_id, provider) =
        resolve_source(&state, b.connector_id, b.provider.as_deref(), "histviz").await?;
    histdata::validate_request(&provider, &b.asset_type, &b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let tf_secs = histdata::timeframe_secs(&b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let to = match b.to.as_deref() {
        Some(s) => parse_rfc3339(s, "to")?,
        None => OffsetDateTime::now_utc(),
    };
    let from = match b.from.as_deref() {
        Some(s) => parse_rfc3339(s, "from")?,
        None => {
            let want = b.bars.unwrap_or(PREVIEW_DEFAULT_BARS).clamp(1, PREVIEW_MAX_BARS);
            to - time::Duration::seconds(tf_secs * want)
        }
    };
    if from >= to {
        return Err(ApiError::bad_request("'from' must be before 'to'"));
    }

    let dataset_id =
        store::upsert_dataset(&state.pool, &provider, &b.asset_type, ticker, &b.timeframe).await?;
    let job_id = store::enqueue_job(
        &state.pool,
        &store::NewJob {
            dataset_id,
            connector_id,
            provider: &provider,
            asset_type: &b.asset_type,
            ticker,
            timeframe: &b.timeframe,
            range_from: from,
            range_to: to,
            kind: "download",
            batch_id: None,
        },
    )
    .await?;
    Ok(Json(json!({ "dataset_id": dataset_id, "job_id": job_id })))
}

// ── Symbol lookup ────────────────────────────────────────────────────────────────
//
// The chart opens any instrument a connector serves, so the user must be able to *find*
// it: one search box over every connector granted to the chart, or over the subset they
// ticked. Providers without a search endpoint are reported in `notes` rather than
// silently dropped — their symbols are typed by hand.

/// Hits returned per connector, and across all of them.
const SYMBOLS_PER_CONNECTOR: usize = 12;
const SYMBOLS_TOTAL: usize = 60;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SymbolsQuery {
    /// Free text: symbol fragment or name.
    q: Option<String>,
    /// Restrict to one asset type (must be one the connector supports).
    asset_type: Option<String>,
    /// Comma-separated connector ids (the whitelist). Absent = every granted connector.
    connectors: Option<String>,
    limit: Option<usize>,
    /// Whose grants the lookup spends: `histviz` (default) or `scanner`.
    module: Option<String>,
}

/// GET /api/histdata/symbols — instrument lookup across the connectors the chart may use.
async fn symbols(
    State(state): State<AppState>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Query(q): Query<SymbolsQuery>,
) -> Result<Json<Value>, ApiError> {
    demo_series_allowed(ip)?;
    let needle = q.q.unwrap_or_default();
    let asset_type = q.asset_type.unwrap_or_default();
    let per = q.limit.unwrap_or(SYMBOLS_PER_CONNECTOR).clamp(1, 50);

    let wanted: Vec<Uuid> = q
        .connectors
        .as_deref()
        .map(|s| s.split(',').filter_map(|p| Uuid::parse_str(p.trim()).ok()).collect())
        .unwrap_or_default();
    let module = match q.module.as_deref() {
        None | Some("histviz") => "histviz",
        Some("scanner") => "scanner",
        Some(other) => return Err(ApiError::bad_request(&format!("symbol lookup is not offered to {other}"))),
    };
    let rows = conn_store::list_for_module(&state.pool, module).await?;

    // Mark hits already in the catalog so the UI can say "you have this one".
    let stored: std::collections::HashSet<(String, String)> =
        store::list_datasets(&state.pool)
            .await?
            .into_iter()
            .map(|d| (d.provider, d.ticker.to_ascii_uppercase()))
            .collect();

    let mut results: Vec<Value> = Vec::new();
    let mut notes: Vec<Value> = Vec::new();
    for c in rows {
        if !wanted.is_empty() && !wanted.contains(&c.id) {
            continue;
        }
        let Ok(connector) = histdata::connector_for(&c.provider) else {
            continue;
        };
        let cap = connector.capability();
        // Asked for an asset type this provider doesn't serve: skip it silently, the
        // connector simply has nothing to say about e.g. options.
        if !asset_type.is_empty() && !cap.asset_types.contains(&asset_type.as_str()) {
            continue;
        }
        if !cap.searchable {
            notes.push(json!({
                "connector_id": c.id,
                "connector": c.name,
                "provider": c.provider,
                "code": "unsupported",
                "message": format!("{} has no symbol search — type the symbol directly", cap.label),
            }));
            continue;
        }
        let secrets = conn_store::load_creds(&state.pool, &state.cipher, c.id).await?;
        if let Some(missing) = missing_secret(cap, &secrets) {
            notes.push(json!({
                "connector_id": c.id,
                "connector": c.name,
                "provider": c.provider,
                "code": "auth",
                "message": format!("{} needs its '{missing}' credential", cap.label),
            }));
            continue;
        }
        match connector
            .search_symbols(&state.http, &secrets, &needle, &asset_type, per)
            .await
        {
            Ok(hits) => {
                for h in hits {
                    if results.len() >= SYMBOLS_TOTAL {
                        break;
                    }
                    results.push(json!({
                        "connector_id": c.id,
                        "connector": c.name,
                        "provider": c.provider,
                        "label": cap.label,
                        "symbol": h.symbol,
                        "name": h.name,
                        "asset_type": h.asset_type,
                        "exchange": h.exchange,
                        "timeframes": cap.timeframes,
                        // Live reach is per asset type: a hit carries no timeframe yet, so
                        // the client pairs this with `stream_timeframes` to grey out the
                        // chips that cannot go live.
                        "stream": live::streams_asset(&c.provider, &h.asset_type),
                        "stream_timeframes": live::stream_timeframes(&c.provider),
                        "stream_note": live::stream_note(&c.provider),
                        "stored": stored.contains(&(c.provider.clone(), h.symbol.to_ascii_uppercase())),
                    }));
                }
            }
            Err(e) => notes.push(json!({
                "connector_id": c.id,
                "connector": c.name,
                "provider": c.provider,
                "code": notice_code(&format!("{e:#}")),
                "message": short_reason(&format!("{e:#}")),
            })),
        }
    }
    Ok(Json(json!({ "results": results, "notes": notes })))
}

/// The first required credential a connector is missing, if any.
fn missing_secret<'a>(
    cap: &'a histdata::Capability,
    secrets: &std::collections::HashMap<String, String>,
) -> Option<&'a str> {
    cap.required_secrets
        .iter()
        .copied()
        .find(|n| secrets.get(*n).map(|v| v.trim().is_empty()).unwrap_or(true))
}

/// Trim a provider error down to what a user can act on: the error chain's own words,
/// without the reqwest URL tail that would otherwise leak an API key into the UI.
fn short_reason(msg: &str) -> String {
    let cut = msg.find(" for url (").unwrap_or(msg.len());
    let s = msg[..cut].trim().trim_end_matches(':').trim();
    if s.len() > 240 {
        format!("{}…", &s[..240])
    } else {
        s.to_string()
    }
}

/// Sort a provider/transport failure into a code the UI can phrase for the user. The
/// message itself is always carried through — this only picks the icon and the tone.
pub(crate) fn notice_code(msg: &str) -> &'static str {
    let m = msg.to_ascii_lowercase();
    if m.contains("429")
        || m.contains("too many requests")
        || m.contains("rate limit")
        || m.contains("ratelimit")
        || m.contains("exceeded")
    {
        "rate_limit"
    } else if m.contains("api key")
        || m.contains("apikey")
        || m.contains("api_key")
        || m.contains("credential")
        || m.contains("unauthorized")
        || m.contains("not_authorized")
        || m.contains("401")
        || m.contains("403")
    {
        "auth"
    } else if m.contains("bad symbol")
        || m.contains("bad pair")
        || m.contains("bad product")
        || m.contains("no data")
        || m.contains("no bars")
        || m.contains("no series")
        || m.contains("404")
    {
        "symbol"
    } else if m.contains("days") || m.contains("history") || m.contains("range") {
        "depth"
    } else {
        "provider"
    }
}

// ── Chart series (stored + provider, gap-filled) ─────────────────────────────────
//
// The chart is not bound to a dataset: it asks for a *window* of an instrument and this
// endpoint answers with whatever it can — bars already in the catalog, plus the missing
// edges fetched through the connector. Nothing is written. One call = one slice; the
// client's "load more" button walks backwards a slice at a time, which is why no fetch
// ever happens without a user gesture.

/// One "load more" click, and the ceiling a client may ask for in a single slice.
const SLICE_BARS: i64 = 1_500;
const SLICE_MAX_BARS: i64 = 5_000;
/// Provider round-trips one slice may spend (a 300-bar pager still fits 1500 bars).
const SLICE_MAX_CHUNKS: usize = 12;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SeriesBody {
    /// Connector to read through (preferred); must be granted to the chart module.
    connector_id: Option<Uuid>,
    /// Provider id, when no connector is named (uses that provider's default connector).
    provider: Option<String>,
    asset_type: String,
    ticker: String,
    timeframe: String,
    /// Exclusive upper bound (RFC3339). Absent = now, i.e. the newest slice.
    to: Option<String>,
    /// Bars in this slice (default 1500, max 5000).
    bars: Option<i64>,
    /// Read stored bars only — never call the provider (no quota spent).
    #[serde(default)]
    stored_only: bool,
}

/// One workspace's worth of instruments. Capped because a batch is one HTTP request but N
/// provider conversations: past this it is a download, not a chart refresh.
const BATCH_MAX: usize = 12;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SeriesBatchBody {
    items: Vec<SeriesBody>,
    /// Answer with one shared clock over the batch, so instruments drawn together sit on
    /// the same rows. Off by default: a workspace of unrelated panes has nothing to align.
    #[serde(default)]
    align: bool,
    /// The caller's own bars, when the clock has to be *theirs* rather than the batch's own
    /// union (a comparison overlay is drawn on the price pane's rows, not on a new set).
    clock: Option<ClockRef>,
}

/// A reference clock the caller already holds. The provider is part of it because refusing
/// an intraday cross-provider alignment needs to know whose stamps these are.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ClockRef {
    provider: Option<String>,
    ts: Vec<String>,
}

/// Chart reads that may reach a provider, in the public sandbox.
///
/// The series routes are the demo's only outbound market-data path (`/api/histdata` is
/// closed wholesale), and they are cheap enough to call in a loop: without a budget a
/// scripted visitor would walk years of history through the demo's IP until the provider
/// blocks it for everyone. Generous enough that scrolling back through a chart, which is
/// one call per slice, never notices it.
static DEMO_SERIES_QUOTA: crate::demo::WindowQuota =
    crate::demo::WindowQuota::new(600, 120, std::time::Duration::from_secs(600));

fn demo_series_allowed(
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
) -> Result<(), ApiError> {
    if crate::demo::enabled() {
        // Requests that bypassed the gate (in-process MCP dispatch) share one bucket.
        let ip = ip.map(|e| e.0 .0).unwrap_or_else(|| "internal".to_string());
        if !DEMO_SERIES_QUOTA.allow(&ip) {
            return Err(ApiError::too_many(
                "the shared demo market-data budget is used up for now — try again in a few minutes",
            ));
        }
    }
    Ok(())
}

/// POST /api/histviz/series — a window of an instrument, stored bars first, provider only
/// for what is missing.
async fn series(
    State(state): State<AppState>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Json(b): Json<SeriesBody>,
) -> Result<Json<Value>, ApiError> {
    demo_series_allowed(ip)?;
    Ok(Json(series_window(&state, &b).await?))
}

/// POST /api/histviz/series/batch , one window per instrument in a single round trip.
///
/// What a workspace opens with: four panes used to be four requests plus four connections,
/// and the client had no way to know what the set would cost before firing them. The
/// instruments are read **in order, one at a time**, on purpose: they routinely share a
/// connector, and a connector's quota and its rate limiter are both counted per account, so
/// firing them in parallel only trades a few milliseconds for a burst refusal.
///
/// A failure is per instrument, never for the batch: one pane pointing at a symbol its
/// provider does not know must not blank the other three, so its slot carries `error` and
/// the rest carry their bars.
async fn series_batch(
    State(state): State<AppState>,
    ip: Option<axum::Extension<crate::demo::ClientIp>>,
    Json(b): Json<SeriesBatchBody>,
) -> Result<Json<Value>, ApiError> {
    demo_series_allowed(ip)?;
    if b.items.is_empty() {
        return Err(ApiError::bad_request("items is required"));
    }
    if b.items.len() > BATCH_MAX {
        return Err(ApiError::bad_request(&format!(
            "a batch reads at most {BATCH_MAX} instruments"
        )));
    }
    if let Some(c) = &b.clock {
        if c.ts.len() as i64 > SLICE_MAX_BARS {
            return Err(ApiError::bad_request(&format!(
                "a reference clock holds at most {SLICE_MAX_BARS} bars"
            )));
        }
    }
    let mut out = Vec::with_capacity(b.items.len());
    for item in &b.items {
        out.push(match series_window(&state, item).await {
            Ok(v) => v,
            Err(e) => json!({
                "error": e.message(),
                "asset_type": item.asset_type,
                "ticker": item.ticker,
                "timeframe": item.timeframe,
            }),
        });
    }
    if !b.align {
        return Ok(Json(json!({ "results": out })));
    }
    // Align on the answers, never on what was asked for: an instrument that came back short
    // (or not at all) still has to leave the others on a clock that describes them.
    let served: Vec<AlignItem> = out
        .iter()
        .enumerate()
        .filter(|(_, r)| r.get("error").is_none())
        .map(|(i, r)| AlignItem {
            at: i,
            provider: r["provider"].as_str().unwrap_or_default().to_string(),
            timeframe: r["timeframe"].as_str().unwrap_or_default().to_string(),
            ts: r["ts"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        })
        .collect();
    let aligned = align_batch(b.clock.as_ref(), &served);
    for (item, map) in served.iter().zip(aligned.maps) {
        if let Some(map) = map {
            out[item.at]["map"] = json!(map);
        }
    }
    Ok(Json(json!({ "results": out, "clock": aligned.clock })))
}

/// One answered instrument, reduced to what alignment needs.
struct AlignItem {
    /// Its slot in the batch's results, so a failed instrument does not shift the rest.
    at: usize,
    provider: String,
    timeframe: String,
    ts: Vec<String>,
}

/// The clock plus, per item, its row → bar index map (absent when it was not aligned).
struct AlignedBatch {
    clock: Value,
    maps: Vec<Option<Vec<Option<usize>>>>,
}

/// Put the batch on one bucket set, through the same [`align::merge`] the quant and backtest
/// sides use.
///
/// Two rules, both borrowed rather than invented:
///
/// - **A row is a period, not a timestamp.** Providers stamp the same daily close
///   differently (Binance at the candle open, 00:00 UTC; Alpaca at the New York session
///   start), so pairing them by raw timestamp slides one series a day against the other.
/// - **Intraday across providers is refused, not approximated**, exactly as
///   `/api/quant/portfolio` refuses it: a session-anchored 4h bar and an epoch-anchored one
///   are 90 minutes apart, and there is no honest bucket that holds both.
///
/// With a `base`, the rows *are* the caller's own bars: an overlay is drawn on the price
/// pane's candles, and giving it a wider clock would move the price series. Without one, the
/// rows are the union of what came back.
fn align_batch(base: Option<&ClockRef>, items: &[AlignItem]) -> AlignedBatch {
    let none = |items: &[AlignItem]| vec![None; items.len()];
    if items.is_empty() {
        return AlignedBatch { clock: Value::Null, maps: none(items) };
    }
    let mut timeframes: Vec<&str> = items.iter().map(|i| i.timeframe.as_str()).collect();
    timeframes.sort_unstable();
    timeframes.dedup();
    if timeframes.len() > 1 {
        return AlignedBatch {
            clock: refused(
                "mixed_timeframe",
                format!(
                    "one clock cannot hold {} at once, they are different periods",
                    timeframes.join(" and ")
                ),
            ),
            maps: none(items),
        };
    }
    let tf = crate::timeframe::Timeframe::parse(timeframes[0]).ok();
    let mut providers: Vec<&str> = items
        .iter()
        .map(|i| i.provider.as_str())
        .chain(base.and_then(|b| b.provider.as_deref()))
        .collect();
    providers.sort_unstable();
    providers.dedup();
    if providers.len() > 1 && tf.as_ref().is_some_and(|t| t.is_intraday()) {
        return AlignedBatch {
            clock: refused(
                "intraday_mixed",
                format!(
                    "{} bars from {} cannot be put on one clock: intraday periods are anchored                      differently per provider, so any pairing would be off by up to a period",
                    timeframes[0],
                    providers.join(", ")
                ),
            ),
            maps: none(items),
        };
    }

    let mut series: Vec<align::Series<'_>> = Vec::with_capacity(items.len() + 1);
    if let Some(b) = base {
        series.push(align::Series { label: "clock", ts: &b.ts });
    }
    for it in items {
        series.push(align::Series { label: &it.provider, ts: &it.ts });
    }
    let merged = align::merge(&series, align::Grain::Infer, align::Join::Union);
    let grain = merged.grain.map(|g| g.label());
    // With a base, only the rows the caller actually has candles for survive: its own bars
    // are the chart, and a row it has no bar on is a row it cannot draw.
    let rows: Vec<usize> = match base {
        Some(_) => (0..merged.rows()).filter(|&r| merged.maps[0][r].is_some()).collect(),
        None => (0..merged.rows()).collect(),
    };
    let offset = usize::from(base.is_some());
    let maps = items
        .iter()
        .enumerate()
        .map(|(k, _)| Some(rows.iter().map(|&r| merged.maps[k + offset][r]).collect()))
        .collect();
    let ts: Vec<&str> = match base {
        Some(b) => rows
            .iter()
            .map(|&r| b.ts[merged.maps[0][r].unwrap_or(0)].as_str())
            .collect(),
        None => rows.iter().map(|&r| merged.clock[r].as_str()).collect(),
    };
    AlignedBatch { clock: json!({ "grain": grain, "rows": ts.len(), "ts": ts }), maps }
}

/// A clock that could not be built, said the way the chart's other notices are.
fn refused(code: &str, message: String) -> Value {
    json!({ "refused": { "code": code, "message": message } })
}

/// The read behind both routes: stored bars for the window, then only the missing edges
/// from the provider.
/// Base bars one consolidated slice may read: a 7m slice of 1 500 bars is 10 500 1m bars.
const CONSOLIDATE_MAX_BASE_BARS: i64 = 20_000;

/// One chart slice. A timeframe the provider does not serve but that a served one divides
/// (2m from 1m, 10m from 5m, 3h from 1h, 2w from 1w) is **consolidated**: the slice is read
/// at that base timeframe (stored bars first, as always) and aggregated here. Nothing is
/// written under the consolidated timeframe; storing it stores its base bars.
async fn series_window(state: &AppState, b: &SeriesBody) -> Result<Value, ApiError> {
    if let Some(base) = consolidation_target(state, b).await? {
        return consolidated_window(state, b, base).await;
    }
    series_window_at(state, b, SLICE_MAX_BARS).await
}

/// The base timeframe to consolidate `b.timeframe` from, or `None` when the provider serves
/// it as is (or nothing divides it, which the plain read then reports as unsupported).
async fn consolidation_target(
    state: &AppState,
    b: &SeriesBody,
) -> Result<Option<&'static str>, ApiError> {
    let (_, provider) =
        resolve_source(state, b.connector_id, b.provider.as_deref(), "histviz").await?;
    let Ok(connector) = histdata::connector_for(&provider) else { return Ok(None) };
    let cap = connector.capability();
    if !cap.asset_types.contains(&b.asset_type.as_str()) {
        return Ok(None);
    }
    Ok(consolidation_base(cap.timeframes, &b.timeframe))
}

/// The coarsest served timeframe that divides `target` a whole number of times, when
/// `target` itself is not served. Months only divide months (a month has no fixed length).
pub(crate) fn consolidation_base(supported: &[&'static str], target: &str) -> Option<&'static str> {
    if supported.contains(&target) {
        return None;
    }
    let t = crate::timeframe::Timeframe::parse(target).ok()?;
    // One spelling per period: `7m`, never `7min`, so a layout or a cache key cannot split.
    if t.label() != target {
        return None;
    }
    let mut best: Option<(&'static str, i64)> = None;
    for &s in supported {
        let Ok(base) = crate::timeframe::Timeframe::parse(s) else { continue };
        let ratio = match (t.secs(), base.secs()) {
            (Some(ts), Some(bs)) if ts > bs && ts % bs == 0 => ts / bs,
            (None, None) if t.n > base.n && t.n % base.n == 0 => (t.n / base.n) as i64,
            _ => continue,
        };
        if best.is_none_or(|(_, r)| ratio < r) {
            best = Some((s, ratio));
        }
    }
    best.map(|(s, _)| s)
}

async fn consolidated_window(
    state: &AppState,
    b: &SeriesBody,
    base: &'static str,
) -> Result<Value, ApiError> {
    let target = crate::timeframe::Timeframe::parse(&b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let base_tf = crate::timeframe::Timeframe::parse(base)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let ratio = match (target.secs(), base_tf.secs()) {
        (Some(t), Some(s)) => t / s,
        _ => (target.n / base_tf.n.max(1)) as i64,
    }
    .max(1);
    let want = b.bars.unwrap_or(SLICE_BARS).clamp(1, SLICE_MAX_BARS);
    // One bar more than asked: the oldest bucket is usually cut by the window and dropped.
    // An intraday grid starts at each UTC day's first bar, so the read also reaches a day of
    // base bars further back: every day it returns then has its own opening in the read,
    // instead of a grid starting wherever the slice happened to (a short read of today alone
    // drew a different 7m grid on every call).
    let day_extra = match (target.secs(), base_tf.secs()) {
        (Some(t), Some(s)) if t < 86_400 => 86_400 / s,
        _ => 0,
    };
    let base_bars = ((want + 1) * ratio).min(CONSOLIDATE_MAX_BASE_BARS) + day_extra;
    let inner = SeriesBody {
        connector_id: b.connector_id,
        provider: b.provider.clone(),
        asset_type: b.asset_type.clone(),
        ticker: b.ticker.clone(),
        timeframe: base.to_string(),
        to: b.to.clone(),
        bars: Some(base_bars),
        stored_only: b.stored_only,
    };
    let mut out = series_window_at(state, &inner, CONSOLIDATE_MAX_BASE_BARS + day_extra).await?;

    let rows = bars_from_payload(&out);
    let history_start = out["history_start"].as_bool().unwrap_or(false);
    let mut bars = consolidate(&rows, target);
    // The oldest bucket is short whenever the window starts inside it; only the very start
    // of the provider's history is allowed to show one.
    if !history_start && bars.len() > 1 {
        bars.remove(0);
    }
    if bars.len() as i64 > want {
        bars.drain(..bars.len() - want as usize);
    }
    let tail = newest_bucket_base(&rows, &bars);
    let payload = bars_payload(&bars);
    for k in ["count", "ts", "o", "h", "l", "c", "v"] {
        out[k] = payload[k].clone();
    }
    out["timeframe"] = json!(b.timeframe);
    out["consolidated_from"] = json!(base);
    out["consolidated_tail"] = bars_payload(&tail);
    // A consolidated period has no feed of its own: the chart streams the base timeframe and
    // folds it, so live reach is the base's.
    let provider = out["provider"].as_str().unwrap_or_default().to_string();
    out["stream"] = json!(live::stream_capable_for(&provider, &b.asset_type, base));
    Ok(out)
}

/// The base bars of the newest consolidated bucket: what the chart's live fold starts from,
/// so the feed's next base bar extends that candle on this same grid instead of restarting it.
fn newest_bucket_base(rows: &[store::Bar], consolidated: &[store::Bar]) -> Vec<store::Bar> {
    match consolidated.last() {
        Some(last) => rows.iter().filter(|r| r.ts >= last.ts).cloned().collect(),
        None => Vec::new(),
    }
}

/// The bars back out of a slice payload (the shape [`bars_payload`] writes).
fn bars_from_payload(v: &Value) -> Vec<store::Bar> {
    let col = |k: &str| -> Vec<f64> {
        v[k].as_array()
            .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
            .unwrap_or_default()
    };
    let (o, h, l, c, vol) = (col("o"), col("h"), col("l"), col("c"), col("v"));
    v["ts"]
        .as_array()
        .map(|a| {
            a.iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    let ts = OffsetDateTime::parse(t.as_str()?, &Rfc3339).ok()?;
                    Some(store::Bar {
                        ts,
                        open: *o.get(i)?,
                        high: *h.get(i)?,
                        low: *l.get(i)?,
                        close: *c.get(i)?,
                        volume: vol.get(i).copied().unwrap_or(0.0),
                        adj_open: None,
                        adj_high: None,
                        adj_low: None,
                        adj_close: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Aggregate sorted base bars into `tf` periods: first open, highest high, lowest low, last
/// close, summed volume, stamped at the period's start.
///
/// Periods under a day are anchored on the **session**, not the epoch: each UTC day's grid
/// starts at its first bar, so a 2h chart of a stock opens at 09:30 New York like the market
/// does, and a 24/7 market (first bar at 00:00 UTC) lands on the plain epoch grid. The first
/// day of a window may start mid-session; it borrows the next day's opening time when that
/// one is earlier. Day and longer periods use [`Timeframe::bucket`].
fn consolidate(rows: &[store::Bar], tf: crate::timeframe::Timeframe) -> Vec<store::Bar> {
    const DAY: i64 = 86_400;
    let step = tf.secs().filter(|s| *s < DAY);
    // Seconds after UTC midnight of each day's first bar, keyed by day number.
    let mut opens: Vec<(i64, i64)> = Vec::new();
    if step.is_some() {
        for r in rows {
            let t = r.ts.unix_timestamp();
            let day = t.div_euclid(DAY);
            if opens.last().is_none_or(|(d, _)| *d != day) {
                opens.push((day, t.rem_euclid(DAY)));
            }
        }
        if opens.len() > 1 && opens[1].1 < opens[0].1 {
            opens[0].1 = opens[1].1;
        }
    }
    let start_of = |ts: OffsetDateTime| -> OffsetDateTime {
        let Some(step) = step else { return tf.bucket(ts) };
        let t = ts.unix_timestamp();
        let day = t.div_euclid(DAY);
        let off = opens
            .binary_search_by_key(&day, |(d, _)| *d)
            .map(|i| opens[i].1)
            .unwrap_or(0);
        let anchor = day * DAY + off;
        let start = if t >= anchor {
            anchor + (t - anchor).div_euclid(step) * step
        } else {
            // Before the borrowed opening time: the plain epoch grid.
            t.div_euclid(step) * step
        };
        OffsetDateTime::from_unix_timestamp(start).unwrap_or(ts)
    };
    let mut out: Vec<store::Bar> = Vec::new();
    let mut cur: Option<OffsetDateTime> = None;
    for r in rows {
        let start = start_of(r.ts);
        match out.last_mut() {
            Some(bar) if cur == Some(start) => {
                bar.high = bar.high.max(r.high);
                bar.low = bar.low.min(r.low);
                bar.close = r.close;
                bar.volume += r.volume;
            }
            _ => {
                cur = Some(start);
                out.push(store::Bar {
                    ts: start,
                    open: r.open,
                    high: r.high,
                    low: r.low,
                    close: r.close,
                    volume: r.volume,
                    adj_open: None,
                    adj_high: None,
                    adj_low: None,
                    adj_close: None,
                });
            }
        }
    }
    out
}

async fn series_window_at(
    state: &AppState,
    b: &SeriesBody,
    max_bars: i64,
) -> Result<Value, ApiError> {
    let state = state.clone();
    let ticker = b.ticker.trim();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    if b.connector_id.is_none() && b.provider.as_deref() == Some(histdata::IMPORT_PROVIDER) {
        return imported_window(&state, b, ticker, max_bars).await;
    }
    let (connector_id, provider) =
        resolve_source(&state, b.connector_id, b.provider.as_deref(), "histviz").await?;
    let cap = histdata::validate_request(&provider, &b.asset_type, &b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let tf_secs = histdata::timeframe_secs(&b.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let want = b.bars.unwrap_or(SLICE_BARS).clamp(1, max_bars);

    let to = match b.to.as_deref() {
        Some(s) => parse_rfc3339(s, "to")?,
        None => OffsetDateTime::now_utc(),
    };
    let from = to - time::Duration::seconds(tf_secs * want);

    // What the catalog already holds for this window — free, and never re-downloaded.
    let dataset = store::find_dataset(&state.pool, &provider, &b.asset_type, ticker, &b.timeframe)
        .await?;
    // Read a couple of bars more than the slice: `read_bars` keeps the *newest* `limit`
    // rows, and a window trimmed exactly to `want` would look like it were missing its
    // oldest bar — turning a covered window into a one-bar provider call.
    let mut rows: Vec<otw_store::histdata::Bar> = match &dataset {
        Some(d) => store::read_bars(&state.pool, d.id, Some(from), Some(to), want + 2).await?,
        None => Vec::new(),
    };
    let stored_here = rows.len();

    // The holes to ask the provider for: the stretch before the oldest stored bar and the
    // stretch after the newest. One bar of slack keeps a boundary bar from being refetched.
    let step = time::Duration::seconds(tf_secs);
    let mut gaps: Vec<(OffsetDateTime, OffsetDateTime)> = Vec::new();
    let mut asked_older = false;
    match (rows.first(), rows.last()) {
        (Some(first), Some(last)) => {
            if first.ts - from >= step {
                gaps.push((from, first.ts));
                asked_older = true;
            }
            if to - last.ts > step {
                gaps.push((last.ts + step, to));
            }
        }
        _ => {
            gaps.push((from, to));
            asked_older = true;
        }
    }

    let mut notice: Option<Value> = None;
    let mut fetched = 0usize; // bars the provider served *inside* the window
    // Bars served that sit *after* the window. That is the shape of a provider ignoring the
    // upper bound; bars sitting before it are the opposite, an honest answer to a stretch
    // that holds no session (see the depth notice below).
    let mut served_newer = 0usize;
    let mut got_older = false;

    if b.stored_only {
        gaps.clear();
        asked_older = false;
    }

    // Holes *inside* the stored stretch: an outage the live recorder sat through, a download
    // that came back short. Read as they are, the chart draws the bars either side as
    // neighbours. Each is asked of the provider once: bars that come back are stored, and an
    // empty answer (a night, a weekend, a quiet minute) is remembered as confirmed, so a
    // reload never pays for the same hole twice. Neighbouring holes share a request
    // (`histdata_check::plan`). Not in the demo, which writes nothing.
    let mut interior: Vec<histdata_check::Window> = Vec::new();
    if let (Some(d), false, false) = (&dataset, b.stored_only, crate::demo::enabled()) {
        let confirmed: std::collections::HashSet<(i64, i64)> = store::gap_rows(&state.pool, d.id)
            .await?
            .into_iter()
            .filter(|g| g.status == "confirmed")
            .map(|g| (g.gap_from.unix_timestamp(), g.gap_to.unix_timestamp()))
            .collect();
        let holes = interior_holes(&rows, tf_secs, &confirmed);
        if !holes.is_empty() {
            let page = tf_secs * cap.max_bars_per_req.max(1) as i64;
            interior = histdata_check::plan(&holes, tf_secs, page, SLICE_MAX_CHUNKS);
        }
    }
    // Interior holes nobody could check this time (the provider failed, or the request budget
    // ran out). Returned to the chart, which marks them and asks again later.
    let mut unchecked: Vec<(i64, i64)> = Vec::new();

    if !gaps.is_empty() || !interior.is_empty() {
        let secrets = match connector_id {
            Some(id) => conn_store::load_creds(&state.pool, &state.cipher, id).await?,
            None => Default::default(),
        };
        // Refuse before spending a request when the reason is already knowable: no key,
        // or a limit the user themselves declared on this connector.
        if let Some(missing) = missing_secret(cap, &secrets) {
            notice = Some(json!({
                "code": "auth",
                "message": format!("{} needs its '{missing}' credential — set it on the connector", cap.label),
            }));
        } else if let Some(q) = quota_exhausted(&state, connector_id).await {
            notice = Some(q);
        } else {
            let connector = histdata::connector_for(&provider)
                .map_err(|e| ApiError::bad_request(&e.to_string()))?;
            let page = time::Duration::seconds(tf_secs * cap.max_bars_per_req.max(1) as i64);
            let mut chunks = 0usize;
'gaps: for (g_from, g_to) in gaps {
                let older = g_from <= from;
                // A gap of one or two bars is widened backwards into a window the provider
                // will actually serve; what comes back over the part we already hold is
                // dropped by the filters below.
                let mut cursor = ask_from(g_from, g_to, step);
                while cursor < g_to {
                    if chunks >= SLICE_MAX_CHUNKS {
                        // Said, not swallowed: the bars past this point were never asked
                        // for, and a short answer with no reason reads as a market that did
                        // not trade there.
                        notice = Some(json!({
                            "code": "partial",
                            "message": format!(
                                "stopped after {SLICE_MAX_CHUNKS} {} requests, the rest of this window was not fetched",
                                cap.label
                            ),
                        }));
                        break 'gaps;
                    }
                    let chunk_to = (cursor + page).min(g_to);
                    if let Some(id) = connector_id {
                        let _ = otw_store::api_quota::bump(
                            &state.pool,
                            &crate::connectors_api::quota_scope(id),
                        )
                        .await;
                    }
                    chunks += 1;
                    match connector
                        .fetch_chunk(
                            &state.http,
                            &secrets,
                            ticker,
                            &b.asset_type,
                            &b.timeframe,
                            cursor,
                            chunk_to,
                        )
                        .await
                    {
                        Ok(chunk) => {
                            served_newer += chunk.bars.iter().filter(|x| x.ts >= to).count();
                            let served_older = chunk.bars.iter().any(|x| x.ts < from);
                            // Only bars inside the asked window count as an answer: some
                            // providers (Kraken) ignore the upper bound and always reply
                            // with the most recent page, which is not the past we asked for.
                            let usable: Vec<_> = chunk
                                .bars
                                .into_iter()
                                .filter(|x| x.ts >= from && x.ts < to)
                                .collect();
                            fetched += usable.len();
                            // Bars from before the window prove the provider holds older
                            // history just as well as bars inside it, and a widened ask is
                            // exactly how they turn up.
                            if older && (!usable.is_empty() || served_older) {
                                got_older = true;
                            }
                            rows.extend(usable);
                        }
                        Err(e) => {
                            // Keep whatever we already have on screen and say why the rest
                            // is missing — an unreachable provider is not an empty chart.
                            let msg = format!("{e:#}");
                            notice = Some(json!({
                                "code": notice_code(&msg),
                                "message": short_reason(&msg),
                            }));
                            break 'gaps;
                        }
                    }
                    cursor = chunk_to;
                }
            }
            let dataset_id = dataset.as_ref().map(|d| d.id);
            let at = |secs: i64| OffsetDateTime::from_unix_timestamp(secs).unwrap_or(from);
            'interior: for w in std::mem::take(&mut interior) {
                if notice.is_some() {
                    unchecked.extend(w.holes);
                    continue;
                }
                let (w_from, w_to) = (at(w.from), at(w.to));
                let mut got: Vec<otw_store::histdata::Bar> = Vec::new();
                // Bars past the window: a provider ignoring the upper bound (Kraken answers
                // with its newest page). Its silence about the window proves nothing.
                let mut beyond = false;
                let mut cursor = w_from;
                while cursor < w_to {
                    if chunks >= SLICE_MAX_CHUNKS {
                        notice = Some(json!({
                            "code": "partial",
                            "message": format!(
                                "stopped after {SLICE_MAX_CHUNKS} {} requests, some holes in the stored bars were not checked",
                                cap.label
                            ),
                        }));
                        unchecked.extend(w.holes);
                        continue 'interior;
                    }
                    let chunk_to = (cursor + page).min(w_to);
                    if let Some(id) = connector_id {
                        let _ = otw_store::api_quota::bump(
                            &state.pool,
                            &crate::connectors_api::quota_scope(id),
                        )
                        .await;
                    }
                    chunks += 1;
                    match connector
                        .fetch_chunk(&state.http, &secrets, ticker, &b.asset_type, &b.timeframe, cursor, chunk_to)
                        .await
                    {
                        Ok(chunk) => {
                            beyond |= chunk.bars.iter().any(|x| x.ts > w_to);
                            got.extend(chunk.bars.into_iter().filter(|x| x.ts >= w_from && x.ts < w_to));
                        }
                        Err(e) => {
                            let msg = format!("{e:#}");
                            notice = Some(json!({
                                "code": notice_code(&msg),
                                "message": short_reason(&msg),
                            }));
                            unchecked.extend(w.holes);
                            continue 'interior;
                        }
                    }
                    cursor = chunk_to;
                }
                // The window was answered whole: what came back is stored, and a hole it left
                // empty is one the provider has nothing for.
                if let Some(id) = dataset_id {
                    if !got.is_empty() {
                        store::write_bars(&state.pool, id, &got).await?;
                    }
                    for &(a, z) in &w.holes {
                        let filled = got.iter().any(|x| {
                            let t = x.ts.unix_timestamp();
                            t > a && t < z
                        });
                        if filled {
                            continue;
                        }
                        if beyond && got.is_empty() {
                            unchecked.push((a, z));
                        } else {
                            store::confirm_empty_gap(&state.pool, id, at(a), at(z)).await?;
                        }
                    }
                }
                fetched += got.iter().filter(|x| x.ts >= from && x.ts < to).count();
                rows.extend(got);
            }
        }
    }
    // A refusal before any request (no key, quota spent) leaves every interior hole unchecked.
    for w in interior {
        unchecked.extend(w.holes);
    }

    // Providers overlap pages and ignore the window bounds; normalize to one clean series.
    rows.retain(|r| r.ts >= from && r.ts < to);
    rows.sort_by_key(|r| r.ts);
    rows.dedup_by_key(|r| r.ts);
    if rows.len() as i64 > want {
        rows.drain(..rows.len() - want as usize);
    }

    // Nothing older came back for a window we did ask about → this is the start of what
    // the provider will serve; the client stops offering "load more".
    let history_start = asked_older && notice.is_none() && !got_older;
    // It answered, but with bars from *after* the window: the provider ignored the upper
    // bound and replied with its most recent page, which is a history-depth refusal in
    // disguise and deserves to be read as one.
    //
    // Bars from *before* the window are the opposite and must stay silent. A stretch that
    // holds no session (a weekend, a holiday, the days either side of one) has nothing to
    // serve, so a provider answering with the last bars it does have is answering
    // correctly. Calling that a depth limit accuses it of a refusal it never made.
    if notice.is_none() && served_newer > 0 && fetched == 0 {
        notice = Some(json!({
            "code": "depth",
            "message": format!(
                "{} served no bars inside this window — its history for {} may not reach that far back",
                cap.label, b.timeframe
            ),
        }));
    }

    let mut out = bars_payload(&rows);
    out["provider"] = json!(provider);
    out["connector_id"] = json!(connector_id);
    out["asset_type"] = json!(b.asset_type);
    out["ticker"] = json!(ticker);
    out["timeframe"] = json!(b.timeframe);
    out["from"] = json!(from.format(&Rfc3339).unwrap_or_default());
    out["to"] = json!(to.format(&Rfc3339).unwrap_or_default());
    out["from_store"] = json!(stored_here);
    out["from_provider"] = json!(fetched);
    out["history_start"] = json!(history_start);
    out["stream"] = json!(live::stream_capable_for(&provider, &b.asset_type, &b.timeframe));
    out["stream_timeframes"] = json!(live::stream_timeframes(&provider));
    out["stream_note"] = json!(live::stream_note(&provider));
    out["stored"] = dataset
        .map(|d| json!({ "id": d.id, "bar_count": d.bar_count }))
        .unwrap_or(Value::Null);
    out["notice"] = notice.unwrap_or(Value::Null);
    let stamp = |secs: i64| {
        OffsetDateTime::from_unix_timestamp(secs)
            .ok()
            .and_then(|t| t.format(&Rfc3339).ok())
    };
    out["holes"] = json!(unchecked
        .iter()
        .filter_map(|&(a, z)| Some(json!({ "before": stamp(a)?, "after": stamp(z)? })))
        .collect::<Vec<_>>());
    Ok(out)
}

/// A window of an imported series: stored bars only, since there is no provider behind it to
/// fill a gap or extend it (`connector_for` refuses `import`). Same payload as a provider
/// window, with nothing to stream and no holes to ask about.
async fn imported_window(state: &AppState, b: &SeriesBody, ticker: &str, max_bars: i64) -> Result<Value, ApiError> {
    let provider = histdata::IMPORT_PROVIDER;
    histdata::timeframe_secs(&b.timeframe).map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let dataset = store::find_dataset(&state.pool, provider, &b.asset_type, ticker, &b.timeframe)
        .await?
        .ok_or_else(|| ApiError::not_found(&format!("no imported {ticker} series at {}", b.timeframe)))?;
    let want = b.bars.unwrap_or(SLICE_BARS).clamp(1, max_bars);
    let to = match b.to.as_deref() {
        Some(s) => parse_rfc3339(s, "to")?,
        None => OffsetDateTime::now_utc(),
    };
    // The newest `want` bars before `to`: an imported file has its own calendar (weekends,
    // holidays), so the window is counted in bars, not stretched over a span of time.
    let mut rows = store::read_bars(&state.pool, dataset.id, None, Some(to), want + 2).await?;
    rows.retain(|r| r.ts < to);
    let history_start = rows.len() as i64 <= want;
    if rows.len() as i64 > want {
        rows.drain(..rows.len() - want as usize);
    }
    let from = rows.first().map(|r| r.ts).unwrap_or(to);
    let mut out = bars_payload(&rows);
    out["provider"] = json!(provider);
    out["connector_id"] = Value::Null;
    out["asset_type"] = json!(b.asset_type);
    out["ticker"] = json!(ticker);
    out["timeframe"] = json!(b.timeframe);
    out["from"] = json!(from.format(&Rfc3339).unwrap_or_default());
    out["to"] = json!(to.format(&Rfc3339).unwrap_or_default());
    out["from_store"] = json!(rows.len());
    out["from_provider"] = json!(0);
    out["history_start"] = json!(history_start);
    out["stream"] = json!(false);
    out["stream_timeframes"] = json!(Vec::<String>::new());
    out["stream_note"] = Value::Null;
    out["stored"] = json!({ "id": dataset.id, "bar_count": dataset.bar_count });
    out["notice"] = Value::Null;
    out["holes"] = json!(Vec::<Value>::new());
    Ok(out)
}

/// Holes inside a run of stored bars, as (bar before, bar after) in Unix seconds, leaving out
/// the ones the provider already answered empty.
fn interior_holes(
    rows: &[otw_store::histdata::Bar],
    step: i64,
    confirmed: &std::collections::HashSet<(i64, i64)>,
) -> Vec<(i64, i64)> {
    rows.windows(2)
        .map(|w| (w[0].ts.unix_timestamp(), w[1].ts.unix_timestamp()))
        .filter(|&(a, z)| z - a > step && !confirmed.contains(&(a, z)))
        .collect()
}

/// A connector whose user-declared request limit is spent, as a ready-made notice.
///
/// Quotas are observe-only everywhere else; here they do gate, because the chart can fire
/// a request per click and the user set that ceiling precisely to bound this.
async fn quota_exhausted(state: &AppState, connector_id: Option<Uuid>) -> Option<Value> {
    let id = connector_id?;
    let q = otw_store::api_quota::get(&state.pool, &crate::connectors_api::quota_scope(id))
        .await
        .ok()??;
    let max = q.max_requests?;
    (q.used >= max).then(|| {
        json!({
            "code": "quota",
            "message": format!(
                "connector request limit reached ({}/{} per {}) — resets {}",
                q.used, max, q.period,
                q.resets_at.format(&Rfc3339).unwrap_or_default()
            ),
        })
    })
}

// ── Datasets (management page) ───────────────────────────────────────────────────

async fn datasets(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let datasets = store::list_datasets(&state.pool).await?;
    Ok(Json(json!({ "datasets": datasets })))
}

/// Append/gap-fill: queue a download from the dataset's latest bar up to now.
/// Stored bars a stock's top-up asks for again, to compare the provider's prices on them.
const REBASE_OVERLAP_BARS: i64 = 7;

async fn append(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let ds = store::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    // Start one timeframe after the last bar (or 30d back if empty), or on the last bar itself
    // when it was written before its period ended: that one is a fragment to read again.
    let step = histdata::timeframe_secs(&ds.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let from = match ds.range_to {
        Some(t) if histdata::last_bar_unsettled(&ds.timeframe, t, ds.last_updated) => t,
        Some(t) => t + time::Duration::seconds(step),
        None => OffsetDateTime::now_utc() - time::Duration::days(30),
    };
    let to = OffsetDateTime::now_utc();
    if from >= to {
        return Ok(Json(json!({ "ok": true, "skipped": "already current" })));
    }
    // A stock's top-up re-reads its last few stored bars: a split or a dividend since the last
    // download shows on them, and the store rebases the older history onto the new prices.
    let from = if ds.range_to.is_some() && ["stock", "etf", "fund", "equity"].contains(&ds.asset_type.as_str()) {
        from - time::Duration::seconds(step * REBASE_OVERLAP_BARS)
    } else {
        from
    };
    // Datasets are provider-keyed, not connector-keyed; append through the default one.
    let connector_id = conn_store::default_for(&state.pool, &ds.provider)
        .await?
        .map(|c| c.id);
    let job_id = store::enqueue_job(
        &state.pool,
        &store::NewJob {
            dataset_id: id,
            connector_id,
            provider: &ds.provider,
            asset_type: &ds.asset_type,
            ticker: &ds.ticker,
            timeframe: &ds.timeframe,
            range_from: from,
            range_to: to,
            kind: "append",
            batch_id: None,
        },
    )
    .await?;
    Ok(Json(json!({ "job_id": job_id })))
}

// ── Integrity check ──────────────────────────────────────────────────────────────
//
// Reads every stamp of a dataset, finds where the market traded and no candle is stored
// (`histdata_check`), and says what asking the provider again would cost. Filling is a
// separate click: the holes are remembered against the downloads sent for them
// (`histdata_gaps`), so one the provider has nothing for is confirmed and never asked twice.

/// Holes the check lists one by one; the totals always cover them all.
const CHECK_MAX_LISTED: usize = 1000;
/// Ceiling on the downloads one fill queues (neighbouring holes are bridged past it).
const CHECK_MAX_JOBS: usize = 200;

const KINDS: [(Kind, &str); 3] = [(Kind::Missing, "missing"), (Kind::Holiday, "holiday"), (Kind::Quiet, "quiet")];

struct Checked {
    ds: store::Dataset,
    step: i64,
    bars: usize,
    around_the_clock: bool,
    /// Each hole with where its fill stands: `missing`, `filling` or `confirmed`.
    holes: Vec<(histdata_check::Finding, &'static str)>,
}

impl Checked {
    /// Holes of these kinds nobody has asked for yet, as (bar before, bar after).
    fn open(&self, kinds: &[Kind]) -> Vec<(i64, i64)> {
        self.holes
            .iter()
            .filter(|(f, st)| *st == "missing" && kinds.contains(&f.kind))
            .map(|(f, _)| (f.before, f.after))
            .collect()
    }
}

async fn check_holes(state: &AppState, id: Uuid) -> Result<Checked, ApiError> {
    let ds = store::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let tf = crate::timeframe::Timeframe::parse(&ds.timeframe)
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    // A month has no fixed length; thirty days, rounded, still counts one bar per month.
    let step = tf.secs().unwrap_or(30 * 86_400);
    let epochs = store::bar_epochs(&state.pool, id).await?;
    let scan = histdata_check::scan(&epochs, step, ds.asset_type == "crypto");
    let at = |s: i64| OffsetDateTime::from_unix_timestamp(s).map_err(|e| ApiError::internal(&e.to_string()));
    let pairs = scan
        .findings
        .iter()
        .map(|f| Ok((at(f.before)?, at(f.after)?)))
        .collect::<Result<Vec<_>, ApiError>>()?;
    let statuses = crate::data_gaps::statuses(&state.pool, id, &ds.ticker, &pairs).await?;
    let holes = scan.findings.into_iter().zip(statuses.iter().map(|g| g.status)).collect();
    Ok(Checked { ds, step, bars: epochs.len(), around_the_clock: scan.around_the_clock, holes })
}

/// Why this dataset cannot be filled from a provider, or the provider's paging (seconds of
/// history one request returns) and pacing.
fn fill_path(c: &Checked) -> Result<(i64, u64), String> {
    if crate::demo::enabled() {
        return Err("the demo does not download market data".into());
    }
    let cap = histdata::connector_for(&c.ds.provider).map_err(|e| e.to_string())?.capability();
    Ok((c.step * cap.max_bars_per_req.max(1) as i64, cap.min_interval_ms))
}

fn requests_for(c: &Checked, holes: &[(i64, i64)], page: i64) -> (Vec<histdata_check::Window>, i64) {
    let windows = histdata_check::plan(holes, c.step, page, CHECK_MAX_JOBS);
    let requests = windows.iter().map(|w| histdata_check::chunks(w.to - w.from, page)).sum();
    (windows, requests)
}

/// GET /api/histdata/datasets/{id}/check
async fn check_dataset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let c = check_holes(&state, id).await?;
    let path = fill_path(&c);
    let page = path.as_ref().ok().map(|(p, _)| *p);
    let ts = |s: i64| {
        OffsetDateTime::from_unix_timestamp(s)
            .ok()
            .and_then(|t| t.format(&Rfc3339).ok())
            .unwrap_or_default()
    };

    let mut kinds = serde_json::Map::new();
    for (kind, name) in KINDS {
        let of: Vec<_> = c.holes.iter().filter(|(f, _)| f.kind == kind).collect();
        let open = c.open(&[kind]);
        let open_bars: i64 = of.iter().filter(|(_, st)| *st == "missing").map(|(f, _)| f.bars).sum();
        let requests = page.map(|p| requests_for(&c, &open, p).1);
        kinds.insert(
            name.into(),
            json!({
                "holes": of.len(),
                "bars": of.iter().map(|(f, _)| f.bars).sum::<i64>(),
                "open": open.len(),
                "open_bars": open_bars,
                "requests": requests,
            }),
        );
    }
    let count = |st: &str| c.holes.iter().filter(|(_, s)| *s == st).count();
    // Coverage counts what the market certainly printed: closures and short silences are
    // probably the market's own, not the data's.
    let expected = c.bars as i64
        + c.holes.iter().filter(|(f, _)| f.kind == Kind::Missing).map(|(f, _)| f.bars).sum::<i64>();
    let listed: Vec<Value> = c
        .holes
        .iter()
        .take(CHECK_MAX_LISTED)
        .map(|(f, st)| {
            json!({
                "from": ts(f.before),
                "to": ts(f.after),
                "bars": f.bars,
                "kind": f.kind,
                "status": st,
            })
        })
        .collect();
    let fill = match path {
        Ok((_, min_interval_ms)) => {
            let connector_id = conn_store::default_for(&state.pool, &c.ds.provider).await?.map(|x| x.id);
            let (used, max, period, resets_at) = quota_now(&state, connector_id).await;
            json!({
                "available": true,
                "min_interval_ms": min_interval_ms,
                "quota_used": used,
                "quota_max": max,
                "quota_period": period,
                "quota_resets_at": resets_at,
            })
        }
        Err(reason) => json!({ "available": false, "reason": reason }),
    };
    Ok(Json(json!({
        "dataset": {
            "id": c.ds.id,
            "provider": c.ds.provider,
            "asset_type": c.ds.asset_type,
            "ticker": c.ds.ticker,
            "timeframe": c.ds.timeframe,
            "label": c.ds.label,
            "range_from": c.ds.range_from.and_then(|t| t.format(&Rfc3339).ok()),
            "range_to": c.ds.range_to.and_then(|t| t.format(&Rfc3339).ok()),
        },
        "around_the_clock": c.around_the_clock,
        "step_secs": c.step,
        "bars": c.bars,
        "expected": expected,
        "kinds": kinds,
        "filling": count("filling"),
        "confirmed": count("confirmed"),
        "holes": listed,
        "total_holes": c.holes.len(),
        "fill": fill,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CheckFillBody {
    /// Which holes to ask the provider for: any of "missing", "holiday", "quiet".
    kinds: Vec<String>,
}

/// POST /api/histdata/datasets/{id}/check/fill: queue downloads for the open holes of the
/// chosen kinds, neighbouring holes sharing one download when that costs nothing more.
async fn fill_checked_holes(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<CheckFillBody>,
) -> Result<Json<Value>, ApiError> {
    let kinds = b
        .kinds
        .iter()
        .map(|k| Kind::parse(k).ok_or_else(|| ApiError::bad_request(&format!("unknown hole kind {k:?}"))))
        .collect::<Result<Vec<_>, _>>()?;
    if kinds.is_empty() {
        return Err(ApiError::bad_request("choose at least one kind of hole to fill"));
    }
    let c = check_holes(&state, id).await?;
    let (page, _) = fill_path(&c).map_err(|e| {
        if crate::demo::enabled() { ApiError::forbidden(&e) } else { ApiError::bad_request(&e) }
    })?;
    let open = c.open(&kinds);
    if open.is_empty() {
        return Ok(Json(json!({ "jobs": 0, "holes": 0, "requests": 0, "batch_id": null })));
    }
    let (windows, requests) = requests_for(&c, &open, page);
    let connector_id = conn_store::default_for(&state.pool, &c.ds.provider).await?.map(|x| x.id);
    let (batch_id, jobs) = crate::data_gaps::fill_windows(&state.pool, &c.ds, connector_id, &windows).await?;
    Ok(Json(json!({
        "jobs": jobs,
        "holes": open.len(),
        "requests": requests,
        "batch_id": batch_id,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct BarsQuery {
    /// RFC3339 inclusive lower bound (optional).
    from: Option<String>,
    /// RFC3339 inclusive upper bound (optional).
    to: Option<String>,
    /// Max bars to return; clamped to [1, 50000]. Defaults to 5000.
    limit: Option<i64>,
}

/// OHLCV for the visualization module. Returns the dataset header plus parallel arrays
/// (ts as RFC3339 strings, o/h/l/c/v as numbers) — compact and ECharts-ready.
async fn bars(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<BarsQuery>,
) -> Result<Json<Value>, ApiError> {
    let ds = store::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let from = q.from.as_deref().map(|s| parse_rfc3339(s, "from")).transpose()?;
    let to = q.to.as_deref().map(|s| parse_rfc3339(s, "to")).transpose()?;
    let limit = q.limit.unwrap_or(5000).clamp(1, 50_000);

    let rows = store::read_bars(&state.pool, id, from, to, limit).await?;
    let mut out = bars_payload(&rows);
    out["id"] = json!(ds.id);
    out["provider"] = json!(ds.provider);
    out["asset_type"] = json!(ds.asset_type);
    out["ticker"] = json!(ds.ticker);
    out["timeframe"] = json!(ds.timeframe);
    Ok(Json(out))
}

/// Bars as parallel arrays (ts as RFC3339 strings, o/h/l/c/v as numbers) — compact and
/// ECharts-ready. Shared by the stored-bars read and the ad-hoc preview so the chart
/// consumes one shape either way.
pub(crate) fn bars_payload(rows: &[otw_store::histdata::Bar]) -> Value {
    let mut ts = Vec::with_capacity(rows.len());
    let (mut o, mut h, mut l, mut c, mut v) = (
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
        Vec::with_capacity(rows.len()),
    );
    for b in rows {
        ts.push(b.ts.format(&Rfc3339).unwrap_or_default());
        o.push(b.open);
        h.push(b.high);
        l.push(b.low);
        c.push(b.close);
        v.push(b.volume);
    }
    json!({ "count": rows.len(), "ts": ts, "o": o, "h": h, "l": l, "c": c, "v": v })
}

#[derive(Deserialize, Default, schemars::JsonSchema)]
pub(crate) struct ExportQuery {
    /// "csv" (default) or "parquet".
    #[serde(default)]
    format: Option<String>,
}

/// GET /api/histdata/datasets/{id}/export — every bar of a dataset as a file.
///
/// CSV is the default because it is what opens anywhere. Parquet is the same bars typed
/// and compressed, and it carries the dataset's identity in its own metadata, so a file
/// exported here re-imports without the instrument being typed back in.
async fn export(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let ds = store::get_dataset(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("dataset not found"))?;
    let rows = store::export_rows(&state.pool, id).await?;

    let (body, mime, ext): (Vec<u8>, &str, &str) = match q.format.as_deref().unwrap_or("csv") {
        "csv" => (crate::histdata_export::csv_bytes(&rows).into_bytes(), "text/csv", "csv"),
        "parquet" => (
            crate::histdata_export::parquet_bytes(&ds, &rows)
                .map_err(|e| ApiError::internal(&format!("{e:#}")))?,
            "application/vnd.apache.parquet",
            "parquet",
        ),
        other => {
            return Err(ApiError::bad_request(&format!(
                "unknown export format {other:?}: use csv or parquet"
            )))
        }
    };

    let filename = crate::histdata_export::filename(&ds, ext);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}

async fn remove_dataset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_dataset(&state.pool, id).await? {
        return Err(ApiError::not_found("dataset not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_failures_are_sorted_into_codes() {
        assert_eq!(notice_code("HTTP 429 Too Many Requests"), "rate_limit");
        assert_eq!(notice_code("EODHD requires a 'api_key' credential"), "auth");
        assert_eq!(notice_code("massive (NOT_AUTHORIZED): plan does not include"), "auth");
        assert_eq!(notice_code("binance status (bad symbol?)"), "symbol");
        assert_eq!(notice_code("yahoo: 1h data is limited to the last 730 days"), "depth");
        assert_eq!(notice_code("connection reset"), "provider");
    }

    #[test]
    fn reasons_drop_the_url_tail_that_could_carry_a_key() {
        let msg = "eodhd status: HTTP status client error (401 Unauthorized) \
                   for url (https://eodhd.com/api/eod/AAPL?api_token=SECRET)";
        let short = short_reason(msg);
        assert!(!short.contains("SECRET"), "{short}");
        assert!(short.ends_with("(401 Unauthorized)"), "{short}");
    }

    #[test]
    fn a_catch_up_covers_only_bars_that_have_closed() {
        use time::macros::datetime;
        let step = time::Duration::minutes(5);
        let since = datetime!(2026-09-05 10:00:00 UTC);

        // Inside the very next period: nothing has closed, so nothing is asked for and no
        // request is spent. This is the common case — the seam is usually seconds wide.
        assert!(catch_up_window(since, step, datetime!(2026-09-05 10:07:00 UTC)).is_none());
        // Exactly on the boundary the 10:05 bar is only just complete, and it counts.
        let (from, to) = catch_up_window(since, step, datetime!(2026-09-05 10:10:00 UTC)).unwrap();
        assert_eq!(from, datetime!(2026-09-05 10:05:00 UTC));
        assert_eq!(to, datetime!(2026-09-05 10:10:00 UTC));
        // A period and a half on: the window still opens at the bar after the viewer's.
        let (from, _) = catch_up_window(since, step, datetime!(2026-09-05 10:12:30 UTC)).unwrap();
        assert_eq!(from, datetime!(2026-09-05 10:05:00 UTC));
    }

    #[test]
    fn a_top_up_asks_for_a_window_worth_a_request() {
        use time::macros::datetime;
        let step = time::Duration::minutes(5);
        let to = datetime!(2026-09-05 10:10:00 UTC);

        // Two candles missing: the ask reaches back far enough to be a real request, and
        // the caller still keeps the window it asked about.
        let from = datetime!(2026-09-05 10:00:00 UTC);
        assert_eq!(ask_from(from, to, step), to - step * MIN_ASK_BARS);
        // A window already wide enough is left exactly where it is: never widened for its
        // own sake, and never narrowed.
        let wide = to - step * (MIN_ASK_BARS + 10);
        assert_eq!(ask_from(wide, to, step), wide);
    }

    fn item(at: usize, provider: &str, tf: &str, ts: &[&str]) -> AlignItem {
        AlignItem {
            at,
            provider: provider.into(),
            timeframe: tf.into(),
            ts: ts.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// The reason the shared clock exists: two providers stamp the same trading day at two
    /// different instants, and a raw timestamp match pairs none of them.
    #[test]
    fn a_daily_row_is_a_day_whoever_stamped_it() {
        let crypto = item(
            0,
            "binance",
            "1d",
            &["2026-09-01T00:00:00Z", "2026-09-02T00:00:00Z", "2026-09-03T00:00:00Z"],
        );
        let equity = item(
            1,
            "alpaca",
            "1d",
            &["2026-09-01T04:00:00Z", "2026-09-02T04:00:00Z", "2026-09-03T04:00:00Z"],
        );
        let out = align_batch(None, &[crypto, equity]);
        assert_eq!(out.clock["rows"], json!(3), "three days, not six");
        assert_eq!(out.clock["grain"], json!("1d"));
        assert_eq!(out.maps[0].as_ref().unwrap(), &vec![Some(0), Some(1), Some(2)]);
        assert_eq!(out.maps[1].as_ref().unwrap(), &vec![Some(0), Some(1), Some(2)]);
    }

    /// A comparison overlay is drawn on the price pane's own candles: the rows are the
    /// caller's, and a day the other instrument did not trade is a hole, not a shifted row.
    #[test]
    fn a_reference_clock_keeps_its_own_rows() {
        let base = ClockRef {
            provider: Some("alpaca".into()),
            ts: vec![
                "2026-09-01T04:00:00Z".into(),
                "2026-09-02T04:00:00Z".into(),
                "2026-09-03T04:00:00Z".into(),
            ],
        };
        // The other instrument skipped the middle day, and starts a day earlier.
        let other = item(
            0,
            "binance",
            "1d",
            &["2026-08-31T00:00:00Z", "2026-09-01T00:00:00Z", "2026-09-03T00:00:00Z"],
        );
        let out = align_batch(Some(&base), &[other]);
        assert_eq!(out.clock["rows"], json!(3), "the base's rows, not the union's four");
        assert_eq!(out.clock["ts"][0], json!("2026-09-01T04:00:00Z"), "the base's own stamps");
        assert_eq!(out.maps[0].as_ref().unwrap(), &vec![Some(1), None, Some(2)]);
    }

    /// Same rule as `/api/quant/portfolio`: an intraday bucket cannot hold two providers'
    /// anchors, so the batch says so instead of pairing bars 90 minutes apart.
    #[test]
    fn intraday_across_providers_is_refused_not_approximated() {
        let a = item(0, "binance", "4h", &["2026-09-01T00:00:00Z"]);
        let b = item(1, "alpaca", "4h", &["2026-09-01T13:30:00Z"]);
        let out = align_batch(None, &[a, b]);
        assert_eq!(out.clock["refused"]["code"], json!("intraday_mixed"));
        assert!(out.maps.iter().all(Option::is_none), "no map is better than a wrong one");

        // One provider at a time is exact matching, which is honest and still useful.
        let a = item(0, "binance", "4h", &["2026-09-01T00:00:00Z", "2026-09-01T04:00:00Z"]);
        let b = item(1, "binance", "4h", &["2026-09-01T04:00:00Z"]);
        let out = align_batch(None, &[a, b]);
        assert_eq!(out.clock["rows"], json!(2));
        assert_eq!(out.maps[1].as_ref().unwrap(), &vec![None, Some(0)]);
    }

    /// Two timeframes are two clocks. Aligning them would put a week of one instrument on
    /// one row of the other and call it a period.
    #[test]
    fn two_timeframes_are_not_one_clock() {
        let a = item(0, "binance", "1d", &["2026-09-01T00:00:00Z"]);
        let b = item(1, "binance", "1w", &["2026-09-01T00:00:00Z"]);
        let out = align_batch(None, &[a, b]);
        assert_eq!(out.clock["refused"]["code"], json!("mixed_timeframe"));
    }

    #[test]
    fn a_stale_window_is_reloaded_rather_than_walked_forward() {
        use time::macros::datetime;
        let step = time::Duration::minutes(5);
        let since = datetime!(2026-09-05 10:00:00 UTC);
        // CATCH_UP_MAX periods still walk forward…
        let edge = since + step * (CATCH_UP_MAX as i32);
        assert!(catch_up_window(since, step, edge).is_some());
        // …one more and the chart is looking at an old window, not a seam.
        assert!(catch_up_window(since, step, edge + step * 2).is_none());
    }

    fn bar(ts: &str, o: f64, h: f64, l: f64, c: f64, v: f64) -> store::Bar {
        store::Bar {
            ts: OffsetDateTime::parse(ts, &Rfc3339).unwrap(),
            open: o,
            high: h,
            low: l,
            close: c,
            volume: v,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        }
    }

    /// The outage the live recorder sat through is a hole to ask about; neighbours are not,
    /// and a hole the provider already answered empty (the futures daily break) is never
    /// asked again.
    #[test]
    fn interior_holes_skip_neighbours_and_confirmed_breaks() {
        let rows = [
            bar("2026-10-21T20:58:00Z", 1.0, 1.0, 1.0, 1.0, 1.0),
            bar("2026-10-21T20:59:00Z", 1.0, 1.0, 1.0, 1.0, 1.0),
            bar("2026-10-21T22:00:00Z", 1.0, 1.0, 1.0, 1.0, 1.0),
            bar("2026-10-22T07:06:00Z", 1.0, 1.0, 1.0, 1.0, 1.0),
            bar("2026-10-22T10:58:00Z", 1.0, 1.0, 1.0, 1.0, 1.0),
            bar("2026-10-22T10:59:00Z", 1.0, 1.0, 1.0, 1.0, 1.0),
        ];
        let s = |t: &str| OffsetDateTime::parse(t, &Rfc3339).unwrap().unix_timestamp();
        let mut confirmed = std::collections::HashSet::new();
        confirmed.insert((s("2026-10-21T20:59:00Z"), s("2026-10-21T22:00:00Z")));
        assert_eq!(
            interior_holes(&rows, 60, &confirmed),
            vec![
                (s("2026-10-21T22:00:00Z"), s("2026-10-22T07:06:00Z")),
                (s("2026-10-22T07:06:00Z"), s("2026-10-22T10:58:00Z")),
            ]
        );
    }

    #[test]
    fn consolidation_picks_the_coarsest_divisor() {
        let served = ["1m", "5m", "15m", "1h", "4h", "1d", "1w"];
        assert_eq!(consolidation_base(&served, "2m"), Some("1m"));
        assert_eq!(consolidation_base(&served, "7m"), Some("1m"));
        assert_eq!(consolidation_base(&served, "10m"), Some("5m"));
        assert_eq!(consolidation_base(&served, "30m"), Some("15m"));
        assert_eq!(consolidation_base(&served, "2h"), Some("1h"));
        assert_eq!(consolidation_base(&served, "8h"), Some("4h"));
        assert_eq!(consolidation_base(&served, "2w"), Some("1w"));
        assert_eq!(consolidation_base(&served, "3d"), Some("1d"));
        // Served as is, a month from fixed periods, or a non-canonical spelling: no.
        assert_eq!(consolidation_base(&served, "1h"), None);
        assert_eq!(consolidation_base(&served, "3M"), None);
        assert_eq!(consolidation_base(&served, "7min"), None);
    }

    /// A stock's 2h bars open with its session (13:30 UTC), not on the epoch grid; the first
    /// day of the window starts mid-session and borrows the next day's opening time.
    #[test]
    fn intraday_consolidation_is_session_anchored() {
        let h = |d: &str, hh: u32, mm: u32, c: f64| bar(&format!("2024-01-{d}T{hh:02}:{mm:02}:00Z"), c, c + 1.0, c - 1.0, c, 10.0);
        let rows = vec![
            h("02", 14, 30, 1.0),
            h("02", 15, 30, 2.0),
            h("02", 16, 30, 3.0),
            h("03", 13, 30, 4.0),
            h("03", 14, 30, 5.0),
            h("03", 15, 30, 6.0),
        ];
        let tf = crate::timeframe::Timeframe::parse("2h").unwrap();
        let out = consolidate(&rows, tf);
        let ts: Vec<String> = out.iter().map(|b| b.ts.format(&Rfc3339).unwrap()).collect();
        assert_eq!(
            ts,
            [
                "2024-01-02T13:30:00Z",
                "2024-01-02T15:30:00Z",
                "2024-01-03T13:30:00Z",
                "2024-01-03T15:30:00Z"
            ]
        );
        assert_eq!((out[2].open, out[2].close, out[2].high, out[2].low), (4.0, 5.0, 6.0, 3.0));
        assert_eq!(out[2].volume, 20.0);
        // The live fold starts from the newest bucket's own base bars, and only those.
        let tail = newest_bucket_base(&rows, &out);
        let ts: Vec<String> = tail.iter().map(|b| b.ts.format(&Rfc3339).unwrap()).collect();
        assert_eq!(ts, ["2024-01-03T15:30:00Z"]);
        assert!(newest_bucket_base(&rows, &[]).is_empty());
    }
}

#[cfg(test)]
mod quote_stream_tests {
    use super::*;
    use tokio::sync::broadcast;

    fn hub() -> live::hub::LiveHub {
        let pool = sqlx::postgres::PgPoolOptions::new().connect_lazy("postgres://otw@127.0.0.1:1/none").unwrap();
        live::hub::LiveHub::new(pool, reqwest::Client::new(), otw_store::crypto::SecretCipher::from_master("t").unwrap())
    }

    fn kind_and_data(ev: &Event) -> String {
        // `Event` only renders itself; the kind and payload are what the client reads.
        format!("{ev:?}")
    }

    /// A book ticker can fire hundreds of times a second: a viewer gets the first at once,
    /// then at most one per period, always the newest, and the last move is never lost.
    #[tokio::test]
    async fn quotes_are_thinned_to_the_newest_and_the_last_is_kept() {
        let (tx, rx) = broadcast::channel(1024);
        let sub = hub().detached_subscription(rx);
        let mut s = quote_stream(sub, Some(3), "native");
        let first = s.next().await.unwrap().unwrap();
        assert!(kind_and_data(&first).contains("quote_status"), "the status opens the stream");
        let at = OffsetDateTime::now_utc();
        for i in 0..40 {
            let q = live::LiveQuote::new(100.0, 100.01 + i as f64 * 0.01, at).unwrap();
            tx.send(live::hub::LiveMsg::Quote(q)).unwrap();
        }
        let mut got = Vec::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(700);
        while let Ok(Some(Ok(ev))) = tokio::time::timeout_at(deadline, s.next()).await {
            got.push(kind_and_data(&ev));
        }
        assert!(!got.is_empty() && got.len() <= 4, "thinned: {got:?}");
        assert!(got[0].contains("100.01"), "the first goes out at once: {got:?}");
        assert!(got.last().unwrap().contains("100.4"), "the newest is the last sent: {got:?}");
        assert!(got.iter().all(|e| e.contains("event: quote") && e.contains(r#"\"i\":3"#)), "quote events tagged with the pane: {got:?}");
        // The hub closing the channel ends the stream.
        drop(tx);
        assert!(tokio::time::timeout(std::time::Duration::from_secs(1), s.next()).await.unwrap().is_none());
    }
}
