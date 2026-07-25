//! Live subscription hub: one supervised WS session per live *instrument*, shared by all
//! viewers.
//!
//! An instrument is addressed by coordinates (provider, asset type, ticker, timeframe), not
//! by a dataset: watching a symbol must not write anything. Persistence is opt-in and comes
//! from the coordinates already having a dataset — save an instrument and the same feed
//! starts recording its closed bars; until then it is broadcast-only.
//!
//! An instrument goes live the moment its first viewer subscribes and is torn down a short
//! grace period after the last one leaves (ref-counted). Each live instrument owns:
//!   - a [`broadcast`] channel every viewer's SSE stream reads from;
//!   - a shared `current` cell holding the forming bar, so a viewer that connects mid-bar
//!     gets an immediate snapshot;
//!   - a supervisor task that runs the sync algorithm and reconnects from cold on any drop.
//!
//! Sync algorithm (per session, cold start and every reconnect alike):
//!   1. open the WS and **buffer** events without applying them;
//!   2. *stored instruments only* — REST-seed the gap since stored history (or the last N
//!      bars) and write it, so the recorded series has no hole around the reconnect;
//!   3. replay the buffer, dropping events older than the seed (the overlap merge — this is
//!      what removes the gap between REST and the stream);
//!   4. stream on, persisting each closed bar when there is a dataset, backfilling any
//!      detected gap, broadcasting all.
//!
//! A broadcast-only feed skips step 2 entirely: seeded bars are never sent to viewers (the
//! chart loads its history from `/api/histviz/series`), so fetching them would spend a
//! provider request to feed nothing but the overlap check.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use reqwest::Client;
use sqlx::PgPool;
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use uuid::Uuid;

use otw_store::crypto::SecretCipher;
use otw_store::connectors as conn_store;
use otw_store::histdata::{self as store, Bar};

use super::{missing_buckets, stream_connector_for, LiveEvent, SEED_BARS};

/// Trailing bars to seed for a provider, kept strictly under its per-request cap so a
/// boundary-exact request (e.g. Coinbase's 300-candle limit) never 400s.
fn seed_bar_count(rest: &dyn crate::histdata::Connector) -> i64 {
    let cap = rest.capability().max_bars_per_req.saturating_sub(1) as i64;
    SEED_BARS.min(cap.max(1))
}

/// Seed window start: `count` bars back from the *current bar's open*, aligned to the
/// timeframe boundary. Alignment makes the aggregation count deterministic — an unaligned
/// `from` (fractional seconds) can tip a provider's per-request candle count one over its
/// cap and get the whole request rejected (Coinbase's "count exceeds 300").
fn seed_from(now: OffsetDateTime, tf: i64, count: i64) -> OffsetDateTime {
    super::bar_start(now, tf) - TimeDuration::seconds(tf * (count - 1).max(0))
}

/// Broadcast backlog per dataset. A viewer that lags past this is dropped by `BroadcastStream`
/// and resyncs from the `current` snapshot on its next frame.
const CHANNEL_CAP: usize = 1024;
/// How long a dataset stays live after its last viewer leaves, so a quick chart re-open or
/// timeframe flip doesn't thrash the WS connection.
const TEARDOWN_GRACE: Duration = Duration::from_secs(20);
/// Reconnect backoff bounds after a dropped/closed session.
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// The coordinates a supervisor needs to run one instrument's live feed.
#[derive(Clone)]
pub struct LiveTarget {
    /// Dataset to record closed bars into. `None` = broadcast-only: the user is looking at
    /// an instrument they never saved, and looking must not write.
    pub dataset_id: Option<Uuid>,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
}

impl LiveTarget {
    /// Identity of the feed — the instrument, not the storage. Two viewers of the same
    /// symbol/timeframe share one WS session whether or not it is being recorded.
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.provider, self.asset_type, self.ticker, self.timeframe
        )
    }
}

struct Entry {
    tx: broadcast::Sender<LiveEvent>,
    current: Arc<Mutex<Option<LiveEvent>>>,
    subs: usize,
    task: JoinHandle<()>,
    /// Bumped on each (re)creation so a delayed teardown can tell a resurrected entry apart.
    gen: u64,
    /// What this supervisor is recording into, so a feed that started broadcast-only can be
    /// restarted as a recording one the moment the instrument gets saved.
    dataset_id: Option<Uuid>,
}

struct Inner {
    pool: PgPool,
    http: Client,
    cipher: SecretCipher,
    entries: Mutex<HashMap<String, Entry>>,
    next_gen: Mutex<u64>,
}

/// Shared, cheap-to-clone handle to the live hub.
#[derive(Clone)]
pub struct LiveHub(Arc<Inner>);

/// What a viewer receives on subscribe: the live event stream, plus the forming bar (if any)
/// so it can render immediately without waiting for the next update.
pub struct Subscription {
    pub rx: broadcast::Receiver<LiveEvent>,
    pub snapshot: Option<LiveEvent>,
    /// Hold this for the stream's lifetime; dropping it releases the dataset's ref-count.
    pub guard: LiveGuard,
}

impl LiveHub {
    pub fn new(pool: PgPool, http: Client, cipher: SecretCipher) -> Self {
        LiveHub(Arc::new(Inner {
            pool,
            http,
            cipher,
            entries: Mutex::new(HashMap::new()),
            next_gen: Mutex::new(0),
        }))
    }

    /// Start (or join) the live feed for an instrument and return a viewer subscription.
    /// Errors only if the provider has no WS feed.
    pub fn subscribe(&self, target: LiveTarget) -> Result<Subscription> {
        if stream_connector_for(&target.provider).is_none() {
            return Err(anyhow!("provider {} has no live feed", target.provider));
        }
        let key = target.key();
        let mut entries = self.0.entries.lock().unwrap();
        if let Some(e) = entries.get_mut(&key) {
            // Same instrument, same recording state → share the running session.
            if e.dataset_id == target.dataset_id {
                e.subs += 1;
                let rx = e.tx.subscribe();
                let snapshot = e.current.lock().unwrap().clone();
                return Ok(Subscription {
                    rx,
                    snapshot,
                    guard: LiveGuard { hub: self.clone(), key, gen: e.gen },
                });
            }
            // The instrument was saved (or its dataset deleted) while live: restart the
            // supervisor so it records — or stops recording — from here on. Existing viewers
            // see their broadcast channel close and reconnect onto the new session.
            e.task.abort();
            entries.remove(&key);
        }
        // First viewer → spin up the supervisor.
        let (tx, rx) = broadcast::channel(CHANNEL_CAP);
        let current = Arc::new(Mutex::new(None));
        let gen = {
            let mut g = self.0.next_gen.lock().unwrap();
            *g += 1;
            *g
        };
        let dataset_id = target.dataset_id;
        let task = tokio::spawn(supervise(
            self.0.clone(),
            target,
            tx.clone(),
            current.clone(),
        ));
        entries.insert(
            key.clone(),
            Entry { tx, current, subs: 1, task, gen, dataset_id },
        );
        Ok(Subscription {
            rx,
            snapshot: None,
            guard: LiveGuard { hub: self.clone(), key, gen },
        })
    }

    /// Drop one viewer. When the count hits zero, schedule a graced teardown.
    fn release(&self, key: &str, gen: u64) {
        let should_schedule = {
            let mut entries = self.0.entries.lock().unwrap();
            match entries.get_mut(key) {
                Some(e) if e.gen == gen => {
                    e.subs = e.subs.saturating_sub(1);
                    e.subs == 0
                }
                _ => false,
            }
        };
        if should_schedule {
            let hub = self.clone();
            let key = key.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(TEARDOWN_GRACE).await;
                let mut entries = hub.0.entries.lock().unwrap();
                if let Some(e) = entries.get(&key) {
                    if e.gen == gen && e.subs == 0 {
                        e.task.abort();
                        entries.remove(&key);
                    }
                }
            });
        }
    }
}

/// RAII viewer handle: decrements the instrument's ref-count when the SSE stream is dropped.
pub struct LiveGuard {
    hub: LiveHub,
    key: String,
    gen: u64,
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.hub.release(&self.key, self.gen);
    }
}

// ── Supervisor ─────────────────────────────────────────────────────────────────

/// Run one instrument's live feed forever (until the task is aborted at teardown), reconnecting
/// with bounded backoff after any clean close or error.
async fn supervise(
    inner: Arc<Inner>,
    target: LiveTarget,
    tx: broadcast::Sender<LiveEvent>,
    current: Arc<Mutex<Option<LiveEvent>>>,
) {
    let mut backoff = BACKOFF_MIN;
    loop {
        match session(&inner, &target, &tx, &current).await {
            Ok(()) => {
                // Clean close (server rotated / stream ended) → reconnect promptly.
                backoff = BACKOFF_MIN;
            }
            Err(e) => {
                tracing::warn!("live {} ({}): session ended: {e:#}", target.ticker, target.provider);
            }
        }
        *current.lock().unwrap() = None;
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// One WS session: open, seed, overlap-merge, then stream until it drops.
async fn session(
    inner: &Inner,
    target: &LiveTarget,
    tx: &broadcast::Sender<LiveEvent>,
    current: &Arc<Mutex<Option<LiveEvent>>>,
) -> Result<()> {
    let connector = stream_connector_for(&target.provider)
        .ok_or_else(|| anyhow!("no live feed for {}", target.provider))?;
    let rest = crate::histdata::connector_for(&target.provider)?;
    let tf = crate::histdata::timeframe_secs(&target.timeframe)?;
    let secrets = load_secrets(inner, &target.provider).await?;

    // 1. Open + start buffering before any REST call.
    let mut stream = connector.open(&target.ticker, &target.timeframe).await?;
    let mut buffer: Vec<LiveEvent> = Vec::new();

    // 2. REST-seed — recording feeds only: fill the gap since stored history (else the last
    //    SEED_BARS) so the persisted series has no hole around this (re)connect. A
    //    broadcast-only feed has nothing to keep whole, and its viewers get their history
    //    from the series endpoint, so it spends no request here.
    let (stored, seed) = match target.dataset_id {
        Some(dataset_id) => {
            let now = OffsetDateTime::now_utc();
            let stored = store::latest_bar_ts(&inner.pool, dataset_id).await?;
            let from = stored.unwrap_or_else(|| seed_from(now, tf, seed_bar_count(rest.as_ref())));
            let seed_fut = rest.fetch_chunk(&inner.http, &secrets, &target.ticker, &target.asset_type, &target.timeframe, from, now);
            tokio::pin!(seed_fut);
            let seed = loop {
                tokio::select! {
                    item = stream.next() => match item {
                        Some(Ok(e)) => buffer.push(e),
                        Some(Err(e)) => return Err(e),
                        None => return Ok(()), // socket closed mid-seed → reconnect
                    },
                    r = &mut seed_fut => break r.context("live REST seed")?.bars,
                }
            };
            if !seed.is_empty() {
                store::write_bars(&inner.pool, dataset_id, &seed).await?;
            }
            (stored, seed)
        }
        None => (None, Vec::new()),
    };
    let seed_last = seed.last().map(|b| b.ts);
    let mut last_closed = seed_last.or(stored);

    // 3. Replay the buffered overlap: drop anything strictly older than the seed's last bar
    //    (that bar is either history or the still-forming one an update will refine).
    let buffered: Vec<LiveEvent> = std::mem::take(&mut buffer);
    for e in buffered {
        if let Some(sl) = seed_last {
            if e.bar_ts < sl {
                continue;
            }
        }
        apply(inner, target, tf, &secrets, rest.as_ref(), tx, current, &mut last_closed, e).await?;
    }

    // 4. Stream on.
    while let Some(item) = stream.next().await {
        let e = item?;
        apply(inner, target, tf, &secrets, rest.as_ref(), tx, current, &mut last_closed, e).await?;
    }
    Ok(())
}

/// Apply one event: advance on close (backfilling any gap first, persisting when the
/// instrument is stored), else update the forming bar. Broadcasts every event to viewers.
#[allow(clippy::too_many_arguments)]
async fn apply(
    inner: &Inner,
    target: &LiveTarget,
    tf: i64,
    secrets: &std::collections::HashMap<String, String>,
    rest: &dyn crate::histdata::Connector,
    tx: &broadcast::Sender<LiveEvent>,
    current: &Arc<Mutex<Option<LiveEvent>>>,
    last_closed: &mut Option<OffsetDateTime>,
    e: LiveEvent,
) -> Result<()> {
    if e.closed {
        // Technical gap: closed bar skipped one or more buckets → backfill just that span.
        if let Some(prev) = *last_closed {
            if !missing_buckets(prev, e.bar_ts, tf).is_empty() {
                backfill(inner, target, secrets, rest, tx, prev + TimeDuration::seconds(tf), e.bar_ts).await;
            }
        }
        if let Some(dataset_id) = target.dataset_id {
            store::write_bars(&inner.pool, dataset_id, &[e.as_bar()]).await?;
        }
        *last_closed = Some(e.bar_ts);
        *current.lock().unwrap() = None;
    } else {
        *current.lock().unwrap() = Some(e.clone());
    }
    let _ = tx.send(e);
    Ok(())
}

/// Fetch a missing `[from, to)` span from REST, persist it when the instrument is stored, and
/// broadcast each recovered bar as closed so viewers fill the hole either way. Best-effort: a
/// failed backfill is logged, not fatal.
async fn backfill(
    inner: &Inner,
    target: &LiveTarget,
    secrets: &std::collections::HashMap<String, String>,
    rest: &dyn crate::histdata::Connector,
    tx: &broadcast::Sender<LiveEvent>,
    from: OffsetDateTime,
    to: OffsetDateTime,
) {
    match rest
        .fetch_chunk(&inner.http, secrets, &target.ticker, &target.asset_type, &target.timeframe, from, to)
        .await
    {
        Ok(chunk) => {
            let bars: Vec<Bar> = chunk.bars.into_iter().filter(|b| b.ts >= from && b.ts < to).collect();
            if !bars.is_empty() {
                if let Some(dataset_id) = target.dataset_id {
                    if let Err(e) = store::write_bars(&inner.pool, dataset_id, &bars).await {
                        tracing::warn!("live backfill write {}: {e:#}", target.ticker);
                        return;
                    }
                }
                for b in &bars {
                    let _ = tx.send(bar_to_event(b));
                }
                tracing::info!("live backfilled {} gap bar(s) for {}", bars.len(), target.ticker);
            }
        }
        Err(e) => tracing::warn!("live backfill fetch {}: {e:#}", target.ticker),
    }
}

fn bar_to_event(b: &Bar) -> LiveEvent {
    LiveEvent {
        bar_ts: b.ts,
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
        volume: b.volume,
        closed: true,
        provider_ts: b.ts,
    }
}

/// Credentials for the provider's default connector (empty for keyless crypto). Keeps the
/// live seed/backfill on the same creds path as downloads, ready for keyed providers later.
async fn load_secrets(
    inner: &Inner,
    provider: &str,
) -> Result<std::collections::HashMap<String, String>> {
    match conn_store::default_for(&inner.pool, provider).await? {
        Some(c) => Ok(conn_store::load_creds(&inner.pool, &inner.cipher, c.id).await?),
        None => Ok(Default::default()),
    }
}
