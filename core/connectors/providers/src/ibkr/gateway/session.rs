//! One connection per IB Gateway / TWS endpoint, and the two things it has to get right:
//! **which client id it takes** and **how fast it may ask**.
//!
//! IB is a single long-lived socket speaking a request/response protocol, so a session is a
//! process-wide resource keyed by `host:port`, shared by the download worker, the chart, the
//! symbol search, the live feed and the derivatives tools.
//!
//! **Client ids.** TWS identifies a connection by a client id and refuses a second one
//! claiming an id in use. Id `0` receives the orders a human placed in TWS and the low ids are
//! what every tutorial grabs, so the user never picks one and that range is never touched: the
//! first free id in a high private band is taken, the next one when TWS says it is in use.
//!
//! **Pacing.** IB's historical-data limit is 60 requests in any rolling ten minutes, counted
//! **per account, not per client id**: one connection, and a rolling window in front of it
//! rather than a fixed delay, so a short backfill runs at full speed and only a long one slows
//! down. This is the transport's own pacing (the socket is the adapter's, not the driver's).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ibapi::Client;
use otw_connector_model::{ConnectorError, ErrorKind};
use tokio::sync::{broadcast, watch, Mutex, OnceCell};

/// First client id taken.
const CLIENT_ID_BASE: i32 = 3100;
/// How many ids to try before giving up.
const CLIENT_ID_SPAN: i32 = 16;
/// A TCP connect that has not answered by now is not going to.
const TCP_TIMEOUT: Duration = Duration::from_secs(5);
/// The API handshake is quick when the API is on; when it is off the socket stays silent,
/// which this bound turns into a diagnosis instead of a hang.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// One historical request should never take this long.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
/// The address the OTW container is pinned to in `deploy/docker-compose.yml`, and therefore
/// the one an IB Gateway allow-list has to name on Docker Engine.
const CONTAINER_IP: &str = "172.28.53.10";
/// IB's rolling historical-data window and how many requests fit in it (60, with a margin).
const PACE_WINDOW: Duration = Duration::from_secs(600);
const PACE_MAX_IN_WINDOW: usize = 55;
/// Floor between two requests: TWS also refuses more than a handful of *identical* requests
/// inside two seconds.
const PACE_MIN_GAP: Duration = Duration::from_millis(1_500);
/// How long a connection has to answer a clock request before it is declared dead.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Order messages a slow reader of [`Session::order_updates`] may fall behind by before it is
/// told it missed some.
const ORDER_UPDATES: usize = 256;

/// A live connection to one endpoint, plus its limiter.
pub struct Session {
    addr: String,
    inner: Mutex<Option<Live>>,
    pace: Mutex<VecDeque<Instant>>,
    link: Arc<watch::Sender<Link>>,
    /// Every order message the connection receives (open order, status, execution), whichever
    /// connection is current: what [`super::events`] follows an account by.
    orders: broadcast::Sender<Arc<ibapi::orders::OrderUpdate>>,
    /// Whether a live account's dry run passed on the current connection (spec 9, E4): cleared
    /// by every new connection, which may be signed in to another account.
    dry_run: AtomicBool,
}

/// IB's own word on its link to its servers, from the system notices TWS sends with no
/// request id. ibapi only logs those, except on the order-update stream, which receives every
/// notice; its order messages go on to [`Session::order_updates`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    /// Set by 1100 (IB lost its servers), cleared by 1101 / 1102 (restored) and by a new
    /// connection.
    pub lost: Option<String>,
    /// Bumped whenever the live subscriptions have to be placed again: 1101 (restored, market
    /// data lost) and 1300 (TWS moved its socket port).
    pub resubscribe: u64,
}

struct Live {
    client: Arc<Client>,
    client_id: i32,
}

static SESSIONS: OnceCell<Mutex<HashMap<String, Arc<Session>>>> = OnceCell::const_new();

async fn registry() -> &'static Mutex<HashMap<String, Arc<Session>>> {
    SESSIONS.get_or_init(|| async { Mutex::new(HashMap::new()) }).await
}

fn invalid(msg: impl Into<String>) -> ConnectorError {
    ConnectorError::invalid(msg)
}

/// The endpoint a connector's settings name.
pub fn address(settings: &HashMap<String, String>) -> Result<String, ConnectorError> {
    let host = settings
        .get("host")
        .map(|h| h.trim())
        .filter(|h| !h.is_empty())
        .ok_or_else(|| invalid("this connector has no IB Gateway host. Set it on the connector, e.g. 127.0.0.1"))?;
    let port = settings
        .get("port")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| invalid("this connector has no IB Gateway port. Use 4001/4002 for the Gateway, 7496/7497 for TWS"))?;
    let port: u16 = port.parse().map_err(|_| invalid(format!("{port:?} is not a port number")))?;
    if port == 0 {
        return Err(invalid("port must be between 1 and 65535"));
    }
    Ok(format!("{host}:{port}"))
}

/// The session for these settings, connecting on first use.
pub async fn get(settings: &HashMap<String, String>) -> Result<Arc<Session>, ConnectorError> {
    let addr = address(settings)?;
    let mut map = registry().await.lock().await;
    Ok(map
        .entry(addr.clone())
        .or_insert_with(|| {
            Arc::new(Session {
                addr,
                inner: Mutex::new(None),
                pace: Mutex::new(VecDeque::new()),
                link: Arc::new(watch::channel(Link::default()).0),
                orders: broadcast::channel(ORDER_UPDATES).0,
                dry_run: AtomicBool::new(false),
            })
        })
        .clone())
}

impl Session {
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// The connection, opening one if there is none, with the id it ended up with.
    pub async fn client(&self) -> Result<(Arc<Client>, i32), ConnectorError> {
        let mut guard = self.inner.lock().await;
        if let Some(live) = guard.as_ref() {
            return Ok((live.client.clone(), live.client_id));
        }
        let (client, client_id) = connect_probing(&self.addr).await?;
        tracing::info!("ibkr: connected to {} as client {client_id}", self.addr);
        // A loss the old connection heard of says nothing about this one.
        self.link.send_modify(|l| l.lost = None);
        self.dry_run.store(false, Ordering::SeqCst);
        listen(&client, self.link.clone(), self.orders.clone(), &self.addr).await;
        *guard = Some(Live { client: client.clone(), client_id });
        Ok((client, client_id))
    }

    /// The current connection, if one is open. Never connects.
    pub async fn current(&self) -> Option<Arc<Client>> {
        self.inner.lock().await.as_ref().map(|l| l.client.clone())
    }

    /// Every order message from here on, on this connection and the ones that replace it.
    /// A reader that falls behind gets `Lagged` and has to read the account again.
    pub fn order_updates(&self) -> broadcast::Receiver<Arc<ibapi::orders::OrderUpdate>> {
        self.orders.subscribe()
    }

    /// Whether the dry run before a live account's first order passed on this connection
    /// ([`super::margin::dry_run`]).
    pub fn dry_run_passed(&self) -> bool {
        self.dry_run.load(Ordering::SeqCst)
    }

    /// Note the dry run passed, for as long as this connection lives.
    pub fn dry_run_done(&self) {
        self.dry_run.store(true, Ordering::SeqCst);
    }

    /// What IB last said about its link to its servers.
    pub fn link(&self) -> watch::Receiver<Link> {
        self.link.subscribe()
    }

    /// Whether the connection still answers. A socket the Gateway stopped serving without
    /// closing it (the machine slept, a NAT forgot the flow) looks alive to every subscription
    /// on it and just goes quiet; a clock request is the cheapest question that tells. Not
    /// paced: it is not a historical request. A dead connection is forgotten so the next caller
    /// opens a fresh one, and the error carries [`super::mapper::DROPPED`].
    pub async fn probe(&self) -> Result<(), ConnectorError> {
        let client = self.inner.lock().await.as_ref().map(|l| l.client.clone());
        let failure = match client {
            Some(c) if c.is_connected() => match tokio::time::timeout(PROBE_TIMEOUT, c.server_time()).await {
                Ok(Ok(_)) => return Ok(()),
                // TWS answered, if only with a refusal: the socket is alive.
                Ok(Err(e)) if !dropped(&e) => return Ok(()),
                Ok(Err(e)) => e.to_string(),
                Err(_) => format!("no answer within {}s", PROBE_TIMEOUT.as_secs()),
            },
            _ => "the connection was closed".to_string(),
        };
        self.reset().await;
        Err(ConnectorError::new(ErrorKind::Transient, format!("Interactive Brokers at {}: {failure}", self.addr)).with_code(super::mapper::DROPPED))
    }

    /// Forget the current connection so the next call opens a fresh one. IB Gateway restarts
    /// itself once a day, so a dropped socket is routine.
    pub async fn reset(&self) {
        if self.inner.lock().await.take().is_some() {
            tracing::info!("ibkr: dropped the connection to {}", self.addr);
        }
    }

    /// Wait until IB's rolling window has room for one more request.
    async fn gate(&self) {
        let mut hist = self.pace.lock().await;
        loop {
            let now = Instant::now();
            while hist.front().is_some_and(|t| now.duration_since(*t) >= PACE_WINDOW) {
                hist.pop_front();
            }
            let window_wait = (hist.len() >= PACE_MAX_IN_WINDOW)
                .then(|| hist.front().map(|t| PACE_WINDOW.saturating_sub(now.duration_since(*t))))
                .flatten()
                .unwrap_or_default();
            let gap_wait = hist.back().map(|t| PACE_MIN_GAP.saturating_sub(now.duration_since(*t))).unwrap_or_default();
            let wait = window_wait.max(gap_wait);
            if wait.is_zero() {
                hist.push_back(now);
                return;
            }
            tokio::time::sleep(wait).await;
        }
    }

    /// One request against the connection: paced, bounded in time, and retried once on a
    /// socket that died between two calls (the daily Gateway restart, or a `reset`).
    pub async fn call<T, F, Fut>(&self, what: &str, run: F) -> Result<T, ConnectorError>
    where
        F: Fn(Arc<Client>) -> Fut,
        Fut: std::future::Future<Output = Result<T, ibapi::Error>>,
    {
        self.request(what, true, run).await
    }

    /// [`Session::call`] for a request IB's historical-data limit does not count: an account
    /// read (positions, open orders) or a contract definition. Sent at once, so following a
    /// working order every few seconds never spends the window the charts' history needs.
    pub async fn ask<T, F, Fut>(&self, what: &str, run: F) -> Result<T, ConnectorError>
    where
        F: Fn(Arc<Client>) -> Fut,
        Fut: std::future::Future<Output = Result<T, ibapi::Error>>,
    {
        self.request(what, false, run).await
    }

    async fn request<T, F, Fut>(&self, what: &str, paced: bool, run: F) -> Result<T, ConnectorError>
    where
        F: Fn(Arc<Client>) -> Fut,
        Fut: std::future::Future<Output = Result<T, ibapi::Error>>,
    {
        for attempt in 0..2 {
            if paced {
                self.gate().await;
            }
            let (client, _) = self.client().await?;
            match tokio::time::timeout(REQUEST_TIMEOUT, run(client)).await {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(e)) if attempt == 0 && dropped(&e) => {
                    self.reset().await;
                    continue;
                }
                Ok(Err(e)) if attempt == 0 && notice(&e) => continue,
                Ok(Err(e)) => return Err(super::mapper::request_error(what, &e)),
                Err(_) => {
                    self.reset().await;
                    return Err(ConnectorError::new(ErrorKind::Transient, format!("{what}: Interactive Brokers did not answer within {}s", REQUEST_TIMEOUT.as_secs())));
                }
            }
        }
        Err(ConnectorError::new(ErrorKind::Transient, format!("{what}: the connection to Interactive Brokers keeps dropping")))
    }
}

/// Follow a connection's system notices and order messages for as long as it lives (its stream
/// ends when the client is dropped). Order messages are passed on as they come; nobody
/// listening is not an error.
async fn listen(client: &Client, link: Arc<watch::Sender<Link>>, orders: broadcast::Sender<Arc<ibapi::orders::OrderUpdate>>, addr: &str) {
    match client.order_update_stream().await {
        Ok(mut notices) => {
            tokio::spawn(async move {
                while let Some(item) = notices.next().await {
                    match item {
                        Ok(ibapi::orders::OrderUpdate::Message(n)) => apply(&link, n.code, &n.message),
                        Ok(update) => {
                            let _ = orders.send(Arc::new(update));
                        }
                        Err(e) if dropped(&e) => return,
                        Err(_) => {}
                    }
                }
            });
        }
        Err(e) => tracing::warn!("ibkr: no system notices from {addr} ({e}): a lost IB link will not show on live charts, nor its orders live on the book"),
    }
}

/// One notice applied to the link. Anything but the connectivity codes is some request's own
/// answer and already reaches that request.
fn apply(link: &watch::Sender<Link>, code: i32, message: &str) {
    match code {
        1100 => link.send_modify(|l| {
            l.lost = Some(format!("Interactive Brokers lost the connection to its servers ({message}). Live data resumes once IB restores it"))
        }),
        1101 => link.send_modify(|l| {
            l.lost = None;
            l.resubscribe += 1;
        }),
        1102 => link.send_modify(|l| l.lost = None),
        1300 => link.send_modify(|l| l.resubscribe += 1),
        _ => return,
    }
    tracing::warn!("ibkr: IB notice {code}: {message}");
}

/// Whether an error means "this socket is gone", as opposed to "TWS said no".
pub fn dropped(e: &ibapi::Error) -> bool {
    matches!(
        e,
        ibapi::Error::Io(_) | ibapi::Error::ConnectionReset | ibapi::Error::ConnectionFailed | ibapi::Error::Shutdown | ibapi::Error::UnexpectedEndOfStream | ibapi::Error::EndOfStream
    )
}

/// A notice IB sends in a request's place instead of an error: 165 ("HMDS server connection
/// was successful") arrives on the first historical request after its data farm reconnects.
/// The client ends the request on it, so the bars it was asked for never come; asking again
/// gets them. Failing on it left the chart's live seed empty after every gateway restart.
pub fn notice(e: &ibapi::Error) -> bool {
    matches!(e, ibapi::Error::Message(165, _))
}

/// Open a connection, walking the id band until one is free.
async fn connect_probing(addr: &str) -> Result<(Arc<Client>, i32), ConnectorError> {
    preflight(addr).await?;
    let mut busy = 0;
    for client_id in CLIENT_ID_BASE..CLIENT_ID_BASE + CLIENT_ID_SPAN {
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, Client::connect(addr, client_id)).await {
            Ok(Ok(client)) => return Ok((Arc::new(client), client_id)),
            // TWS drops the socket on a client id it already handed out; the preflight proved
            // something is listening, so that is what a handshake death means.
            Ok(Err(e)) if id_taken(&e) => {
                busy += 1;
                tracing::debug!("ibkr: client id {client_id} busy on {addr} ({e})");
            }
            Ok(Err(e)) => return Err(explain_connect(addr, &e)),
            Err(_) => {
                return Err(ConnectorError::new(
                    ErrorKind::ProviderDown,
                    format!(
                        "connected to {addr} but Interactive Brokers never completed the API handshake. In TWS / IB Gateway open Global Configuration → API → Settings and tick \"Enable ActiveX and Socket Clients\", then check the port matches."
                    ),
                ))
            }
        }
    }
    Err(ConnectorError::new(
        ErrorKind::ProviderDown,
        format!(
            "every client id between {CLIENT_ID_BASE} and {} is already in use on {addr} ({busy} tried). Close whatever else is connected to this gateway, or restart it.",
            CLIENT_ID_BASE + CLIENT_ID_SPAN - 1
        ),
    ))
}

fn id_taken(e: &ibapi::Error) -> bool {
    match e {
        ibapi::Error::Message(code, _) => *code == 326,
        other => dropped(other),
    }
}

/// A plain TCP connect first, so "the gateway is not running" is never reported as something
/// subtler.
async fn preflight(addr: &str) -> Result<(), ConnectorError> {
    match tokio::time::timeout(TCP_TIMEOUT, tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(ConnectorError::new(
            ErrorKind::ProviderDown,
            format!(
                "nothing is listening on {addr} ({e}). Check that IB Gateway or TWS is running and that the port matches the one it was started with (4001/4002 for the Gateway, 7496/7497 for TWS). From a container the host is host.docker.internal, not localhost."
            ),
        )),
        Err(_) => Err(ConnectorError::new(
            ErrorKind::ProviderDown,
            format!("{addr} did not answer within {}s. A firewall on the machine running IB Gateway is the usual cause.", TCP_TIMEOUT.as_secs()),
        )),
    }
}

/// A handshake failure as the setting the user has to change. The allow-list refusal has no
/// error of its own: TWS accepts the TCP connection then closes it without a word.
fn explain_connect(addr: &str, e: &ibapi::Error) -> ConnectorError {
    if dropped(e) {
        return ConnectorError::new(
            ErrorKind::Permission,
            format!(
                "{addr} accepted the connection then closed it ({e}). In TWS / IB Gateway open Global Configuration → API → Settings and untick \"Allow connections from localhost only\", then add this container to Trusted IPs as {CONTAINER_IP}. Trusted IPs takes single addresses, not a range."
            ),
        );
    }
    ConnectorError::new(ErrorKind::ProviderDown, format!("could not connect to Interactive Brokers on {addr}: {e}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// 165 is the data farm saying hello, not a refusal: asked again, never reported.
    #[test]
    fn the_hmds_notice_is_retried_not_reported() {
        assert!(notice(&ibapi::Error::Message(165, "Historical Market Data Service query message:HMDS server connection was successful.".into())));
        assert!(!notice(&ibapi::Error::Message(162, "pacing violation".into())));
        assert!(!dropped(&ibapi::Error::Message(165, String::new())), "the session itself is fine");
    }

    fn settings(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn address_needs_both_halves_and_a_real_port() {
        assert_eq!(address(&settings(&[("host", "127.0.0.1"), ("port", "4002")])).unwrap(), "127.0.0.1:4002");
        assert_eq!(address(&settings(&[("host", " host.docker.internal "), ("port", " 7496 ")])).unwrap(), "host.docker.internal:7496");
        for bad in [&[("port", "4002")][..], &[("host", "127.0.0.1")], &[("host", "127.0.0.1"), ("port", "0")], &[("host", "127.0.0.1"), ("port", "gateway")]] {
            assert!(address(&settings(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_id_band_never_touches_the_ids_other_tools_take() {
        // 0 is TWS's own connection and 1..99 is what every example uses.
        const { assert!(CLIENT_ID_BASE > 100 && CLIENT_ID_SPAN > 1) };
    }

    #[tokio::test]
    async fn a_gateway_that_is_not_running_is_named_as_such() {
        // A port nothing listens on: the preflight answers before any handshake.
        let s = get(&settings(&[("host", "127.0.0.1"), ("port", "9")])).await.unwrap();
        let e = s.client().await.err().unwrap();
        assert!(e.kind == ErrorKind::ProviderDown && (e.message.contains("nothing is listening") || e.message.contains("did not answer")), "{e}");
    }

    #[test]
    fn connectivity_notices_drive_the_link_and_nothing_else_does() {
        let link = watch::channel(Link::default()).0;
        apply(&link, 2104, "Market data farm connection is OK:usfarm");
        apply(&link, 162, "Historical Market Data Service error message");
        assert_eq!(*link.borrow(), Link::default());
        apply(&link, 1100, "Connectivity between IB and Trader Workstation has been lost.");
        assert!(link.borrow().lost.as_deref().is_some_and(|m| m.contains("lost the connection")));
        apply(&link, 1102, "Connectivity restored - data maintained.");
        assert_eq!(*link.borrow(), Link::default());
        apply(&link, 1100, "lost");
        apply(&link, 1101, "Connectivity restored - data lost.");
        assert_eq!(*link.borrow(), Link { lost: None, resubscribe: 1 });
        apply(&link, 1300, "TWS socket port has been reset");
        assert_eq!(link.borrow().resubscribe, 2);
    }

    #[tokio::test]
    async fn a_probe_without_a_connection_reports_it_dropped() {
        let s = get(&settings(&[("host", "127.0.0.1"), ("port", "9")])).await.unwrap();
        let e = s.probe().await.err().unwrap();
        assert_eq!(e.provider_code.as_deref(), Some(super::super::mapper::DROPPED));
    }
}
