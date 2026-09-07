//! Live subscription hub: one supervised socket per *connector and socket class*, carrying
//! every instrument the charts are watching on it.
//!
//! An instrument is addressed by coordinates (provider, asset type, ticker, timeframe), not
//! by a dataset: watching a symbol must not write anything. Persistence is opt-in and comes
//! from the coordinates already having a dataset, so saving an instrument is what turns
//! recording on.
//!
//! **One socket, N instruments.** A session used to be one instrument, which meant a second
//! pane was a second connection: Alpaca allows one per key, so split screen on equities was
//! not slow, it was impossible. The socket is now keyed by the connector and the provider's
//! own socket class (Alpaca's market, Kraken's interval, nothing at all for the rest), and
//! instruments join and leave it by frame. Two panes on one symbol at two timeframes share a
//! single channel: the fold up to each timeframe happens here, not in the connector.
//!
//! Layers:
//!   - a [`Session`] owns the socket, the per-subscription state and the reconnect loop;
//!   - a *subscription* is an instrument at a timeframe: its broadcast channel, its forming
//!     bar, its status, and (when the instrument is stored) its recording;
//!   - a *viewer* is one SSE stream. Subscriptions are ref-counted and torn down a short
//!     grace period after their last viewer leaves; a session ends when its last
//!     subscription does.
//!
//! Sync algorithm, per subscription, on join and on every reconnect:
//!   1. the socket is open and its events for that instrument are **buffered**;
//!   2. *stored instruments only*, REST-seed the gap since stored history (or the last N
//!      bars) and write it, so the recorded series has no hole around the reconnect;
//!   3. replay the buffer, dropping events older than the seed (the overlap merge);
//!   4. stream on, persisting each closed bar when there is a dataset, backfilling any
//!      detected gap, broadcasting all.
//!
//! A broadcast-only subscription skips step 2 entirely: seeded bars are never sent to viewers
//! (the chart loads its history from `/api/histviz/series`), so fetching them would spend a
//! provider request to feed nothing but the overlap check. The exception is a session-shaped
//! timeframe ([`anchored`]): there the seed is what tells the fold where a day starts, so it
//! runs whether or not anything is being recorded.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use sqlx::PgPool;
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::sync::{broadcast, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Duration;
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::crypto::SecretCipher;
use otw_store::histdata::{self as store, Bar};
use reqwest::Client;

use super::agg::{Bucketer, TICK};
use super::{
    fault_message, fault_of, missing_buckets, stream_connector_for, FeedUpdate, Instrument,
    LiveEvent, LiveFault, Payload, SessionRequest, SourceGrain, SEED_BARS,
};

/// What a viewer's SSE stream carries. Bars are the point; the status is what makes a feed
/// that cannot run *legible* , before this the only symptom of a rejected key was a dot that
/// stayed grey while the supervisor reconnected forever.
#[derive(Debug, Clone)]
pub enum LiveMsg {
    Bar(LiveEvent),
    Status(LiveStatus),
}

/// The feed's current condition, sent on connect and on every change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveStatus {
    /// `connecting` | `live` | `retrying` | `stopped`.
    pub state: &'static str,
    /// The [`LiveFault`] code when there is one, for styling and translation.
    pub code: Option<&'static str>,
    /// The sentence to show, already naming the connector and the fix.
    pub message: Option<String>,
}

impl LiveStatus {
    fn connecting() -> Self {
        LiveStatus { state: "connecting", code: None, message: None }
    }
    fn live() -> Self {
        LiveStatus { state: "live", code: None, message: None }
    }
    fn from_fault(fault: LiveFault, message: String) -> Self {
        LiveStatus {
            state: if fault.terminal() { "stopped" } else { "retrying" },
            code: Some(fault.code()),
            message: Some(message),
        }
    }
}

/// Trailing bars to seed for a provider, kept strictly under its per-request cap so a
/// boundary-exact request (e.g. Coinbase's 300-candle limit) never 400s.
fn seed_bar_count(rest: &dyn crate::histdata::Connector) -> i64 {
    let cap = rest.capability().max_bars_per_req.saturating_sub(1) as i64;
    SEED_BARS.min(cap.max(1))
}

/// Seed window start: `count` bars back from the *current bar's open*, aligned to the
/// timeframe boundary. Alignment makes the aggregation count deterministic: an unaligned
/// `from` (fractional seconds) can tip a provider's per-request candle count one over its
/// cap and get the whole request rejected (Coinbase's "count exceeds 300").
/// Whether this timeframe's bars are periods the *provider* delimits rather than epoch
/// buckets. A daily equity bar is a session stamped at the exchange's midnight and a weekly
/// one starts on the session's Monday, so the fold has to take its alignment from real bars
/// (and prime the period already in progress) instead of dividing the epoch. Everything
/// intraday is epoch-aligned on every venue we stream, and needs none of this.
pub fn anchored(tf: i64) -> bool {
    tf >= 86_400
}

/// How long to wait after a session-shaped bar closes before re-reading it from REST. The
/// official daily bar carries the closing print and excludes extended hours, neither of which
/// a fold of minute bars can know, so the recorded row is corrected by the provider rather
/// than left as the approximation that was on screen.
const SETTLE_DELAY: Duration = Duration::from_secs(90);

/// How long a remembered session anchor stays usable. A venue moves its open twice a year
/// (DST) and otherwise never, so hours is the right order; the settle re-read refreshes it
/// at every session close anyway.
const ANCHOR_TTL: Duration = Duration::from_secs(6 * 3600);

/// How recent the remembered bar must be for a joining pane to adopt it as the period in
/// progress. Past that it is stale, and starting the fold from now is more honest than
/// showing a range that is minutes or hours old.
const ANCHOR_PRIME_FRESH: Duration = Duration::from_secs(120);

/// A session-shaped instrument's alignment, remembered across subscriptions.
///
/// Anchoring costs one REST request per session start, and Massive's free tier is five
/// requests a minute: four daily panes opening on one connector at page load is most of that
/// budget, spent to re-learn a number that changes twice a year. The entry outlives the
/// subscription and is refreshed by every seed and every settle re-read.
#[derive(Clone)]
struct Anchor {
    /// Seconds into the timeframe cycle where the period starts.
    offset: i64,
    /// The provider's newest bar when this was read.
    last: Bar,
    at: std::time::Instant,
}

/// Cache identity: the coordinates, not the connector. Two keys on one provider see the same
/// exchange calendar, and the anchor is a property of the venue.
fn anchor_key(t: &LiveTarget) -> String {
    format!("{}|{}|{}|{}", t.provider, t.asset_type, t.ticker, t.timeframe)
}

fn seed_from(now: OffsetDateTime, tf: i64, count: i64) -> OffsetDateTime {
    super::bar_start(now, tf) - TimeDuration::seconds(tf * (count - 1).max(0))
}

/// Broadcast backlog per subscription. A viewer that lags past this is dropped by
/// `BroadcastStream` and resyncs from the `current` snapshot on its next frame.
const CHANNEL_CAP: usize = 1024;
/// How long a subscription stays live after its last viewer leaves, so a quick chart re-open
/// or timeframe flip doesn't thrash the socket.
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
    /// Which of the user's connectors for this provider to spend. Not optional: the
    /// entitlements differ between two keys of the same provider (a free Alpaca key and a
    /// paid one open different sockets), and every provider that streams under a key limits
    /// how many connections that key may hold, so *which* one is part of the feed's identity.
    pub connector_id: Uuid,
    /// The connector's name, carried so a refusal can say which key was refused.
    pub connector: String,
    pub provider: String,
    pub asset_type: String,
    pub ticker: String,
    pub timeframe: String,
}

impl LiveTarget {
    /// The instrument, as the transport layer addresses it.
    pub fn instrument(&self) -> Instrument {
        Instrument::new(&self.ticker, &self.asset_type, &self.timeframe)
    }

    /// Identity of the *subscription*: an instrument at a timeframe. The socket it rides on
    /// is a separate key (see [`session_key`]).
    pub fn key(&self) -> String {
        self.instrument().key()
    }
}

/// Identity of the socket: the connector plus whatever the provider cannot multiplex over
/// (Alpaca's market, Kraken's interval). Two connectors are two entitlements and two seats,
/// so they never share one.
fn session_key(connector_id: Uuid, class: &str) -> String {
    format!("{connector_id}|{class}")
}

/// How many distinct instruments one connector is streaming. A market-data line is an
/// instrument, so the same ticker at three timeframes is one line, and two connectors on the
/// same provider are two accounts and two separate tallies.
fn lines_of<'a>(targets: impl Iterator<Item = &'a LiveTarget>, connector_id: Uuid) -> usize {
    targets
        .filter(|t| t.connector_id == connector_id)
        .map(|t| t.ticker.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// One subscription as the hub tracks it: what the viewers read, and what a resurrection
/// needs in order to rebuild it.
struct SubEntry {
    tx: broadcast::Sender<LiveMsg>,
    current: Arc<Mutex<Option<LiveEvent>>>,
    status: Arc<Mutex<LiveStatus>>,
    viewers: usize,
    dataset_id: Option<Uuid>,
    target: LiveTarget,
    /// Bumped on each (re)creation so a delayed teardown can tell a resurrected one apart.
    gen: u64,
}

impl SubEntry {
    fn spec(&self) -> SubSpec {
        SubSpec {
            target: self.target.clone(),
            tx: self.tx.clone(),
            current: self.current.clone(),
            status: self.status.clone(),
        }
    }
}

/// What the hub hands a supervisor when a subscription joins: the coordinates plus the
/// channels the viewers are already reading, so a resurrected session keeps them attached.
struct SubSpec {
    target: LiveTarget,
    tx: broadcast::Sender<LiveMsg>,
    current: Arc<Mutex<Option<LiveEvent>>>,
    status: Arc<Mutex<LiveStatus>>,
}

enum HubCmd {
    Add(Box<SubSpec>),
    Remove(String),
}

struct SessionEntry {
    cmds: mpsc::UnboundedSender<HubCmd>,
    task: JoinHandle<()>,
    subs: HashMap<String, SubEntry>,
}

struct Inner {
    pool: PgPool,
    http: Client,
    cipher: SecretCipher,
    sessions: Mutex<HashMap<String, SessionEntry>>,
    next_gen: Mutex<u64>,
    /// Session alignments, kept past the subscription that learned them. See [`Anchor`].
    anchors: Mutex<HashMap<String, Anchor>>,
}

impl Inner {
    /// The remembered alignment for these coordinates, if it is still fresh enough to use.
    fn warm_anchor(&self, target: &LiveTarget) -> Option<Anchor> {
        let anchors = self.anchors.lock().unwrap();
        anchors.get(&anchor_key(target)).filter(|a| a.at.elapsed() < ANCHOR_TTL).cloned()
    }

    /// Remember what a provider bar says about where the period starts. Called on every seed
    /// and every settle re-read, so a venue that shifts its open (DST) is re-learned rather
    /// than kept wrong for the life of the process.
    fn remember_anchor(&self, target: &LiveTarget, tf: i64, last: &Bar) {
        let offset = last.ts.unix_timestamp().rem_euclid(tf);
        let mut anchors = self.anchors.lock().unwrap();
        anchors.insert(
            anchor_key(target),
            Anchor { offset, last: last.clone(), at: std::time::Instant::now() },
        );
    }
}

/// Shared, cheap-to-clone handle to the live hub.
#[derive(Clone)]
pub struct LiveHub(Arc<Inner>);

/// What a viewer receives on subscribe: the live event stream, plus the forming bar (if any)
/// so it can render immediately without waiting for the next update.
pub struct Subscription {
    pub rx: broadcast::Receiver<LiveMsg>,
    pub snapshot: Option<LiveEvent>,
    /// The feed's condition right now, sent to the viewer before anything else, so a chart
    /// that joins a feed already stopped on a rejected key says so immediately.
    pub status: LiveStatus,
    /// Hold this for the stream's lifetime; dropping it releases the subscription's
    /// ref-count.
    pub guard: LiveGuard,
}

impl LiveHub {
    pub fn new(pool: PgPool, http: Client, cipher: SecretCipher) -> Self {
        LiveHub(Arc::new(Inner {
            pool,
            http,
            cipher,
            sessions: Mutex::new(HashMap::new()),
            next_gen: Mutex::new(0),
            anchors: Mutex::new(HashMap::new()),
        }))
    }

    /// Market-data lines this connector is holding right now: distinct **instruments**,
    /// not subscriptions.
    ///
    /// Only Interactive Brokers charges for these, and it charges per *account*: two
    /// timeframes of one symbol ride one subscription upstream, four panes on four symbols
    /// cost four of the ~100 lines the account gets. The number is read off the hub rather
    /// than off IB, because IB never states the cap, it only starts refusing (error 322)
    /// once it is reached, which is far too late to tell the user anything useful.
    pub fn lines_held(&self, connector_id: Uuid) -> usize {
        let sessions = self.0.sessions.lock().unwrap();
        lines_of(
            sessions.values().flat_map(|e| e.subs.values()).map(|s| &s.target),
            connector_id,
        )
    }

    fn next_gen(&self) -> u64 {
        let mut g = self.0.next_gen.lock().unwrap();
        *g += 1;
        *g
    }

    /// Start (or join) the live feed for an instrument and return a viewer subscription.
    /// Errors only if the provider has no live feed, or cannot say which socket the
    /// instrument belongs on.
    pub fn subscribe(&self, target: LiveTarget) -> Result<Subscription> {
        let connector = stream_connector_for(&target.provider)
            .ok_or_else(|| anyhow!("provider {} has no live feed", target.provider))?;
        let inst = target.instrument();
        let class = connector.socket_class(&inst)?;
        let skey = session_key(target.connector_id, &class);
        let sub_key = inst.key();

        let mut sessions = self.0.sessions.lock().unwrap();
        if let Some(entry) = sessions.get_mut(&skey) {
            // Three cases, and only the first shares a running subscription.
            //
            // A supervisor that gave up on a terminal fault (a rejected key, a plan without
            // streaming) leaves its entry behind so a late viewer still learns why. Joining
            // it would be joining a corpse: a new subscription (the user pressing Live again
            // after fixing the connector) has to get a fresh supervisor.
            if !entry.task.is_finished() {
                if let Some(sub) = entry.subs.get_mut(&sub_key) {
                    if sub.dataset_id == target.dataset_id {
                        sub.viewers += 1;
                        return Ok(Subscription {
                            rx: sub.tx.subscribe(),
                            snapshot: sub.current.lock().unwrap().clone(),
                            status: sub.status.lock().unwrap().clone(),
                            guard: LiveGuard {
                                hub: self.clone(),
                                session: skey.clone(),
                                sub: sub_key,
                                gen: sub.gen,
                            },
                        });
                    }
                    // The instrument was saved (or its dataset deleted) while live, so this
                    // subscription has to restart in order to record, or to stop recording,
                    // from here on. Its viewers see the broadcast channel close and
                    // reconnect onto the new one.
                    let _ = entry.cmds.send(HubCmd::Remove(sub_key.clone()));
                    entry.subs.remove(&sub_key);
                }
                let gen = self.next_gen();
                let (sub, subscription) = self.new_sub(&skey, &sub_key, target, gen);
                let _ = entry.cmds.send(HubCmd::Add(Box::new(sub.spec())));
                entry.subs.insert(sub_key, sub);
                return Ok(subscription);
            }
            // The session is dead. Everything on it that still has viewers comes back with
            // the new subscription: one pane pressing Live again revives the rest of the
            // workspace rather than leaving it staring at a stopped feed.
            let entry = sessions.remove(&skey).expect("checked above");
            let mut carried: Vec<SubSpec> = entry
                .subs
                .iter()
                .filter(|(k, s)| s.viewers > 0 && k.as_str() != sub_key)
                .map(|(_, s)| s.spec())
                .collect();
            let mut subs: HashMap<String, SubEntry> = entry
                .subs
                .into_iter()
                .filter(|(k, s)| s.viewers > 0 && k.as_str() != sub_key)
                .collect();
            let gen = self.next_gen();
            let (sub, subscription) = self.new_sub(&skey, &sub_key, target, gen);
            carried.push(sub.spec());
            subs.insert(sub_key, sub);
            self.spawn_session(&mut sessions, skey, carried, subs);
            return Ok(subscription);
        }

        // First viewer on this socket.
        let gen = self.next_gen();
        let (sub, subscription) = self.new_sub(&skey, &sub_key, target, gen);
        let specs = vec![sub.spec()];
        let subs = HashMap::from([(sub_key, sub)]);
        self.spawn_session(&mut sessions, skey, specs, subs);
        Ok(subscription)
    }

    /// A fresh subscription: the channels the viewers read and the handle they release.
    fn new_sub(
        &self,
        session: &str,
        sub_key: &str,
        target: LiveTarget,
        gen: u64,
    ) -> (SubEntry, Subscription) {
        let (tx, rx) = broadcast::channel(CHANNEL_CAP);
        let dataset_id = target.dataset_id;
        let entry = SubEntry {
            tx,
            current: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(LiveStatus::connecting())),
            viewers: 1,
            dataset_id,
            target,
            gen,
        };
        let subscription = Subscription {
            rx,
            snapshot: None,
            status: LiveStatus::connecting(),
            guard: LiveGuard {
                hub: self.clone(),
                session: session.to_string(),
                sub: sub_key.to_string(),
                gen,
            },
        };
        (entry, subscription)
    }

    fn spawn_session(
        &self,
        sessions: &mut HashMap<String, SessionEntry>,
        skey: String,
        specs: Vec<SubSpec>,
        subs: HashMap<String, SubEntry>,
    ) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(supervise(self.0.clone(), skey.clone(), specs, cmd_rx));
        sessions.insert(skey, SessionEntry { cmds: cmd_tx, task, subs });
    }

    /// Drop one viewer. When a subscription's count hits zero, schedule a graced teardown;
    /// when a session's last subscription goes, the socket closes with it.
    fn release(&self, session: &str, sub_key: &str, gen: u64) {
        let should_schedule = {
            let mut sessions = self.0.sessions.lock().unwrap();
            match sessions.get_mut(session).and_then(|e| e.subs.get_mut(sub_key)) {
                Some(s) if s.gen == gen => {
                    s.viewers = s.viewers.saturating_sub(1);
                    s.viewers == 0
                }
                _ => false,
            }
        };
        if !should_schedule {
            return;
        }
        let hub = self.clone();
        let session = session.to_string();
        let sub_key = sub_key.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(TEARDOWN_GRACE).await;
            let mut sessions = hub.0.sessions.lock().unwrap();
            let Some(entry) = sessions.get_mut(&session) else { return };
            let still_idle = entry.subs.get(&sub_key).is_some_and(|s| s.gen == gen && s.viewers == 0);
            if !still_idle {
                return;
            }
            entry.subs.remove(&sub_key);
            let _ = entry.cmds.send(HubCmd::Remove(sub_key));
            if entry.subs.is_empty() {
                entry.task.abort();
                sessions.remove(&session);
            }
        });
    }
}

/// RAII viewer handle: decrements the subscription's ref-count when the SSE stream is
/// dropped.
pub struct LiveGuard {
    hub: LiveHub,
    session: String,
    sub: String,
    gen: u64,
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.hub.release(&self.session, &self.sub, self.gen);
    }
}

// ── Supervisor ─────────────────────────────────────────────────────────────────

/// One subscription's running state inside a session.
struct SubRun {
    target: LiveTarget,
    inst: Instrument,
    /// The socket channel this instrument's updates arrive on. Several subscriptions can
    /// share it: one symbol at three timeframes is one channel and three folds.
    channel: String,
    tf: i64,
    tx: broadcast::Sender<LiveMsg>,
    current: Arc<Mutex<Option<LiveEvent>>>,
    status: Arc<Mutex<LiveStatus>>,
    /// The fold up to this subscription's timeframe. `None` when the provider publishes the
    /// timeframe natively.
    agg: Option<Bucketer>,
    /// Seconds one source bar covers, which is what the bucketer needs in order to know when
    /// a source bar completes a bucket. Declared by the connector when the socket opened.
    src_secs: i64,
    last_closed: Option<OffsetDateTime>,
    /// True while the REST seed is in flight: events are held rather than applied, so the
    /// overlap merge can drop the ones the seed already covers.
    seeding: bool,
    buffer: Vec<LiveEvent>,
    /// Set by a terminal fault that named this instrument alone (an unknown symbol, a
    /// market-data subscription the account lacks). The socket keeps serving the others.
    stopped: bool,
    /// A gap backfill already running for this subscription, so a burst of gaps does not
    /// fire a provider request each.
    backfilling: Arc<std::sync::atomic::AtomicBool>,
    /// The settle re-read's own flag. It used to share the one above, which meant a gap
    /// backfill in flight silently swallowed the re-read and left the folded session bar as
    /// the stored row: the two are different jobs and neither may cancel the other.
    settling: Arc<std::sync::atomic::AtomicBool>,
    /// Where a re-read hands the provider's own bar back, so the fold re-anchors on it (a
    /// venue's DST switch) and the hub refreshes its remembered [`Anchor`].
    anchor_tx: mpsc::UnboundedSender<(String, Bar)>,
}

impl SubRun {
    fn publish(&self, next: LiveStatus) {
        let mut cur = self.status.lock().unwrap();
        if *cur != next {
            *cur = next.clone();
            let _ = self.tx.send(LiveMsg::Status(next));
        }
    }
}

/// Run one socket until the task is aborted at teardown, reconnecting with bounded backoff
/// after a clean close or a transport error.
///
/// **A terminal fault ends the loop.** A rejected key, a plan without streaming and an
/// instrument the account is not subscribed to do not become true by being asked again: the
/// old behaviour retried them forever, which spent the provider's connection allowance on a
/// refusal and told the user nothing. The supervisor publishes the reason and stops; the
/// viewers see it, and a new subscription (the user pressing Live again after fixing the
/// connector) starts a fresh session.
///
/// A [`LiveFault::Conflict`] is the middle case, another program is holding the one live seat
/// the account gets, so it keeps retrying, but straight at the slowest backoff rather than
/// hammering.
async fn supervise(
    inner: Arc<Inner>,
    session: String,
    specs: Vec<SubSpec>,
    mut cmds: mpsc::UnboundedReceiver<HubCmd>,
) {
    let mut subs: HashMap<String, SubRun> = HashMap::new();
    let Some(meta) = specs.first().map(|s| s.target.clone()) else { return };
    let Some(connector) = stream_connector_for(&meta.provider) else { return };
    // Settle re-reads are detached tasks, but their answer belongs to the fold: the bar they
    // bring back is the provider's own, so it re-anchors the bucketer. The channel lives
    // across reconnects, and the supervisor's own sender keeps it open.
    let (anchor_tx, mut anchor_rx) = mpsc::unbounded_channel::<(String, Bar)>();
    for spec in specs {
        if let Some(run) = build_run(connector.as_ref(), spec, anchor_tx.clone()) {
            subs.insert(run.inst.key(), run);
        }
    }

    let mut backoff = BACKOFF_MIN;
    loop {
        if subs.is_empty() {
            // The hub aborts this task once the last subscription is gone; until it does,
            // opening a socket for nobody would be the only thing worse than idling.
            match cmds.recv().await {
                Some(cmd) => {
                    apply_cmd(connector.as_ref(), &mut subs, cmd, &anchor_tx);
                    continue;
                }
                None => return,
            }
        }
        for run in subs.values() {
            if !run.stopped {
                run.publish(LiveStatus::connecting());
            }
        }
        let outcome = run_session(
            &inner,
            connector.as_ref(),
            &meta,
            &mut subs,
            &mut cmds,
            &anchor_tx,
            &mut anchor_rx,
        )
        .await;
        match outcome {
            Ok(()) => {
                // Clean close (server rotated / stream ended) → reconnect promptly.
                backoff = BACKOFF_MIN;
            }
            Err(e) => {
                let fault = fault_of(&e);
                let message = fault_message(&e);
                tracing::warn!(
                    "live session {} ({}/{}): ended [{}]: {message}",
                    session,
                    meta.provider,
                    meta.connector,
                    fault.code(),
                );
                for run in subs.values() {
                    if !run.stopped {
                        run.publish(LiveStatus::from_fault(fault, message.clone()));
                        *run.current.lock().unwrap() = None;
                    }
                }
                if fault.terminal() {
                    return;
                }
                // A seat someone else is holding clears on its own, but not in a second.
                if fault == LiveFault::Conflict {
                    backoff = BACKOFF_MAX;
                }
            }
        }
        for run in subs.values_mut() {
            *run.current.lock().unwrap() = None;
            let (agg, src_secs) = fresh_agg(connector.as_ref(), &run.inst);
            run.agg = agg;
            run.src_secs = src_secs;
            run.buffer.clear();
        }
        // Stay reachable while waiting: a pane opening during the backoff joins the next
        // attempt instead of waiting a reconnect out.
        let wait = tokio::time::sleep(backoff);
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                cmd = cmds.recv() => match cmd {
                    Some(cmd) => apply_cmd(connector.as_ref(), &mut subs, cmd, &anchor_tx),
                    None => return,
                },
            }
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// The bucketer a subscription needs and the grain it folds, or `None` when the provider
/// publishes the timeframe itself.
fn fresh_agg(
    connector: &dyn super::StreamConnector,
    inst: &Instrument,
) -> (Option<Bucketer>, i64) {
    let Ok(tf) = crate::histdata::timeframe_secs(&inst.timeframe) else {
        return (None, 0);
    };
    match connector.grain(inst) {
        Ok(SourceGrain::Native) | Err(_) => (None, tf),
        Ok(SourceGrain::Trades) => (Some(Bucketer::new(tf)), 0),
        Ok(SourceGrain::Bars { secs, grace }) => (Some(Bucketer::with_grace(tf, grace)), secs),
    }
}

fn build_run(
    connector: &dyn super::StreamConnector,
    spec: SubSpec,
    anchor_tx: mpsc::UnboundedSender<(String, Bar)>,
) -> Option<SubRun> {
    let inst = spec.target.instrument();
    let tf = match crate::histdata::timeframe_secs(&inst.timeframe) {
        Ok(tf) => tf,
        Err(e) => {
            let _ = spec.tx.send(LiveMsg::Status(LiveStatus::from_fault(
                LiveFault::Unsupported,
                format!("{e:#}"),
            )));
            return None;
        }
    };
    let channel = match connector.channel_of(&inst) {
        Ok(c) => c,
        Err(e) => {
            let fault = fault_of(&e);
            let status = LiveStatus::from_fault(fault, fault_message(&e));
            *spec.status.lock().unwrap() = status.clone();
            let _ = spec.tx.send(LiveMsg::Status(status));
            return None;
        }
    };
    let (agg, src_secs) = fresh_agg(connector, &inst);
    Some(SubRun {
        agg,
        src_secs,
        target: spec.target,
        inst,
        channel,
        tf,
        tx: spec.tx,
        current: spec.current,
        status: spec.status,
        last_closed: None,
        seeding: false,
        buffer: Vec::new(),
        stopped: false,
        backfilling: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        settling: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        anchor_tx,
    })
}

fn apply_cmd(
    connector: &dyn super::StreamConnector,
    subs: &mut HashMap<String, SubRun>,
    cmd: HubCmd,
    anchor_tx: &mpsc::UnboundedSender<(String, Bar)>,
) {
    match cmd {
        HubCmd::Add(spec) => {
            if let Some(run) = build_run(connector, *spec, anchor_tx.clone()) {
                subs.insert(run.inst.key(), run);
            }
        }
        HubCmd::Remove(key) => {
            subs.remove(&key);
        }
    }
}

/// One session: open the socket for the current instrument set, seed each recording
/// subscription, then route updates until the socket drops or a command changes the set.
#[allow(clippy::too_many_arguments)]
async fn run_session(
    inner: &Arc<Inner>,
    connector: &dyn super::StreamConnector,
    meta: &LiveTarget,
    subs: &mut HashMap<String, SubRun>,
    cmds: &mut mpsc::UnboundedReceiver<HubCmd>,
    anchor_tx: &mpsc::UnboundedSender<(String, Bar)>,
    anchor_rx: &mut mpsc::UnboundedReceiver<(String, Bar)>,
) -> Result<()> {
    let secrets = load_secrets(inner, meta).await?;
    let instruments: Vec<Instrument> =
        subs.values().filter(|s| !s.stopped).map(|s| s.inst.clone()).collect();
    if instruments.is_empty() {
        return Ok(());
    }
    let session = connector
        .open(SessionRequest {
            secrets: &secrets,
            connector: &meta.connector,
            instruments: &instruments,
        })
        .await?;
    let mut stream = session.stream;
    let handle = session.handle;
    for run in subs.values() {
        if !run.stopped {
            run.publish(LiveStatus::live());
        }
    }

    // Seeds run off the event loop: a socket that is not being read is a socket the provider
    // drops, and a workspace opening four recording panes at once would otherwise pause
    // everything for four REST round trips.
    let mut seeds: JoinSet<(String, Result<Vec<Bar>>, Option<OffsetDateTime>)> = JoinSet::new();
    for (key, run) in subs.iter_mut() {
        begin_sync(inner, &secrets, key.clone(), run, &mut seeds);
    }

    let mut timer = tokio::time::interval(TICK);
    loop {
        tokio::select! {
            item = stream.next() => match item {
                Some(Ok(update)) => route(inner, &secrets, meta, subs, &handle, update).await?,
                Some(Err(e)) => return Err(e),
                None => return Ok(()), // socket closed → reconnect
            },
            Some(done) = seeds.join_next() => {
                if let Ok((key, seed, stored)) = done {
                    finish_seed(inner, &secrets, meta, subs, key, seed, stored).await;
                }
            }
            // A settle re-read landed: the bar it brought back is the provider delimiting
            // the period itself, so it is both the alignment for the next one (a venue that
            // just switched to or from DST re-anchors here, one bar after the switch instead
            // of at the next reconnect) and what the hub remembers for the panes to come.
            Some((key, bar)) = anchor_rx.recv() => {
                if let Some(run) = subs.get_mut(&key) {
                    if let Some(agg) = run.agg.as_mut() {
                        agg.anchor(bar.ts);
                    }
                    inner.remember_anchor(&run.target, run.tf, &bar);
                }
            }
            _ = timer.tick() => {
                let now = OffsetDateTime::now_utc();
                let keys: Vec<String> = subs.keys().cloned().collect();
                for key in keys {
                    let events = match subs.get_mut(&key) {
                        Some(run) if !run.stopped => match run.agg.as_mut() {
                            Some(agg) => agg.tick(now),
                            None => Vec::new(),
                        },
                        _ => Vec::new(),
                    };
                    for e in events {
                        deliver(inner, &secrets, meta, subs, &key, e).await;
                    }
                }
            }
            cmd = cmds.recv() => match cmd {
                Some(HubCmd::Add(spec)) => {
                    let key = spec.target.key();
                    if let Some(mut run) = build_run(connector, *spec, anchor_tx.clone()) {
                        let joining = run.inst.clone();
                        begin_sync(inner, &secrets, key.clone(), &mut run, &mut seeds);
                        run.publish(LiveStatus::live());
                        subs.insert(key, run);
                        // The socket is already open: a joining pane costs a frame, which is
                        // the whole point of multiplexing.
                        handle.add(vec![joining]).await;
                    }
                }
                Some(HubCmd::Remove(key)) => {
                    if let Some(run) = subs.remove(&key) {
                        // Only drop the channel when nothing else is reading it: two
                        // timeframes of one symbol share a subscription on the wire.
                        if !subs.values().any(|s| s.channel == run.channel) {
                            handle.remove(vec![run.inst]).await;
                        }
                    }
                }
                None => return Ok(()),
            },
        }
    }
}

/// Put one subscription into sync at the start of a session (and when a pane joins one
/// already running): reset the fold's history, then choose how this instrument learns where
/// it stands.
///
/// Three cases, cheapest first:
///   - **nothing to record and epoch-bucketed**: no request at all, the series is known whole
///     up to the bucket before the current one;
///   - **nothing to record, session-shaped, and the hub still remembers the alignment**: no
///     request either, the remembered [`Anchor`] is what the seed was for;
///   - **anything else**: the REST seed (a recording subscription must write the gap, and a
///     cold session-shaped fold has to be told where the day starts).
fn begin_sync(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    key: String,
    run: &mut SubRun,
    seeds: &mut JoinSet<(String, Result<Vec<Bar>>, Option<OffsetDateTime>)>,
) {
    run.last_closed = None;
    run.buffer.clear();
    run.seeding = false;
    if run.stopped {
        return;
    }
    let records = run.target.dataset_id.is_some();
    if anchored(run.tf) {
        if !records {
            if let Some(a) = inner.warm_anchor(&run.target) {
                adopt_anchor(run, &a);
                return;
            }
        }
    } else if !records {
        // No seed, so the series is assumed whole up to the bucket before the current one
        // and `missing_buckets` catches anything further ahead. Epoch-bucketed only: on a
        // session-shaped timeframe this boundary is a UTC midnight the venue never used, and
        // subtracting a period from it reports a bucket that never existed.
        run.last_closed = Some(
            super::bar_start(OffsetDateTime::now_utc(), run.tf) - TimeDuration::seconds(run.tf),
        );
        return;
    }
    run.seeding = true;
    spawn_seed(inner, secrets, key, run, seeds);
}

/// Align a fold on a remembered anchor instead of a fresh seed, adopting the period in
/// progress when the remembered bar is still both current and recent.
fn adopt_anchor(run: &mut SubRun, a: &Anchor) {
    let tf = run.tf;
    let Some(agg) = run.agg.as_mut() else { return };
    agg.anchor_at(a.offset);
    if a.last.ts + TimeDuration::seconds(tf) > OffsetDateTime::now_utc()
        && a.at.elapsed() < ANCHOR_PRIME_FRESH
    {
        agg.prime(bar_to_event(&a.last));
    }
}

/// Kick off one subscription's REST seed: the gap since stored history, or the last
/// [`SEED_BARS`] bars when there is none.
///
/// A subscription with no dataset normally skips this, but a session-shaped timeframe seeds
/// anyway and simply writes nothing: the bars are what tell the fold where the period starts
/// and what the one in progress already looks like.
fn spawn_seed(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    key: String,
    run: &SubRun,
    seeds: &mut JoinSet<(String, Result<Vec<Bar>>, Option<OffsetDateTime>)>,
) {
    let inner = inner.clone();
    let secrets = secrets.clone();
    let target = run.target.clone();
    let tf = run.tf;
    let dataset_id = target.dataset_id;
    seeds.spawn(async move {
        let out = async {
            let rest = crate::histdata::connector_for(&target.provider)?;
            let now = OffsetDateTime::now_utc();
            let stored = match dataset_id {
                Some(id) => store::latest_bar_ts(&inner.pool, id).await?,
                None => None,
            };
            let from =
                stored.unwrap_or_else(|| seed_from(now, tf, seed_bar_count(rest.as_ref())));
            let seed = rest
                .fetch_chunk(
                    &inner.http,
                    &secrets,
                    &target.ticker,
                    &target.asset_type,
                    &target.timeframe,
                    from,
                    now,
                )
                .await
                .context("live REST seed")?
                .bars;
            if let (Some(id), false) = (dataset_id, seed.is_empty()) {
                store::write_bars(&inner.pool, id, &seed).await?;
            }
            Ok::<_, anyhow::Error>((seed, stored))
        }
        .await;
        match out {
            Ok((seed, stored)) => (key, Ok(seed), stored),
            Err(e) => (key, Err(e), None),
        }
    });
}

/// Apply a landed seed: broadcast it, then replay the events buffered while it was in
/// flight, dropping the ones it already covers (the overlap merge).
async fn finish_seed(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    subs: &mut HashMap<String, SubRun>,
    key: String,
    seed: Result<Vec<Bar>>,
    stored: Option<OffsetDateTime>,
) {
    let (buffered, seed_last) = {
        let Some(run) = subs.get_mut(&key) else { return };
        run.seeding = false;
        // Where a series the seed could not place is known to be whole: the bucket before
        // the current one, leaving `missing_buckets` to catch anything further ahead. Only
        // on an epoch-bucketed timeframe — that boundary is a UTC midnight a session-shaped
        // series never has a bar on, and gap detection is skipped there anyway.
        let fallback = (!anchored(run.tf)).then(|| {
            super::bar_start(OffsetDateTime::now_utc(), run.tf) - TimeDuration::seconds(run.tf)
        });
        match seed {
            Ok(bars) => {
                // Seeded bars are broadcast, not just written. The viewer loaded its history
                // from the series endpoint *before* this socket opened, so a bar that closed
                // in between is in the store and nowhere on the chart. Re-sending it is
                // idempotent: the client keys on the timestamp and replaces in place.
                for b in &bars {
                    let _ = run.tx.send(LiveMsg::Bar(bar_to_event(b)));
                }
                let mut seed_last = bars.last().map(|b| b.ts);
                // A session-shaped fold takes its alignment from the bars themselves, and
                // adopts the period already in progress rather than starting a new bucket
                // from the next tick: today's open and range belong to the day, not to the
                // moment the socket opened.
                if anchored(run.tf) {
                    if let Some(last) = bars.last() {
                        // Learned once, reused by every pane that opens on these coordinates
                        // for the next few hours instead of paying for the same request.
                        inner.remember_anchor(&run.target, run.tf, last);
                    }
                    if let (Some(agg), Some(last)) = (run.agg.as_mut(), bars.last()) {
                        agg.anchor(last.ts);
                        if last.ts + TimeDuration::seconds(run.tf) > OffsetDateTime::now_utc() {
                            agg.prime(bar_to_event(last));
                            // The primed bar is not closed, so the newest *closed* one is the
                            // bar before it (none, when the seed returned only this session).
                            seed_last =
                                bars.len().checked_sub(2).and_then(|i| bars.get(i)).map(|b| b.ts);
                        }
                    }
                }
                run.last_closed = seed_last.or(stored).or(fallback);
                (std::mem::take(&mut run.buffer), seed_last)
            }
            Err(e) => {
                // A seed that failed is not a session failure: the socket is fine and the
                // chart still gets its live bars, only the recorded series may keep the hole
                // this was meant to close.
                tracing::warn!("live seed {} ({}): {e:#}", run.target.ticker, meta.connector);
                run.last_closed = stored.or(fallback);
                (std::mem::take(&mut run.buffer), None)
            }
        }
    };
    // The overlap merge: anything the seed already covers is history, not an update.
    for e in buffered {
        if let Some(sl) = seed_last {
            if e.bar_ts < sl {
                continue;
            }
        }
        deliver(inner, secrets, meta, subs, &key, e).await;
    }
}

/// Hand one socket update to every subscription reading that channel.
async fn route(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    subs: &mut HashMap<String, SubRun>,
    handle: &super::FeedHandle,
    update: FeedUpdate,
) -> Result<()> {
    let keys: Vec<String> = subs
        .iter()
        .filter(|(_, s)| !s.stopped && s.channel == update.channel)
        .map(|(k, _)| k.clone())
        .collect();

    // A refusal that named one instrument stops that instrument, never the socket: the other
    // panes on this connector are still being served.
    if let Payload::Fault { fault, message } = &update.payload {
        let mut orphan: Option<Instrument> = None;
        for key in &keys {
            let Some(run) = subs.get_mut(key) else { continue };
            run.publish(LiveStatus::from_fault(*fault, message.clone()));
            if fault.terminal() {
                run.stopped = true;
                *run.current.lock().unwrap() = None;
                orphan = Some(run.inst.clone());
            }
        }
        if let Some(inst) = orphan {
            // Stop paying for a channel nothing can read any more.
            if !subs.values().any(|s| !s.stopped && s.channel == update.channel) {
                handle.remove(vec![inst]).await;
            }
        }
        return Ok(());
    }

    for key in keys {
        let events = {
            let Some(run) = subs.get_mut(&key) else { continue };
            let src_secs = run.src_secs;
            match (&update.payload, run.agg.as_mut()) {
                // The provider publishes this timeframe itself.
                (Payload::Bar(ev), None) => vec![ev.clone()],
                (Payload::Bar(ev), Some(agg)) => agg.bar(ev.clone(), src_secs),
                (Payload::Trade { price, size, ts }, Some(agg)) => agg.trade(*price, *size, *ts),
                // A trade feed with no bucketer cannot happen (the grain says Trades), and
                // inventing a bar from a single print would be worse than dropping it.
                _ => Vec::new(),
            }
        };
        for e in events {
            deliver(inner, secrets, meta, subs, &key, e).await;
        }
    }
    Ok(())
}

/// Deliver one derived event to a subscription: buffer it while the seed is in flight, else
/// apply it.
async fn deliver(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    subs: &mut HashMap<String, SubRun>,
    key: &str,
    e: LiveEvent,
) {
    let Some(run) = subs.get_mut(key) else { return };
    if run.seeding {
        run.buffer.push(e);
        return;
    }
    if let Err(err) = apply(inner, secrets, meta, run, e).await {
        tracing::warn!("live apply {} ({}): {err:#}", run.target.ticker, meta.connector);
    }
}

/// Apply one event: advance on close (backfilling any gap first, persisting when the
/// instrument is stored), else update the forming bar. Broadcasts every event to viewers.
async fn apply(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    run: &mut SubRun,
    e: LiveEvent,
) -> Result<()> {
    if e.closed {
        // Technical gap: closed bar skipped one or more buckets → backfill just that span.
        //
        // **Epoch-bucketed timeframes only.** On a session-shaped one every non-session day
        // counts as a missing bucket, so a weekend or a holiday looks exactly like a dropped
        // bar: Monday's first close would fire a REST request for a Saturday that was never
        // going to exist. What actually closes a hole there is the seed on the next
        // reconnect, which asks from stored history rather than from a calendar we do not
        // have.
        if !anchored(run.tf) {
            if let Some(prev) = run.last_closed {
                if !missing_buckets(prev, e.bar_ts, run.tf).is_empty() {
                    spawn_gap_backfill(
                        inner,
                        secrets,
                        meta,
                        run,
                        prev + TimeDuration::seconds(run.tf),
                        e.bar_ts,
                    );
                }
            }
        }
        if let Some(dataset_id) = run.target.dataset_id {
            store::write_bars(&inner.pool, dataset_id, &[e.as_bar()]).await?;
        }
        // The fold is an approximation for a period the provider delimits itself: it cannot
        // know the closing print, and it counts extended-hours minutes the official session
        // bar leaves out. Re-read that one bar once it has settled and let the provider's own
        // values overwrite it, on screen as much as in the store — a pane that records
        // nothing is looking at the same approximation.
        if anchored(run.tf) {
            spawn_settle_reread(
                inner,
                secrets,
                meta,
                run,
                e.bar_ts,
                e.bar_ts + TimeDuration::seconds(run.tf),
            );
        }
        run.last_closed = Some(e.bar_ts);
        *run.current.lock().unwrap() = None;
    } else {
        *run.current.lock().unwrap() = Some(e.clone());
    }
    let _ = run.tx.send(LiveMsg::Bar(e));
    Ok(())
}

/// Fetch a missing `[from, to)` span from REST, persist it when the instrument is stored, and
/// broadcast each recovered bar as closed so viewers fill the hole either way.
///
/// Detached on purpose: it is a provider round trip, and the socket it would otherwise block
/// is now shared by every pane on this connector. Best-effort throughout, a failed backfill
/// is logged, not fatal, and only one runs at a time per subscription.
fn spawn_gap_backfill(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    run: &SubRun,
    from: OffsetDateTime,
    to: OffsetDateTime,
) {
    spawn_refetch(
        inner,
        secrets,
        meta,
        run,
        Refetch {
            from,
            to,
            delay: Duration::ZERO,
            flag: run.backfilling.clone(),
            reanchor: false,
        },
    );
}

/// Re-read a session-shaped bar once the provider has finalized it, and re-anchor the fold on
/// what comes back.
///
/// **Its own in-flight flag.** Sharing the gap backfill's meant a backfill still running when
/// the session closed made the re-read return early, leaving the fold's approximation as the
/// stored row with nothing to say so.
fn spawn_settle_reread(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    run: &SubRun,
    from: OffsetDateTime,
    to: OffsetDateTime,
) {
    spawn_refetch(
        inner,
        secrets,
        meta,
        run,
        Refetch {
            from,
            to,
            delay: SETTLE_DELAY,
            flag: run.settling.clone(),
            reanchor: true,
        },
    );
}

/// One detached REST re-read: which span, how long to wait first, which in-flight flag it
/// holds, and whether its answer re-anchors the fold.
struct Refetch {
    from: OffsetDateTime,
    to: OffsetDateTime,
    delay: Duration,
    flag: Arc<std::sync::atomic::AtomicBool>,
    reanchor: bool,
}

fn spawn_refetch(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    run: &SubRun,
    job: Refetch,
) {
    use std::sync::atomic::Ordering;
    let Refetch { from, to, delay, flag, reanchor } = job;
    if flag.swap(true, Ordering::SeqCst) {
        return;
    }
    let inner = inner.clone();
    let secrets = secrets.clone();
    let target = run.target.clone();
    let connector_name = meta.connector.clone();
    let tx = run.tx.clone();
    let anchor = reanchor.then(|| (run.anchor_tx.clone(), run.inst.key()));
    tokio::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let outcome = async {
            let rest = crate::histdata::connector_for(&target.provider)?;
            let chunk = rest
                .fetch_chunk(
                    &inner.http,
                    &secrets,
                    &target.ticker,
                    &target.asset_type,
                    &target.timeframe,
                    from,
                    to,
                )
                .await?;
            let bars: Vec<Bar> =
                chunk.bars.into_iter().filter(|b| b.ts >= from && b.ts < to).collect();
            if !bars.is_empty() {
                if let Some(dataset_id) = target.dataset_id {
                    store::write_bars(&inner.pool, dataset_id, &bars).await?;
                }
                for b in &bars {
                    let _ = tx.send(LiveMsg::Bar(bar_to_event(b)));
                }
                // The provider's own bar for a period it delimits: the alignment the fold
                // should use from here on, DST switch included.
                if let (Some((anchor_tx, key)), Some(last)) = (&anchor, bars.last()) {
                    let _ = anchor_tx.send((key.clone(), last.clone()));
                }
                tracing::info!(
                    "live re-read {} bar(s) for {}",
                    bars.len(),
                    target.ticker
                );
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(e) = outcome {
            tracing::warn!("live backfill {} ({connector_name}): {e:#}", target.ticker);
        }
        flag.store(false, Ordering::SeqCst);
    });
}

/// A stored/fetched bar as a closed live event, the shape the SSE wire speaks. Shared with
/// the catch-up the API sends on subscribe, so a recovered bar and a streamed one are
/// indistinguishable to the chart.
pub fn bar_to_event(b: &Bar) -> LiveEvent {
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

/// Credentials for **the connector this session was started on**, the settings in the clear
/// with the secrets over them, the same map a download reads.
///
/// It is the target's connector and never "the provider's first one": with two Alpaca keys
/// on one account, one free and one paid, the wrong pick opens the wrong socket and is
/// refused. The row is re-read on every reconnect so a credential the user just fixed is
/// picked up without restarting anything.
async fn load_secrets(
    inner: &Arc<Inner>,
    meta: &LiveTarget,
) -> Result<std::collections::HashMap<String, String>> {
    conn_store::load_creds(&inner.pool, &inner.cipher, meta.connector_id)
        .await
        .with_context(|| format!("credentials for connector {}", meta.connector))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(provider: &str, connector: Uuid, ticker: &str, tf: &str) -> LiveTarget {
        LiveTarget {
            dataset_id: None,
            connector_id: connector,
            connector: "test".into(),
            provider: provider.into(),
            asset_type: "crypto".into(),
            ticker: ticker.into(),
            timeframe: tf.into(),
        }
    }

    /// The point of the whole change: instruments that can share a socket do, and the ones
    /// that cannot say so themselves.
    #[test]
    fn what_shares_a_socket_and_what_does_not() {
        let key = Uuid::new_v4();
        let other = Uuid::new_v4();
        let binance = stream_connector_for("binance").unwrap();
        let btc = target("binance", key, "BTCUSDT", "1m");
        let eth = target("binance", key, "ETHUSDT", "1h");
        let session_of = |c: &dyn super::super::StreamConnector, t: &LiveTarget| {
            session_key(t.connector_id, &c.socket_class(&t.instrument()).unwrap())
        };
        // Two symbols, two timeframes, one connector: one socket.
        assert_eq!(session_of(binance.as_ref(), &btc), session_of(binance.as_ref(), &eth));
        // Two connectors are two entitlements and two seats, never one socket.
        let btc_other = target("binance", other, "BTCUSDT", "1m");
        assert_ne!(session_of(binance.as_ref(), &btc), session_of(binance.as_ref(), &btc_other));

        // Kraken's OHLC rows do not name their interval, so the interval is the socket.
        let kraken = stream_connector_for("kraken").unwrap();
        let k1 = target("kraken", key, "BTC/USD", "1m");
        let k2 = target("kraken", key, "ETH/USD", "1m");
        let k3 = target("kraken", key, "BTC/USD", "1h");
        assert_eq!(session_of(kraken.as_ref(), &k1), session_of(kraken.as_ref(), &k2));
        assert_ne!(session_of(kraken.as_ref(), &k1), session_of(kraken.as_ref(), &k3));

        // Alpaca runs one socket per market: equities and crypto never share.
        let alpaca = stream_connector_for("alpaca").unwrap();
        let mut equity = target("alpaca", key, "AAPL", "1m");
        equity.asset_type = "equity".into();
        let crypto = target("alpaca", key, "BTC/USD", "1m");
        assert_ne!(session_of(alpaca.as_ref(), &equity), session_of(alpaca.as_ref(), &crypto));
    }

    #[test]
    fn a_subscription_is_an_instrument_at_a_timeframe() {
        let key = Uuid::new_v4();
        let a = target("binance", key, "BTCUSDT", "1m");
        let b = target("binance", key, "BTCUSDT", "1h");
        assert_ne!(a.key(), b.key());
        // On a native-candle provider each timeframe is its own channel…
        let binance = stream_connector_for("binance").unwrap();
        assert_ne!(
            binance.channel_of(&a.instrument()).unwrap(),
            binance.channel_of(&b.instrument()).unwrap()
        );
        // …and on a minute-bar provider both fold from the same one, which is what stops a
        // second timeframe from costing a second subscription upstream.
        let alpaca = stream_connector_for("alpaca").unwrap();
        let mut a = a;
        let mut b = b;
        a.asset_type = "equity".into();
        b.asset_type = "equity".into();
        a.ticker = "AAPL".into();
        b.ticker = "AAPL".into();
        assert_eq!(
            alpaca.channel_of(&a.instrument()).unwrap(),
            alpaca.channel_of(&b.instrument()).unwrap()
        );
    }

    /// A market-data line is an instrument, and IB counts them per account. Getting this
    /// wrong in either direction is a bad warning: per subscription over-counts a trader
    /// watching one symbol on three timeframes, and ignoring the connector would add up two
    /// accounts that pay separately.
    #[test]
    fn lines_are_instruments_per_account() {
        let key = Uuid::new_v4();
        let other = Uuid::new_v4();
        let targets = vec![
            target("ibkr", key, "AAPL", "1m"),
            target("ibkr", key, "AAPL", "1h"),
            target("ibkr", key, "MSFT", "5m"),
            target("ibkr", other, "AAPL", "1m"),
        ];
        assert_eq!(lines_of(targets.iter(), key), 2, "one line per instrument, not per pane");
        assert_eq!(lines_of(targets.iter(), other), 1, "the other account pays for its own");
        assert_eq!(lines_of([].iter(), key), 0);
    }

    fn run_of(provider: &str, asset: &str, ticker: &str, tf: &str) -> SubRun {
        let connector = stream_connector_for(provider).unwrap();
        let (tx, _rx) = broadcast::channel(4);
        let (anchor_tx, _arx) = mpsc::unbounded_channel();
        let mut t = target(provider, Uuid::new_v4(), ticker, tf);
        t.asset_type = asset.into();
        let spec = SubSpec {
            target: t,
            tx,
            current: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(LiveStatus::connecting())),
        };
        build_run(connector.as_ref(), spec, anchor_tx).unwrap()
    }

    /// Which timeframes the provider delimits itself. Everything from a day up is a session
    /// (a venue's midnight, a Monday), everything under it is an epoch bucket on every venue
    /// we stream.
    #[test]
    fn a_session_is_a_day_and_up() {
        assert!(!anchored(14_400), "4h is epoch-bucketed");
        assert!(anchored(86_400), "a daily bar is a session");
        assert!(anchored(604_800), "so is a week");
    }

    /// The settle re-read and the gap backfill are separate jobs with separate flags. They
    /// shared one, so a backfill still in flight when the session closed made the re-read
    /// return early and left the fold's approximation as the stored row, silently.
    #[test]
    fn a_gap_backfill_cannot_swallow_the_settle_reread() {
        let run = run_of("alpaca", "equity", "AAPL", "1d");
        assert!(!Arc::ptr_eq(&run.backfilling, &run.settling));
        run.backfilling.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!run.settling.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// The remembered anchor is a property of the venue, not of the key that read it: two
    /// connectors on one provider share it, and a second timeframe does not.
    #[test]
    fn an_anchor_is_remembered_per_instrument_and_timeframe() {
        let mut a = target("massive", Uuid::new_v4(), "AAPL", "1d");
        a.asset_type = "equity".into();
        let mut other_key = a.clone();
        other_key.connector_id = Uuid::new_v4();
        assert_eq!(anchor_key(&a), anchor_key(&other_key));
        let mut weekly = a.clone();
        weekly.timeframe = "1w".into();
        assert_ne!(anchor_key(&a), anchor_key(&weekly));
    }

    /// Who folds the timeframe, per provider. A wrong answer here is a chart drawing the
    /// provider's grain instead of the user's.
    #[test]
    fn the_fold_belongs_to_the_hub_only_when_the_provider_cannot_do_it() {
        let inst = Instrument::new("BTCUSDT", "crypto", "1h");
        let binance = stream_connector_for("binance").unwrap();
        let (agg, src) = fresh_agg(binance.as_ref(), &inst);
        assert!(agg.is_none(), "binance publishes the interval itself");
        assert_eq!(src, 3600);

        let coinbase = stream_connector_for("coinbase").unwrap();
        let (agg, _) = fresh_agg(coinbase.as_ref(), &Instrument::new("BTC-USD", "crypto", "1h"));
        assert!(agg.is_some(), "trades are folded here");

        let alpaca = stream_connector_for("alpaca").unwrap();
        let (agg, src) = fresh_agg(alpaca.as_ref(), &Instrument::new("AAPL", "equity", "5m"));
        assert!(agg.is_some());
        assert_eq!(src, 60, "alpaca's grain is the minute bar");
    }
}
