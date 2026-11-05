//! Binance accounts followed live: the user data stream as the book's events
//! ([`AccountEvent`]). The adapter names the socket and reads its frames
//! ([`otw_connector_providers::binance::user_stream`]); this holds the socket.
//!
//! - **One socket per follower.** USDⓈ-M: Binance keeps one listen key per account and a POST
//!   answers the active one, so followers share it; each keeps it alive every [`KEEPALIVE`].
//!   Spot: one WebSocket API subscription per socket.
//! - **The first connection is made before the stream is handed back**, so a refused key is
//!   the account's reason for not being live, not a retry loop nobody sees.
//! - **A gap is said, never papered over.** A reconnect, an expired listen key, a frame that
//!   cannot be read or a hedge-mode position (whose row the book names by both sides) is a
//!   [`AccountEvent::Resync`]: the follower reads the account again. After [`GIVE_UP`] failed
//!   connections in a row the stream ends, and the page goes back to reading on a cycle.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use otw_connector_model::{ConnectorError, Decimal, ErrorKind, InstrumentId, Mapped};
use otw_connector_providers::binance::market::{Kind, Market};
use otw_connector_providers::binance::trade::{self, VenueOrder};
use otw_connector_providers::binance::user_stream::{self, OrderUpdate, PositionUpdate, Update};
use otw_connector_providers::binance::Binance;
use otw_connector_sdk::{Driver, HttpRequest, Signer};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::native::next_id;
use super::{AccountEvent, OpenOrder, Position};

/// A USDⓈ-M listen key lives 60 min: kept alive well before that.
const KEEPALIVE: Duration = Duration::from_secs(30 * 60);
/// Connecting, and on spot the subscription's answer, within this or not at all.
const CONNECT: Duration = Duration::from_secs(15);
/// Silence after which the socket is pinged, and how long it then has to answer.
const IDLE: Duration = Duration::from_secs(60);
const PONG_WAIT: Duration = Duration::from_secs(20);
const TICK: Duration = Duration::from_secs(5);
/// Wait between connection attempts: doubled on each failure, from the first to the last.
const BACKOFF: (Duration, Duration) = (Duration::from_secs(1), Duration::from_secs(60));
/// A connection that lasted this long resets the wait.
const STABLE: Duration = Duration::from_secs(120);
const GIVE_UP: u32 = 10;
/// Events a follower may fall behind by before the socket waits for it.
const CHANNEL: usize = 512;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What one follower needs to keep an account's stream open.
pub(super) struct Follow {
    pub market: &'static Market,
    pub testnet: bool,
    pub driver: Arc<Driver<Binance>>,
    pub signer: Arc<dyn Signer>,
    /// The venue the book names (the same as its order rows).
    pub venue: String,
    /// How the book writes a working order ([`super::binance`]'s `open_order_of`).
    pub row: fn(&VenueOrder, String, String) -> OpenOrder,
}

struct Ctx {
    f: Follow,
    api_key: String,
    account: String,
}

/// An open socket, with what came on it before the subscription was confirmed.
struct Conn {
    ws: Socket,
    early: Vec<String>,
}

/// Follow the account: the stream ends only when its socket cannot be opened again.
pub(super) async fn follow(f: Follow, settings: &HashMap<String, String>) -> Result<futures::stream::BoxStream<'static, AccountEvent>, ConnectorError> {
    let api_key = settings.get("api_key").map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).ok_or_else(|| ConnectorError::new(ErrorKind::Auth, "api_key is not set on this broker account"))?;
    let account = settings.get("account_id").map(|a| a.trim().to_string()).unwrap_or_default();
    let ctx = Arc::new(Ctx { f, api_key, account });
    let first = connect(&ctx).await?;
    let (tx, rx) = mpsc::channel(CHANNEL);
    tokio::spawn(run(ctx, first, tx));
    Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|ev| (ev, rx)) })))
}

/// One call to the account's REST host: the body, or Binance's refusal.
async fn rest(ctx: &Ctx, req: HttpRequest) -> Result<bytes::Bytes, ConnectorError> {
    let m = ctx.f.market;
    let frame = ctx.f.driver.send_as(req, None).await?;
    if let Some(e) = trade::refused(&frame, m.name) {
        return Err(e);
    }
    match frame {
        otw_connector_sdk::RawFrame::Http { body, .. } => Ok(body),
        _ => Err(ConnectorError::new(ErrorKind::Transient, format!("{}: no answer", m.name))),
    }
}

fn lost(m: &Market, what: impl std::fmt::Display) -> ConnectorError {
    ConnectorError::new(ErrorKind::Transient, format!("{} account stream: {what}", m.name))
}

/// Open the socket and subscribe it: USDⓈ-M by the listen key in its path, spot by a signed
/// request whose answer is waited for.
async fn connect(ctx: &Ctx) -> Result<Conn, ConnectorError> {
    let m = ctx.f.market;
    let key = match m.kind {
        Kind::UsdM => Some(user_stream::decode_listen_key(&rest(ctx, user_stream::start(&ctx.api_key, next_id())).await?)?),
        Kind::Spot => None,
    };
    let url = user_stream::socket_url(m, ctx.f.testnet, key.as_deref())?;
    let (mut ws, _) = tokio::time::timeout(CONNECT, connect_async(url.as_str())).await.map_err(|_| lost(m, "no connection")).and_then(|r| r.map_err(|e| lost(m, e)))?;
    let mut early = Vec::new();
    if m.kind == Kind::Spot {
        let id = next_id().0;
        let frame = user_stream::subscribe(id, &ctx.api_key, ctx.f.signer.as_ref(), ctx.f.driver.venue_now())?;
        ws.send(Message::Text(frame)).await.map_err(|e| lost(m, e))?;
        let answered = tokio::time::timeout(CONNECT, async {
            while let Some(msg) = ws.next().await {
                match msg.map_err(|e| lost(m, e))? {
                    Message::Text(t) => {
                        let ups = user_stream::decode(m, t.as_str())?;
                        if let Some(result) = ups.iter().find_map(|u| match u {
                            Update::Answer { id: got, result } if *got == id => Some(result.clone()),
                            _ => None,
                        }) {
                            return result;
                        }
                        early.push(t.to_string());
                    }
                    Message::Ping(p) => ws.send(Message::Pong(p)).await.map_err(|e| lost(m, e))?,
                    Message::Close(_) => return Err(lost(m, "closed before the subscription was answered")),
                    _ => {}
                }
            }
            Err(lost(m, "closed before the subscription was answered"))
        })
        .await;
        answered.map_err(|_| lost(m, "the subscription was not answered"))??;
    }
    Ok(Conn { ws, early })
}

/// Keep the stream open until the follower leaves, connecting again whenever it drops.
async fn run(ctx: Arc<Ctx>, first: Conn, tx: mpsc::Sender<AccountEvent>) {
    let m = ctx.f.market;
    let mut conn = Some(first);
    let (mut wait, mut failed) = (BACKOFF.0, 0u32);
    loop {
        if let Some(c) = conn.take() {
            let opened = tokio::time::Instant::now();
            match pump(&ctx, c, &tx).await {
                None => return,
                Some(why) => tracing::info!("{}: account stream dropped ({why}), connecting again", m.name),
            }
            if opened.elapsed() >= STABLE {
                wait = BACKOFF.0;
            }
        }
        tokio::select! {
            _ = tx.closed() => return,
            _ = tokio::time::sleep(wait) => {}
        }
        match connect(&ctx).await {
            Ok(c) => {
                failed = 0;
                // What happened while the socket was down is only in the book.
                if tx.send(AccountEvent::Resync).await.is_err() {
                    return;
                }
                conn = Some(c);
            }
            Err(e) => {
                failed += 1;
                tracing::warn!("{}: account stream, attempt {failed}: {e}", m.name);
                if failed >= GIVE_UP {
                    return;
                }
                wait = (wait * 2).min(BACKOFF.1);
            }
        }
    }
}

/// Pass one socket's events on. `None`: the follower left. `Some(why)`: the socket is done.
async fn pump(ctx: &Ctx, conn: Conn, tx: &mpsc::Sender<AccountEvent>) -> Option<String> {
    let m = ctx.f.market;
    let (mut write, mut read) = conn.ws.split();
    let mut tried: HashSet<String> = HashSet::new();
    for text in conn.early {
        if let Err(why) = forward(ctx, &text, tx, &mut tried).await {
            return why;
        }
    }
    let mut keep = tokio::time::interval(KEEPALIVE);
    keep.tick().await;
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut heard = tokio::time::Instant::now();
    let mut pinged: Option<tokio::time::Instant> = None;
    loop {
        tokio::select! {
            _ = tx.closed() => return None,
            _ = keep.tick(), if m.kind == Kind::UsdM => match rest(ctx, user_stream::keepalive(&ctx.api_key, next_id())).await {
                Ok(_) => {}
                Err(e) if user_stream::listen_key_gone(&e) => return Some("the listen key is gone".into()),
                Err(e) => tracing::warn!("{}: listen key keepalive: {e}", m.name),
            },
            _ = tick.tick() => {
                if let Some(at) = pinged {
                    if at.elapsed() >= PONG_WAIT {
                        return Some(format!("silent for {}s", heard.elapsed().as_secs()));
                    }
                } else if heard.elapsed() >= IDLE {
                    if write.send(Message::Ping(Vec::new())).await.is_err() {
                        return Some("ping failed".into());
                    }
                    pinged = Some(tokio::time::Instant::now());
                }
            }
            msg = read.next() => {
                if matches!(msg, Some(Ok(_))) {
                    heard = tokio::time::Instant::now();
                    pinged = None;
                }
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Err(why) = forward(ctx, t.as_str(), tx, &mut tried).await {
                            return why;
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if write.send(Message::Pong(p)).await.is_err() {
                            return Some("pong failed".into());
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return Some("closed by Binance".into()),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Some(e.to_string()),
                }
            }
        }
    }
}

/// One frame's events, sent on. `Err(None)`: the follower left; `Err(Some(why))`: the stream
/// ended and the socket is to be opened again.
async fn forward(ctx: &Ctx, text: &str, tx: &mpsc::Sender<AccountEvent>, tried: &mut HashSet<String>) -> Result<(), Option<String>> {
    let m = ctx.f.market;
    let mut out = Vec::new();
    let updates = match user_stream::decode(m, text) {
        Ok(u) => u,
        Err(e) => {
            // It may have been an order's: the book is read again.
            tracing::warn!("{}: account stream frame not read: {e}", m.name);
            out.push(AccountEvent::Resync);
            Vec::new()
        }
    };
    for u in updates {
        match u {
            Update::Order(o) => {
                let currency = currency(ctx, &o.order.r.symbol, false, tried).await;
                out.extend(order_events(*o, currency, &ctx.f.venue, ctx.f.row));
            }
            Update::Position(p) => {
                let currency = currency(ctx, &p.symbol, true, tried).await;
                out.push(position_event(&p, currency, &ctx.f.venue, &ctx.account));
            }
            Update::Ended(why) => {
                for ev in out {
                    tx.send(ev).await.map_err(|_| None)?;
                }
                return Err(Some(why));
            }
            Update::Answer { .. } | Update::Other => {}
        }
    }
    for ev in out {
        tx.send(ev).await.map_err(|_| None)?;
    }
    Ok(())
}

/// The currency the book states a row in, from the instrument list (reloaded once per symbol
/// it lacks). Best effort: a row is still a row without it.
async fn currency(ctx: &Ctx, symbol: &str, settlement: bool, tried: &mut HashSet<String>) -> String {
    let d = &ctx.f.driver;
    let id = InstrumentId::new(ctx.f.market.venue, symbol);
    let read = || {
        d.instruments().ok().and_then(|c| {
            c.get(&id).map(|s| if settlement { s.settlement_ccy.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| s.quote.clone()) } else { s.quote.clone() })
        })
    };
    if let Some(c) = read() {
        return c;
    }
    if tried.insert(symbol.to_string()) {
        if let Err(e) = d.load_instruments(next_id()).await {
            tracing::debug!("{}: instrument list for {symbol}: {e}", ctx.f.market.name);
        }
    }
    read().unwrap_or_default()
}

fn f64_of(d: Decimal) -> f64 {
    d.to_string().parse().unwrap_or(0.0)
}

/// An order event as the book's: a working order whole, a fill on one still working, or the
/// end of it. A state the adapter does not know is a gap.
fn order_events(u: OrderUpdate, currency: String, venue: &str, row: fn(&VenueOrder, String, String) -> OpenOrder) -> Vec<AccountEvent> {
    let id = u.order.r.id();
    match u.order.status {
        // The old half of a spot change: the new order's event follows under the same id.
        Mapped::Known(otw_connector_model::OrderStatus::Cancelled) if super::binance::replaced_half(&u.order.venue_id) => vec![],
        Mapped::Known(st) if st.is_final() => {
            let status = serde_json::to_value(st).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
            vec![AccountEvent::Done { id, status, reason: u.reason }]
        }
        Mapped::Known(_) if u.trade => vec![AccountEvent::Filled { id, filled_qty: f64_of(u.order.filled) }],
        Mapped::Known(_) => {
            let key = Some(u.order.r.client_id.clone());
            vec![AccountEvent::Order { order: row(&u.order, currency, venue.to_string()), key }]
        }
        Mapped::Unknown(_) => vec![AccountEvent::Resync],
    }
}

/// A USDⓈ-M position as the book's. Hedge mode names a symbol held both ways by its side and
/// one held one way by the symbol alone, which one event cannot tell: the book is read again.
fn position_event(p: &PositionUpdate, currency: String, venue: &str, account: &str) -> AccountEvent {
    if p.side != "BOTH" {
        return AccountEvent::Resync;
    }
    let flat = p.amount.is_zero();
    AccountEvent::Position {
        position: Position {
            symbol: p.symbol.clone(),
            side: if p.amount < Decimal::ZERO { "short".into() } else { "long".into() },
            qty: f64_of(p.amount.abs()),
            avg_price: p.entry.filter(|_| !flat).map(f64_of),
            currency,
            asset_class: String::new(),
            multiplier: 1.0,
            venue: venue.to_string(),
            account: account.to_string(),
            native_id: Some(p.symbol.clone()),
            market_price: None,
            unrealized_pnl: p.unrealized.map(f64_of),
            opened_at: None,
            entry_fee: None,
            fee_currency: String::new(),
            leverage: None,
            // `isolated` / `cross`, as the positions read writes `ISOLATED` / `CROSSED`.
            margin_mode: p.margin_type.as_deref().map(str::to_ascii_lowercase).map(|m| if m == "crossed" { "cross".into() } else { m }),
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use otw_connector_providers::binance::market::{SPOT, USDM};

    fn row(o: &VenueOrder, currency: String, venue: String) -> OpenOrder {
        OpenOrder {
            id: o.r.id(),
            symbol: o.r.symbol.clone(),
            side: "buy".into(),
            order_type: o.kind.clone(),
            qty: f64_of(o.qty),
            filled_qty: f64_of(o.filled),
            limit_price: o.limit.map(f64_of),
            stop_price: o.stop.map(f64_of),
            trail_start: None,
            trail_rate: None,
            currency,
            asset_class: "crypto".into(),
            placed_at: None,
            venue,
            account: String::new(),
            parent_id: None,
            reduce_only: false,
        }
    }

    fn update(m: &Market, text: &str) -> Update {
        user_stream::decode(m, text).unwrap().remove(0)
    }

    fn events(m: &Market, text: &str) -> Vec<AccountEvent> {
        match update(m, text) {
            Update::Order(o) => order_events(*o, "BTC".into(), "Binance", row),
            Update::Position(p) => vec![position_event(&p, "USDT".into(), "Binance USDⓈ-M", "")],
            other => panic!("{other:?}"),
        }
    }

    // Source: user-data-stream.md, Order Update payload, trimmed to the fields read.
    const NEW: &str = r#"{"subscriptionId":0,"event":{"e":"executionReport","E":1499405658658,"s":"ETHBTC","c":"mUvoqJxFIILMdfAW5iGSOW","S":"BUY","o":"LIMIT","f":"GTC","q":"1.00000000","p":"0.10264410","P":"0.00000000","g":-1,"C":"","x":"NEW","X":"NEW","r":"NONE","i":4293153,"z":"0.00000000","O":1499405658657}}"#;

    #[test]
    fn a_working_order_is_sent_whole_under_its_book_id() {
        match &events(&SPOT, NEW)[..] {
            [AccountEvent::Order { order, key }] => {
                assert_eq!(order.id, "ETHBTC:mUvoqJxFIILMdfAW5iGSOW");
                assert_eq!((order.limit_price, order.currency.as_str(), order.venue.as_str()), (Some(0.1026441), "BTC", "Binance"));
                assert_eq!(key.as_deref(), Some("mUvoqJxFIILMdfAW5iGSOW"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_fill_moves_the_filled_quantity_and_the_end_drops_the_order() {
        let part = NEW.replace(r#""x":"NEW","X":"NEW""#, r#""x":"TRADE","X":"PARTIALLY_FILLED""#).replace(r#""z":"0.00000000""#, r#""z":"0.25000000""#);
        assert!(matches!(&events(&SPOT, &part)[..], [AccountEvent::Filled { id, filled_qty }] if id == "ETHBTC:mUvoqJxFIILMdfAW5iGSOW" && *filled_qty == 0.25));
        let full = part.replace("PARTIALLY_FILLED", "FILLED");
        assert!(matches!(&events(&SPOT, &full)[..], [AccountEvent::Done { status, reason: None, .. }] if status == "filled"));
        let cancel = NEW.replace(r#""c":"mUvoqJxFIILMdfAW5iGSOW""#, r#""c":"x9""#).replace(r#""C":"""#, r#""C":"mUvoqJxFIILMdfAW5iGSOW""#).replace(r#""x":"NEW","X":"NEW""#, r#""x":"CANCELED","X":"CANCELED""#);
        assert!(matches!(&events(&SPOT, &cancel)[..], [AccountEvent::Done { id, status, .. }] if id == "ETHBTC:mUvoqJxFIILMdfAW5iGSOW" && status == "cancelled"));
        // A change's cancelled half (Binance order number 4293153 being replaced) is not an end.
        super::super::binance::mark_replacing("4293153");
        assert!(events(&SPOT, &cancel).is_empty(), "the old half of a change does not drop the order");
        assert!(matches!(&events(&SPOT, &cancel)[..], [AccountEvent::Done { .. }]), "taken once: a later cancel is one");
        let rejected = NEW.replace(r#""x":"NEW","X":"NEW","r":"NONE""#, r#""x":"REJECTED","X":"REJECTED","r":"WOULD_MATCH_IMMEDIATELY""#);
        assert!(matches!(&events(&SPOT, &rejected)[..], [AccountEvent::Done { status, reason: Some(r), .. }] if status == "rejected" && r.contains("immediately match")));
        let odd = NEW.replace(r#""X":"NEW""#, r#""X":"SOMETHING_NEW""#);
        assert!(matches!(&events(&SPOT, &odd)[..], [AccountEvent::Resync]));
    }

    #[test]
    fn a_futures_stop_is_named_in_the_algo_book() {
        // Source: USDⓈ-M ws-streams schema `algoUpdate`, its example values (status NEW).
        let text = r#"{"e":"ALGO_UPDATE","T":1750515742297,"E":1750515742303,"o":{"caid":"Q5xaq5EGKgXXa0fD7fs0Ip","aid":2148719,"at":"CONDITIONAL","o":"TAKE_PROFIT","s":"BNBUSDT","S":"SELL","ps":"BOTH","f":"GTC","q":"0.01","X":"NEW","ai":"","ap":"0.00000","aq":"0.00000","act":"0","tp":"750","p":"750","V":"EXPIRE_MAKER","wt":"CONTRACT_PRICE","pm":"NONE","cp":false,"pP":false,"R":false,"tt":0,"gtd":0,"rm":"","ia":false}}"#;
        assert!(matches!(&events(&USDM, text)[..], [AccountEvent::Order { order, .. }] if order.id == "BNBUSDT:algo:Q5xaq5EGKgXXa0fD7fs0Ip" && order.stop_price == Some(750.0)));
    }

    #[test]
    fn positions_one_way_are_rows_hedge_mode_is_a_read() {
        // Source: USDⓈ-M ws-streams schema `accountUpdate`, its example values, then a short.
        let text = r#"{"e":"ACCOUNT_UPDATE","E":1564745798939,"T":1564745798938,"a":{"m":"ORDER","B":[],"P":[{"s":"BTCUSDT","pa":"-0.002","ep":"65000.5","bep":"0","cr":"200","up":"-1.5","mt":"isolated","iw":"0.00000000","ps":"BOTH"}]}}"#;
        match &events(&USDM, text)[..] {
            [AccountEvent::Position { position: p }] => {
                assert_eq!((p.symbol.as_str(), p.side.as_str(), p.qty, p.avg_price), ("BTCUSDT", "short", 0.002, Some(65000.5)));
                assert_eq!((p.native_id.as_deref(), p.currency.as_str(), p.unrealized_pnl, p.margin_mode.as_deref()), (Some("BTCUSDT"), "USDT", Some(-1.5), Some("isolated")));
            }
            other => panic!("{other:?}"),
        }
        let flat = text.replace(r#""pa":"-0.002""#, r#""pa":"0""#);
        assert!(matches!(&events(&USDM, &flat)[..], [AccountEvent::Position { position }] if position.qty == 0.0 && position.avg_price.is_none()));
        let hedge = text.replace(r#""ps":"BOTH""#, r#""ps":"SHORT""#);
        assert!(matches!(&events(&USDM, &hedge)[..], [AccountEvent::Resync]));
    }
}
