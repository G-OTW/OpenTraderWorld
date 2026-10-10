//! IMAP transport for the Mailbox module — connect, list what is new, download it.
//!
//! Strictly read-only: the folder is opened with EXAMINE and bodies are fetched with
//! BODY.PEEK, so nothing is flagged, moved or deleted on the server. The module is a
//! reader, not a mail client, and a user must be able to point it at a live personal
//! mailbox without fearing what it might do there.
//!
//! TLS is rustls (matching the rest of the workspace — no native-tls anywhere). Three
//! transports: implicit TLS (993), STARTTLS (143), and plaintext for loopback bridges
//! (Proton Bridge listens on 127.0.0.1 and is the only sane use of it).

use std::fmt::Debug;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{anyhow, Context};
use async_imap::Client;
use futures::TryStreamExt;
use sqlx::types::time::OffsetDateTime;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

/// How long a single TCP connect may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// UIDs fetched in one round-trip. Small: each mail is held in memory until the sink
/// has stored it, and a broker statement can be megabytes.
const FETCH_CHUNK: usize = 5;

/// How to prove who we are. Both variants hold a live secret — decrypted password or
/// access token — which is why an `ImapConfig` never outlives one session.
pub enum Auth {
    /// LOGIN with an app password.
    Password(String),
    /// SASL XOAUTH2 with a short-lived access token (Microsoft).
    XOAuth2(String),
}

/// Everything needed to open one session.
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub security: String,
    pub username: String,
    pub auth: Auth,
}

/// One downloaded mail, still raw RFC-822 bytes.
pub struct FetchedMail {
    pub uid: u32,
    pub size: u32,
    pub internal_date: Option<OffsetDateTime>,
    pub raw: Vec<u8>,
}

/// What one poll did, whatever the sink made of it.
#[derive(Debug, Default)]
pub struct PollReport {
    pub uid_validity: u32,
    /// Highest UID considered — including mails skipped for size, so an oversized mail
    /// is not re-downloaded on every poll.
    pub highest_uid: u32,
    pub fetched: usize,
    pub skipped_too_big: usize,
    /// New UIDs left for the next poll (the per-poll batch is capped).
    pub remaining: usize,
}

/// What a connection test reports back to the UI.
pub struct ProbeReport {
    /// Messages in the folder — proof the credential opened a real mailbox.
    pub exists: u32,
}

// ── Connection ───────────────────────────────────────────────────────────────

/// Process-wide rustls config: webpki roots, ring provider (explicit, so a second
/// provider linked in by another crate can never make this ambiguous at runtime).
fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("rustls default protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

type TlsStream = tokio_rustls::client::TlsStream<TcpStream>;

async fn tcp_connect(host: &str, port: u16) -> anyhow::Result<TcpStream> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| anyhow!("connection to {host}:{port} timed out"))?
        .with_context(|| format!("connecting to {host}:{port}"))?;
    tcp.set_nodelay(true).ok();
    Ok(tcp)
}

async fn tls_handshake(host: &str, tcp: TcpStream) -> anyhow::Result<TlsStream> {
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| anyhow!("invalid TLS server name: {host}"))?;
    tokio_rustls::TlsConnector::from(tls_config())
        .connect(name, tcp)
        .await
        .with_context(|| format!("TLS handshake with {host}"))
}

/// A logged-out client, one variant per transport. Both variants converge on the same
/// generic session code below.
enum Conn {
    Tls(Client<TlsStream>),
    Plain(Client<TcpStream>),
}

/// Consume the server greeting. IMAP sends it unsolicited on connect; skipping it would
/// desynchronise every later response.
async fn greeting<T>(client: &mut Client<T>) -> anyhow::Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    client
        .read_response()
        .await
        .context("reading IMAP greeting")?
        .ok_or_else(|| anyhow!("server closed the connection before greeting"))?;
    Ok(())
}

async fn connect(cfg: &ImapConfig) -> anyhow::Result<Conn> {
    match cfg.security.as_str() {
        "starttls" => {
            let tcp = tcp_connect(&cfg.host, cfg.port).await?;
            let mut client = Client::new(tcp);
            greeting(&mut client).await?;
            client
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .context("STARTTLS refused by the server")?;
            // The upgraded connection does not re-send a greeting (RFC 2595).
            let tcp = client.into_inner();
            let tls = tls_handshake(&cfg.host, tcp).await?;
            Ok(Conn::Tls(Client::new(tls)))
        }
        "none" => {
            let tcp = tcp_connect(&cfg.host, cfg.port).await?;
            let mut client = Client::new(tcp);
            greeting(&mut client).await?;
            Ok(Conn::Plain(client))
        }
        // Implicit TLS is the default for anything unrecognised: never silently downgrade.
        _ => {
            let tcp = tcp_connect(&cfg.host, cfg.port).await?;
            let tls = tls_handshake(&cfg.host, tcp).await?;
            let mut client = Client::new(tls);
            greeting(&mut client).await?;
            Ok(Conn::Tls(client))
        }
    }
}

// ── Public operations ────────────────────────────────────────────────────────

/// Connect, log in, open the folder, log out. Used by the "Test connection" button:
/// it proves the credential works and the folder exists, and stores nothing.
pub async fn probe(cfg: &ImapConfig, folder: &str) -> anyhow::Result<ProbeReport> {
    match connect(cfg).await? {
        Conn::Tls(c) => probe_inner(c, cfg, folder).await,
        Conn::Plain(c) => probe_inner(c, cfg, folder).await,
    }
}

/// Fetch the mails whose UID is above `last_uid`, handing them to `sink` in small
/// batches together with the mailbox's UIDVALIDITY. `max_msgs` caps one poll; on a first
/// sync (`last_uid == 0`) the newest `max_msgs` are taken instead of the oldest, so
/// connecting a ten-year-old mailbox does not import a decade of mail before showing
/// anything. A UIDVALIDITY that no longer matches `uid_validity` restarts the sync: the
/// stored watermark refers to a mailbox the server no longer has.
#[allow(clippy::too_many_arguments)]
pub async fn poll<F, Fut>(
    cfg: &ImapConfig,
    folder: &str,
    uid_validity: u32,
    last_uid: u32,
    max_msgs: usize,
    max_bytes: u32,
    sink: F,
) -> anyhow::Result<PollReport>
where
    F: FnMut(u32, Vec<FetchedMail>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let args = PollArgs { folder, uid_validity, last_uid, max_msgs, max_bytes };
    match connect(cfg).await? {
        Conn::Tls(c) => poll_inner(c, cfg, args, sink).await,
        Conn::Plain(c) => poll_inner(c, cfg, args, sink).await,
    }
}

/// The knobs of one poll, bundled so the generic inner function keeps a readable shape.
struct PollArgs<'a> {
    folder: &'a str,
    uid_validity: u32,
    last_uid: u32,
    max_msgs: usize,
    max_bytes: u32,
}

// ── Session code (generic over the transport) ────────────────────────────────

async fn login<T>(client: Client<T>, cfg: &ImapConfig) -> anyhow::Result<async_imap::Session<T>>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    // The error carries the client back; drop it and keep the reason. Providers put useful
    // text there ("Application-specific password required"), so it is surfaced verbatim.
    match &cfg.auth {
        Auth::Password(password) => client
            .login(&cfg.username, password)
            .await
            .map_err(|(e, _)| anyhow!("login failed: {e}")),
        Auth::XOAuth2(token) => client
            .authenticate("XOAUTH2", super::oauth::XOAuth2::new(&cfg.username, token))
            .await
            .map_err(|(e, _)| anyhow!("OAuth sign-in refused by the server: {e}")),
    }
}

async fn probe_inner<T>(
    client: Client<T>,
    cfg: &ImapConfig,
    folder: &str,
) -> anyhow::Result<ProbeReport>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let mut session = login(client, cfg).await?;
    let mbox = session
        .examine(folder)
        .await
        .with_context(|| format!("opening folder {folder}"))?;
    let report = ProbeReport { exists: mbox.exists };
    let _ = session.logout().await;
    Ok(report)
}

async fn poll_inner<T, F, Fut>(
    client: Client<T>,
    cfg: &ImapConfig,
    args: PollArgs<'_>,
    mut sink: F,
) -> anyhow::Result<PollReport>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
    F: FnMut(u32, Vec<FetchedMail>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let PollArgs { folder, max_msgs, max_bytes, .. } = args;
    let mut session = login(client, cfg).await?;
    let mbox = session
        .examine(folder)
        .await
        .with_context(|| format!("opening folder {folder}"))?;

    let uid_validity = mbox.uid_validity.unwrap_or(0);
    let mut report = PollReport { uid_validity, ..Default::default() };
    // A mailbox recreated server-side gets a new UIDVALIDITY; the old watermark is void.
    let last_uid = if args.uid_validity != 0 && uid_validity != args.uid_validity {
        0
    } else {
        args.last_uid
    };

    // `UID n:*` always matches the highest UID even when it is below n, so the result is
    // filtered rather than trusted.
    let found = session
        .uid_search(format!("UID {}:*", last_uid.saturating_add(1)))
        .await
        .context("searching for new mail")?;
    let mut uids: Vec<u32> = found.into_iter().filter(|u| *u > last_uid).collect();
    uids.sort_unstable();

    let batch: Vec<u32> = if uids.len() <= max_msgs {
        uids.clone()
    } else if last_uid == 0 {
        uids[uids.len() - max_msgs..].to_vec()
    } else {
        uids[..max_msgs].to_vec()
    };
    report.remaining = uids.len() - batch.len();

    if batch.is_empty() {
        let _ = session.logout().await;
        return Ok(report);
    }

    // Sizes first: an oversized mail is skipped without ever crossing the wire.
    let sizes: Vec<(u32, u32)> = {
        let stream = session
            .uid_fetch(uid_set(&batch), "(UID RFC822.SIZE)")
            .await
            .context("fetching mail sizes")?;
        let fetches: Vec<_> = stream.try_collect().await.context("reading mail sizes")?;
        fetches
            .iter()
            .filter_map(|f| Some((f.uid?, f.size.unwrap_or(0))))
            .collect()
    };

    let mut wanted: Vec<u32> = Vec::new();
    for (uid, size) in &sizes {
        report.highest_uid = report.highest_uid.max(*uid);
        if *size > max_bytes {
            report.skipped_too_big += 1;
        } else {
            wanted.push(*uid);
        }
    }
    // A server that answered no size at all (rare, but legal) must not stall the sync.
    if sizes.is_empty() {
        wanted = batch.clone();
        report.highest_uid = batch.iter().copied().max().unwrap_or(0);
    }

    for chunk in wanted.chunks(FETCH_CHUNK) {
        let mails: Vec<FetchedMail> = {
            let stream = session
                .uid_fetch(uid_set(chunk), "(UID INTERNALDATE BODY.PEEK[])")
                .await
                .context("fetching mail")?;
            let fetches: Vec<_> = stream.try_collect().await.context("reading mail")?;
            fetches
                .iter()
                .filter_map(|f| {
                    let uid = f.uid?;
                    let raw = f.body()?.to_vec();
                    Some(FetchedMail {
                        uid,
                        size: raw.len() as u32,
                        internal_date: f
                            .internal_date()
                            .and_then(|d| OffsetDateTime::from_unix_timestamp(d.timestamp()).ok()),
                        raw,
                    })
                })
                .collect()
        };
        report.fetched += mails.len();
        for m in &mails {
            report.highest_uid = report.highest_uid.max(m.uid);
        }
        sink(uid_validity, mails).await?;
    }

    let _ = session.logout().await;
    Ok(report)
}

/// IMAP UID set, e.g. `12,17,18`.
fn uid_set(uids: &[u32]) -> String {
    uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::uid_set;

    #[test]
    fn uid_set_is_comma_separated() {
        assert_eq!(uid_set(&[3, 4, 9]), "3,4,9");
        assert_eq!(uid_set(&[]), "");
    }
}
