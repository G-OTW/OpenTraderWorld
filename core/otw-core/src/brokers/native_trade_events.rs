//! Accounts of the generic trading broker followed live: the venue's private stream as the
//! book's events ([`AccountEvent`]), through the venue's mapper ([`TradeMapper::private_socket`],
//! [`TradeMapper::decode_private`]) and its signer's socket login. Polling stays the fallback: a
//! venue or a host with no private stream, or a key whose socket login is not documented,
//! answers no stream and the account is read on the reconciliation cycle.
//!
//! - **Connected before it is handed back**: the login and the subscription are answered first,
//!   so a refused key is the account's reason for not being live, not a retry loop.
//! - **Heartbeat** at the venue's interval; silence for two intervals drops the socket.
//! - **A gap is said, never papered over.** A reconnect, a frame that cannot be read, an order
//!   in a state nobody mapped, a position that changed size and an execution that moved a
//!   position without an order (ADL, takeover, delivery) are a [`AccountEvent::Resync`]: the
//!   follower reads the account again. Where the venue numbers its frames on each connection
//!   ([`TradeMapper::private_sequence`], Coinbase), a jump of more than one is a dropped frame
//!   and a `Resync` too (a lower number, out of order, is passed over); elsewhere a gap shows
//!   only as a reconnect.
//! - **Announced maintenance** ([`PrivateEvent::Maintenance`]) is held by the trader, which
//!   refuses new orders with the reason until the stream says it ended; every connection
//!   (a reconnect) clears it.
//! - **Position mode and leverage** are taken from every position frame into the trader's
//!   cache, so neither an order nor the chart reads them.
//! - **Socket variants** are the mapper's: several sockets per account
//!   ([`TradeMapper::private_sockets`], each followed on its own), a signed call before each
//!   connection that hands out the URL and the heartbeat ([`TradeMapper::private_socket_bootstrap`]),
//!   several subscribe frames, each passed through the signer ([`Signer::sign_frame`]) and
//!   answered in turn.
//! - **What a venue may leave out**: binary frames are read like text (MEXC spot protobuf); a
//!   socket with nothing to subscribe is opened by its login alone; a socket with no heartbeat
//!   frame sends none and is still dropped after two silent intervals; a venue that keeps its
//!   socket alive by a signed call on a timer gets it ([`TradeMapper::private_socket_keepalive`]).
//! - **An order frame that is not whole** ([`PrivateEvent::OrderChanged`]) reads the order again
//!   by its venue id, or by its client id where the frame names only that
//!   ([`PrivateEvent::OrderChangedByClient`]); a pushed position mode ([`PrivateEvent::PositionMode`]) is held for the
//!   order path. A minted client id maps back to OTW's key through the trader.
//! - **A nonce key** ([`Signer::serial_key`]) logs its socket in during its turn, so the login's
//!   nonce never crosses a signed REST call's.
//! - Reconnects back off from one second to a minute; after [`GIVE_UP`] failures in a row the
//!   stream ends and the page goes back to reading on a cycle.

#![deny(clippy::float_arithmetic)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use otw_connector_engine::trading::{Account, Trader};
use otw_connector_model::{ConnectorError, Decimal, ErrorKind, InstrumentId, Mapped};
use otw_connector_sdk::trade::{DecodeCtx, PositionSide, PrivateEvent, PrivateSocket, TradeMapper, VenueOrder};
use otw_connector_sdk::{Driver, Signer};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::{AccountEvent, OpenOrder};

const CONNECT: Duration = Duration::from_secs(15);
const BACKOFF: (Duration, Duration) = (Duration::from_secs(1), Duration::from_secs(60));
const STABLE: Duration = Duration::from_secs(120);
pub(super) const GIVE_UP: u32 = 10;
const CHANNEL: usize = 512;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What one follower needs to keep an account's stream open.
pub(super) struct Follow<M: TradeMapper + Clone> {
    /// The book's name, the first part of an order's id (`BOOK:SYMBOL:id`).
    pub book: &'static str,
    pub driver: Arc<Driver<M>>,
    pub signer: Arc<dyn Signer>,
    pub trader: Arc<Trader<M>>,
    pub scope: String,
    pub settings: HashMap<String, String>,
    pub socket: PrivateSocket,
    /// The book's venue label and asset class, as its read rows name them.
    pub venue: String,
    pub asset_class: String,
    /// Whether this follower reads the position modes at connect (the first socket of the
    /// account's book; the others would read the same).
    pub prefill: bool,
}

struct Conn {
    ws: Socket,
    early: Vec<Vec<u8>>,
    /// The highest frame number seen while connecting (answers and early frames alike), where
    /// the venue numbers its frames: what the next frame is counted from.
    seq: Option<u64>,
    /// The socket as connected (completed by its bootstrap, when it has one).
    socket: PrivateSocket,
}

fn lost(what: impl std::fmt::Display) -> ConnectorError {
    ConnectorError::new(ErrorKind::Transient, format!("account stream: {what}"))
}

/// The socket URL: the one the venue handed out, else the base the driver resolves (a test's
/// mock server answers on http).
fn url_of<M: TradeMapper>(d: &Driver<M>, s: &PrivateSocket) -> Result<String, ConnectorError> {
    if let Some(url) = s.url.as_deref().filter(|u| !u.is_empty()) {
        return Ok(url.to_string());
    }
    let base = d.base_url(s.base)?;
    let base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base
    };
    Ok(format!("{base}{}", s.path))
}

/// Follow the account: the stream ends only when its socket cannot be opened again.
pub(super) async fn follow<M: TradeMapper + Clone>(f: Follow<M>) -> Result<futures::stream::BoxStream<'static, AccountEvent>, ConnectorError> {
    let f = Arc::new(f);
    {
        // A book that counts in contracts: the sizes its frames are read with, held for them.
        let a = Account { settings: &f.settings, signer: f.signer.as_ref(), scope: &f.scope };
        if let Err(e) = f.trader.contract_sizes(&f.driver, &a).await {
            tracing::warn!("{}: contract sizes not read at connect ({e}), frames that need them read the book again", f.venue);
        }
    }
    let first = connect(&f).await?;
    prefill(f.clone());
    let (tx, rx) = mpsc::channel(CHANNEL);
    tokio::spawn(run(f, first, tx));
    Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|ev| (ev, rx)) })))
}

/// The position mode of every held instrument, read once at connect (in the background).
fn prefill<M: TradeMapper + Clone>(f: Arc<Follow<M>>) {
    if !f.prefill {
        return;
    }
    tokio::spawn(async move {
        let a = Account { settings: &f.settings, signer: f.signer.as_ref(), scope: &f.scope };
        if let Err(e) = f.trader.prefill_modes(&f.driver, &a).await {
            tracing::warn!("{}: position modes not read at connect, read on first use: {e}", f.venue);
        }
    });
}

/// Wait for the venue's answer to `op`, keeping what else came meanwhile.
async fn answered<M: TradeMapper>(d: &Driver<M>, ctx: &DecodeCtx<'_>, ws: &mut Socket, op: &str, early: &mut Vec<Vec<u8>>, seq: &mut Option<u64>) -> Result<(), ConnectorError> {
    let wait = async {
        while let Some(msg) = ws.next().await {
            let payload: Vec<u8> = match msg.map_err(lost)? {
                Message::Text(t) => t.as_bytes().to_vec(),
                Message::Binary(b) => b.to_vec(),
                Message::Ping(p) => {
                    ws.send(Message::Pong(p)).await.map_err(lost)?;
                    continue;
                }
                Message::Close(_) => return Err(lost(format!("closed before the {op} was answered"))),
                _ => continue,
            };
            if let Some(n) = d.mapper().private_sequence(&payload) {
                *seq = Some(seq.map_or(n, |s| s.max(n)));
            }
            match d.mapper().decode_private_in(ctx, &payload) {
                Ok(evs) => {
                    if let Some(PrivateEvent::Answer { ok, message, .. }) = evs.iter().find(|e| matches!(e, PrivateEvent::Answer { op: o, .. } if o == op)) {
                        if *ok {
                            return Ok(());
                        }
                        let kind = if op == "auth" { ErrorKind::Auth } else { ErrorKind::InvalidRequest };
                        return Err(ConnectorError::new(kind, format!("the venue refused the stream's {op}: {message}")));
                    }
                    early.push(payload);
                }
                Err(_) => early.push(payload),
            }
        }
        Err(lost(format!("closed before the {op} was answered")))
    };
    tokio::time::timeout(CONNECT, wait).await.map_err(|_| lost(format!("the {op} was not answered")))?
}

/// The socket as this connection opens it: completed by the venue's bootstrap call when it has
/// one (asked again on every connection: a token handed out is the connection's).
async fn bootstrap<M: TradeMapper + Clone>(f: &Follow<M>) -> Result<PrivateSocket, ConnectorError> {
    let m = f.driver.mapper();
    let Some(req) = m.private_socket_bootstrap(&f.settings, &f.socket, otw_connector_model::CorrId(0))? else { return Ok(f.socket.clone()) };
    let endpoint = req.endpoint_id;
    let frame = f.driver.send_as(req, Some(f.signer.as_ref())).await?;
    if let Some(e) = m.decode_error(&frame) {
        return Err(e.to_connector());
    }
    match frame {
        otw_connector_sdk::RawFrame::Http { body, .. } => m.decode_private_socket_bootstrap(f.socket.clone(), &body),
        _ => Err(lost(format!("no answer to {endpoint}"))),
    }
}

/// Open the socket, log in, subscribe (each subscribe frame signed by the signer when the venue
/// signs them, and answered before the next).
async fn connect<M: TradeMapper + Clone>(f: &Follow<M>) -> Result<Conn, ConnectorError> {
    let socket = bootstrap(f).await?;
    let url = url_of(&f.driver, &socket)?;
    let (mut ws, _) = tokio::time::timeout(CONNECT, connect_async(url.as_str())).await.map_err(|_| lost("no connection")).and_then(|r| r.map_err(lost))?;
    let (mut early, mut seq) = (Vec::new(), None);
    let a = Account { settings: &f.settings, signer: f.signer.as_ref(), scope: &f.scope };
    let (sizes, names) = (f.trader.held_sizes(&f.driver, &a), f.trader.held_names(&f.driver, &a));
    let ctx = DecodeCtx::new(&socket.path, &sizes).with_names(&names);
    {
        // A nonce key logs in during its turn: its signed REST calls wait meanwhile.
        let _turn = match f.signer.serial_key() {
            Some(k) => Some(otw_connector_sdk::signer::serial_lock(&k).lock_owned().await),
            None => None,
        };
        if let Some(login) = f.signer.ws_login(f.driver.venue_now())? {
            ws.send(Message::Text(String::from_utf8_lossy(&login).into_owned().into())).await.map_err(lost)?;
            answered(&f.driver, &ctx, &mut ws, "auth", &mut early, &mut seq).await?;
        }
    }
    for frame in socket.subscribes() {
        let signed = f.signer.sign_frame(bytes::Bytes::copy_from_slice(frame), f.driver.venue_now())?;
        ws.send(Message::Text(String::from_utf8_lossy(&signed).into_owned().into())).await.map_err(lost)?;
        answered(&f.driver, &ctx, &mut ws, "subscribe", &mut early, &mut seq).await?;
    }
    // A new connection: maintenance announced on the one before is not held over (the stream
    // says it again if it still runs).
    f.trader.set_maintenance(&a, None);
    Ok(Conn { ws, early, seq, socket })
}

async fn run<M: TradeMapper + Clone>(f: Arc<Follow<M>>, first: Conn, tx: mpsc::Sender<AccountEvent>) {
    let mut conn = Some(first);
    let (mut wait, mut failed) = (BACKOFF.0, 0u32);
    let mut sizes: HashMap<(String, PositionSide), Decimal> = HashMap::new();
    loop {
        if let Some(c) = conn.take() {
            let opened = tokio::time::Instant::now();
            match pump(&f, c, &tx, &mut sizes).await {
                None => return,
                Some(why) => tracing::info!("{}: account stream dropped ({why}), connecting again", f.venue),
            }
            if opened.elapsed() >= STABLE {
                wait = BACKOFF.0;
            }
        }
        tokio::select! {
            _ = tx.closed() => return,
            _ = tokio::time::sleep(wait) => {}
        }
        match connect(&f).await {
            Ok(c) => {
                failed = 0;
                // What happened while the socket was down is only in the book.
                if tx.send(AccountEvent::Resync).await.is_err() {
                    return;
                }
                prefill(f.clone());
                conn = Some(c);
            }
            Err(e) => {
                failed += 1;
                tracing::warn!("{}: account stream, attempt {failed}: {e}", f.venue);
                if failed >= GIVE_UP {
                    return;
                }
                wait = (wait * 2).min(BACKOFF.1);
            }
        }
    }
}

/// Pass one socket's events on. `None`: the follower left. `Some(why)`: the socket is done.
async fn pump<M: TradeMapper + Clone>(f: &Follow<M>, conn: Conn, tx: &mpsc::Sender<AccountEvent>, sizes: &mut HashMap<(String, PositionSide), Decimal>) -> Option<String> {
    let socket = conn.socket;
    let mut seq = conn.seq;
    let (mut write, mut read) = conn.ws.split();
    for payload in conn.early {
        if let Err(why) = forward(f, &socket, &payload, tx, sizes, &mut seq).await {
            return why;
        }
    }
    let every = Duration::from_millis(socket.ping_every_ms.max(1_000));
    let mut ping = tokio::time::interval(every);
    ping.tick().await;
    // The venue's keepalive call, on its own timer (a listen key extended).
    let keep = match f.driver.mapper().private_socket_keepalive(&f.settings, &socket, otw_connector_model::CorrId(0)) {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("{}: account stream keepalive not built ({e})", f.venue);
            None
        }
    };
    let mut keepalive = tokio::time::interval(Duration::from_millis(keep.as_ref().map(|(_, ms)| (*ms).max(1_000)).unwrap_or(86_400_000)));
    keepalive.tick().await;
    let mut heard = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = tx.closed() => return None,
            _ = keepalive.tick(), if keep.is_some() => {
                if let Some((req, _)) = keep.as_ref() {
                    keep_alive(f, req.clone()).await;
                }
            }
            _ = ping.tick() => {
                if heard.elapsed() >= every * 2 {
                    return Some(format!("silent for {}s", heard.elapsed().as_secs()));
                }
                // A venue with no client heartbeat: only the silence check runs.
                if !socket.ping.is_empty() && write.send(Message::Text(String::from_utf8_lossy(&socket.ping).into_owned().into())).await.is_err() {
                    return Some("ping failed".into());
                }
            }
            msg = read.next() => {
                if matches!(msg, Some(Ok(_))) {
                    heard = tokio::time::Instant::now();
                }
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Err(why) = forward(f, &socket, t.as_bytes(), tx, sizes, &mut seq).await {
                            return why;
                        }
                    }
                    Some(Ok(Message::Binary(b))) => {
                        if let Err(why) = forward(f, &socket, &b, tx, sizes, &mut seq).await {
                            return why;
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if write.send(Message::Pong(p)).await.is_err() {
                            return Some("pong failed".into());
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return Some("closed by the venue".into()),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Some(e.to_string()),
                }
            }
        }
    }
}

/// The venue's keepalive call: a failure is said, and the socket runs on (it drops by itself if
/// the venue lets it lapse, and is opened again).
async fn keep_alive<M: TradeMapper + Clone>(f: &Follow<M>, req: otw_connector_sdk::HttpRequest) {
    let m = f.driver.mapper();
    match f.driver.send_as(req, Some(f.signer.as_ref())).await {
        Ok(frame) => {
            if let Some(e) = m.decode_error(&frame) {
                tracing::warn!("{}: account stream keepalive refused: {e}", f.venue);
            }
        }
        Err(e) => tracing::warn!("{}: account stream keepalive not sent: {e}", f.venue),
    }
}

/// Whether frame number `n` comes after a gap: more than one past the last one counted on this
/// connection (`last`, moved on to `n`). A lower or equal number (out of order, or repeated)
/// is passed over and leaves `last` where it was.
pub(super) fn sequence_gap(last: &mut Option<u64>, n: u64) -> bool {
    match *last {
        Some(l) if n <= l => false,
        Some(l) => {
            *last = Some(n);
            n > l.saturating_add(1)
        }
        None => {
            *last = Some(n);
            false
        }
    }
}

/// One frame's events, sent on. `Err(None)`: the follower left; `Err(Some(why))`: the socket is
/// to be opened again. `seq`: the last frame number counted on this connection, where the
/// venue numbers its frames.
async fn forward<M: TradeMapper + Clone>(
    f: &Follow<M>,
    socket: &PrivateSocket,
    payload: &[u8],
    tx: &mpsc::Sender<AccountEvent>,
    sizes: &mut HashMap<(String, PositionSide), Decimal>,
    seq: &mut Option<u64>,
) -> Result<(), Option<String>> {
    let a = Account { settings: &f.settings, signer: f.signer.as_ref(), scope: &f.scope };
    let mut out = Vec::new();
    if let Some(n) = f.driver.mapper().private_sequence(payload) {
        let before = seq.unwrap_or_default();
        if sequence_gap(seq, n) {
            // A frame was dropped (the venue's numbering jumped): it may have been an order's.
            tracing::warn!("{}: account stream skipped frames (number {n} after {before}), reading the book again", f.venue);
            out.push(AccountEvent::Resync);
        }
    }
    let contract_sizes = f.trader.held_sizes(&f.driver, &a);
    let names = f.trader.held_names(&f.driver, &a);
    let events = match f.driver.mapper().decode_private_in(&DecodeCtx::new(&socket.path, &contract_sizes).with_names(&names), payload) {
        Ok(e) => e,
        Err(e) => {
            // It may have been an order's: the book is read again.
            tracing::warn!("{}: account stream frame not read: {e}", f.venue);
            out.push(AccountEvent::Resync);
            vec![]
        }
    };
    for ev in events {
        match ev {
            PrivateEvent::Order(o) => out.push(order_event(f, &o)),
            // Not the whole order: read again by its venue id; unread, the book is.
            PrivateEvent::OrderChanged { symbol, venue_id } => match f.trader.by_venue_id(&f.driver, &a, symbol.as_deref().unwrap_or(""), &venue_id).await {
                Ok(Some(o)) => out.push(order_event(f, &o)),
                Ok(None) | Err(_) => out.push(AccountEvent::Resync),
            },
            // Named by its client id only: read again by it; unread, the book is.
            PrivateEvent::OrderChangedByClient { symbol, client_id } => {
                let found = match (symbol.as_deref().filter(|s| !s.is_empty()), f.trader.otw_key(&f.driver, &a, &client_id)) {
                    (Some(sym), Some(key)) => f.trader.lookup(&f.driver, &a, sym, &key).await.ok().flatten(),
                    _ => None,
                };
                out.push(match found {
                    Some(o) => order_event(f, &o),
                    None => AccountEvent::Resync,
                });
            }
            PrivateEvent::Maintenance { on, message } => f.trader.set_maintenance(&a, on.then_some(message)),
            PrivateEvent::PositionMode { symbol, mode } => f.trader.mode_pushed(&a, symbol.as_deref(), mode),
            // A fill moves its order, whose own frame says how far; an execution with no order
            // of OTW behind it (ADL, takeover, delivery) moved a position: read the book.
            PrivateEvent::Fill(x) if x.position_only => out.push(AccountEvent::Resync),
            PrivateEvent::Fill(_) => {}
            PrivateEvent::Position(p) => {
                // Position mode and leverage, held for the order path and the chart.
                f.trader.set_position(&f.driver, &a, &p);
                let key = (p.symbol.clone(), p.side);
                let changed = sizes.insert(key, p.qty).is_some_and(|before| before != p.qty);
                if changed {
                    out.push(AccountEvent::Resync);
                }
            }
            PrivateEvent::Answer { op, ok: false, message } => return Err(Some(format!("the venue refused {op}: {message}"))),
            PrivateEvent::Answer { .. } => {}
        }
    }
    for ev in out {
        tx.send(ev).await.map_err(|_| None)?;
    }
    Ok(())
}

fn f64_of(d: Decimal) -> f64 {
    d.to_string().parse().unwrap_or_default()
}

/// A streamed order as the book writes it: working, or done.
fn order_event<M: TradeMapper + Clone>(f: &Follow<M>, o: &VenueOrder) -> AccountEvent {
    let id = format!("{}:{}:{}", f.book, o.symbol, o.venue_id);
    match o.status {
        Mapped::Known(st) if st.is_final() => AccountEvent::Done { id, status: super::reconcile::word(st).to_string(), reason: None },
        Mapped::Known(_) => {
            let m = f.driver.mapper();
            let currency = f
                .driver
                .instruments()
                .ok()
                .and_then(|c| c.get(&InstrumentId::new(&m.capabilities().venue, &o.symbol)).map(|s| s.quote.clone()))
                .unwrap_or_default();
            let order = OpenOrder {
                id,
                symbol: o.symbol.clone(),
                side: match o.side {
                    Mapped::Known(otw_connector_model::Side::Buy) => "buy".into(),
                    Mapped::Known(otw_connector_model::Side::Sell) => "sell".into(),
                    Mapped::Unknown(ref w) => w.to_ascii_lowercase(),
                },
                order_type: o.order_type.clone(),
                qty: f64_of(o.qty),
                filled_qty: f64_of(o.filled_qty),
                limit_price: o.limit.map(f64_of),
                stop_price: o.stop.map(f64_of),
                trail_start: None,
                trail_rate: None,
                currency,
                asset_class: f.asset_class.clone(),
                placed_at: o.created_ms.and_then(|ms| time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).ok()),
                venue: f.venue.clone(),
                account: String::new(),
                parent_id: None,
                reduce_only: o.reduce_only,
            };
            let a = Account { settings: &f.settings, signer: f.signer.as_ref(), scope: &f.scope };
            AccountEvent::Order { order, key: o.client_id.as_deref().and_then(|c| f.trader.otw_key(&f.driver, &a, c)) }
        }
        // A state nobody mapped: the book is read again.
        Mapped::Unknown(ref w) => {
            tracing::warn!("{}: order {} streamed in a state OTW does not know ({w})", f.venue, o.venue_id);
            AccountEvent::Resync
        }
    }
}
