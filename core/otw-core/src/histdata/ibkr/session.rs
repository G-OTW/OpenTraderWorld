//! One connection per IB Gateway / TWS endpoint, and the two things it has to get right:
//! **which client id we take** and **how fast we are allowed to ask**.
//!
//! Every other provider here is a stateless HTTP call. IB is a single long-lived socket
//! that speaks a request/response protocol, so a session is a process-wide resource keyed
//! by `host:port`, shared by the download worker, the chart preview and the symbol search.
//!
//! **Client ids.** TWS identifies a connection by a client id, and a second connection
//! claiming an id already in use is refused. Id `0` is special (it receives the orders a
//! human placed by hand in TWS) and the low ids are what every tutorial and third-party
//! tool grabs, so we never let the user pick one and never touch that range: we take the
//! first free id in a high private band, and if TWS says it is taken we move to the next.
//! Nothing the user runs alongside us gets kicked off its own connection.
//!
//! **Pacing.** IB's historical-data limit is 60 requests in any rolling ten minutes,
//! counted **per account, not per client id**: opening a second connection buys nothing
//! and only burns another id. So there is one connection, and the limiter below is a
//! rolling window in front of it rather than a fixed delay: a short backfill runs at full
//! speed and only a long one slows down.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use ibapi::Client;
use tokio::sync::{Mutex, OnceCell};

/// First client id we are willing to take.
const CLIENT_ID_BASE: i32 = 3100;
/// How many ids to try before giving up.
const CLIENT_ID_SPAN: i32 = 16;

/// A TCP connect that has not answered by now is not going to.
const TCP_TIMEOUT: Duration = Duration::from_secs(5);
/// The API handshake (version exchange, then the account snapshot TWS pushes) is quick
/// when the API is on; when it is off the socket simply stays silent, which is the case
/// this bound turns into a diagnosis instead of a hang.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// One historical request should never take this long.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

/// The address this container is pinned to in `deploy/docker-compose.yml`, and therefore
/// the one an IB Gateway allow-list has to name on Docker Engine. Quoted in the error the
/// user actually reads, so the two are kept together on purpose.
const CONTAINER_IP: &str = "172.28.53.10";

/// IB's rolling historical-data window and how many requests fit in it.
const PACE_WINDOW: Duration = Duration::from_secs(600);
const PACE_MAX_IN_WINDOW: usize = 55;
/// Floor between two requests, whatever the window says: TWS also refuses more than a
/// handful of *identical* requests inside two seconds.
const PACE_MIN_GAP: Duration = Duration::from_millis(1_500);

/// A live connection to one endpoint, plus its limiter.
pub struct Session {
    addr: String,
    inner: Mutex<Option<Live>>,
    pace: Mutex<VecDeque<Instant>>,
}

struct Live {
    client: Arc<Client>,
    client_id: i32,
}

/// Sessions by `host:port`. A `OnceCell` rather than a `LazyLock` because the map behind
/// it is an async mutex.
static SESSIONS: OnceCell<Mutex<HashMap<String, Arc<Session>>>> = OnceCell::const_new();

async fn registry() -> &'static Mutex<HashMap<String, Arc<Session>>> {
    SESSIONS.get_or_init(|| async { Mutex::new(HashMap::new()) }).await
}

/// The endpoint a connector's settings name.
pub fn address(settings: &HashMap<String, String>) -> Result<String> {
    let host = settings
        .get("host")
        .map(|h| h.trim())
        .filter(|h| !h.is_empty())
        .ok_or_else(|| {
            anyhow!("this connector has no IB Gateway host. Set it on the connector, e.g. 127.0.0.1")
        })?;
    let port = settings
        .get("port")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| {
            anyhow!("this connector has no IB Gateway port. Use 4001/4002 for the Gateway, 7496/7497 for TWS")
        })?;
    let port: u16 = port
        .parse()
        .map_err(|_| anyhow!("{port:?} is not a port number"))?;
    if port == 0 {
        return Err(anyhow!("port must be between 1 and 65535"));
    }
    Ok(format!("{host}:{port}"))
}

/// The session for these settings, connecting on first use.
pub async fn get(settings: &HashMap<String, String>) -> Result<Arc<Session>> {
    let addr = address(settings)?;
    let mut map = registry().await.lock().await;
    Ok(map
        .entry(addr.clone())
        .or_insert_with(|| {
            Arc::new(Session {
                addr,
                inner: Mutex::new(None),
                pace: Mutex::new(VecDeque::new()),
            })
        })
        .clone())
}

impl Session {
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// The connection, opening one if there is none. The id we ended up with is returned
    /// alongside so a diagnostic can name it.
    pub async fn client(&self) -> Result<(Arc<Client>, i32)> {
        let mut guard = self.inner.lock().await;
        if let Some(live) = guard.as_ref() {
            return Ok((live.client.clone(), live.client_id));
        }
        let (client, client_id) = connect_probing(&self.addr).await?;
        tracing::info!("ibkr: connected to {} as client {client_id}", self.addr);
        *guard = Some(Live { client: client.clone(), client_id });
        Ok((client, client_id))
    }

    /// Forget the current connection so the next call opens a fresh one. IB Gateway
    /// restarts itself once a day, so a dropped socket is routine, not an incident.
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
            let gap_wait = hist
                .back()
                .map(|t| PACE_MIN_GAP.saturating_sub(now.duration_since(*t)))
                .unwrap_or_default();
            let wait = window_wait.max(gap_wait);
            if wait.is_zero() {
                hist.push_back(now);
                return;
            }
            tokio::time::sleep(wait).await;
        }
    }

    /// Run one request against the connection, paced, bounded in time, and retried once on
    /// a socket that died between two calls (the daily Gateway restart, or a `reset`).
    pub async fn call<T, F, Fut>(&self, what: &str, run: F) -> Result<T>
    where
        F: Fn(Arc<Client>) -> Fut,
        Fut: std::future::Future<Output = Result<T, ibapi::Error>>,
    {
        for attempt in 0..2 {
            self.gate().await;
            let (client, _) = self.client().await?;
            match tokio::time::timeout(REQUEST_TIMEOUT, run(client)).await {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(e)) if attempt == 0 && dropped(&e) => {
                    self.reset().await;
                    continue;
                }
                Ok(Err(e)) => return Err(explain_request(what, e)),
                Err(_) => {
                    self.reset().await;
                    return Err(anyhow!(
                        "{what}: Interactive Brokers did not answer within {}s",
                        REQUEST_TIMEOUT.as_secs()
                    ));
                }
            }
        }
        Err(anyhow!("{what}: the connection to Interactive Brokers keeps dropping"))
    }
}

/// Whether an error means "this socket is gone", as opposed to "TWS said no". Public to the
/// crate because the live feed holds its subscription open across the daily Gateway restart
/// and has to tell that apart from a refusal it should stop retrying.
pub(crate) fn dropped(e: &ibapi::Error) -> bool {
    matches!(
        e,
        ibapi::Error::Io(_)
            | ibapi::Error::ConnectionReset
            | ibapi::Error::ConnectionFailed
            | ibapi::Error::Shutdown
            | ibapi::Error::UnexpectedEndOfStream
            | ibapi::Error::EndOfStream
    )
}

/// Open a connection, walking the id band until one is free.
async fn connect_probing(addr: &str) -> Result<(Arc<Client>, i32)> {
    preflight(addr).await?;
    let mut busy = 0;
    for client_id in CLIENT_ID_BASE..CLIENT_ID_BASE + CLIENT_ID_SPAN {
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, Client::connect(addr, client_id)).await {
            Ok(Ok(client)) => return Ok((Arc::new(client), client_id)),
            // TWS drops the socket on a client id it has already handed out. The
            // preflight above proved something is listening, so a socket that dies
            // during the handshake is that, and the answer is the next id.
            Ok(Err(e)) if id_taken(&e) => {
                busy += 1;
                tracing::debug!("ibkr: client id {client_id} busy on {addr} ({e})");
            }
            Ok(Err(e)) => return Err(explain_connect(addr, e)),
            Err(_) => {
                return Err(anyhow!(
                    "connected to {addr} but Interactive Brokers never completed the API handshake. \
                     In TWS / IB Gateway open Global Configuration → API → Settings and tick \
                     \"Enable ActiveX and Socket Clients\", then check the port matches."
                ))
            }
        }
    }
    Err(anyhow!(
        "every client id between {CLIENT_ID_BASE} and {} is already in use on {addr} ({busy} tried). \
         Close whatever else is connected to this gateway, or restart it.",
        CLIENT_ID_BASE + CLIENT_ID_SPAN - 1
    ))
}

fn id_taken(e: &ibapi::Error) -> bool {
    match e {
        ibapi::Error::Message(code, _) => *code == 326,
        other => dropped(other),
    }
}

/// A plain TCP connect first, so "the gateway is not running" is never reported as
/// something subtler.
async fn preflight(addr: &str) -> Result<()> {
    match tokio::time::timeout(TCP_TIMEOUT, tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(anyhow!(
            "nothing is listening on {addr} ({e}). Check that IB Gateway or TWS is running and \
             that the port matches the one it was started with (4001/4002 for the Gateway, \
             7496/7497 for TWS). From a container the host is host.docker.internal, not localhost."
        )),
        Err(_) => Err(anyhow!(
            "{addr} did not answer within {}s. A firewall on the machine running IB Gateway is \
             the usual cause.",
            TCP_TIMEOUT.as_secs()
        )),
    }
}

/// Turn a handshake failure into the setting the user has to change.
///
/// The allow-list refusal has no error of its own: TWS accepts the TCP connection and then
/// closes it without a word, which surfaces as a plain transport error. Since the preflight
/// already proved something is listening, that shape means the caller was refused.
fn explain_connect(addr: &str, e: ibapi::Error) -> anyhow::Error {
    if dropped(&e) {
        return anyhow!(
            "{addr} accepted the connection then closed it ({e}). In TWS / IB Gateway open \
             Global Configuration → API → Settings and untick \"Allow connections from localhost \
             only\", then add this container to Trusted IPs as {CONTAINER_IP}. Trusted IPs takes \
             single addresses, not a range."
        );
    }
    anyhow!("could not connect to Interactive Brokers on {addr}: {e}")
}

/// Name the common per-request refusals, which are account settings rather than bugs.
/// Whether a failed request means "IB knows no such contract" (error 200), as opposed to a
/// refusal worth reporting. A symbol search types a character at a time, so most of its
/// queries name nothing yet: those are an empty list, not a warning under the box.
pub(crate) fn unknown_contract(e: &anyhow::Error) -> bool {
    format!("{e:#}").contains(UNKNOWN_CONTRACT)
}

/// The sentence code 200 is turned into, shared with [`unknown_contract`] so the two cannot
/// drift apart.
const UNKNOWN_CONTRACT: &str = "has no contract matching this symbol";

fn explain_request(what: &str, e: ibapi::Error) -> anyhow::Error {
    let ibapi::Error::Message(code, message) = &e else {
        return anyhow!("{what}: {e}");
    };
    match *code {
        162 => anyhow!(
            "{what}: Interactive Brokers refused the request ({}). Code 162 covers both a pacing \
             violation and a symbol with no data in the window asked for.",
            message
        ),
        200 => anyhow!(
            "{what}: Interactive Brokers {UNKNOWN_CONTRACT} ({}). Check the \
             spelling, and add the currency for a non-US instrument, e.g. SAN:EUR.",
            message
        ),
        354 | 10089 | 10090 => anyhow!(
            "{what}: this Interactive Brokers account has no market-data subscription for the \
             instrument ({}). Historical data needs the same subscription as live data.",
            message
        ),
        10197 | 10167 => anyhow!("{what}: no data permission or no data available ({message})"),
        other => anyhow!("{what}: Interactive Brokers error {other}: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn address_needs_both_halves_and_a_real_port() {
        assert_eq!(
            address(&settings(&[("host", "127.0.0.1"), ("port", "4002")])).unwrap(),
            "127.0.0.1:4002"
        );
        // Whitespace from a form field is not an address.
        assert_eq!(
            address(&settings(&[("host", " host.docker.internal "), ("port", " 7496 ")])).unwrap(),
            "host.docker.internal:7496"
        );
        assert!(address(&settings(&[("port", "4002")])).is_err());
        assert!(address(&settings(&[("host", "127.0.0.1")])).is_err());
        assert!(address(&settings(&[("host", "127.0.0.1"), ("port", "0")])).is_err());
        assert!(address(&settings(&[("host", "127.0.0.1"), ("port", "gateway")])).is_err());
    }

    #[test]
    fn the_id_band_never_touches_the_ids_other_tools_take() {
        // 0 is TWS's own connection and 1..99 is what every example uses.
        assert!(CLIENT_ID_BASE > 100);
        assert!(CLIENT_ID_SPAN > 1);
    }
}
