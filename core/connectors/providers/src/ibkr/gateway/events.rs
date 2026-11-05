//! The account followed live: one feed per gateway, however many pages follow it.
//!
//! IB pushes what changes on an account, it does not have to be asked again. Order messages
//! (open order, status, execution) already reach the session on its order-update stream
//! ([`Session::order_updates`]); positions come from `reqPositions`, which replays the
//! account's positions once and then sends each change as it happens. The feed holds that
//! subscription open while someone follows the account, and fans both out on one broadcast
//! channel: N pages cost IB one subscription, not N reads every few seconds.
//!
//! **One `reqPositions` per connection.** IB keeps a single positions subscription per
//! client, and cancelling any reader cancels it for all. So nothing else asks for positions
//! on its own: a caller that needs what is held right now ([`held`], the close of a
//! position) reads the feed's copy, starting the feed when it is not running.
//!
//! **A gap is said, never papered over.** A new connection, a positions stream that ended, or
//! a reader that fell behind is a [`Event::Resync`]: the follower reads the account again.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ibapi::accounts::{Position, PositionUpdate};
use ibapi::orders::OrderUpdate;
use ibapi::Client;
use otw_connector_model::{ConnectorError, ErrorKind};
use tokio::sync::{broadcast, watch, Mutex, OnceCell};

use super::session::{dropped, Session};

/// Events a follower may fall behind by before it is told to read the account again.
const CHANNEL: usize = 512;
/// A feed nobody followed for this long is closed, and its positions subscription with it.
const IDLE: Duration = Duration::from_secs(60);
/// How often a feed checks that it still has followers and that its connection is current.
const TICK: Duration = Duration::from_secs(5);
/// Wait before subscribing again after the connection failed.
const RETRY: Duration = Duration::from_secs(5);
/// How long [`held`] waits for IB's first list of positions.
const FIRST_LIST: Duration = Duration::from_secs(10);

/// What a follower is told.
#[derive(Debug, Clone)]
pub enum Event {
    /// An order message as IB sent it: an open order, a status, an execution.
    Order(Arc<OrderUpdate>),
    /// A position as it now stands. Zero means flat.
    Position(Arc<Position>),
    /// Events may have been missed: read the account again.
    Resync,
}

/// The positions IB listed last, by `(account, contract id)`. `None` until the first list.
type Held = Option<Arc<HashMap<(String, i32), Position>>>;

struct Feed {
    tx: broadcast::Sender<Event>,
    held: watch::Receiver<Held>,
    wanted: Arc<std::sync::Mutex<Instant>>,
    task: tokio::task::JoinHandle<()>,
}

/// Running feeds by gateway address.
static FEEDS: OnceCell<Mutex<HashMap<String, Feed>>> = OnceCell::const_new();

async fn feeds() -> &'static Mutex<HashMap<String, Feed>> {
    FEEDS.get_or_init(|| async { Mutex::new(HashMap::new()) }).await
}

/// The feed of this gateway, started when none runs, and marked as wanted now.
async fn feed<T>(s: &Arc<Session>, take: impl FnOnce(&Feed) -> T) -> T {
    let mut map = feeds().await.lock().await;
    let f = match map.entry(s.addr().to_string()) {
        Entry::Occupied(e) if !e.get().task.is_finished() => e.into_mut(),
        e => {
            let (tx, _) = broadcast::channel(CHANNEL);
            let (held_tx, held) = watch::channel(None);
            let wanted = Arc::new(std::sync::Mutex::new(Instant::now()));
            let task = tokio::spawn(run(s.clone(), tx.clone(), held_tx, wanted.clone()));
            let feed = Feed { tx, held, wanted, task };
            match e {
                Entry::Occupied(mut o) => {
                    o.insert(feed);
                    o.into_mut()
                }
                Entry::Vacant(v) => v.insert(feed),
            }
        }
    };
    if let Ok(mut w) = f.wanted.lock() {
        *w = Instant::now();
    }
    take(f)
}

/// Follow the account from here on. The receiver sees `Closed` only if the feed stopped,
/// which it does not while someone holds a receiver.
pub async fn follow(s: &Arc<Session>) -> broadcast::Receiver<Event> {
    feed(s, |f| f.tx.subscribe()).await
}

/// What the account holds right now, from the feed's copy: IB's positions as of their last
/// change, flat ones included.
pub async fn held(s: &Arc<Session>) -> Result<Vec<Position>, ConnectorError> {
    let mut rx = feed(s, |f| f.held.clone()).await;
    let first = tokio::time::timeout(FIRST_LIST, rx.wait_for(|h| h.is_some())).await;
    match first {
        Ok(Ok(h)) => Ok(h.as_ref().map(|m| m.values().cloned().collect()).unwrap_or_default()),
        Ok(Err(_)) => Err(ConnectorError::new(ErrorKind::Transient, "the positions feed of Interactive Brokers stopped")),
        Err(_) => Err(ConnectorError::new(
            ErrorKind::Transient,
            format!("Interactive Brokers did not list the account's positions within {}s", FIRST_LIST.as_secs()),
        )),
    }
}

/// One feed: subscribe, pass on, subscribe again whenever the connection changes, until
/// nobody wants it.
async fn run(s: Arc<Session>, tx: broadcast::Sender<Event>, held: watch::Sender<Held>, wanted: Arc<std::sync::Mutex<Instant>>) {
    let idle = || {
        if tx.receiver_count() > 0 {
            if let Ok(mut w) = wanted.lock() {
                *w = Instant::now();
            }
            return false;
        }
        wanted.lock().map(|w| w.elapsed() >= IDLE).unwrap_or(true)
    };
    let mut first = true;
    loop {
        if idle() {
            return;
        }
        let client = match s.client().await {
            Ok((c, _)) => c,
            Err(e) => {
                tracing::warn!("ibkr: account feed of {}: {e}", s.addr());
                tokio::time::sleep(RETRY).await;
                continue;
            }
        };
        // Subscribed before positions are asked, so no order message falls between the two.
        let orders = s.order_updates();
        let positions = match client.positions().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("ibkr: no positions feed from {}: {e}", s.addr());
                tokio::time::sleep(RETRY).await;
                continue;
            }
        };
        if !first {
            let _ = tx.send(Event::Resync);
        }
        first = false;
        if follow_connection(&s, &client, orders, positions, &tx, &held, &idle).await {
            return;
        }
    }
}

/// What a change of IB's link to its servers asks of the feed (spec 7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkChange {
    /// 1101 (restored, data lost) or 1300 (socket port moved): subscribe again, then read the
    /// account again.
    Resubscribe,
    /// 1102 (restored, data kept): read the account again.
    Resync,
}

/// The feed's answer to IB's link going from `was` to `now`. A loss (1100) asks nothing of the
/// feed: orders are refused meanwhile ([`super::orders::check_execution`]).
pub fn after_link(was: &super::session::Link, now: &super::session::Link) -> Option<LinkChange> {
    if now.resubscribe != was.resubscribe {
        Some(LinkChange::Resubscribe)
    } else if was.lost.is_some() && now.lost.is_none() {
        Some(LinkChange::Resync)
    } else {
        None
    }
}

/// Pass one connection's events on until it is replaced or drops (`false`: subscribe again)
/// or nobody wants the feed any more (`true`).
async fn follow_connection(
    s: &Session,
    client: &Arc<Client>,
    mut orders: broadcast::Receiver<Arc<OrderUpdate>>,
    mut positions: ibapi::subscriptions::Subscription<PositionUpdate>,
    tx: &broadcast::Sender<Event>,
    held: &watch::Sender<Held>,
    idle: &impl Fn() -> bool,
) -> bool {
    let mut list: HashMap<(String, i32), Position> = HashMap::new();
    let mut listed = false;
    let mut tick = tokio::time::interval(TICK);
    tick.tick().await;
    let mut link = s.link();
    let mut was = link.borrow_and_update().clone();
    loop {
        tokio::select! {
            Ok(()) = link.changed() => {
                let now = link.borrow_and_update().clone();
                match after_link(&was, &now) {
                    Some(LinkChange::Resubscribe) => {
                        held.send_replace(None);
                        return false;
                    }
                    Some(LinkChange::Resync) => {
                        let _ = tx.send(Event::Resync);
                    }
                    None => {}
                }
                was = now;
            },
            update = orders.recv() => match update {
                Ok(u) => {
                    let _ = tx.send(Event::Order(u));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let _ = tx.send(Event::Resync);
                }
                Err(broadcast::error::RecvError::Closed) => return false,
            },
            item = positions.next() => match item {
                Some(Ok(PositionUpdate::Position(p))) => {
                    let key = (p.account.clone(), p.contract.contract_id);
                    list.insert(key, p.clone());
                    // The first list is a replay of what the follower reads anyway.
                    if listed {
                        held.send_replace(Some(Arc::new(list.clone())));
                        let _ = tx.send(Event::Position(Arc::new(p)));
                    }
                }
                Some(Ok(PositionUpdate::PositionEnd)) => {
                    listed = true;
                    held.send_replace(Some(Arc::new(list.clone())));
                }
                Some(Err(e)) if !dropped(&e) => tracing::warn!("ibkr: positions feed of {}: {e}", s.addr()),
                _ => {
                    held.send_replace(None);
                    return false;
                }
            },
            _ = tick.tick() => {
                if idle() {
                    return true;
                }
                // The session moved to a new connection (a reset, the daily restart): this
                // subscription lives on the old one.
                if !s.current().await.is_some_and(|c| Arc::ptr_eq(&c, client)) {
                    held.send_replace(None);
                    return false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ibkr::gateway::session::Link;

    #[test]
    fn a_restored_link_reads_the_account_again() {
        let up = Link::default();
        let lost = Link { lost: Some("1100".into()), ..Link::default() };
        // 1100: nothing for the feed, orders are refused meanwhile.
        assert_eq!(after_link(&up, &lost), None);
        // 1102, data kept: read again.
        assert_eq!(after_link(&lost, &up), Some(LinkChange::Resync));
        // 1101, data lost: subscribe again (and the new subscription reads again).
        assert_eq!(after_link(&lost, &Link { lost: None, resubscribe: 1 }), Some(LinkChange::Resubscribe));
        assert_eq!(after_link(&up, &up), None);
    }
}
